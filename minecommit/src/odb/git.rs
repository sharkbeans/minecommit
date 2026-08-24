use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::odb::{OdbReader, OdbWriter};
use crate::utils::cmd::{exec, git_cmd};

pub struct LocalGitOdb {
    repo: gix::ThreadSafeRepository,
    /// Accumulated blobs not yet committed: path → sha1.
    pending: HashMap<String, String>,
    /// Blob path → oid and size in bytes, populated once per commit.
    path_to_oid: HashMap<String, (gix::ObjectId, u64)>,
}

impl LocalGitOdb {
    /// How much work restoring this commit represents: how many stored files,
    /// and how many bytes between them.
    ///
    /// This is the denominator of the restore progress bar, and it is free:
    /// the sizes come from the same `ls-tree` that located the blobs.
    pub fn weight(&self) -> (u64, u64) {
        (
            self.path_to_oid.len() as u64,
            self.path_to_oid.values().map(|(_, size)| size).sum(),
        )
    }

    pub fn new(git_dir: PathBuf) -> Result<Self> {
        Ok(Self {
            repo: gix::open(git_dir.to_owned())
                .with_context(|| format!("try 'git init --bare {}'", git_dir.to_string_lossy()))?
                .into(),
            pending: HashMap::new(),
            path_to_oid: HashMap::new(),
        })
    }

    pub fn from_commit(git_dir: PathBuf, commit: String) -> Result<Self> {
        let repo: gix::ThreadSafeRepository = gix::open(&git_dir)
            .context("failed to open git repository")?
            .into();
        let path_to_oid = if commit.is_empty() {
            HashMap::new()
        } else {
            build_path_to_oid(&git_dir, &commit)?
        };
        Ok(Self {
            repo,
            pending: HashMap::new(),
            path_to_oid,
        })
    }

    /// Create a commit from all pending blobs, consuming self.
    ///
    /// `parents` is a list of 0 or more commit-ish strings. The first becomes
    /// the `from` parent and the rest are additional `merge` parents.  Each is
    /// resolved with the `^0` suffix so that refs and tags are dereferenced to
    /// their underlying commit objects.
    ///
    /// Returns the sha1 of the new commit.
    ///
    /// When `author_name` and `author_email` are both `Some`, the
    /// `GIT_AUTHOR_*` and `GIT_COMMITTER_*` environment variables are set
    /// on the `git commit-tree` process.
    pub fn commit(
        self,
        parents: &[impl AsRef<str>],
        message: &str,
        author_name: Option<&str>,
        author_email: Option<&str>,
    ) -> Result<String> {
        log::info!("Building Git tree objects");
        let tree_sha = build_tree(self.repo.git_dir(), &self.pending, "")?;

        let mut cmd = git_cmd(self.repo.git_dir(), [] as [&str; 0]);
        cmd.arg("commit-tree").arg(&tree_sha);
        for parent in parents {
            cmd.arg("-p").arg(&format!("{}^0", parent.as_ref()));
        }
        cmd.arg("-m").arg(message);
        if let (Some(name), Some(email)) = (author_name, author_email) {
            cmd.env("GIT_AUTHOR_NAME", name)
                .env("GIT_AUTHOR_EMAIL", email)
                .env("GIT_COMMITTER_NAME", name)
                .env("GIT_COMMITTER_EMAIL", email);
        }

        let commit = exec(cmd, None)
            .context("failed to run commit-tree")?
            .trim()
            .to_string();
        Ok(commit)
    }
}

/// Recursively build tree objects for `entries` rooted at `prefix`.
/// Returns the sha1 of the root tree.
fn build_tree(
    git_dir: &std::path::Path,
    entries: &HashMap<String, String>,
    prefix: &str,
) -> Result<String> {
    let mut blobs: Vec<(String, String)> = Vec::new();
    let mut dirs: std::collections::BTreeMap<String, HashMap<String, String>> =
        std::collections::BTreeMap::new();

    for (path, sha1) in entries {
        let rel = if prefix.is_empty() {
            path.as_str()
        } else {
            path.strip_prefix(&format!("{prefix}/")).unwrap_or(path)
        };
        if let Some((dir, _rest)) = rel.split_once('/') {
            dirs.entry(dir.to_string())
                .or_default()
                .insert(path.clone(), sha1.clone());
        } else {
            blobs.push((rel.to_string(), sha1.clone()));
        }
    }

    let mut dir_shas: Vec<(String, String)> = dirs
        .into_par_iter()
        .map(|(name, sub_entries)| {
            let sub_prefix = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let sub_sha = build_tree(git_dir, &sub_entries, &sub_prefix)?;
            Ok((name, sub_sha))
        })
        .collect::<Result<_>>()?;
    dir_shas.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut mktree_input = String::new();
    for (name, sub_sha) in &dir_shas {
        mktree_input.push_str(&format!("040000 tree {sub_sha}\t{name}\n"));
    }
    for (name, sha1) in &blobs {
        mktree_input.push_str(&format!("100644 blob {sha1}\t{name}\n"));
    }

    let cmd = git_cmd(git_dir, ["mktree"]);
    Ok(exec(cmd, Some(mktree_input))
        .context("failed to run mktree")?
        .trim()
        .to_string())
}

/// Build a path → (oid, size) map for a commit using `git ls-tree -r -l`.
///
/// `-z` is what makes this safe on the paths a modded world can contain:
/// without it Git quotes and backslash-escapes any path holding a space, a
/// quote or a non-ASCII character, and the escaped name would be restored as a
/// file the world never had. `-l` adds the blob size, which the progress bar
/// uses as its denominator and which costs nothing to ask for here.
fn build_path_to_oid(
    git_dir: &PathBuf,
    commit_sha: &str,
) -> Result<HashMap<String, (gix::ObjectId, u64)>> {
    let cmd = git_cmd(git_dir, ["ls-tree", "-r", "-l", "-z", "--", commit_sha]);
    Ok(exec(cmd, None)
        .context("failed to run ls-tree")?
        .split('\0')
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            // "<mode> SP <type> SP <oid> SP <size> TAB <path>", where the size
            // is right-aligned in a fixed-width field and so may carry leading
            // spaces. A tree entry has "-" for its size, but `-r` reports only
            // blobs.
            let (meta, path) = record.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let _mode = fields.next()?;
            let _kind = fields.next()?;
            let oid: gix::ObjectId = fields.next()?.parse().ok()?;
            let size = fields.next()?.parse().unwrap_or(0);
            Some((path.to_string(), (oid, size)))
        })
        .collect())
}

impl OdbReader for LocalGitOdb {
    fn get(&self, key: &str) -> Result<Vec<u8>> {
        let (oid, _) = self
            .path_to_oid
            .get(key)
            .with_context(|| format!("key not found: {key}"))?;
        let data = self
            .repo
            .to_thread_local()
            .find_blob(*oid)
            .with_context(|| format!("failed to find blob for key: {key}"))?
            .data
            .to_vec();
        // A restore reaches every stored file through here, so this is the one
        // place that sees the whole job. Nothing reads the repository while a
        // backup is being written, so this only ever counts a restore.
        crate::progress::advance(1, data.len() as u64);
        Ok(data)
    }

    fn get_par(&self, keys: &[&str]) -> Result<Vec<Vec<u8>>> {
        let repo = self.repo.clone();
        let path_to_oid = &self.path_to_oid;
        keys.into_par_iter()
            .map(|key| {
                let (oid, _) = path_to_oid
                    .get(*key)
                    .with_context(|| format!("key not found: {key}"))?;
                let data = repo
                    .to_thread_local()
                    .find_blob(*oid)
                    .with_context(|| format!("failed to find blob for key: {key}"))?
                    .data
                    .to_vec();
                crate::progress::advance(1, data.len() as u64);
                Ok(data)
            })
            .collect()
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>> {
        let pat = glob::Pattern::new(pattern).context("failed to compile glob pattern")?;
        Ok(self
            .path_to_oid
            .par_iter()
            .map(|(p, _)| p)
            .filter(|p| pat.matches(p.as_str()))
            .cloned()
            .collect())
    }
}

impl OdbWriter for LocalGitOdb {
    fn put(&mut self, key: &str, value: impl AsRef<[u8]>) -> Result<()> {
        let sha1 = self
            .repo
            .to_thread_local()
            .write_blob(value)
            .with_context(|| format!("failed to write blob for key: {key}"))?
            .to_hex()
            .to_string();
        self.pending.insert(key.to_string(), sha1);
        Ok(())
    }

    fn put_par(
        &mut self,
        entries: impl IntoParallelIterator<Item = (String, impl AsRef<[u8]>)>,
    ) -> Result<()> {
        let ts_repo = self.repo.clone();
        let results: Vec<(String, String)> = entries
            .into_par_iter()
            .map(|(key, value)| {
                let repo = ts_repo.to_thread_local();
                let sha1 = repo
                    .write_blob(value.as_ref())
                    .with_context(|| format!("failed to write blob for key: {key}"))?
                    .to_hex()
                    .to_string();
                Ok((key, sha1))
            })
            .collect::<Result<_>>()?;
        for (key, sha1) in results {
            self.pending.insert(key, sha1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// Initialise a bare git repo in a tempdir and return its path.
    fn init_bare_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        Command::new("git")
            .args([
                "init",
                "--bare",
                dir.path()
                    .to_str()
                    .expect("temp dir path is not valid utf-8"),
            ])
            .output()
            .expect("failed to run git init");
        // git commit-tree needs author/committer config
        Command::new("git")
            .args([
                "--git-dir",
                dir.path()
                    .to_str()
                    .expect("temp dir path is not valid utf-8"),
            ])
            .args(["config", "user.email", "test@test"])
            .output()
            .expect("failed to run git config user.email");
        Command::new("git")
            .args([
                "--git-dir",
                dir.path()
                    .to_str()
                    .expect("temp dir path is not valid utf-8"),
            ])
            .args(["config", "user.name", "Test"])
            .output()
            .expect("failed to run git config user.name");
        dir
    }

    #[test]
    fn git_put_commit_get_roundtrip() {
        let repo = init_bare_repo();
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), String::new()).unwrap();

        let data = b"hello git odb".to_vec();
        odb.put("src/hello.txt", &data).unwrap();
        let commit_sha = odb.commit(&[] as &[&str], "initial", None, None).unwrap();
        assert_eq!(commit_sha.len(), 40);

        let odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), commit_sha).unwrap();
        let got = odb.get("src/hello.txt").unwrap();
        assert_eq!(got, data);
    }

    /// A world folder is not all ASCII. Data packs, resource packs and mods
    /// put spaces, quotes and accented letters into file names, and Git quotes
    /// and backslash-escapes every one of those in `ls-tree` output unless it
    /// is asked for NUL-terminated records. Reading the escaped form back would
    /// restore a file under a name the world never had -- and leave the real
    /// one missing.
    #[test]
    fn a_file_whose_name_git_would_escape_still_comes_back_under_its_own_name() {
        let repo = init_bare_repo();
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), String::new()).unwrap();

        let awkward = [
            "datapacks/My Data Pack/pack.mcmeta",
            "datapacks/café/data.json",
            "datapacks/quote\"name/x.json",
            "datapacks/back\\slash/x.json",
        ];
        for (index, key) in awkward.iter().enumerate() {
            odb.put(key, format!("contents {index}").into_bytes()).unwrap();
        }
        let commit = odb.commit(&[] as &[&str], "awkward names", None, None).unwrap();

        let odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), commit).unwrap();
        for (index, key) in awkward.iter().enumerate() {
            assert_eq!(
                odb.get(key).unwrap_or_else(|e| panic!("{key} must restore: {e}")),
                format!("contents {index}").into_bytes()
            );
        }
    }

    /// The restore bar divides by this, and a zero denominator draws no bar at
    /// all -- which is what a restore of a large world used to show.
    #[test]
    fn a_stored_commit_knows_how_much_work_restoring_it_is() {
        let repo = init_bare_repo();
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), String::new()).unwrap();
        odb.put("region/r.0.0.mca", vec![7u8; 4096]).unwrap();
        odb.put("level.dat", vec![1u8; 900]).unwrap();
        let commit = odb.commit(&[] as &[&str], "sized", None, None).unwrap();

        let odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), commit).unwrap();
        assert_eq!(odb.weight(), (2, 4096 + 900));
    }

    #[test]
    fn git_glob_after_commit() {
        let repo = init_bare_repo();
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), String::new()).unwrap();

        odb.put("a/x.rs", &b"fn x(){}".to_vec()).unwrap();
        odb.put("a/y.rs", &b"fn y(){}".to_vec()).unwrap();
        odb.put("b/z.md", &b"# Z".to_vec()).unwrap();
        let commit_sha = odb.commit(&[] as &[&str], "add files", None, None).unwrap();

        let odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), commit_sha).unwrap();
        let mut matches = odb.glob("a/*.rs").unwrap();
        matches.sort();
        assert_eq!(matches, vec!["a/x.rs", "a/y.rs"]);
    }

    #[test]
    fn git_commit_with_parent() {
        let repo = init_bare_repo();
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), String::new()).unwrap();

        odb.put("a.txt", &b"v1".to_vec()).unwrap();
        let first = odb.commit(&[] as &[&str], "first", None, None).unwrap();

        // Second commit only puts b.txt — a.txt is NOT inherited
        let mut odb = LocalGitOdb::from_commit(repo.path().to_path_buf(), first.clone()).unwrap();
        odb.put("b.txt", &b"v2".to_vec()).unwrap();
        let second = odb.commit(&[&first], "second", None, None).unwrap();

        // second commit's tree contains only b.txt
        let files: Vec<String> = String::from_utf8(
            Command::new("git")
                .args([
                    "--git-dir",
                    repo.path().to_str().expect("repo path is not valid utf-8"),
                ])
                .args(["ls-tree", "--name-only", &second])
                .output()
                .expect("failed to run git ls-tree")
                .stdout,
        )
        .expect("git ls-tree output is not valid utf-8")
        .lines()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(files, vec!["b.txt"]);

        // parent linkage is recorded
        let parent = String::from_utf8(
            Command::new("git")
                .args([
                    "--git-dir",
                    repo.path().to_str().expect("repo path is not valid utf-8"),
                ])
                .args(["rev-parse", &format!("{second}^1")])
                .output()
                .expect("failed to run git rev-parse")
                .stdout,
        )
        .expect("git rev-parse output is not valid utf-8");
        assert_eq!(parent.trim(), first);
    }
}
