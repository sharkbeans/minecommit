use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::odb::{OdbReader, OdbWriter};

pub struct LocalFsOdb {
    root_dir: PathBuf,
}

impl LocalFsOdb {
    pub fn from_dir(root: PathBuf) -> Self {
        Self { root_dir: root }
    }
}

// Convert a /-separated ODB key into a native PathBuf for filesystem operations.
fn key_to_native(key: &str) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(key.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(key)
    }
}

/// Convert a native path back to a /-separated ODB key.
fn native_to_key(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    #[cfg(windows)]
    {
        Some(s.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Some(s.to_string())
    }
}

/// Validate that an ODB key stays within the root directory.
fn assert_safe_key(key: &str) -> Result<()> {
    if key.contains('\\') {
        anyhow::bail!("ODB key contains backslash: {key:?}");
    }
    for component in std::path::Path::new(key).components() {
        match component {
            Component::ParentDir => anyhow::bail!("ODB key contains '..': {key:?}"),
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("ODB key is absolute: {key:?}")
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

impl OdbReader for LocalFsOdb {
    fn get(&self, key: &str) -> Result<Vec<u8>> {
        assert_safe_key(key)?;
        let bytes = fs::read(self.root_dir.join(key_to_native(key)))
            .context("failed to read file from odb")?;
        // Every handler reaches a save file through here, including the
        // parallel readers, so this is the one place that sees the whole job.
        crate::progress::advance(1);
        Ok(bytes)
    }

    fn get_par(&self, keys: &[&str]) -> Result<Vec<Vec<u8>>> {
        Ok(keys
            .into_par_iter()
            .map(|key| self.get(key))
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>> {
        assert_safe_key(pattern)?;
        let full_pattern = self.root_dir.join(key_to_native(pattern));
        let root = self.root_dir.clone();
        Ok(glob::glob(
            full_pattern
                .to_str()
                .context("glob pattern path is not valid utf-8")?,
        )
        .context("failed to run glob")?
        .filter_map(|e| e.ok())
        .filter(|path| path.is_file())
        .filter_map(|path| path.strip_prefix(&root).ok().and_then(|p| native_to_key(p)))
        .collect())
    }
}

impl OdbWriter for LocalFsOdb {
    fn put(&mut self, key: &str, value: impl AsRef<[u8]>) -> Result<()> {
        assert_safe_key(key)?;
        let path = self.root_dir.join(key_to_native(key));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent directory for key {key:?}"))?;
        }
        fs::write(&path, value)
            .with_context(|| format!("failed to write file to odb at {path:?}"))?;
        crate::progress::advance(1);
        Ok(())
    }

    fn put_par(
        &mut self,
        entries: impl IntoParallelIterator<Item = (String, impl AsRef<[u8]>)>,
    ) -> Result<()> {
        entries.into_par_iter().try_for_each(|(key, value)| {
            assert_safe_key(&key)?;
            let path = self.root_dir.join(key_to_native(&key));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create parent directory for key {key:?}")
                })?;
            }
            fs::write(&path, value)
                .with_context(|| format!("failed to write file to odb at {path:?}"))?;
            crate::progress::advance(1);
            Ok::<(), anyhow::Error>(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_put_get_roundtrip() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());
        let data = b"hello minecommit".to_vec();
        odb.put("foo/bar.bin", &data).unwrap();
        let got = odb.get("foo/bar.bin").unwrap();
        assert_eq!(got, data);
    }

    #[test]
    fn fs_glob_returns_matching_keys() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());
        odb.put("a/x.txt", &b"1".to_vec()).unwrap();
        odb.put("a/y.txt", &b"2".to_vec()).unwrap();
        odb.put("b/z.bin", &b"3".to_vec()).unwrap();
        let mut matches = odb.glob("a/*.txt").unwrap();
        matches.sort();
        assert_eq!(matches, vec!["a/x.txt", "a/y.txt"]);
    }
}
