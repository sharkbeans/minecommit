//! Safe synchronization for MineCommit's bare Git repositories.
//!
//! This module deliberately treats a Minecraft save history differently from
//! normal source code: it only permits fast-forwards.  It never creates merge
//! commits and never force-pushes.

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[cfg(not(unix))]
use std::fs::TryLockError;

use crate::{utils::cmd::git_cmd, Config};

pub const DEFAULT_BRANCH: &str = "main";
const DEFAULT_REMOTE: &str = "origin";
const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// The relationship between a local MineCommit branch and its fetched remote
/// tracking branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// No MineCommit remote has been configured.
    NotConfigured,
    /// Neither the local nor remote branch has a commit yet.
    Empty,
    /// Both refs name the same commit.
    UpToDate,
    /// The local branch has commits that have not reached the remote.
    LocalAhead,
    /// The fetched remote branch can fast-forward the local branch.
    RemoteAhead,
    /// Both sides have changed independently. Automatic sync must stop.
    Diverged,
}

impl SyncState {
    pub fn description(self) -> &'static str {
        match self {
            Self::NotConfigured => "No cloud remote is configured.",
            Self::Empty => "No backups exist locally or remotely yet.",
            Self::UpToDate => "Local and cloud backups are up to date.",
            Self::LocalAhead => "Local backups are waiting to upload.",
            Self::RemoteAhead => "A newer cloud backup is available.",
            Self::Diverged => {
                "Both local and cloud backups changed. MineCommit will not merge Minecraft worlds."
            }
        }
    }
}

/// A snapshot of the configured remote and the two branch tips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteStatus {
    pub remote_name: Option<String>,
    pub remote_url: Option<String>,
    pub branch: String,
    pub local_commit: Option<String>,
    pub local_timestamp: Option<String>,
    pub local_device: Option<String>,
    pub remote_commit: Option<String>,
    pub remote_timestamp: Option<String>,
    pub remote_device: Option<String>,
    pub state: SyncState,
}

/// Return a friendly, cross-platform name for the device that created a backup.
/// The name is committed with each new backup, so it is available on every
/// computer after synchronization without requiring a separate cloud service.
pub fn current_device_name() -> String {
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "This device".to_string());
    let name = name.trim().replace(['\r', '\n'], " ");
    if name.is_empty() {
        "This device".to_string()
    } else {
        name
    }
}

/// Attach versioned device metadata to a user-facing backup message.
pub fn backup_message_with_device(message: &str) -> String {
    format!("{message}\n\nMineCommit-Device: {}", current_device_name())
}

/// Result of a pre-play sync. `restored` is true only when a newer remote
/// commit was safely restored into the local Minecraft save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeforePlayResult {
    pub status: RemoteStatus,
    pub restored: bool,
    pub backup_path: Option<PathBuf>,
}

/// A MineCommit bare repository and the branch being synchronized.
#[derive(Debug, Clone)]
pub struct RemoteSync {
    git_dir: PathBuf,
    branch: String,
}

#[derive(Debug)]
struct GitOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl RemoteSync {
    /// Open a bare MineCommit repository and validate the requested branch.
    pub fn new(git_dir: impl Into<PathBuf>, branch: impl Into<String>) -> Result<Self> {
        let git_dir = git_dir.into();
        let branch = branch.into();
        anyhow::ensure!(!branch.trim().is_empty(), "branch name cannot be empty");

        let bare = run_git(&git_dir, ["rev-parse", "--is-bare-repository"])?;
        anyhow::ensure!(
            bare.success && bare.stdout.trim() == "true",
            "{git_dir:?} is not a bare Git repository"
        );
        ensure_git_success(
            run_git(&git_dir, ["check-ref-format", "--branch", &branch])?,
            "invalid branch name",
        )?;

        Ok(Self { git_dir, branch })
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// Associate this bare repository with a provider-agnostic Git remote.
    ///
    /// MineCommit uses the conventional `origin` remote and delegates normal
    /// authentication to Git; this core module never handles credentials.
    pub fn configure_remote(&self, url: &str) -> Result<()> {
        anyhow::ensure!(!url.trim().is_empty(), "remote URL cannot be empty");
        ensure_no_embedded_https_credentials(url)?;

        let existing = run_git(&self.git_dir, ["config", "--get", "remote.origin.url"])?;
        if existing.success {
            ensure_git_success(
                run_git(&self.git_dir, ["remote", "set-url", DEFAULT_REMOTE, url])?,
                "failed to update cloud remote",
            )?;
        } else {
            ensure_git_success(
                run_git(&self.git_dir, ["remote", "add", DEFAULT_REMOTE, url])?,
                "failed to add cloud remote",
            )?;
        }

        // `git remote add` normally installs a force-prefixed fetch refspec.
        // MineCommit avoids that too: a rewritten remote branch is reported
        // instead of silently replacing a tracking ref.
        ensure_git_success(
            run_git(
                &self.git_dir,
                [
                    "config",
                    "--replace-all",
                    "remote.origin.fetch",
                    "refs/heads/*:refs/remotes/origin/*",
                ],
            )?,
            "failed to configure safe remote fetches",
        )?;
        Ok(())
    }

    /// Fetch the selected branch into `refs/remotes/origin/<branch>`.
    ///
    /// This does not touch the local branch. An empty remote is valid during
    /// first upload and is represented by a missing remote commit.
    pub fn fetch(&self) -> Result<()> {
        self.require_remote()?;
        let refspec = format!(
            "refs/heads/{}:refs/remotes/{}/{}",
            self.branch, DEFAULT_REMOTE, self.branch
        );
        log::info!("Fetching cloud branch '{}'", self.branch);
        let output = run_git(&self.git_dir, ["fetch", "--no-tags", DEFAULT_REMOTE, &refspec])?;
        if output.success {
            return Ok(());
        }
        if output.stderr.contains("couldn't find remote ref") {
            log::info!("Cloud branch '{}' has no commits yet", self.branch);
            return Ok(());
        }
        ensure_git_success(output, "failed to fetch cloud backups")
    }

    /// Return the status using the most recently fetched remote-tracking ref.
    /// Call [`Self::fetch`] first when current cloud status is required.
    pub fn status(&self) -> Result<RemoteStatus> {
        let remote_url = self.remote_url()?;
        if remote_url.is_none() {
            return Ok(RemoteStatus {
                remote_name: None,
                remote_url: None,
                branch: self.branch.clone(),
                local_commit: None,
                local_timestamp: None,
                local_device: None,
                remote_commit: None,
                remote_timestamp: None,
                remote_device: None,
                state: SyncState::NotConfigured,
            });
        }

        let local_ref = format!("refs/heads/{}", self.branch);
        let remote_ref = format!("refs/remotes/{}/{}", DEFAULT_REMOTE, self.branch);
        let local_commit = self.resolve_commit(&local_ref)?;
        let remote_commit = self.resolve_commit(&remote_ref)?;
        let (local_timestamp, local_device) = self.commit_metadata(local_commit.as_deref())?;
        let (remote_timestamp, remote_device) = self.commit_metadata(remote_commit.as_deref())?;

        let state = match (&local_commit, &remote_commit) {
            (None, None) => SyncState::Empty,
            (Some(_), None) => SyncState::LocalAhead,
            (None, Some(_)) => SyncState::RemoteAhead,
            (Some(local), Some(remote)) if local == remote => SyncState::UpToDate,
            (Some(local), Some(remote)) if self.is_ancestor(local, remote)? => SyncState::RemoteAhead,
            (Some(remote), Some(local)) if self.is_ancestor(remote, local)? => SyncState::LocalAhead,
            (Some(_), Some(_)) => SyncState::Diverged,
        };

        Ok(RemoteStatus {
            remote_name: Some(DEFAULT_REMOTE.to_string()),
            remote_url,
            branch: self.branch.clone(),
            local_commit,
            local_timestamp,
            local_device,
            remote_commit,
            remote_timestamp,
            remote_device,
            state,
        })
    }

    /// Move the local branch to a fetched descendant with a compare-and-swap
    /// update. This is the only local branch update performed by sync.
    pub fn fast_forward(&self) -> Result<RemoteStatus> {
        let status = self.status()?;
        anyhow::ensure!(
            status.state == SyncState::RemoteAhead,
            "cannot fast-forward: {}",
            status.state.description()
        );

        let local_ref = format!("refs/heads/{}", self.branch);
        let remote_commit = status
            .remote_commit
            .as_deref()
            .context("remote branch unexpectedly has no commit")?;
        let expected_local = status.local_commit.as_deref().unwrap_or(ZERO_OID);
        ensure_git_success(
            run_git(
                &self.git_dir,
                ["update-ref", &local_ref, remote_commit, expected_local],
            )?,
            "could not fast-forward local backup branch; it changed while syncing",
        )?;
        self.status()
    }

    /// Fetch, fast-forward when safe, and restore a newer remote save through
    /// the existing MineCommit checkout engine.
    pub fn sync_before_playing(&self, save_dir: impl AsRef<Path>) -> Result<BeforePlayResult> {
        self.fetch()?;
        let status = self.status()?;
        match status.state {
            SyncState::RemoteAhead => {
                let commit = status
                    .remote_commit
                    .clone()
                    .context("remote branch unexpectedly has no commit")?;
                let prior_status = status;
                let status = self.fast_forward()?;
                let restore = match restore_commit(save_dir, &self.git_dir, &commit) {
                    Ok(result) => result,
                    Err(error) => {
                        if let Err(rollback_error) = self.undo_fast_forward(&prior_status, &status) {
                            return Err(error).context(format!(
                                "restore failed after fast-forward, and MineCommit could not roll back the local branch: {rollback_error:#}"
                            ));
                        }
                        return Err(error).context(
                            "restore failed; MineCommit rolled the local branch back and left the existing world unchanged",
                        );
                    }
                };
                Ok(BeforePlayResult {
                    status,
                    restored: true,
                    backup_path: restore.backup_path,
                })
            }
            SyncState::Diverged => anyhow::bail!(
                "cloud conflict detected: both local and remote Minecraft backups changed. \
                 MineCommit will not merge them; both histories have been preserved."
            ),
            SyncState::NotConfigured => anyhow::bail!("no cloud remote is configured"),
            _ => Ok(BeforePlayResult {
                status,
                restored: false,
                backup_path: None,
            }),
        }
    }

    /// Fetch immediately before a normal, non-force Git push. A push is only
    /// attempted when the local branch is strictly ahead of the fetched
    /// remote. Git still protects against a remote race at push time.
    pub fn push(&self) -> Result<RemoteStatus> {
        self.fetch()?;
        let status = self.status()?;
        match status.state {
            SyncState::LocalAhead => {
                let source = format!("refs/heads/{}:refs/heads/{}", self.branch, self.branch);
                log::info!("Pushing cloud branch '{}'", self.branch);
                ensure_git_success(
                    run_git(&self.git_dir, ["push", DEFAULT_REMOTE, &source])?,
                    "failed to push cloud backup without force",
                )?;
                self.fetch()?;
                self.status()
            }
            SyncState::UpToDate => Ok(status),
            SyncState::Diverged => anyhow::bail!(
                "cloud conflict detected: both local and remote Minecraft backups changed. \
                 MineCommit will not merge them; both histories have been preserved."
            ),
            SyncState::RemoteAhead => anyhow::bail!(
                "cloud has newer backups. Sync and restore them before creating or pushing another backup."
            ),
            SyncState::Empty => anyhow::bail!("there is no local backup to push"),
            SyncState::NotConfigured => anyhow::bail!("no cloud remote is configured"),
        }
    }

    fn require_remote(&self) -> Result<String> {
        self.remote_url()?.context("no cloud remote is configured")
    }

    fn remote_url(&self) -> Result<Option<String>> {
        let output = run_git(&self.git_dir, ["config", "--get", "remote.origin.url"])?;
        if output.success {
            Ok(Some(output.stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    fn resolve_commit(&self, reference: &str) -> Result<Option<String>> {
        let expression = format!("{reference}^{{commit}}");
        let output = run_git(&self.git_dir, ["rev-parse", "--verify", "--quiet", &expression])?;
        if output.success {
            Ok(Some(output.stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let output = run_git(
            &self.git_dir,
            ["merge-base", "--is-ancestor", ancestor, descendant],
        )?;
        if output.success {
            return Ok(true);
        }
        if output.stderr.is_empty() {
            return Ok(false);
        }
        anyhow::bail!("failed to compare cloud backup history: {}", output.stderr.trim())
    }

    fn commit_metadata(&self, commit: Option<&str>) -> Result<(Option<String>, Option<String>)> {
        let Some(commit) = commit else {
            return Ok((None, None));
        };
        let output = run_git(&self.git_dir, ["show", "-s", "--format=%cI%n%B", commit])?;
        ensure_git_success(output, "failed to read cloud backup metadata")?;

        let mut lines = output.stdout.lines();
        let timestamp = lines
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let device = lines.find_map(|line| {
            line
                .trim()
                .strip_prefix("MineCommit-Device:")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        Ok((timestamp, device))
    }

    fn undo_fast_forward(&self, prior: &RemoteStatus, current: &RemoteStatus) -> Result<()> {
        let local_ref = format!("refs/heads/{}", self.branch);
        let remote_commit = current
            .local_commit
            .as_deref()
            .context("local branch unexpectedly has no commit to roll back")?;
        if let Some(previous_commit) = prior.local_commit.as_deref() {
            ensure_git_success(
                run_git(
                    &self.git_dir,
                    ["update-ref", &local_ref, previous_commit, remote_commit],
                )?,
                "could not roll back local backup branch",
            )
        } else {
            ensure_git_success(
                run_git(&self.git_dir, ["update-ref", "-d", &local_ref, remote_commit])?,
                "could not roll back newly created local backup branch",
            )
        }
    }
}

fn ensure_no_embedded_https_credentials(url: &str) -> Result<()> {
    let is_http = url
        .split_once("://")
        .is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
        });
    let authority = url
        .split_once("://")
        .map(|(_, remainder)| remainder.split('/').next().unwrap_or_default());
    if is_http && authority.is_some_and(|authority| authority.contains('@')) {
        anyhow::bail!(
            "remote URL contains embedded HTTPS credentials; use Git Credential Manager or an SSH key instead"
        );
    }
    Ok(())
}

fn run_git<const N: usize>(git_dir: &Path, args: [&str; N]) -> Result<GitOutput> {
    let output = git_cmd(git_dir, args)
        .output()
        .context("failed to start Git; install Git and ensure it is on PATH")?;
    Ok(GitOutput {
        success: output.status.success(),
        stdout: String::from_utf8(output.stdout).context("Git emitted non-UTF-8 stdout")?,
        stderr: String::from_utf8(output.stderr).context("Git emitted non-UTF-8 stderr")?,
    })
}

fn ensure_git_success(output: GitOutput, operation: &str) -> Result<()> {
    if output.success {
        return Ok(());
    }
    let detail = output.stderr.trim();
    if detail.is_empty() {
        anyhow::bail!("{operation}");
    }
    anyhow::bail!("{operation}: {detail}")
}

/// Holds MineCommit's exclusive lock on a world's `session.lock` for the
/// lifetime of a backup or restore operation.
pub struct WorldSessionLock {
    _file: Option<File>,
}

/// Confirm that an existing directory is a Minecraft Java world, and obtain
/// its `session.lock` when present. A held lock means Minecraft may still be
/// writing the world, so the caller must stop rather than risk corruption.
pub fn lock_inactive_world(save_dir: impl AsRef<Path>) -> Result<WorldSessionLock> {
    let save_dir = save_dir.as_ref();
    validate_minecraft_save(save_dir)?;
    let lock_path = save_dir.join("session.lock");
    if !lock_path.exists() {
        return Ok(WorldSessionLock { _file: None });
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open Minecraft session lock {lock_path:?}"))?;
    try_lock_minecraft_session(&file, &lock_path, save_dir)?;
    Ok(WorldSessionLock { _file: Some(file) })
}

#[cfg(unix)]
fn try_lock_minecraft_session(file: &File, lock_path: &Path, save_dir: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;

    // Java's FileChannel locks (including Minecraft's session.lock) use the
    // POSIX fcntl lock family on Unix. std::fs::File::try_lock currently uses
    // flock there, so using fcntl directly is necessary to detect the lock
    // Minecraft itself holds.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &lock) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    if matches!(error.kind(), std::io::ErrorKind::WouldBlock) {
        anyhow::bail!(
            "Minecraft appears to be using {save_dir:?}; close the world before backing up or restoring it"
        );
    }
    Err(error).with_context(|| format!("failed to verify Minecraft session lock {lock_path:?}"))
}

#[cfg(not(unix))]
fn try_lock_minecraft_session(file: &File, lock_path: &Path, save_dir: &Path) -> Result<()> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => anyhow::bail!(
            "Minecraft appears to be using {save_dir:?}; close the world before backing up or restoring it"
        ),
        Err(TryLockError::Error(error)) => Err(error)
            .with_context(|| format!("failed to verify Minecraft session lock {lock_path:?}")),
    }
}

/// Validate the minimal on-disk identity of an ordinary Minecraft Java save.
pub fn validate_minecraft_save(save_dir: impl AsRef<Path>) -> Result<()> {
    let save_dir = save_dir.as_ref();
    anyhow::ensure!(save_dir.is_dir(), "Minecraft save directory does not exist: {save_dir:?}");
    anyhow::ensure!(
        save_dir.join("level.dat").is_file(),
        "{save_dir:?} does not look like a Minecraft Java save (level.dat is missing)"
    );
    Ok(())
}

/// Information about a completed staged restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreResult {
    pub backup_path: Option<PathBuf>,
}

/// Restore a commit using MineCommit's existing checkout engine without ever
/// writing directly over a live world. The commit is first reconstructed into
/// a sibling staging directory; only after a successful reconstruction is the
/// old world renamed to a timestamped `.snapshot` backup.
pub fn restore_commit(
    save_dir: impl AsRef<Path>,
    git_dir: impl AsRef<Path>,
    commit: impl AsRef<str>,
) -> Result<RestoreResult> {
    let save_dir = save_dir.as_ref();
    let git_dir = git_dir.as_ref();
    let existing_world = save_dir.exists();
    let _world_lock = if existing_world {
        Some(lock_inactive_world(save_dir)?)
    } else {
        anyhow::ensure!(
            save_dir.file_name().is_some(),
            "invalid destination save path: {save_dir:?}"
        );
        let parent = save_dir
            .parent()
            .context("destination save path has no parent directory")?;
        anyhow::ensure!(
            parent.is_dir(),
            "destination save parent directory does not exist: {parent:?}"
        );
        None
    };

    let staging_dir = create_staging_dir(save_dir)?;
    let mut staging = StagingDir {
        path: staging_dir.clone(),
        keep: false,
    };

    Config::new(staging_dir.clone(), git_dir.to_path_buf(), vec![], vec![])
        .checkout(commit.as_ref().to_string())
        .context("failed to reconstruct Minecraft save in staging directory")?;
    validate_minecraft_save(&staging_dir)
        .context("restored commit did not produce a valid Minecraft Java save")?;

    let backup_path = if existing_world {
        let backup = next_snapshot_path(save_dir)?;
        fs::rename(save_dir, &backup).with_context(|| {
            format!("failed to preserve existing Minecraft save at {save_dir:?} as {backup:?}")
        })?;
        Some(backup)
    } else {
        None
    };

    if let Err(error) = fs::rename(&staging_dir, save_dir) {
        if let Some(backup) = &backup_path {
            let _ = fs::rename(backup, save_dir);
        }
        return Err(error).context("failed to activate reconstructed Minecraft save");
    }
    staging.keep = true;
    Ok(RestoreResult { backup_path })
}

struct StagingDir {
    path: PathBuf,
    keep: bool,
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.keep && self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn create_staging_dir(save_dir: &Path) -> Result<PathBuf> {
    let parent = save_dir
        .parent()
        .context("destination save path has no parent directory")?;
    let stem = save_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("world");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..100 {
        let candidate = parent.join(format!(".{stem}.minecommit-restore-{}-{nonce}-{attempt}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create restore staging directory {candidate:?}"));
            }
        }
    }
    anyhow::bail!("could not create a unique restore staging directory near {save_dir:?}")
}

fn next_snapshot_path(save_dir: &Path) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = save_dir.with_extension(format!("{stamp}.snapshot"));
    if !base.exists() {
        return Ok(base);
    }
    for attempt in 1..100 {
        let candidate = save_dir.with_extension(format!("{stamp}.{attempt}.snapshot"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    anyhow::bail!("could not create a unique snapshot path for {save_dir:?}")
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("--git-dir")
            .arg(dir)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git stdout utf-8")
    }

    fn init_bare() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = Command::new("git")
            .args(["init", "--bare", dir.path().to_str().expect("utf8 path")])
            .output()
            .expect("git init");
        assert!(output.status.success());
        git(dir.path(), &["config", "user.name", "MineCommit Test"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        dir
    }

    fn commit(repo: &Path, parent: Option<&str>, message: &str) -> String {
        let tree = git(repo, &["mktree"]);
        let mut command = Command::new("git");
        command
            .arg("--git-dir")
            .arg(repo)
            .args(["commit-tree", tree.trim()]);
        if let Some(parent) = parent {
            command.args(["-p", parent]);
        }
        let output = command.args(["-m", message]).output().expect("commit-tree");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("commit hash utf8")
            .trim()
            .to_string()
    }

    fn set_branch(repo: &Path, commit: &str) {
        git(repo, &["update-ref", "refs/heads/main", commit]);
    }

    fn configured(repo: &Path, remote: &Path) -> RemoteSync {
        let sync = RemoteSync::new(repo, DEFAULT_BRANCH).expect("open sync repo");
        sync.configure_remote(remote.to_str().expect("utf8 path"))
            .expect("configure remote");
        sync
    }

    #[test]
    fn status_distinguishes_fast_forward_and_divergence() {
        let remote = init_bare();
        let device_a = init_bare();
        let device_b = init_bare();

        let initial = commit(device_a.path(), None, "initial");
        set_branch(device_a.path(), &initial);
        let sync_a = configured(device_a.path(), remote.path());
        assert_eq!(sync_a.status().unwrap().state, SyncState::LocalAhead);
        assert_eq!(sync_a.push().unwrap().state, SyncState::UpToDate);

        let sync_b = configured(device_b.path(), remote.path());
        sync_b.fetch().unwrap();
        assert_eq!(sync_b.status().unwrap().state, SyncState::RemoteAhead);
        assert_eq!(sync_b.fast_forward().unwrap().state, SyncState::UpToDate);

        let local_only = commit(device_b.path(), Some(&initial), "local only");
        set_branch(device_b.path(), &local_only);
        assert_eq!(sync_b.status().unwrap().state, SyncState::LocalAhead);

        let remote_only = commit(device_a.path(), Some(&initial), "remote only");
        set_branch(device_a.path(), &remote_only);
        sync_a.push().unwrap();
        sync_b.fetch().unwrap();
        let status = sync_b.status().unwrap();
        assert_eq!(status.state, SyncState::Diverged);
        assert_eq!(status.local_commit.as_deref(), Some(local_only.as_str()));
        assert_eq!(status.remote_commit.as_deref(), Some(remote_only.as_str()));
    }

    #[test]
    fn remote_ahead_fast_forwards_without_merge() {
        let remote = init_bare();
        let device_a = init_bare();
        let device_b = init_bare();

        let initial = commit(device_a.path(), None, "initial");
        set_branch(device_a.path(), &initial);
        let sync_a = configured(device_a.path(), remote.path());
        sync_a.push().unwrap();

        let sync_b = configured(device_b.path(), remote.path());
        sync_b.fetch().unwrap();
        sync_b.fast_forward().unwrap();

        let newer = commit(device_a.path(), Some(&initial), "newer");
        set_branch(device_a.path(), &newer);
        sync_a.push().unwrap();

        sync_b.fetch().unwrap();
        assert_eq!(sync_b.status().unwrap().state, SyncState::RemoteAhead);
        let final_status = sync_b.fast_forward().unwrap();
        assert_eq!(final_status.state, SyncState::UpToDate);
        assert_eq!(final_status.local_commit.as_deref(), Some(newer.as_str()));
        let parents = git(device_b.path(), &["show", "-s", "--format=%P", &newer]);
        assert_eq!(parents.trim(), initial);
    }

    #[test]
    fn fetch_requires_a_configured_reachable_remote() {
        let repo = init_bare();
        let sync = RemoteSync::new(repo.path(), DEFAULT_BRANCH).unwrap();
        assert_eq!(sync.status().unwrap().state, SyncState::NotConfigured);
        assert!(sync.fetch().is_err());

        sync.configure_remote("/path/that/does/not/exist.git").unwrap();
        assert!(sync.fetch().is_err());
    }

    #[test]
    fn rejects_https_urls_with_embedded_credentials() {
        let repo = init_bare();
        let sync = RemoteSync::new(repo.path(), DEFAULT_BRANCH).unwrap();
        assert!(sync
            .configure_remote("https://token@example.com/owner/world.git")
            .is_err());
        assert!(sync
            .configure_remote("git@github.com:owner/world.git")
            .is_ok());
    }

    #[test]
    fn status_reports_versioned_backup_time_and_device() {
        let remote = init_bare();
        let device = init_bare();
        let backup = commit(
            device.path(),
            None,
            "initial backup\n\nMineCommit-Device: Bedroom PC",
        );
        set_branch(device.path(), &backup);

        let status = configured(device.path(), remote.path()).status().unwrap();
        assert_eq!(status.local_device.as_deref(), Some("Bedroom PC"));
        assert!(status.local_timestamp.is_some());
        assert_eq!(status.remote_device, None);
        assert_eq!(status.remote_timestamp, None);
    }

    #[test]
    fn restore_validation_requires_level_dat() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("world");
        fs::create_dir(&world).unwrap();
        assert!(validate_minecraft_save(&world).is_err());
        File::create(world.join("level.dat")).unwrap();
        assert!(validate_minecraft_save(&world).is_ok());
    }
}
