use anyhow::{Context, Result};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use simdnbt::owned::{BaseNbt, NbtCompound, NbtList};
use std::io::{Cursor, Read, Write};

use super::Handler;
use crate::{
    odb::{OdbReader, OdbWriter},
    utils::nbt::{dump_nbt, load_nbt, sort_nbt},
};

const GZIP_NBT_GLOB_PATTERNS: &[&str] = &["**/*.dat", "**/*.dat_old", "**/*.nbt"];

/// Files that carried one of those extensions but turned out not to be NBT.
const RAW_UNFLATTEN_PATTERNS: &[&str] =
    &["**/*.dat/raw", "**/*.dat_old/raw", "**/*.nbt/raw"];

pub(crate) struct GzipNbtHandler {}

impl Handler for GzipNbtHandler {
    fn workspace(&self) -> &'static str {
        "gzip-nbt"
    }

    fn flatten(self, save: &impl OdbReader, storage: &mut impl OdbWriter) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        for pattern in GZIP_NBT_GLOB_PATTERNS {
            for key in save.glob(pattern)? {
                log::info!("Process gzip nbt file {key}");
                let original = save.get(&key)?;
                let mut decoder = GzDecoder::new(original.as_slice());
                let decompressed = if decoder.header().is_some() {
                    let mut decompressed = Vec::new();
                    decoder
                        .read_to_end(&mut decompressed)
                        .context("failed to decompress gzip data")?;
                    decompressed
                } else {
                    log::info!(
                        "Failed to decompress because header is invalid, treat as uncompressed"
                    );
                    original.clone()
                };

                // These patterns claim every `.dat` in a world, and mods put
                // files there that are not NBT at all: Lunar Client's minimap
                // tiles, for one. Refusing to store them fails the whole
                // backup over a file the player does not even know about, so
                // keep them byte for byte instead.
                let Ok(parsed) = load_nbt(Cursor::new(&decompressed)) else {
                    log::info!("{key} is not NBT data, storing it byte for byte");
                    storage.put(&format!("{key}/raw"), &original)?;
                    processed.push(key);
                    continue;
                };

                let sorted = {
                    let nbt = sort_nbt(parsed);

                    // Sort recipe book for player data
                    let nbt = {
                        let match_file_old = glob::Pattern::new("playerdata/*.dat")
                            .context("failed to compile glob pattern")?
                            .matches(&key);
                        let match_file_new = glob::Pattern::new("players/data/*.dat")
                            .context("failed to compile glob pattern")?
                            .matches(&key);
                        if match_file_old || match_file_new {
                            let name = nbt.name().to_owned();
                            let mut comp = nbt.as_compound();
                            sort_recipe_book(&mut comp);
                            BaseNbt::new(name, comp)
                        } else {
                            nbt
                        }
                    };

                    // Sort recipe book for player data in level.dat
                    let nbt = {
                        if key == "level.dat" || key == "level.dat_old" {
                            let name = nbt.name().to_owned();
                            let mut comp = nbt.as_compound();
                            if let Some(data) = comp.compound_mut("Data")
                                && let Some(player) = data.compound_mut("Player")
                            {
                                sort_recipe_book(player);
                            }
                            BaseNbt::new(name, comp)
                        } else {
                            nbt
                        }
                    };

                    // Sort player attributes in level.dat
                    let nbt = {
                        if key == "level.dat" || key == "level.dat_old" {
                            let name = nbt.name().to_owned();
                            let mut comp = nbt.as_compound();
                            if let Some(data) = comp.compound_mut("Data")
                                && let Some(player) = data.compound_mut("Player")
                            {
                                sort_player_attributes(player);
                            }
                            BaseNbt::new(name, comp)
                        } else {
                            nbt
                        }
                    };

                    dump_nbt(nbt, decompressed.len())?
                };
                storage.put(&key, &sorted)?;

                processed.push(key);
            }
        }
        Ok(processed)
    }

    fn unflatten(self, save: &mut impl OdbWriter, storage: &impl OdbReader) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        for pattern in RAW_UNFLATTEN_PATTERNS {
            for raw_key in storage.glob(pattern)? {
                let Some(file_key) = raw_key.strip_suffix("/raw") else {
                    continue;
                };
                log::info!("Process non-NBT file {file_key}");
                let data = storage
                    .get(&raw_key)
                    .with_context(|| format!("failed to read {raw_key}"))?;
                // Stored uncompressed and unsorted, so it goes back untouched.
                save.put(file_key, &data)?;
                processed.push(raw_key);
            }
        }
        for pattern in GZIP_NBT_GLOB_PATTERNS {
            for key in storage.glob(pattern)? {
                log::info!("Process gzip nbt file {key}");
                let data = storage.get(&key)?;
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder
                    .write_all(&data)
                    .context("failed to write data to gzip encoder")?;
                let compressed = encoder
                    .finish()
                    .context("failed to finish gzip compression")?;
                save.put(&key, &compressed)?;

                processed.push(key);
            }
        }
        Ok(processed)
    }
}

fn sort_recipe_book(comp: &mut NbtCompound) {
    if let Some(recipe_book) = comp.compound_mut("recipeBook") {
        for key in ["recipes", "toBeDisplayed"] {
            if let Some(NbtList::String(strings)) = recipe_book.list_mut(key) {
                strings.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            }
        }
    }
}

fn sort_player_attributes(comp: &mut NbtCompound) {
    if let Some(NbtList::Compound(attributes)) = comp.list_mut("attributes") {
        attributes.sort_unstable_by(|a, b| {
            a.string("id")
                .map(|s| s.as_bytes())
                .cmp(&b.string("id").map(|s| s.as_bytes()))
        });
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::owned::NbtTag;

    use super::*;
    use crate::odb::LocalFsOdb;

    /// A Lunar Client minimap tile: a `.dat` in the world that is not NBT and
    /// never was. Mods drop files like this into saves all the time.
    const MINIMAP_KEY: &str = "minimap/-1/-1.dat";

    fn odb_with(key: &str, bytes: &[u8]) -> (tempfile::TempDir, LocalFsOdb) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());
        odb.put(key, &bytes.to_vec()).expect("write file");
        (dir, odb)
    }

    #[test]
    fn a_dat_file_that_is_not_nbt_is_stored_raw_and_restored_byte_for_byte() {
        // Not gzip, not NBT: previously this ended the entire backup with
        // "nbt data is empty" over a file the player never chose to have.
        let bytes: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        let (_save_dir, save) = odb_with(MINIMAP_KEY, &bytes);
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let mut storage = LocalFsOdb::from_dir(storage_dir.path().to_path_buf());

        let processed = (GzipNbtHandler {})
            .flatten(&save, &mut storage)
            .expect("a world containing a non-NBT .dat must still back up");
        assert_eq!(processed, vec![MINIMAP_KEY.to_string()]);

        assert_eq!(
            storage
                .get(&format!("{MINIMAP_KEY}/raw"))
                .expect("the file is kept verbatim"),
            bytes
        );

        let restore_dir = tempfile::tempdir().expect("tempdir");
        let mut restored = LocalFsOdb::from_dir(restore_dir.path().to_path_buf());
        (GzipNbtHandler {})
            .unflatten(&mut restored, &storage)
            .expect("unflatten");
        assert_eq!(restored.get(MINIMAP_KEY).expect("restored"), bytes);
    }

    #[test]
    fn a_real_nbt_file_still_takes_the_sorting_path() {
        // The fallback must not swallow genuine NBT: level.dat is canonicalised
        // so that two computers writing the same world produce the same bytes.
        let mut compound = NbtCompound::new();
        compound.insert("b", NbtTag::Int(2));
        compound.insert("a", NbtTag::Int(1));
        let nbt = BaseNbt::new("", compound);
        let mut plain = Vec::new();
        nbt.write(&mut plain);

        let (_save_dir, save) = odb_with("level.dat", &plain);
        let storage_dir = tempfile::tempdir().expect("tempdir");
        let mut storage = LocalFsOdb::from_dir(storage_dir.path().to_path_buf());

        (GzipNbtHandler {})
            .flatten(&save, &mut storage)
            .expect("flatten");

        assert!(
            storage.get("level.dat").is_ok(),
            "genuine NBT belongs on the sorted path"
        );
        assert!(
            storage.get("level.dat/raw").is_err(),
            "genuine NBT must not be diverted to raw storage"
        );
    }
}
