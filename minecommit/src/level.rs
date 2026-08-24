//! What a world says about itself.
//!
//! `level.dat` is a gzipped NBT file at the root of every Minecraft Java world,
//! and it holds the things a player uses to tell one world from another: the
//! version it was last opened in, whether it is hardcore, the seed, the
//! difficulty. MineCommit already reads and rewrites this file during a backup;
//! this reads it for display only, and never writes.
//!
//! Everything here is optional. `level.dat` has changed shape repeatedly across
//! fifteen years of Minecraft, mods add fields and remove them, and a world old
//! enough to predate a field is still a world worth backing up. A missing value
//! is reported as missing rather than guessed at or treated as an error.

use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use simdnbt::owned::NbtCompound;

use crate::utils::nbt::load_nbt;

/// The seed has moved twice. Before 1.16 it sat directly under `Data`; from
/// 1.16 it lived in a `WorldGenSettings` compound inside `level.dat`; and from
/// the 26.x releases it is not in `level.dat` at all, but in its own file.
const LEGACY_SEED_KEY: &str = "RandomSeed";
const SEED_FILE: [&str; 3] = ["data", "minecraft", "world_gen_settings.dat"];

/// The 26.x releases moved difficulty, hardcore and the spawn point out of
/// `Data` and into compounds of their own, and turned the difficulty from a
/// number into a name.
const DIFFICULTY_SETTINGS: &str = "difficulty_settings";
const SPAWN: &str = "spawn";

/// Minecraft's game modes, in the order the older format numbered them.
const GAME_MODES: [&str; 4] = ["survival", "creative", "adventure", "spectator"];

/// Minecraft's difficulties, in the order the older format numbered them.
const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];

/// Every Minecraft world reports this data pack, whether or not the player has
/// added any of their own.
const BUILT_IN_DATA_PACK: &str = "vanilla";

/// One Minecraft day, in ticks.
pub const TICKS_PER_DAY: i64 = 24_000;

/// What `level.dat` says about a world.
///
/// The numeric game mode and difficulty are passed through as Minecraft stores
/// them rather than turned into words here, because the words belong in
/// whichever language the player is reading.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelInfo {
    /// The name shown in Minecraft's own world list, which is not necessarily
    /// the folder name: renaming a world in game leaves the folder alone.
    pub level_name: Option<String>,
    /// The version the world was last saved by, as Minecraft writes it: "1.21.4".
    pub version_name: Option<String>,
    /// The data version, which orders releases including snapshots. Useful when
    /// `version_name` is missing, which it is for worlds older than 1.9.
    pub data_version: Option<i32>,
    /// Whether that version was a snapshot rather than a release.
    pub snapshot_version: bool,
    /// "survival", "creative", "adventure" or "spectator".
    ///
    /// A name rather than the number older worlds store it as, because the
    /// 26.x releases write difficulty as a name and game mode may follow. A
    /// value this build does not recognise is passed through as it was found
    /// rather than dropped.
    pub game_mode: Option<String>,
    pub hardcore: bool,
    /// "peaceful", "easy", "normal" or "hard".
    pub difficulty: Option<String>,
    pub difficulty_locked: bool,
    /// Whether commands are allowed, which Minecraft calls cheats.
    pub cheats: bool,
    /// When Minecraft last saved the world, in milliseconds since the epoch.
    /// More trustworthy than the file's own timestamp, which copying resets.
    pub last_played: Option<i64>,
    /// The age of the world in ticks: twenty a second, twenty-four thousand a
    /// Minecraft day.
    pub world_age_ticks: Option<i64>,
    /// Carried as text over the wire. A Minecraft seed uses the whole 64-bit
    /// range and JSON has only doubles, so a seed sent as a number comes out
    /// the other side rounded -- and a rounded seed generates a different
    /// world, which makes it worse than showing none at all.
    #[serde(with = "seed_text")]
    pub seed: Option<i64>,
    pub spawn: Option<[i32; 3]>,
    /// Enabled data packs, without the built-in one every world reports.
    pub data_packs: Vec<String>,
    /// Whether Minecraft has ever recorded this world as modded.
    pub modded: bool,
}

impl LevelInfo {
    /// How many Minecraft days the world has been alive for.
    pub fn in_game_days(&self) -> Option<i64> {
        self.world_age_ticks.map(|ticks| ticks / TICKS_PER_DAY)
    }
}

/// Read the `level.dat` at the root of a world folder.
///
/// `save_dir` is the world folder itself, not the file.
pub fn read_world(save_dir: impl AsRef<Path>) -> Result<LevelInfo> {
    let save_dir = save_dir.as_ref();
    let level = save_dir.join("level.dat");
    let raw = std::fs::read(&level).with_context(|| format!("failed to read {level:?}"))?;
    let mut info = read_level_bytes(&raw).with_context(|| format!("failed to read {level:?}"))?;

    // From the 26.x releases the seed lives in its own file. Older worlds keep
    // it in level.dat, so only go looking when it was not already found.
    if info.seed.is_none() {
        info.seed = read_seed_file(save_dir);
    }
    Ok(info)
}

/// The seed, from the file the 26.x releases moved it into.
fn read_seed_file(save_dir: &Path) -> Option<i64> {
    let path = SEED_FILE.iter().fold(save_dir.to_path_buf(), |path, part| path.join(part));
    let nbt = parse_nbt(&std::fs::read(path).ok()?).ok()?;
    nbt.compound("data")?.long("seed")
}

/// Read possibly-gzipped NBT.
///
/// Minecraft always gzips these files, but a copy that has been through a tool
/// which unpacked it still describes the same world.
fn parse_nbt(raw: &[u8]) -> Result<simdnbt::owned::BaseNbt> {
    let mut decoder = GzDecoder::new(raw);
    let decompressed = if decoder.header().is_some() {
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .context("failed to decompress")?;
        decompressed
    } else {
        raw.to_vec()
    };
    load_nbt(Cursor::new(decompressed.as_slice()))
}

/// Read a value Minecraft may store either as a name or as an index into
/// `names`, and answer with the name either way.
///
/// The 26.x releases turned difficulty from a number into a string, and the
/// same may yet happen to game mode. A name this build has never heard of is
/// returned as it was found: showing the player "hard" is right even if a later
/// release adds a fifth difficulty, and showing nothing would be wrong.
fn named(compound: &NbtCompound, key: &str, names: &[&str; 4]) -> Option<String> {
    if let Some(name) = compound.string(key) {
        let name = name.to_string();
        // Minecraft namespaces some of these and not others.
        let name = name.rsplit(':').next().unwrap_or(&name).to_ascii_lowercase();
        return (!name.is_empty()).then_some(name);
    }
    let index = compound
        .int(key)
        .or_else(|| compound.byte(key).map(i32::from))?;
    names
        .get(usize::try_from(index).ok()?)
        .map(|name| (*name).to_string())
}

/// Read the contents of a `level.dat`, compressed or not.
///
/// The seed is only found here for worlds older than the 26.x releases; see
/// [`read_world`], which also looks in the file those moved it to.
pub fn read_level_bytes(raw: &[u8]) -> Result<LevelInfo> {
    let nbt = parse_nbt(raw).context("level.dat is not NBT data")?;
    let data = nbt
        .compound("Data")
        .context("level.dat has no Data section")?;

    let version = data.compound("Version");
    let generation = data.compound("WorldGenSettings");
    // Present from the 26.x releases; before that these three sat loose in
    // `Data` under different names.
    let settings = data.compound(DIFFICULTY_SETTINGS);

    Ok(LevelInfo {
        level_name: data.string("LevelName").map(|name| name.to_string()),
        version_name: version
            .and_then(|version| version.string("Name"))
            .map(|name| name.to_string()),
        data_version: version
            .and_then(|version| version.int("Id"))
            .or_else(|| data.int("DataVersion")),
        snapshot_version: version.and_then(|version| version.byte("Snapshot")) == Some(1),
        game_mode: named(&data, "GameType", &GAME_MODES),
        hardcore: settings.map_or_else(
            || data.byte("hardcore") == Some(1),
            |settings| settings.byte("hardcore") == Some(1),
        ),
        difficulty: settings
            .and_then(|settings| named(settings, "difficulty", &DIFFICULTIES))
            .or_else(|| named(&data, "Difficulty", &DIFFICULTIES)),
        difficulty_locked: settings.map_or_else(
            || data.byte("DifficultyLocked") == Some(1),
            |settings| settings.byte("locked") == Some(1),
        ),
        cheats: data.byte("allowCommands") == Some(1),
        last_played: data.long("LastPlayed"),
        world_age_ticks: data.long("Time"),
        seed: generation
            .and_then(|settings| settings.long("seed"))
            .or_else(|| data.long(LEGACY_SEED_KEY)),
        spawn: data
            .compound(SPAWN)
            .and_then(|spawn| spawn.int_array("pos"))
            .and_then(|pos| <[i32; 3]>::try_from(pos).ok())
            .or_else(
                || match (data.int("SpawnX"), data.int("SpawnY"), data.int("SpawnZ")) {
                    (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                    _ => None,
                },
            ),
        data_packs: data
            .compound("DataPacks")
            .and_then(|packs| packs.list("Enabled"))
            .and_then(|enabled| enabled.strings())
            .map(|names| {
                names
                    .iter()
                    .map(|name| name.to_string())
                    .filter(|name| name != BUILT_IN_DATA_PACK)
                    .collect()
            })
            .unwrap_or_default(),
        // `WasModded` is what Minecraft itself records; the `fml` section is
        // what Forge leaves behind, and predates it.
        modded: data.byte("WasModded") == Some(1) || nbt.contains("fml") || data.contains("fml"),
    })
}

/// A 64-bit seed, carried as text so JSON cannot round it.
mod seed_text {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(seed) => serializer.serialize_some(&seed.to_string()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<i64>, D::Error> {
        Ok(Option::<String>::deserialize(deserializer)?.and_then(|text| text.parse().ok()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{write::GzEncoder, Compression};
    use simdnbt::owned::{BaseNbt, NbtCompound, NbtList, NbtTag};

    use super::*;

    fn gzip(nbt: &BaseNbt) -> Vec<u8> {
        let mut plain = Vec::new();
        nbt.write(&mut plain);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&plain).expect("compress");
        encoder.finish().expect("finish")
    }

    /// A world as the 26.x releases write it: difficulty, hardcore and the
    /// spawn point moved into compounds of their own, the difficulty became a
    /// name rather than a number, and the seed left `level.dat` entirely.
    fn current() -> BaseNbt {
        let mut version = NbtCompound::new();
        version.insert("Name", NbtTag::String("26.2".into()));
        version.insert("Id", NbtTag::Int(4903));
        version.insert("Snapshot", NbtTag::Byte(0));

        let mut settings = NbtCompound::new();
        settings.insert("difficulty", NbtTag::String("hard".into()));
        settings.insert("hardcore", NbtTag::Byte(1));
        settings.insert("locked", NbtTag::Byte(1));

        let mut spawn = NbtCompound::new();
        spawn.insert("dimension", NbtTag::String("minecraft:overworld".into()));
        spawn.insert("pos", NbtTag::IntArray(vec![112, 64, -48]));

        let mut data = NbtCompound::new();
        data.insert("LevelName", NbtTag::String("reincarnated".into()));
        data.insert("Version", NbtTag::Compound(version));
        data.insert(DIFFICULTY_SETTINGS, NbtTag::Compound(settings));
        data.insert(SPAWN, NbtTag::Compound(spawn));
        data.insert("GameType", NbtTag::Int(0));
        data.insert("Time", NbtTag::Long(TICKS_PER_DAY * 760));
        data.insert("WasModded", NbtTag::Byte(1));

        let mut root = NbtCompound::new();
        root.insert("Data", NbtTag::Compound(data));
        BaseNbt::new("", root)
    }

    /// A world as 1.21 wrote it.
    fn modern() -> BaseNbt {
        let mut version = NbtCompound::new();
        version.insert("Name", NbtTag::String("1.21.4".into()));
        version.insert("Id", NbtTag::Int(4189));
        version.insert("Snapshot", NbtTag::Byte(0));

        let mut generation = NbtCompound::new();
        generation.insert("seed", NbtTag::Long(-4_172_144_997_902_289_642));

        let mut packs = NbtCompound::new();
        packs.insert(
            "Enabled",
            NbtTag::List(NbtList::String(vec!["vanilla".into(), "my_pack".into()])),
        );

        let mut data = NbtCompound::new();
        data.insert("LevelName", NbtTag::String("Sharkbeans SMP".into()));
        data.insert("Version", NbtTag::Compound(version));
        data.insert("WorldGenSettings", NbtTag::Compound(generation));
        data.insert("DataPacks", NbtTag::Compound(packs));
        data.insert("GameType", NbtTag::Int(0));
        data.insert("hardcore", NbtTag::Byte(1));
        data.insert("Difficulty", NbtTag::Byte(3));
        data.insert("DifficultyLocked", NbtTag::Byte(1));
        data.insert("allowCommands", NbtTag::Byte(0));
        data.insert("LastPlayed", NbtTag::Long(1_756_000_000_000));
        data.insert("Time", NbtTag::Long(TICKS_PER_DAY * 342 + 500));
        data.insert("SpawnX", NbtTag::Int(112));
        data.insert("SpawnY", NbtTag::Int(64));
        data.insert("SpawnZ", NbtTag::Int(-48));

        let mut root = NbtCompound::new();
        root.insert("Data", NbtTag::Compound(data));
        BaseNbt::new("", root)
    }

    #[test]
    fn a_world_describes_itself_the_way_the_game_does() {
        let info = read_level_bytes(&gzip(&modern())).expect("a real level.dat reads");

        // The in-game name is not the folder name: renaming a world in
        // Minecraft leaves the folder alone, and the dashboard shows folders.
        assert_eq!(info.level_name.as_deref(), Some("Sharkbeans SMP"));
        assert_eq!(info.version_name.as_deref(), Some("1.21.4"));
        assert_eq!(info.data_version, Some(4189));
        assert!(!info.snapshot_version);
        assert_eq!(info.game_mode.as_deref(), Some("survival"));
        assert!(info.hardcore, "a hardcore world must never be mistaken for one that is not");
        assert_eq!(info.difficulty.as_deref(), Some("hard"));
        assert!(info.difficulty_locked);
        assert!(!info.cheats);
        assert_eq!(info.last_played, Some(1_756_000_000_000));
        assert_eq!(info.seed, Some(-4_172_144_997_902_289_642));
        assert_eq!(info.spawn, Some([112, 64, -48]));
        assert_eq!(info.in_game_days(), Some(342));
        assert_eq!(
            info.data_packs,
            vec!["my_pack".to_string()],
            "every world reports the built-in pack, so listing it says nothing"
        );
        assert!(!info.modded);
    }

    /// The 26.x releases rearranged `level.dat`: difficulty became a name in a
    /// compound of its own, hardcore moved in beside it, and the spawn point
    /// became a position array. Reading the old places against a new world
    /// reported every one of these as absent -- and reported a hardcore world
    /// as not hardcore, which is the one thing here that must never be wrong,
    /// since a death in hardcore is exactly when somebody reaches for a backup.
    #[test]
    fn the_rearranged_level_dat_of_the_current_releases_reads_the_same() {
        let info = read_level_bytes(&gzip(&current())).expect("a current level.dat reads");

        assert_eq!(info.level_name.as_deref(), Some("reincarnated"));
        assert_eq!(info.version_name.as_deref(), Some("26.2"));
        assert_eq!(info.game_mode.as_deref(), Some("survival"));
        assert!(info.hardcore);
        assert_eq!(info.difficulty.as_deref(), Some("hard"));
        assert!(info.difficulty_locked);
        assert_eq!(info.spawn, Some([112, 64, -48]));
        assert_eq!(info.in_game_days(), Some(760));
        assert!(info.modded);
        assert_eq!(
            info.seed, None,
            "the seed left level.dat in these releases; read_world finds it elsewhere"
        );
    }

    /// A name this build has never seen is still the truth about the world.
    #[test]
    fn an_unfamiliar_difficulty_is_passed_through_rather_than_dropped() {
        let mut settings = NbtCompound::new();
        settings.insert("difficulty", NbtTag::String("minecraft:nightmare".into()));
        let mut data = NbtCompound::new();
        data.insert(DIFFICULTY_SETTINGS, NbtTag::Compound(settings));
        let mut root = NbtCompound::new();
        root.insert("Data", NbtTag::Compound(data));

        let info = read_level_bytes(&gzip(&BaseNbt::new("", root))).expect("reads");
        assert_eq!(info.difficulty.as_deref(), Some("nightmare"));
    }

    /// The seed moved out of `level.dat` in the 26.x releases, into a file of
    /// its own two directories down. Every world of that era would otherwise
    /// show no seed at all.
    #[test]
    fn a_seed_kept_outside_level_dat_is_still_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        let world = dir.path().join("reincarnated");
        std::fs::create_dir_all(world.join("data").join("minecraft")).expect("world");
        std::fs::write(world.join("level.dat"), gzip(&current())).expect("level.dat");

        assert_eq!(
            read_world(&world).expect("reads").seed,
            None,
            "with no settings file there is no seed to report"
        );

        let mut settings = NbtCompound::new();
        settings.insert("seed", NbtTag::Long(-352_129_843_062_846_360));
        let mut root = NbtCompound::new();
        root.insert("data", NbtTag::Compound(settings));
        std::fs::write(
            world.join("data").join("minecraft").join("world_gen_settings.dat"),
            gzip(&BaseNbt::new("", root)),
        )
        .expect("settings");

        let info = read_world(&world).expect("reads");
        assert_eq!(info.seed, Some(-352_129_843_062_846_360));
        // And the rest of the world still reads exactly as it did.
        assert_eq!(info.version_name.as_deref(), Some("26.2"));
        assert!(info.hardcore);
    }

    /// Worlds predating 1.16 keep the seed directly under `Data`, and worlds
    /// predating 1.9 carry no version block at all. Both are still worlds
    /// somebody wants backed up.
    #[test]
    fn an_old_world_gives_up_what_it_has_and_no_more() {
        let mut data = NbtCompound::new();
        data.insert("LevelName", NbtTag::String("2013".into()));
        data.insert("RandomSeed", NbtTag::Long(42));
        let mut root = NbtCompound::new();
        root.insert("Data", NbtTag::Compound(data));

        let info = read_level_bytes(&gzip(&BaseNbt::new("", root))).expect("an old level.dat reads");
        assert_eq!(info.level_name.as_deref(), Some("2013"));
        assert_eq!(info.seed, Some(42), "the seed moved in 1.16; older worlds still have one");
        assert_eq!(info.version_name, None, "unknown is reported, never invented");
        assert_eq!(info.difficulty, None);
        assert_eq!(info.in_game_days(), None);
        assert!(!info.hardcore);
    }

    /// A seed uses the whole 64-bit range, and the largest ones do not survive
    /// a trip through a JSON number: `-4172144997902289642` comes back as
    /// `-4172144997902289400`, which generates an entirely different world.
    #[test]
    fn a_seed_too_large_for_a_json_number_survives_the_trip_to_the_window() {
        let info = read_level_bytes(&gzip(&modern())).expect("reads");
        let seed = info.seed.expect("a seed");

        let json = serde_json::to_string(&info).expect("serialise");
        assert!(
            json.contains(&format!("\"{seed}\"")),
            "the seed must travel as text, not as a number: {json}"
        );
        assert!(
            (seed as f64) as i64 != seed,
            "this seed is only a test of anything if a double really does lose it"
        );

        let back: LevelInfo = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.seed, Some(seed));
        assert_eq!(back, info);
    }

    #[test]
    fn a_forge_world_is_recognised_as_modded() {
        let mut data = NbtCompound::new();
        data.insert("WasModded", NbtTag::Byte(1));
        let mut root = NbtCompound::new();
        root.insert("Data", NbtTag::Compound(data));
        assert!(read_level_bytes(&gzip(&BaseNbt::new("", root)))
            .expect("reads")
            .modded);
    }

    /// Anything that is not a world must fail rather than be shown as a blank
    /// one, and it must fail without panicking: the dashboard reads whatever
    /// folder the player points it at.
    #[test]
    fn a_file_that_is_not_a_level_dat_is_refused() {
        assert!(read_level_bytes(b"not nbt at all").is_err());
        assert!(read_level_bytes(&[]).is_err());

        // Valid NBT, but not a world.
        let mut root = NbtCompound::new();
        root.insert("something", NbtTag::Int(1));
        assert!(read_level_bytes(&gzip(&BaseNbt::new("", root))).is_err());
    }

    /// Minecraft gzips this file, but a copy that has been through a tool that
    /// unpacked it still describes the same world.
    #[test]
    fn an_uncompressed_copy_reads_the_same() {
        let mut plain = Vec::new();
        modern().write(&mut plain);
        assert_eq!(
            read_level_bytes(&plain).expect("plain NBT reads"),
            read_level_bytes(&gzip(&modern())).expect("gzipped NBT reads")
        );
    }
}
