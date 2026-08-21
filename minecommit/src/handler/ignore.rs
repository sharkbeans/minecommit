use anyhow::Result;

use super::Handler;
use crate::odb::{OdbReader, OdbWriter};

const BUILTIN_IGNORE_PATTERNS: &[&str] = &["**/.DS_Store"];

pub(crate) struct IgnoreHandler {
    pub(crate) ignore_patterns: Vec<String>,
}

impl Handler for IgnoreHandler {
    fn workspace(&self) -> &'static str {
        "ignore"
    }

    fn flatten(self, save: &impl OdbReader, _storage: &mut impl OdbWriter) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        let builtin = BUILTIN_IGNORE_PATTERNS.iter().copied();
        let extra = self.ignore_patterns.iter().map(|s| s.as_str());
        for pattern in builtin.chain(extra) {
            for key in save.glob(pattern)? {
                log::info!("Ignore file {key}");
                processed.push(key);
            }
        }
        Ok(processed)
    }

    fn unflatten(
        self,
        _save: &mut impl OdbWriter,
        storage: &impl OdbReader,
    ) -> Result<Vec<String>> {
        let mut processed = Vec::new();
        let builtin = BUILTIN_IGNORE_PATTERNS.iter().copied();
        let extra = self.ignore_patterns.iter().map(|s| s.as_str());
        for pattern in builtin.chain(extra) {
            for key in storage.glob(pattern)? {
                log::info!("Ignore file {key}");
                processed.push(key);
            }
        }
        Ok(processed)
    }
}
