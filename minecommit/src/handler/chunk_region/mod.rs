mod nbt;
mod palette;

use anyhow::{Context, Result};
use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};
use simdnbt::borrow::read;
use simdnbt::owned::{BaseNbt, NbtCompound};
use simdnbt::{Deserialize, Serialize};
use std::io::Cursor;

use super::Handler;
use crate::odb::{OdbReader, OdbWriter};
use crate::utils::nbt::{dump_nbt, load_nbt, sort_nbt};
use crate::utils::region::{parse_xz, read_region, write_region};

use nbt::{SectionsDump, restore_chunk, split_chunk};

const FLATTEN_PATTERNS: &[&str] = &["**/region/r.*.*.mca"];
const UNFLATTEN_PATTERNS: &[&str] = &["**/region/r.*.*.mca/timestamps"]; // timestamps is sentry
/// Sentry for region files kept byte-for-byte because their chunks predate the
/// 1.18 layout. See [`ChunkOutcome::PreAnvil118`].
const RAW_UNFLATTEN_PATTERNS: &[&str] = &["**/region/r.*.*.mca/raw"];

/// What flattening a single chunk produced.
enum ChunkOutcome {
    /// The chunk predates the Minecraft 1.18 layout, so the whole region file
    /// has to be stored byte-for-byte instead.
    ///
    /// MineCommit's flattener understands only the 1.18+ layout, which places
    /// `Status` and `sections` at the root of the chunk NBT. Worlds from 1.13
    /// to 1.17 nest both under `Level`, and worlds older than 1.13 have no
    /// `Status` at all, so neither can be split into sections and palettes.
    /// A world upgraded in place can mix layouts inside one region file, so a
    /// single pre-1.18 chunk disqualifies the entire file.
    PreAnvil118,
    /// The chunk is not fully generated and is intentionally not stored.
    Incomplete,
    /// `(chunk_x, chunk_z, other, sections_dump, inhabited_time, last_update)`
    Flattened(Box<(i32, i32, BaseNbt, Vec<u8>, i64, i64)>),
}

pub(crate) struct ChunkRegionHandler;

impl Handler for ChunkRegionHandler {
    fn workspace(&self) -> &'static str {
        "chunk-region"
    }

    fn flatten(self, save: &impl OdbReader, storage: &mut impl OdbWriter) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        for pattern in FLATTEN_PATTERNS {
            for key in save.glob(pattern)? {
                // Parse region file
                log::info!("Process chunk region file {key}");
                let data = save.get(&key)?;
                let filename = key.split('/').next_back().unwrap_or("");
                let (region_x, region_z) = parse_xz(filename)
                    .with_context(|| format!("failed to parse region coordinates from {key}"))?;
                let Some((timestamp_header, chunks)) =
                    read_region(Cursor::new(&data), region_x, region_z)
                        .with_context(|| format!("failed to read region from {key}"))?
                else {
                    processed.push(key);
                    continue;
                };

                // Each section carries its own local palette, so chunks can be
                // processed independently in parallel without a global mapping pass.
                let outcomes = chunks
                    .into_par_iter()
                    .map(|(chunk_x, chunk_z, nbt)| {
                        let nbt =
                            load_nbt(Cursor::new(&nbt)).context("failed to load chunk nbt")?;
                        // Order matters: a chunk that is merely unfinished must
                        // stay `Incomplete` (skipped, as before) rather than
                        // dragging its whole region file into raw storage. Only
                        // chunks that the flattener could not have handled at
                        // all fall back to `PreAnvil118`.
                        let Some(status) = nbt.string("Status") else {
                            return Ok(ChunkOutcome::PreAnvil118);
                        };
                        if status.to_string_lossy() != "minecraft:full" {
                            return Ok(ChunkOutcome::Incomplete);
                        }
                        if nbt.list("sections").is_none() {
                            return Ok(ChunkOutcome::PreAnvil118);
                        }
                        let (other, sections) = split_chunk(nbt).with_context(|| {
                            format!("failed to process chunk ({chunk_x}, {chunk_z}) at file {key}")
                        })?;

                        // Extract InhabitedTime and LastUpdate from other (will store in timestamps)
                        let name = other.name().to_owned();
                        let mut other_compound = other.as_compound();
                        let inhabited_time = other_compound
                            .remove("InhabitedTime")
                            .and_then(simdnbt::owned::NbtTag::into_long)
                            .context("missing 'InhabitedTime' field")?;
                        let last_update = other_compound
                            .remove("LastUpdate")
                            .and_then(simdnbt::owned::NbtTag::into_long)
                            .context("missing 'LastUpdate' field")?;
                        let other = simdnbt::owned::BaseNbt::new(name, other_compound);

                        let other = sort_nbt(other);
                        let mut sections_dump = Vec::with_capacity(200 * 1024);
                        sections.to_nbt().write(&mut sections_dump);
                        Ok(ChunkOutcome::Flattened(Box::new((
                            chunk_x,
                            chunk_z,
                            other,
                            sections_dump,
                            inhabited_time,
                            last_update,
                        ))))
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("failed to process chunks")?;

                // Old worlds cannot be flattened, but they must still round-trip.
                // Store the file exactly as it is so a restore reproduces it
                // byte-for-byte instead of failing the whole backup.
                if outcomes
                    .iter()
                    .any(|outcome| matches!(outcome, ChunkOutcome::PreAnvil118))
                {
                    log::info!("Store pre-1.18 chunk region file {key} as raw data");
                    storage.put(&format!("{key}/raw"), &data)?;
                    processed.push(key);
                    continue;
                }

                let mut result = outcomes
                    .into_iter()
                    .filter_map(|outcome| match outcome {
                        ChunkOutcome::Flattened(chunk) => Some(*chunk),
                        ChunkOutcome::Incomplete | ChunkOutcome::PreAnvil118 => None,
                    })
                    .collect::<Vec<_>>();

                // Sort by (cz, cx) for deterministic ordering matching unflatten glob order
                result
                    .sort_unstable_by(|(cx1, cz1, ..), (cx2, cz2, ..)| (cz1, cx1).cmp(&(cz2, cx2)));

                // Build timestamps NBT with header byte array + InhabitedTime/LastUpdate long arrays
                {
                    let mut header_compound = simdnbt::owned::NbtCompound::new();
                    header_compound.insert(
                        "TimestampHeader",
                        simdnbt::owned::NbtTag::ByteArray(timestamp_header.to_vec()),
                    );
                    header_compound.insert(
                        "InhabitedTime",
                        simdnbt::owned::NbtTag::LongArray(
                            result.iter().map(|(_, _, _, _, it, _)| *it).collect(),
                        ),
                    );
                    header_compound.insert(
                        "LastUpdate",
                        simdnbt::owned::NbtTag::LongArray(
                            result.iter().map(|(_, _, _, _, _, lu)| *lu).collect(),
                        ),
                    );
                    let header_nbt = simdnbt::owned::BaseNbt::new("", header_compound);
                    let mut header_buf = Vec::with_capacity(4096 + 100);
                    header_nbt.write(&mut header_buf);
                    storage.put(&format!("{key}/timestamps"), &header_buf)?;
                }

                // Build and write others.nbt (all other NBTs in one compound)
                {
                    let mut others_compound = simdnbt::owned::NbtCompound::new();
                    for (chunk_x, chunk_z, other, _, _, _) in &mut result {
                        let key_str = format!("c.{}.{}", chunk_x, chunk_z);
                        others_compound.insert(
                            key_str,
                            simdnbt::owned::NbtTag::Compound(
                                std::mem::replace(other, BaseNbt::default()).as_compound(),
                            ),
                        );
                    }
                    let others_nbt = simdnbt::owned::BaseNbt::new("", others_compound);
                    let mut others_buf = Vec::new();
                    others_nbt.write(&mut others_buf);
                    storage.put(&format!("{key}/others.nbt"), &others_buf)?;
                }

                // Write individual sections dumps
                storage.put_par(
                    result
                        .iter()
                        .map(|(chunk_x, chunk_z, _, dump, ..)| {
                            (
                                format!("{key}/sections/c.{chunk_x}.{chunk_z}.dump"),
                                dump.as_slice(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )?;

                processed.push(key);
            }
        }

        Ok(processed)
    }

    fn unflatten(self, save: &mut impl OdbWriter, storage: &impl OdbReader) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        for pattern in RAW_UNFLATTEN_PATTERNS {
            for raw_key in storage.glob(pattern)? {
                let Some(region_key) = raw_key.strip_suffix("/raw") else {
                    continue;
                };
                log::info!("Process chunk region file (raw) {region_key}");
                let data = storage
                    .get(&raw_key)
                    .with_context(|| format!("failed to read {raw_key}"))?;
                save.put(region_key, &data)?;
                processed.push(raw_key);
            }
        }
        for pattern in UNFLATTEN_PATTERNS {
            for ts_key in storage.glob(pattern)? {
                log::info!("Process chunk region file (timestamps) {ts_key}");
                let Some(region_key) = ts_key.strip_suffix("/timestamps") else {
                    continue;
                };
                let filename = region_key.split('/').next_back().unwrap_or("");
                let (region_x, region_z) = parse_xz(filename)
                    .with_context(|| format!("failed to parse region coordinates from {ts_key}"))?;
                let ts_data = storage.get(&ts_key)?;
                let ts_nbt = load_nbt(std::io::Cursor::new(&ts_data))
                    .context("failed to load timestamp header nbt")?;
                let ts_compound = ts_nbt.as_compound();
                let timestamp_header: [u8; 4096] = ts_compound
                    .byte_array("TimestampHeader")
                    .context("missing 'TimestampHeader' in timestamp nbt")?
                    .try_into()
                    .context("timestamp header must be exactly 4096 bytes")?;
                let inhabited_times: Vec<i64> = ts_compound
                    .long_array("InhabitedTime")
                    .context("missing 'InhabitedTime' in timestamp nbt")?
                    .to_vec();
                let last_updates: Vec<i64> = ts_compound
                    .long_array("LastUpdate")
                    .context("missing 'LastUpdate' in timestamp nbt")?
                    .to_vec();

                // Read others.nbt (all other NBTs in one compound)
                let others_key = format!("{region_key}/others.nbt");
                let others_data = storage
                    .get(&others_key)
                    .with_context(|| format!("failed to read {others_key}"))?;
                let others_nbt = load_nbt(std::io::Cursor::new(&others_data))
                    .context("failed to load others nbt")?;
                let mut others_compound = others_nbt.as_compound();

                // Extract coordinates from compound keys
                let mut coords: Vec<(i32, i32)> = others_compound
                    .keys()
                    .filter_map(|key| {
                        let s = key.to_str();
                        s.strip_prefix("c.").and_then(|rest| {
                            let (x_str, z_str) = rest.split_once('.')?;
                            let x = x_str.parse::<i32>().ok()?;
                            let z = z_str.parse::<i32>().ok()?;
                            Some((x, z))
                        })
                    })
                    .collect();
                coords.sort_unstable_by(|(x1, z1), (x2, z2)| (z1, x1).cmp(&(z2, x2)));

                let dump_keys: Vec<String> = coords
                    .iter()
                    .map(|(cx, cz)| format!("{region_key}/sections/c.{cx}.{cz}.dump"))
                    .collect();

                let dump_data =
                    storage.get_par(&dump_keys.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;

                // Build tasks: pair dump data with other data from compound
                let mut tasks: Vec<(i32, i32, NbtCompound, Vec<u8>)> = coords
                    .into_iter()
                    .zip(dump_data)
                    .map(|((cx, cz), dump)| {
                        let coord_key = format!("c.{}.{}", cx, cz);
                        let other = others_compound
                            .remove(&coord_key)
                            .ok_or_else(|| anyhow::anyhow!("missing '{}' in other", coord_key))?
                            .into_compound()
                            .ok_or_else(|| {
                                anyhow::anyhow!("expect '{}' is NBT Compound", coord_key)
                            })?;
                        Ok((cx, cz, other, dump))
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("failed to build tasks")?;

                // Sort by (cz, cx) to match flatten order for InhabitedTime/LastUpdate indexing
                tasks
                    .sort_unstable_by(|(cx1, cz1, ..), (cx2, cz2, ..)| (cz1, cx1).cmp(&(cz2, cx2)));

                let chunks = tasks
                    .into_par_iter()
                    .enumerate()
                    .map(|(i, (chunk_x, chunk_z, nbt_data, dump_data))| {
                        use simdnbt::borrow::Nbt;

                        // Inject InhabitedTime and LastUpdate back into other
                        let mut compound = nbt_data;
                        compound.insert("InhabitedTime", inhabited_times[i]);
                        compound.insert("LastUpdate", last_updates[i]);
                        let other = simdnbt::owned::BaseNbt::new("", compound);

                        let Nbt::Some(nbt) = read(&mut Cursor::new(dump_data.as_slice()))
                            .context("failed to read sections dump as nbt")?
                        else {
                            anyhow::bail!("sections dump is empty");
                        };
                        let sections_dump: SectionsDump = SectionsDump::from_nbt(&nbt)
                            .context("failed to deserialize sections dump")?;
                        let nbt = dump_nbt(
                            restore_chunk(other, sections_dump)
                                .with_context(|| format!("failed to restore chunk for {ts_key}"))
                                .context("failed to restore chunk")?,
                            300 * 1024, // 300 KiB
                        )
                        .context("failed to dump other nbt")?;
                        Ok((chunk_x, chunk_z, nbt))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let mut mca_buf = Vec::with_capacity(8 * 1024 * 1024); // 8MiB
                write_region(
                    region_x,
                    region_z,
                    &timestamp_header,
                    chunks,
                    Cursor::new(&mut mca_buf),
                )
                .with_context(|| format!("failed to write region for {ts_key}"))?;
                save.put(region_key, &mca_buf)?;

                processed.push(ts_key.to_owned());
                processed.push(others_key);
                processed.extend(dump_keys);
            }
        }
        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use tempfile::TempDir;

    use super::*;
    use crate::odb::LocalFsOdb;

    const REGION_KEY: &str = "region/r.0.0.mca";

    /// A pre-1.18 chunk: `Status` and `Sections` live under `Level`, so the
    /// flattener cannot split it. Mirrors what an old or upgraded world holds.
    fn legacy_chunk() -> Vec<u8> {
        let mut level = NbtCompound::new();
        level.insert("xPos", 0i32);
        level.insert("zPos", 0i32);
        level.insert("LastUpdate", 0i64);
        level.insert("TerrainPopulated", 1i8);
        level.insert("Sections", NbtTag::List(NbtList::Empty));
        let mut root = NbtCompound::new();
        root.insert("Level", NbtTag::Compound(level));
        let mut buf = Vec::new();
        BaseNbt::new("", root).write(&mut buf);
        buf
    }

    /// A chunk carrying a root `Status` that is not `minecraft:full`, and no
    /// root `sections`. Such proto-chunks must be skipped, never treated as
    /// pre-1.18 — otherwise one of them would drag a whole modern region file
    /// into raw storage and silently lose deduplication.
    fn proto_chunk() -> Vec<u8> {
        let mut root = NbtCompound::new();
        root.insert("Status", "minecraft:structure_starts");
        root.insert("xPos", 0i32);
        root.insert("zPos", 0i32);
        let mut buf = Vec::new();
        BaseNbt::new("", root).write(&mut buf);
        buf
    }

    fn region_bytes(chunks: Vec<(i32, i32, Vec<u8>)>) -> Vec<u8> {
        let mut buf = Vec::new();
        write_region(0, 0, &[0u8; 4096], chunks, Cursor::new(&mut buf))
            .expect("write test region");
        buf
    }

    fn save_with_region(bytes: &[u8]) -> (TempDir, LocalFsOdb) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());
        odb.put(REGION_KEY, bytes).expect("write region file");
        (dir, odb)
    }

    #[test]
    fn pre_1_18_region_is_stored_raw_and_restored_byte_for_byte() {
        let bytes = region_bytes(vec![(0, 0, legacy_chunk())]);
        let (_save_dir, save) = save_with_region(&bytes);
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let mut storage = LocalFsOdb::from_dir(storage_dir.path().to_path_buf());

        let processed = ChunkRegionHandler
            .flatten(&save, &mut storage)
            .expect("flatten must not fail on a pre-1.18 world");
        assert_eq!(processed, vec![REGION_KEY.to_string()]);

        // The whole file is kept verbatim under the raw sentry.
        let stored = storage
            .get(&format!("{REGION_KEY}/raw"))
            .expect("region must be stored raw");
        assert_eq!(stored, bytes);

        // ...and restoring reproduces it exactly.
        let restore_dir = tempfile::tempdir().expect("tempdir");
        let mut restored = LocalFsOdb::from_dir(restore_dir.path().to_path_buf());
        ChunkRegionHandler
            .unflatten(&mut restored, &storage)
            .expect("unflatten raw region");
        assert_eq!(restored.get(REGION_KEY).expect("restored region"), bytes);
    }

    #[test]
    fn unfinished_chunk_does_not_force_a_modern_region_to_raw() {
        let bytes = region_bytes(vec![(0, 0, proto_chunk())]);
        let (_save_dir, save) = save_with_region(&bytes);
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let mut storage = LocalFsOdb::from_dir(storage_dir.path().to_path_buf());

        ChunkRegionHandler
            .flatten(&save, &mut storage)
            .expect("flatten a region whose only chunk is unfinished");

        assert!(
            storage.get(&format!("{REGION_KEY}/raw")).is_err(),
            "an unfinished chunk must be skipped, not send the region to raw storage"
        );
        assert!(
            storage.get(&format!("{REGION_KEY}/timestamps")).is_ok(),
            "the region should still be flattened"
        );
    }
}
