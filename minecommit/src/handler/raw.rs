use anyhow::Result;

use super::Handler;
use crate::odb::{OdbReader, OdbWriter};

const RAW_GLOB_PATTERNS: &[&str] = &[
    "**/*.png",
    "**/*.json",
    "**/*.txt",
    "**/*.snbt",
    "**/*.toml",
    // Some launchers and archive tools retain Minecraft's old NBT snapshots
    // with an additional gzip suffix. Preserve them byte-for-byte rather than
    // treating them as unhandled files.
    "**/*.dat_old*.gz",
    "session.lock",
];

pub(crate) struct RawHandler {
    pub(crate) extra_patterns: Vec<String>,
}

impl Handler for RawHandler {
    fn workspace(&self) -> &'static str {
        "raw"
    }

    fn flatten(self, save: &impl OdbReader, storage: &mut impl OdbWriter) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        let builtin = RAW_GLOB_PATTERNS.iter().copied();
        let extra = self.extra_patterns.iter().map(|s| s.as_str());
        for pattern in builtin.chain(extra) {
            for key in save.glob(pattern)? {
                log::info!("Process raw file {key}");
                let data = save.get(&key)?;
                storage.put(&key, &data)?;
                processed.push(key);
            }
        }
        Ok(processed)
    }

    fn unflatten(self, save: &mut impl OdbWriter, storage: &impl OdbReader) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        let builtin = RAW_GLOB_PATTERNS.iter().copied();
        let extra = self.extra_patterns.iter().map(|s| s.as_str());
        for pattern in builtin.chain(extra) {
            for key in storage.glob(pattern)? {
                log::info!("Process raw file {key}");
                let data = storage.get(&key)?;
                save.put(&key, &data)?;
                processed.push(key);
            }
        }
        Ok(processed)
    }
}
