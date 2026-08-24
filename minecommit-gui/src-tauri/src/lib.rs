use chrono::{DateTime, Local};
use log::{LevelFilter, Log, Metadata, Record};
use minecommit::{
    level::{read_world, LevelInfo},
    sync::{
        backup_message_with_device, current_device_name, lock_inactive_world, restore_commit,
        world_is_busy,
        RemoteStatus, RemoteSync, DEFAULT_BRANCH,
    },
    utils::cmd::{git_cmd, git_command, git_count_objects, git_repack, repack_is_worthwhile},
    Config,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Emitter, Manager};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("A save named \"{0}\" already exists")]
    DuplicateName(String),
    #[error("Save \"{0}\" not found")]
    SaveNotFound(String),
    #[error("Invalid path: {0}")]
    InvalidUTF8(String),
    #[error("Git error: {0}")]
    Git(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeriveSaveInfo {
    pub name: String,
    pub repo_path: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Save {
    pub name: String,
    pub path: String,
    pub repo_path: String,
    pub remote_repo_path: String,
    pub last_access: String,
    pub default_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommitAuthor {
    pub name: String,
    pub email: String,
}

// ─── Logger for capturing commit logs ───────────────────────────────────────

static LOGGER: CaptureLogger = CaptureLogger {
    lines: Mutex::new(Vec::new()),
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    level: String,
    message: String,
}

struct CaptureLogger {
    lines: Mutex<Vec<LogLine>>,
}

impl Log for CaptureLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if let Ok(mut lines) = self.lines.lock() {
            let entry = LogLine {
                level: record.level().to_string(),
                message: record.args().to_string(),
            };
            lines.push(entry);
        }
    }

    fn flush(&self) {}
}

fn init_logger() {
    // Safe to call multiple times; only the first call takes effect.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Info);
}

fn take_logs() -> Vec<LogLine> {
    LOGGER
        .lines
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect()
}

// ─── Tauri commands ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformCommitResult {
    pub success: bool,
    pub logs: Vec<LogLine>,
    pub error: Option<String>,
    pub size_before_mib: Option<f64>,
    pub size_after_mib: Option<f64>,
    pub size_change_pct: Option<f64>,
}

/// The blocking half of a backup: lock the world, flatten it into the bare
/// repository and commit. Shared by [`perform_commit`] and
/// [`backup_and_upload`] so the two cannot drift apart.
#[allow(clippy::too_many_arguments)]
fn commit_blocking(
    save_dir: String,
    git_dir: String,
    branch: String,
    message: String,
    author_name: String,
    author_email: String,
    extra_patterns: Vec<String>,
    ignore_patterns: Vec<String>,
    use_repack: bool,
) -> PerformCommitResult {
    let git_dir_path = PathBuf::from(&git_dir);
    let save_dir_path = PathBuf::from(&save_dir);

    // A held session.lock means Minecraft may still be writing the world.
    // Keep the lock for the whole flatten/commit operation.
    let _world_lock = match lock_inactive_world(&save_dir_path) {
        Ok(lock) => lock,
        Err(e) => {
            let msg = format!("{e:#}");
            log::error!("{msg}");
            return PerformCommitResult {
                success: false,
                logs: vec![],
                error: Some(msg),
                size_before_mib: None,
                size_after_mib: None,
                size_change_pct: None,
            };
        }
    };

    // 1. Resolve parents
    let parents = {
        match git_cmd(&git_dir_path, ["rev-parse", &format!("{branch}^{{commit}}")]).output() {
            Ok(out) if out.status.success() => {
                let hash = String::from_utf8(out.stdout).unwrap_or_default().trim().to_owned();
                log::info!("Branch '{branch}' exists at {hash}, creating child commit");
                vec![hash]
            }
            _ => {
                log::info!("Branch '{branch}' has no commits yet, creating initial commit");
                vec![]
            }
        }
    };
    let r#ref = format!("refs/heads/{}", &branch);

    // 2. Count objects before
    let size_before = match git_count_objects(&git_dir_path) {
        Ok(s) => {
            let v = s.total_size_mib();
            log::info!("Repo size before: {v:.3} MiB");
            v
        }
        Err(e) => {
            log::warn!("Failed to count git objects: {e}");
            f64::NAN
        }
    };

    // 3. Run the commit
    let a_name: Option<&str> = if author_name.is_empty() { None } else { Some(&author_name) };
    let a_email: Option<&str> = if author_email.is_empty() { None } else { Some(&author_email) };
    let unprocessed = match Config::new(
        save_dir_path.clone(),
        git_dir_path.clone(),
        extra_patterns,
        ignore_patterns,
    )
    .commit(
        parents,
        &backup_message_with_device(&message),
        Some(r#ref),
        a_name,
        a_email,
    ) {
        Ok(u) => u,
        Err(e) => {
            let msg = format!("{e:#}");
            log::error!("{msg}");
            return PerformCommitResult {
                success: false,
                logs: vec![],
                error: Some(msg),
                size_before_mib: Some(size_before),
                size_after_mib: None,
                size_change_pct: None,
            };
        }
    };

    // 4. Check for unprocessed files
    if !unprocessed.is_empty() {
        for item in &unprocessed {
            log::error!("Skipped file: {item}");
        }
        let msg = format!(
            "Skipped {} files because they are not caught by any handler. Catch them via -p or ignore them via -i.",
            unprocessed.len()
        );
        log::error!("{msg}");
        return PerformCommitResult {
            success: false,
            logs: vec![],
            error: Some(msg),
            size_before_mib: Some(size_before),
            size_after_mib: None,
            size_change_pct: None,
        };
    }

    // 5. Optional repack
    if use_repack {
        // Repacking a large world costs minutes, so it is only paid for when
        // there is enough loose material to be worth folding in. Skipping it
        // leaves loose objects behind, which Git and the next upload both cope
        // with perfectly well.
        let worthwhile = git_count_objects(&git_dir_path)
            .map(|stats| repack_is_worthwhile(&stats))
            .unwrap_or(true);
        if worthwhile {
            if let Err(e) = git_repack(&git_dir_path) {
                log::warn!("Repack failed: {e}");
            }
        } else {
            log::info!("Not enough new data to be worth repacking; skipping");
        }
    } else {
        log::warn!("--repack is not enabled, Git repository can get bloated");
    }

    // 6. Count objects after
    let size_after = match git_count_objects(&git_dir_path) {
        Ok(s) => {
            let v = s.total_size_mib();
            log::info!("Repo size after: {v:.3} MiB");
            v
        }
        Err(e) => {
            log::warn!("Failed to count git objects: {e}");
            f64::NAN
        }
    };

    let size_change_pct = if size_before.is_finite() && size_before > 0.0 {
        Some((size_after - size_before) / size_before * 100.0)
    } else {
        None
    };

    if let Some(pct) = size_change_pct {
        log::info!(
            "Done. Total size: {size_after:.3} MiB ({pct:+.4}% from {size_before:.3} MiB)"
        );
    } else {
        log::info!("Done. Total size: {size_after:.3} MiB");
    }

    // 7. Persist author info to git global config
    if !author_name.is_empty() {
        let _ = git_command()
            .args(["config", "--global", "user.name", &author_name])
            .output();
    }
    if !author_email.is_empty() {
        let _ = git_command()
            .args(["config", "--global", "user.email", &author_email])
            .output();
    }

    PerformCommitResult {
        success: true,
        logs: vec![],
        error: None,
        size_before_mib: Some(size_before),
        size_after_mib: Some(size_after),
        size_change_pct,
    }
}

/// How much of a backup or restore has been done, for the progress bar.
///
/// Both counts are sent: bytes drive the bar, because a world is a few hundred
/// large region files next to a few thousand tiny ones and a bar driven by the
/// file count races and stalls. The file count is still worth showing beside
/// it, since "412 of 3,204 files" says something a size does not.
#[derive(Debug, Clone, Copy, Serialize)]
struct Progress {
    /// "reading", "writing", "downloading" or "uploading". Matches `Phase` in
    /// `cloud.ts`, which chooses the wording from it.
    phase: &'static str,
    files_done: u64,
    files_total: u64,
    bytes_done: u64,
    /// Zero when the size of the job cannot be known ahead of time, which is
    /// the case for a network transfer: Git says how much has arrived but
    /// never how much is coming.
    bytes_total: u64,
}

impl From<minecommit::progress::Report> for Progress {
    fn from(report: minecommit::progress::Report) -> Self {
        Self {
            phase: report.phase.as_str(),
            files_done: report.files_done,
            files_total: report.files_total,
            bytes_done: report.bytes_done,
            bytes_total: report.bytes_total,
        }
    }
}

/// Push whatever the running work has produced to the window: log lines, and
/// how far it has got.
///
/// `reported` is the last reading sent, so a job that spends a minute on one
/// large region file does not emit the same number twenty times a second.
fn pump(app: &tauri::AppHandle, reported: &mut Option<minecommit::progress::Report>) {
    for entry in &take_logs() {
        let _ = app.emit("commit-log", entry);
    }
    let now = minecommit::progress::report();
    if *reported != Some(now) {
        *reported = Some(now);
        let _ = app.emit("backup-progress", Progress::from(now));
    }
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn perform_commit(
    app: tauri::AppHandle,
    save_dir: String,
    git_dir: String,
    branch: String,
    message: String,
    author_name: String,
    author_email: String,
    extra_patterns: Vec<String>,
    ignore_patterns: Vec<String>,
    use_repack: bool,
) -> PerformCommitResult {
    init_logger();
    take_logs(); // drain stale logs from previous calls
    // A restore or a download never sets a total, so without this the file
    // count would carry over from whatever ran before it.
    minecommit::progress::end();

    // Spawn a blocking thread to periodically drain and emit captured logs
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        // Drain remaining logs after commit finishes
        pump(&app_clone, &mut reported);
    });

    // Run the heavy commit work on another blocking thread, streaming logs in real time
    let result = tauri::async_runtime::spawn_blocking(move || {
        commit_blocking(
            save_dir,
            git_dir,
            branch,
            message,
            author_name,
            author_email,
            extra_patterns,
            ignore_patterns,
            use_repack,
        )
    })
    .await;

    // Stop the log task and wait for final drain
    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;

    let _ = app.emit("commit-finished", ());

    result.unwrap_or_else(|e| PerformCommitResult {
        success: false,
        logs: vec![],
        error: Some(format!("Join error: {e}")),
        size_before_mib: None,
        size_after_mib: None,
        size_change_pct: None,
    })
}

// ─── Restore / Checkout ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformRestoreResult {
    pub success: bool,
    pub logs: Vec<LogLine>,
    pub error: Option<String>,
}

fn sync_error_result(error: anyhow::Error) -> PerformRestoreResult {
    let message = format!("{error:#}");
    log::error!("{message}");
    PerformRestoreResult {
        success: false,
        logs: vec![],
        error: Some(message),
    }
}

#[tauri::command]
async fn perform_restore(
    app: tauri::AppHandle,
    save_dir: String,
    git_dir: String,
    branch: String,
    // A specific point in the history to go back to. `None` restores whatever
    // the branch currently points at.
    commit: Option<String>,
) -> PerformRestoreResult {
    init_logger();
    take_logs(); // drain stale logs
    minecommit::progress::end();

    // Spawn a blocking thread to periodically drain and emit captured logs
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        pump(&app_clone, &mut reported);
    });

    // Run the restore work on a blocking thread
    let result = tauri::async_runtime::spawn_blocking(move || {
        let save_dir_path = PathBuf::from(&save_dir);
        let git_dir_path = PathBuf::from(&git_dir);

        let commit = match commit.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(chosen) => {
                log::info!("Restoring the world as it was at {chosen}...");
                chosen.to_string()
            }
            None => {
                log::info!("Restoring save from '{branch}' using staged checkout...");
                format!("refs/heads/{branch}")
            }
        };

        match restore_commit(&save_dir_path, &git_dir_path, &commit) {
            Ok(result) => {
                if let Some(backup) = result.backup_path {
                    log::info!("Existing save preserved at {backup:?}");
                }
                log::info!("Restore completed successfully");
                PerformRestoreResult {
                    success: true,
                    logs: vec![],
                    error: None,
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                log::error!("{msg}");
                PerformRestoreResult {
                    success: false,
                    logs: vec![],
                    error: Some(msg),
                }
            }
        }
    })
    .await;

    // Stop the log task and wait for final drain
    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;

    let _ = app.emit("commit-finished", ());

    result.unwrap_or_else(|e| PerformRestoreResult {
        success: false,
        logs: vec![],
        error: Some(format!("Join error: {e}")),
    })
}

// ─── Push / Pull ────────────────────────────────────────────────────────────

#[tauri::command]
async fn perform_push(
    app: tauri::AppHandle,
    git_dir: String,
    remote: String,
    branch: String,
) -> PerformRestoreResult {
    init_logger();
    take_logs();
    minecommit::progress::end();

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        pump(&app_clone, &mut reported);
    });

    let result = tauri::async_runtime::spawn_blocking(move || {
        let sync = match RemoteSync::new(PathBuf::from(&git_dir), &branch) {
            Ok(sync) => sync,
            Err(e) => return sync_error_result(e),
        };
        if !remote.trim().is_empty() {
            if let Err(e) = sync.configure_remote(&remote) {
                return sync_error_result(e);
            }
        }
        log::info!("Checking cloud branch '{branch}' before push...");
        match sync.push() {
            Ok(status) => {
                log::info!("{}", status.state.description());
                log::info!("Cloud push completed safely without force");
                PerformRestoreResult {
                    success: true,
                    logs: vec![],
                    error: None,
                }
            }
            Err(e) => sync_error_result(e),
        }
    })
    .await;

    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;
    let _ = app.emit("commit-finished", ());

    result.unwrap_or_else(|e| PerformRestoreResult {
        success: false,
        logs: vec![],
        error: Some(format!("Join error: {e}")),
    })
}

#[tauri::command]
async fn perform_pull(
    app: tauri::AppHandle,
    save_dir: String,
    git_dir: String,
    remote: String,
    branch: String,
) -> PerformRestoreResult {
    init_logger();
    take_logs();
    minecommit::progress::end();

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        pump(&app_clone, &mut reported);
    });

    let result = tauri::async_runtime::spawn_blocking(move || {
        let sync = match RemoteSync::new(PathBuf::from(&git_dir), &branch) {
            Ok(sync) => sync,
            Err(e) => return sync_error_result(e),
        };
        if !remote.trim().is_empty() {
            if let Err(e) = sync.configure_remote(&remote) {
                return sync_error_result(e);
            }
        }
        log::info!("Checking cloud branch '{branch}' before playing...");
        match sync.sync_before_playing(PathBuf::from(&save_dir)) {
            Ok(result) => {
                if result.restored {
                    log::info!("Newer cloud backup restored safely");
                    if let Some(backup) = result.backup_path {
                        log::info!("Existing save preserved at {backup:?}");
                    }
                }
                log::info!("{}", result.status.state.description());
                PerformRestoreResult {
                    success: true,
                    logs: vec![],
                    error: None,
                }
            }
            Err(e) => sync_error_result(e),
        }
    })
    .await;

    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;
    let _ = app.emit("commit-finished", ());

    result.unwrap_or_else(|e| PerformRestoreResult {
        success: false,
        logs: vec![],
        error: Some(format!("Join error: {e}")),
    })
}

/// Fetch on demand and expose the exact safe-sync relationship to the React
/// frontend. Authentication and network failures are intentionally returned
/// as errors so the UI can show them without guessing from Git state.
#[tauri::command]
fn get_cloud_status(
    git_dir: String,
    branch: String,
    refresh: bool,
) -> Result<RemoteStatus, String> {
    let sync = RemoteSync::new(PathBuf::from(git_dir), branch).map_err(|e| format!("{e:#}"))?;
    if refresh {
        sync.fetch().map_err(|e| format!("{e:#}"))?;
    }
    sync.status().map_err(|e| format!("{e:#}"))
}

/// Make sure a world has somewhere to keep its backups.
///
/// A tracked world whose bare repository does not exist cannot be backed up,
/// checked or connected: everything that touches it fails with "is not a bare
/// Git repository", and nothing in the interface offers to create one. Since
/// creating it is always the right answer, adding a world does it up front and
/// never leaves a half-added world behind.
fn ensure_bare_repo(repo_path: &str, default_branch: &str) -> Result<(), AppError> {
    let existing = git_command()
        .args(["--git-dir", repo_path, "rev-parse", "--is-bare-repository"])
        .output();
    let is_bare = existing
        .map(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true")
        .unwrap_or(false);
    if is_bare {
        return Ok(());
    }

    if let Some(parent) = Path::new(repo_path).parent() {
        fs::create_dir_all(parent)?;
    }
    let branch = match default_branch.trim() {
        "" => DEFAULT_BRANCH,
        chosen => chosen,
    };
    log::info!("Creating the backup repository at {repo_path}");
    let output = git_command()
        .args([
            "init",
            "--bare",
            &format!("--initial-branch={branch}"),
            repo_path,
        ])
        .output()?;
    if !output.status.success() {
        return Err(AppError::Git(format!(
            "Could not create the backup repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Create the backup repository for a world that was added without one.
#[tauri::command]
fn repair_world_repository(state: tauri::State<AppState>, name: String) -> Result<(), AppError> {
    let saves = state.saves.lock().unwrap();
    let save = saves
        .iter()
        .find(|save| save.name == name)
        .ok_or(AppError::SaveNotFound(name))?;
    ensure_bare_repo(&save.repo_path, &save.default_branch)
}

#[tauri::command]
fn check_repo_exists(repo_path: String) -> Result<bool, String> {
    let output = git_command()
        .args(["--git-dir", &repo_path, "rev-parse", "--is-bare-repository"])
        .output()
        .map_err(|e| format!("Failed to check repository existence: {}", e))?;

    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

#[tauri::command]
fn init_bare_repo(repo_path: String, default_branch: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&repo_path).parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to make parent directory: {}", e))?;
    }

    let output = git_command()
        .args([
            "init",
            "--bare",
            &format!("--initial-branch={}", default_branch),
            &repo_path,
        ])
        .output()
        .map_err(|e| format!("Failed to initialize repository: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to initialize repository: {}", stderr))
    }
}

struct AppState {
    saves: Mutex<Vec<Save>>,
    data_dir: PathBuf,
    /// The GitHub token entered this session, held only so a backup repository
    /// can be created without asking for it twice. It is never written to
    /// MineCommit's own files; Git's credential store keeps the durable copy.
    github_token: Mutex<Option<String>>,
    /// A sign-in the player has been shown a code for but not finished.
    device_flow: Mutex<Option<DeviceFlow>>,
}

fn saves_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("saves.json")
}


fn load_saves(data_dir: &PathBuf) -> Result<Vec<Save>, AppError> {
    let path = saves_file_path(data_dir);
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
        Ok(serde_json::from_str(&content).unwrap_or_else(|_| vec![]))
    } else {
        // Ensure the data directory exists
        fs::create_dir_all(data_dir)?;
        Ok(vec![])
    }
}

fn save_saves(data_dir: &PathBuf, saves: &[Save]) -> Result<(), AppError> {
    let path = saves_file_path(data_dir);
    fs::create_dir_all(data_dir)?;
    let content = serde_json::to_string_pretty(saves)?;
    fs::write(&path, content)?;
    Ok(())
}

#[tauri::command]
fn get_git_author() -> CommitAuthor {
    let name = git_command()
        .args(["config", "--global", "user.name"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let email = git_command()
        .args(["config", "--global", "user.email"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    CommitAuthor { name, email }
}

#[tauri::command]
fn list_saves(state: tauri::State<AppState>) -> Vec<Save> {
    state.saves.lock().unwrap().clone()
}


#[tauri::command]
fn set_git_author(name: String, email: String) -> Result<CommitAuthor, AppError> {
    git_command()
        .args(["config", "--global", "user.name", &name])
        .output()
        .map_err(|e| AppError::Io(e))?;
    git_command()
        .args(["config", "--global", "user.email", &email])
        .output()
        .map_err(|e| AppError::Io(e))?;
    Ok(CommitAuthor { name, email })
}

#[tauri::command]
fn add_save(
    state: tauri::State<AppState>,
    name: String,
    path: String,
    repo_path: String,
    remote_repo_path: String,
    default_branch: String,
) -> Result<Save, AppError> {
    let mut saves = state.saves.lock().unwrap();

    // Check for duplicate name
    if saves.iter().any(|s| s.name == name) {
        return Err(AppError::DuplicateName(name));
    }

    ensure_bare_repo(&repo_path, &default_branch)?;

    if !remote_repo_path.trim().is_empty() {
        let sync = RemoteSync::new(PathBuf::from(&repo_path), &default_branch)
            .map_err(|e| AppError::Git(format!("{e:#}")))?;
        sync.configure_remote(&remote_repo_path)
            .map_err(|e| AppError::Git(format!("{e:#}")))?;
    }

    let save = Save {
        name,
        path,
        repo_path,
        remote_repo_path,
        last_access: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        default_branch,
    };
    saves.push(save.clone());
    save_saves(&state.data_dir, &saves)?;
    Ok(save)
}

/// Connect an already-tracked world to its private cloud repository. This
/// keeps the display settings and the bare Git repository configuration in
/// sync, while Git continues to own authentication.
#[tauri::command]
fn configure_save_cloud(
    state: tauri::State<AppState>,
    name: String,
    remote_url: String,
    branch: String,
) -> Result<Save, AppError> {
    let remote_url = remote_url.trim().to_string();
    let branch = branch.trim().to_string();
    if remote_url.is_empty() {
        return Err(AppError::Git("Cloud repository address is required".to_string()));
    }
    if branch.is_empty() {
        return Err(AppError::Git("Cloud branch is required".to_string()));
    }

    let mut saves = state.saves.lock().unwrap();
    let save = saves
        .iter_mut()
        .find(|save| save.name == name)
        .ok_or_else(|| AppError::SaveNotFound(name.clone()))?;

    let sync = RemoteSync::new(PathBuf::from(&save.repo_path), &branch)
        .map_err(|e| AppError::Git(format!("{e:#}")))?;
    sync.configure_remote(&remote_url)
        .map_err(|e| AppError::Git(format!("{e:#}")))?;

    save.remote_repo_path = remote_url;
    save.default_branch = branch;
    let configured = save.clone();
    save_saves(&state.data_dir, &saves)?;
    Ok(configured)
}

#[tauri::command]
fn access_save(state: tauri::State<AppState>, name: String) -> Result<(), AppError> {
    let mut saves = state.saves.lock().unwrap();
    let save = saves
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or(AppError::SaveNotFound(name))?;
    save.last_access = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    save_saves(&state.data_dir, &saves)?;
    Ok(())
}

#[tauri::command]
fn list_branches(repo_path: String) -> Result<Vec<String>, String> {
    let output = git_command()
        .args([
            "--git-dir",
            &repo_path,
            "branch",
            "--format=%(refname:short)",
        ])
        .output()
        .map_err(|e| format!("Failed to list branches: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to list branches: {}", stderr));
    }

    let branches: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(branches)
}

#[tauri::command]
fn get_head_ref(repo_path: String) -> Result<String, String> {
    let head_path = Path::new(&repo_path).join("HEAD");
    let content =
        fs::read_to_string(&head_path).map_err(|e| format!("Failed to read HEAD file: {}", e))?;

    // HEAD file content is like "ref: refs/heads/main\n"
    let trimmed = content.trim();
    const PREFIX: &str = "ref: refs/heads/";
    if let Some(branch) = trimmed.strip_prefix(PREFIX) {
        Ok(branch.to_string())
    } else {
        // Detached HEAD — fall back to "main"
        Ok("main".to_string())
    }
}

#[tauri::command]
fn derive_save_info(path: String) -> Result<DeriveSaveInfo, AppError> {
    let canonical = Path::new(&path).canonicalize()?;
    #[cfg(target_os = "windows")]
    let canonical = {
        // On Windows, std::fs::canonicalize() prepends \\?\ (verbatim prefix).
        // Strip it so the derived repo_path is user-readable.
        let canonical_str = canonical
            .to_str()
            .ok_or_else(|| AppError::InvalidUTF8(format!("non-UTF8 path: {:?}", canonical)))?;
        PathBuf::from(canonical_str.strip_prefix(r"\\?\").unwrap_or(canonical_str))
    };

    if !canonical.is_dir() {
        return Err(AppError::InvalidUTF8(format!(
            "world path is not a directory: {}",
            path
        )));
    }

    if !canonical.join("level.dat").is_file() {
        return Err(AppError::InvalidUTF8(format!(
            "world folder must contain level.dat: {}",
            path
        )));
    }

    let parts: Vec<&str> = canonical
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_str().ok_or_else(|| {
                AppError::InvalidUTF8(format!("non-UTF8 component in path: {:?}", path))
            })),
            _ => None,
        })
        .collect::<Result<Vec<_>, _>>()?;

    let save_name = parts.last().ok_or_else(|| {
        AppError::InvalidUTF8(format!("path has no meaningful segments: {}", path))
    })?;

    let name = match parts.as_slice() {
        [.., launcher, ".minecraft", "versions", version, "saves", save_name] => {
            format!("{} / {version} / {save_name}", launcher.to_uppercase())
        }
        [.., launcher, ".minecraft", "saves", save_name] => {
            format!("{} / {save_name}", launcher.to_uppercase())
        }
        [.., "saves", save_name] => save_name.to_string(),
        _ => save_name.to_string(),
    };
    let repo_path = match parts.as_slice() {
        [.., _, ".minecraft", "versions", _, "saves", save_name]
        | [.., _, ".minecraft", "saves", save_name] => {
            let mut p = canonical.parent().unwrap().parent().unwrap().to_path_buf();
            p.push("minecommit");
            p.push(format!("{save_name}.git"));
            p.to_str().unwrap().to_string()
        }
        _ => {
            let mut p = canonical.parent().unwrap().to_path_buf();
            p.push(format!("{save_name}.git"));
            p.to_str().unwrap().to_string()
        }
    };

    Ok(DeriveSaveInfo { name, repo_path })
}

/// List the branches that actually exist in a cloud repository.
///
/// Cloud setup offers these instead of a free-text field, so a branch cannot be
/// typed slightly wrong and then silently resolve to "no backups yet".
#[tauri::command]
async fn list_remote_branches(remote_url: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        RemoteSync::list_remote_branches(&remote_url).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| format!("Failed to list cloud branches: {e}"))?
}

/// Branches that already hold backups in a world's local repository.
///
/// Lets the UI catch a world whose history lives on one branch while the cloud
/// is being pointed at another, which otherwise looks the same as having no
/// backups at all.
#[tauri::command]
fn list_local_branches(repo_path: String) -> Result<Vec<String>, String> {
    let sync = RemoteSync::new(PathBuf::from(&repo_path), DEFAULT_BRANCH)
        .map_err(|e| format!("{e:#}"))?;
    sync.local_branches().map_err(|e| format!("{e:#}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneFromCloudResult {
    pub success: bool,
    pub error: Option<String>,
    pub save: Option<Save>,
}

/// Create a brand new world on this computer from an existing cloud backup.
///
/// This is the entry point for a second computer that has never held the world:
/// there is nothing on disk to select, so the save is built from the cloud
/// instead. The bare repository is created beside the destination, pointed at
/// the remote, and the newest backup on the chosen branch is restored into a
/// save folder that does not exist yet.
#[tauri::command]
async fn clone_save_from_cloud(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    name: String,
    save_path: String,
    remote_url: String,
    branch: String,
) -> Result<CloneFromCloudResult, String> {
    init_logger();
    take_logs();
    minecommit::progress::end();

    let name = name.trim().to_string();
    let save_path = save_path.trim().to_string();
    let remote_url = remote_url.trim().to_string();
    let branch = branch.trim().to_string();
    if name.is_empty() || save_path.is_empty() || remote_url.is_empty() || branch.is_empty() {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some("World name, location, cloud address and branch are all required".into()),
            save: None,
        });
    }
    // A world keeps the name it was backed up under, on every computer that has
    // it, so there is no second name to fall back on when one is already taken.
    // Both of these mean the same thing to the player: this world is already
    // here. Neither may overwrite what is there to make room.
    if state.saves.lock().unwrap().iter().any(|s| s.name == name) {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!(
                "\"{name}\" is already one of the worlds MineCommit is looking after on this PC."
            )),
            save: None,
        });
    }

    let save_dir = PathBuf::from(&save_path);
    if save_dir.exists() {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!(
                "There is already a world at {save_path}. Move or rename that folder if you \
                 want to download this one beside it -- nothing there will be overwritten."
            )),
            save: None,
        });
    }
    let repo_path = match save_dir.parent() {
        Some(parent) => parent.join(format!("{name}.git")),
        None => {
            return Ok(CloneFromCloudResult {
                success: false,
                error: Some(format!("Invalid destination: {save_path}")),
                save: None,
            });
        }
    };
    // The download makes this directory and removes it again if anything goes
    // wrong, so it must not already be something -- a world named
    // "<name>.git" would otherwise be deleted by the cleanup.
    if repo_path.exists() {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!(
                "{} already exists. Move or rename it first, so nothing there can be lost.",
                repo_path.display()
            )),
            save: None,
        });
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();
    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        pump(&app_clone, &mut reported);
    });

    let repo_for_task = repo_path.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        clone_worker(&repo_for_task, &save_dir, &remote_url, &branch)
    })
    .await;

    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;
    let _ = app.emit("commit-finished", ());

    let outcome = match outcome {
        Ok(result) => result,
        Err(e) => Err(format!("Join error: {e}")),
    };
    let (remote_url, branch) = match outcome {
        Ok(pair) => pair,
        Err(message) => {
            // Nothing was restored, so leave no half-created repository behind.
            // Only if it really is the repository this attempt made, though: a
            // world folder that happens to be named "<something>.git" would sit
            // at exactly this path.
            if assert_is_backup_repository(&repo_path).is_ok() {
                let _ = fs::remove_dir_all(&repo_path);
            }
            log::error!("{message}");
            return Ok(CloneFromCloudResult {
                success: false,
                error: Some(message),
                save: None,
            });
        }
    };

    let mut saves = state.saves.lock().unwrap();
    let save = Save {
        name,
        path: save_path,
        repo_path: repo_path.to_string_lossy().to_string(),
        remote_repo_path: remote_url,
        last_access: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        default_branch: branch,
    };
    saves.push(save.clone());
    if let Err(e) = save_saves(&state.data_dir, &saves) {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!("{e}")),
            save: None,
        });
    }
    Ok(CloneFromCloudResult {
        success: true,
        error: None,
        save: Some(save),
    })
}

/// Blocking half of [`clone_save_from_cloud`]. Returns the remote and branch on
/// success so the caller can record them.
fn clone_worker(
    repo_path: &Path,
    save_dir: &Path,
    remote_url: &str,
    branch: &str,
) -> Result<(String, String), String> {
    if let Some(parent) = repo_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to make parent directory: {e}"))?;
    }
    log::info!("Creating local backup repository for cloud world");
    let init = git_command()
        .args([
            "init",
            "--bare",
            &format!("--initial-branch={branch}"),
            &repo_path.to_string_lossy(),
        ])
        .output()
        .map_err(|e| format!("Failed to initialize repository: {e}"))?;
    if !init.status.success() {
        return Err(format!(
            "Failed to initialize repository: {}",
            String::from_utf8_lossy(&init.stderr).trim()
        ));
    }

    let sync = RemoteSync::new(repo_path.to_path_buf(), branch).map_err(|e| format!("{e:#}"))?;
    sync.configure_remote(remote_url)
        .map_err(|e| format!("{e:#}"))?;

    // Fetch, fast-forward the empty local branch onto the cloud tip, and
    // reconstruct the world. `restore_commit` creates a save directory that
    // does not exist yet, which is exactly this case.
    let result = sync
        .sync_before_playing(save_dir)
        .map_err(|e| format!("{e:#}"))?;
    if !result.restored {
        return Err(format!(
            "Cloud branch '{branch}' has no backups to download yet."
        ));
    }
    log::info!("Downloaded cloud world into {save_dir:?}");
    Ok((remote_url.to_string(), branch.to_string()))
}

#[tauri::command]
fn delete_save(
    state: tauri::State<AppState>,
    name: String,
    delete_repo: bool,
) -> Result<(), AppError> {
    let mut saves = state.saves.lock().unwrap();
    let save = saves.iter().find(|s| s.name == name).cloned();
    let len_before = saves.len();
    saves.retain(|s| s.name != name);
    if saves.len() == len_before {
        return Err(AppError::SaveNotFound(name));
    }
    save_saves(&state.data_dir, &saves)?;
    drop(saves);

    if delete_repo {
        if let Some(save) = save {
            let repo_path = Path::new(&save.repo_path);
            if repo_path.exists() {
                assert_is_backup_repository(repo_path)
                    .map_err(|message| AppError::InvalidUTF8(message))?;
                fs::remove_dir_all(repo_path)?;
            }
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
// ─── Dashboard ──────────────────────────────────────────────────────────────

/// A world folder found inside the Minecraft saves directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundWorld {
    pub name: String,
    pub path: String,
    /// When level.dat was last written, ISO 8601. `None` if it cannot be read.
    pub last_played: Option<String>,
    /// The name the world shows in Minecraft's own list, when it differs from
    /// the folder name. Renaming a world in game leaves its folder alone, so
    /// this is often the only name the player recognises.
    pub level_name: Option<String>,
    /// The Minecraft version that last saved it, "1.21.4".
    pub version: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Where Minecraft keeps its worlds on this platform.
#[tauri::command]
fn default_saves_folder() -> String {
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join(".minecraft")
            .join("saves")
            .to_string_lossy()
            .to_string();
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = home_dir() {
        return home
            .join("Library")
            .join("Application Support")
            .join("minecraft")
            .join("saves")
            .to_string_lossy()
            .to_string();
    }

    match home_dir() {
        Some(home) => home
            .join(".minecraft")
            .join("saves")
            .to_string_lossy()
            .to_string(),
        None => String::new(),
    }
}

/// Every world in a saves folder, newest first.
///
/// A world is a directory holding `level.dat`; anything else in the folder is
/// not a save and is skipped.
#[tauri::command]
fn list_worlds_in_folder(folder: String) -> Result<Vec<FoundWorld>, String> {
    let dir = Path::new(&folder);
    if !dir.is_dir() {
        return Err(format!("{folder} is not a folder"));
    }

    let mut worlds = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("Could not read {folder}: {e}"))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let level = path.join("level.dat");
        if !level.is_file() {
            continue;
        }
        // A restore leaves the world it replaced beside the new one, and that
        // copy holds a level.dat like any other world. Offering it back as
        // something to track would start a second history for a folder the
        // player never made and cannot tell apart by name.
        if is_restore_snapshot(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let last_played = fs::metadata(&level)
            .and_then(|meta| meta.modified())
            .ok()
            .map(|time| DateTime::<Local>::from(time).to_rfc3339());
        // level.dat is a few kilobytes, and reading it is what lets the picker
        // show a version beside each world. A world whose level.dat cannot be
        // read is still listed: it is still a world, and refusing to offer it
        // would be the worse failure.
        let level_name = read_world(&path).ok();
        let folder = entry.file_name().to_string_lossy().to_string();
        worlds.push(FoundWorld {
            name: folder.clone(),
            path: path.to_string_lossy().to_string(),
            last_played,
            level_name: level_name
                .as_ref()
                .and_then(|info| info.level_name.clone())
                .filter(|name| *name != folder),
            version: level_name.and_then(|info| info.version_name),
        });
    }
    worlds.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    Ok(worlds)
}

/// A world copy an earlier restore left sitting in the saves folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OldCopy {
    /// The world it was made from, without the timestamp.
    pub world: String,
    pub path: String,
    /// When the copy was set aside.
    pub taken: Option<String>,
    pub bytes: u64,
}

/// Add up everything under a directory. A world is mostly small region files,
/// so this is thousands of metadata reads rather than any real I/O.
fn directory_bytes(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => directory_bytes(&entry.path()),
            Ok(_) => entry.metadata().map(|meta| meta.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// The copies an earlier restore left in the saves folder.
///
/// These hold a level.dat, so Minecraft lists every one of them in the game as a
/// world of its own -- which is why they have to be findable and clearable from
/// here rather than left for the player to work out in a file manager.
#[tauri::command]
async fn list_old_copies(folder: String) -> Result<Vec<OldCopy>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&folder);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut copies = Vec::new();
        for entry in fs::read_dir(dir).map_err(|e| format!("Could not read {folder}: {e}"))? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !path.is_dir() || !is_restore_snapshot(&name) {
                continue;
            }
            let taken = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .map(|time| DateTime::<Local>::from(time).to_rfc3339());
            copies.push(OldCopy {
                world: snapshot_world(&name).unwrap_or_else(|| name.clone()),
                path: path.to_string_lossy().to_string(),
                taken,
                bytes: directory_bytes(&path),
            });
        }
        copies.sort_by(|a, b| b.taken.cmp(&a.taken));
        Ok(copies)
    })
    .await
    .map_err(|e| format!("Could not look for old copies: {e}"))?
}

/// Refuse anything that is not one of our own snapshot folders inside `folder`.
///
/// These commands take paths from the window, and between them they move and
/// delete whole worlds. Everything below is a way somebody's save could end up
/// on the wrong end of that, so each is checked rather than assumed.
fn checked_snapshot(
    folder: &Path,
    tracked_worlds: &[PathBuf],
    path: &str,
) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    if !is_restore_snapshot(&name) {
        return Err(format!("{name} is not a copy MineCommit made"));
    }
    if path.parent() != Some(folder) {
        return Err(format!("{name} is not in the saves folder"));
    }

    // symlink_metadata, not metadata: a link pointing at a real world reads as
    // a directory through the link, and following it would delete the world at
    // the other end instead of the link.
    let meta = fs::symlink_metadata(&path).map_err(|_| format!("{name} is no longer there"))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{name} is a shortcut to somewhere else, so it is left alone"));
    }
    if !meta.is_dir() {
        return Err(format!("{name} is not a folder"));
    }

    // The last line of defence, and the one that does not depend on a name
    // being parsed correctly: never touch a folder MineCommit is looking after
    // as a world.
    if tracked_worlds.iter().any(|world| world == &path) {
        return Err(format!("{name} is a world MineCommit is tracking"));
    }

    // A copy holds a world, so a folder with no level.dat is not one of ours
    // however it is named.
    if !path.join("level.dat").is_file() {
        return Err(format!("{name} does not hold a world"));
    }
    Ok(path)
}

/// The world folders MineCommit is looking after, for the guard above.
fn tracked_world_paths(state: &tauri::State<AppState>) -> Vec<PathBuf> {
    state
        .saves
        .lock()
        .unwrap()
        .iter()
        .map(|save| PathBuf::from(&save.path))
        .collect()
}

/// Move copies out of the saves folder, so Minecraft stops listing them.
///
/// Moving rather than deleting by default: a copy is the world as it was before
/// a restore, which can hold an afternoon that was never backed up anywhere.
#[tauri::command]
async fn tidy_old_copies(
    state: tauri::State<'_, AppState>,
    folder: String,
    paths: Vec<String>,
) -> Result<String, String> {
    let tracked = tracked_world_paths(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let saves = Path::new(&folder);
        let destination = saves
            .parent()
            .unwrap_or(saves)
            .join("minecommit")
            .join("snapshots");
        fs::create_dir_all(&destination)
            .map_err(|e| format!("Could not make {}: {e}", destination.display()))?;

        for path in &paths {
            let from = checked_snapshot(saves, &tracked, path)?;
            let name = from.file_name().unwrap_or_default();
            let to = destination.join(name);
            fs::rename(&from, &to).map_err(|e| {
                format!("Could not move {} out of the saves folder: {e}", name.to_string_lossy())
            })?;
        }
        Ok(destination.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("Could not move the old copies: {e}"))?
}

/// Delete copies for good.
#[tauri::command]
async fn delete_old_copies(
    state: tauri::State<'_, AppState>,
    folder: String,
    paths: Vec<String>,
) -> Result<usize, String> {
    let tracked = tracked_world_paths(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let saves = Path::new(&folder);
        let mut removed = 0;
        for path in &paths {
            let target = checked_snapshot(saves, &tracked, path)?;
            fs::remove_dir_all(&target)
                .map_err(|e| format!("Could not delete {}: {e}", target.display()))?;
            removed += 1;
        }
        Ok(removed)
    })
    .await
    .map_err(|e| format!("Could not delete the old copies: {e}"))?
}

/// Refuse to erase anything that is not a MineCommit backup repository.
///
/// Two places delete a whole directory tree by a path that came from stored
/// settings. Settings are a file on disk that anything could have written, and
/// a repository path that somehow named a world would take the world with it.
/// A bare repository has a HEAD and an objects directory; a Minecraft world has
/// a level.dat and must never match.
fn assert_is_backup_repository(path: &Path) -> Result<(), String> {
    let name = path.display();
    if path.join("level.dat").is_file() {
        return Err(format!("{name} holds a world, not backups, so it was left alone"));
    }
    if !path.join("HEAD").is_file() || !path.join("objects").is_dir() {
        return Err(format!("{name} is not a backup repository, so it was left alone"));
    }
    Ok(())
}

/// The world a copy was made from, if the name really is one of ours.
///
/// `next_snapshot_path` writes `<world>.<epoch millis>.snapshot`, with a small
/// counter before `.snapshot` when two restores land in the same millisecond.
///
/// Matching on the `.snapshot` ending alone is not good enough. These names
/// decide what gets hidden from the world list, and what `tidy_old_copies` and
/// `delete_old_copies` are allowed to move and erase. A player who happens to
/// name a world "Backup.snapshot" would have had it hidden from MineCommit and
/// then offered up for deletion as clutter. Requiring the timestamp -- ten
/// digits at least, which no one types by accident -- means only names this
/// program actually produced can match.
fn snapshot_world(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".snapshot")?;
    let parts: Vec<&str> = stem.split('.').collect();

    let is_stamp = |part: &str| part.len() >= 10 && part.bytes().all(|b| b.is_ascii_digit());
    let is_counter =
        |part: &str| !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit());

    let world = if parts.len() >= 2 && is_stamp(parts[parts.len() - 1]) {
        &parts[..parts.len() - 1]
    } else if parts.len() >= 3
        && is_counter(parts[parts.len() - 1])
        && is_stamp(parts[parts.len() - 2])
    {
        &parts[..parts.len() - 2]
    } else {
        return None;
    };

    let world = world.join(".");
    if world.is_empty() { None } else { Some(world) }
}

/// Whether a folder is a copy MineCommit set aside when restoring a world.
fn is_restore_snapshot(name: &str) -> bool {
    snapshot_world(name).is_some()
}

/// One point in a world's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: String,
    pub device: Option<String>,
    pub note: String,
}

/// The backups recorded for a world, newest first.
#[tauri::command]
fn list_history(git_dir: String, branch: String, limit: u32) -> Result<Vec<HistoryEntry>, String> {
    // A note may contain newlines, so lines cannot separate records: use the
    // ASCII unit separator between fields and the record separator between
    // commits.
    let output = git_command()
        .args(["--git-dir", &git_dir, "log", &branch])
        .arg("--format=%H%x1f%cI%x1f%s%x1f%b%x1e")
        .arg(format!("--max-count={limit}"))
        .output()
        .map_err(|e| format!("Could not read the history: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A branch with no backups yet is an empty history, not a failure.
        if stderr.contains("unknown revision") || stderr.contains("bad revision") {
            return Ok(Vec::new());
        }
        return Err(format!("Could not read the history: {}", stderr.trim()));
    }

    Ok(parse_history(&String::from_utf8_lossy(&output.stdout)))
}

/// Split `git log`'s output into entries.
///
/// Kept separate from the command that produces it so the parsing can be
/// tested: a note may contain newlines and the separators are invisible, so a
/// mistake here is easy to make and hard to see.
fn parse_history(stdout: &str) -> Vec<HistoryEntry> {
    stdout
        .split('\x1e')
        .map(|record| record.trim_start_matches('\n'))
        .filter(|record| !record.trim().is_empty())
        .filter_map(|record| {
            let mut fields = record.split('\x1f');
            let id = fields.next()?.to_string();
            let timestamp = fields.next()?.to_string();
            let note = fields.next()?.to_string();
            let device = fields.next().unwrap_or_default().lines().find_map(|line| {
                line.strip_prefix("MineCommit-Device:")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            });
            Some(HistoryEntry {
                id,
                timestamp,
                device,
                note,
            })
        })
        .collect()
}

/// The name this computer records on the backups it makes.
#[tauri::command]
fn device_name() -> String {
    current_device_name()
}

/// What the dashboard needs to know about a world folder itself, as opposed to
/// its backups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    /// False while Minecraft still has the world open, and `None` when that
    /// cannot be answered without taking the lock -- which is never worth doing
    /// for a badge, because holding it is what makes Minecraft drop the world
    /// from its own list. The backup still checks properly before it runs.
    pub idle: Option<bool>,
    /// When level.dat was last written, ISO 8601. Compared against the newest
    /// backup to tell whether the world has been played since.
    pub last_played: Option<String>,
}

/// What a world says about itself, beyond its backup state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldDetails {
    /// What `level.dat` holds, or `None` when it cannot be read -- an old
    /// format, a mod's own layout, or a world mid-save. The panel simply shows
    /// less in that case.
    pub level: Option<LevelInfo>,
    /// Total size of the world folder on disk.
    pub bytes: u64,
}

/// Read a world's own details: version, game mode, seed, size on disk.
///
/// Never fails. Nothing here is needed to back a world up, so a world that
/// cannot describe itself must still be usable rather than showing an error
/// where its version would be.
#[tauri::command]
async fn world_details(save_dir: String) -> WorldDetails {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = Path::new(&save_dir);
        WorldDetails {
            level: read_world(dir)
                .map_err(|error| log::debug!("Could not read {save_dir}/level.dat: {error:#}"))
                .ok(),
            // Thousands of metadata reads on a large world, which is why this
            // is off the main thread.
            bytes: directory_bytes(dir),
        }
    })
    .await
    .unwrap_or(WorldDetails {
        level: None,
        bytes: 0,
    })
}

#[tauri::command]
fn world_state(save_dir: String) -> WorldState {
    let dir = Path::new(&save_dir);
    WorldState {
        // Asked, never taken. This runs on a timer for every world, and holding
        // session.lock -- even for an instant -- makes Minecraft drop the world
        // from the list it shows in the game.
        idle: world_is_busy(dir).map(|busy| !busy),
        last_played: fs::metadata(dir.join("level.dat"))
            .and_then(|meta| meta.modified())
            .ok()
            .map(|time| DateTime::<Local>::from(time).to_rfc3339()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    #[serde(default)]
    pub saves_folder: String,
    /// The GitHub account MineCommit is signed in as. The token itself is not
    /// here: it goes to Git's credential store.
    #[serde(default)]
    pub github_login: String,
    #[serde(default)]
    pub github_avatar: String,
}

fn settings_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

fn read_settings(data_dir: &Path) -> AppSettings {
    fs::read_to_string(settings_file_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_settings(data_dir: &Path, settings: &AppSettings) -> Result<(), String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("Could not save your settings: {e}"))?;
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Could not save your settings: {e}"))?;
    fs::write(settings_file_path(data_dir), text)
        .map_err(|e| format!("Could not save your settings: {e}"))
}

#[tauri::command]
fn get_saves_folder(state: tauri::State<AppState>) -> String {
    let stored = read_settings(&state.data_dir).saves_folder;
    if stored.trim().is_empty() {
        default_saves_folder()
    } else {
        stored
    }
}

#[tauri::command]
fn set_saves_folder(state: tauri::State<AppState>, folder: String) -> Result<(), String> {
    let mut settings = read_settings(&state.data_dir);
    settings.saves_folder = folder;
    write_settings(&state.data_dir, &settings)
}

/// What a backup did. The two halves are reported separately because they can
/// disagree: an upload that fails leaves a perfectly good backup behind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    /// The world was recorded in the backup repository on this computer.
    pub backed_up: bool,
    /// That backup reached the cloud. False when no cloud is set up.
    pub uploaded: bool,
    /// Why the backup failed. Nothing was recorded.
    pub error: Option<String>,
    /// Why the upload failed. The world is safe on this computer, and the next
    /// upload carries this backup up with it.
    pub upload_error: Option<String>,
}

/// Back up a world and send it to the cloud as one action.
///
/// Splitting these was the single most confusing thing about MineCommit: a
/// player who backed up but did not upload believed their world was safe on
/// another computer when it was not.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn backup_and_upload(
    app: tauri::AppHandle,
    save_dir: String,
    git_dir: String,
    branch: String,
    remote: String,
    message: String,
    author_name: String,
    author_email: String,
    extra_patterns: Vec<String>,
    ignore_patterns: Vec<String>,
    commit_first: bool,
) -> BackupResult {
    init_logger();
    take_logs();
    minecommit::progress::end();

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        let mut reported = None;
        while running_clone.load(Ordering::Relaxed) {
            pump(&app_clone, &mut reported);
            std::thread::sleep(Duration::from_millis(50));
        }
        pump(&app_clone, &mut reported);
    });

    let result = tauri::async_runtime::spawn_blocking(move || {
        // A world that has not been played since its last backup has nothing
        // new to record, so recording it again would only add a duplicate
        // entry to the history the player reads. The upload still runs: that
        // is the half that was left undone.
        if commit_first {
            let commit = commit_blocking(
                save_dir,
                git_dir.clone(),
                branch.clone(),
                message,
                author_name,
                author_email,
                extra_patterns,
                ignore_patterns,
                true,
            );
            if !commit.success {
                return BackupResult {
                    backed_up: false,
                    uploaded: false,
                    error: commit.error,
                    upload_error: None,
                };
            }
        } else {
            log::info!("Nothing new has been played, so uploading the existing backup");
        }

        if remote.trim().is_empty() {
            log::info!("No cloud repository is set up, so the backup stays on this computer");
            return BackupResult {
                backed_up: true,
                uploaded: false,
                error: None,
                upload_error: None,
            };
        }

        log::info!("Uploading to the cloud...");
        let upload = RemoteSync::new(PathBuf::from(&git_dir), &branch).and_then(|sync| {
            sync.configure_remote(&remote)?;
            sync.push()
        });
        match upload {
            Ok(status) => {
                log::info!("{}", status.state.description());
                log::info!("Upload complete");
                BackupResult {
                    backed_up: true,
                    uploaded: true,
                    error: None,
                    upload_error: None,
                }
            }
            Err(error) => {
                // The world is already safe on this computer, so this is a
                // partial success, not a failed backup.
                let message = format!("{error:#}");
                log::error!("Backed up on this computer, but the upload failed: {message}");
                BackupResult {
                    backed_up: true,
                    uploaded: false,
                    error: None,
                    upload_error: Some(message),
                }
            }
        }
    })
    .await;

    running.store(false, Ordering::Relaxed);
    let _ = log_task.await;
    let _ = app.emit("commit-finished", ());

    result.unwrap_or_else(|e| BackupResult {
        backed_up: false,
        uploaded: false,
        error: Some(format!("Join error: {e}")),
        upload_error: None,
    })
}

// ─── GitHub account ─────────────────────────────────────────────────────────

/// The GitHub user MineCommit is signed in as.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAccount {
    pub login: String,
    pub avatar_url: Option<String>,
}

/// GitHub asks for this, and refuses requests without one.
const USER_AGENT: &str = concat!("MineCommit/", env!("CARGO_PKG_VERSION"));

/// The OAuth app MineCommit signs in as.
///
/// Public by design: the device flow exists precisely for clients that cannot
/// keep a secret, which is every app a player downloads. Set at build time with
/// `MINECOMMIT_GITHUB_CLIENT_ID`.
const GITHUB_CLIENT_ID: &str = match option_env!("MINECOMMIT_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// The app's name in its URL, for sending the player to choose which
/// repositories MineCommit may touch.
const GITHUB_APP_SLUG: &str = match option_env!("MINECOMMIT_GITHUB_APP_SLUG") {
    Some(slug) => slug,
    None => "",
};

/// A repository the player has given MineCommit access to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantedRepository {
    pub full_name: String,
    pub clone_url: String,
    pub private: bool,
}

/// What GitHub told us to show the player, and how to wait for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignInRequest {
    /// The short code the player types into GitHub.
    pub user_code: String,
    /// The page to type it on.
    pub verification_uri: String,
    /// How long the code is good for.
    pub expires_in_seconds: u64,
    pub retry_in_seconds: u64,
}

/// Where a sign-in has got to. The player is on GitHub's page while this is
/// `Waiting`, so each poll is one question and one answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SignInProgress {
    Waiting { retry_in_seconds: u64 },
    Authorized { account: GitHubAccount },
    /// The player pressed Cancel on GitHub's page.
    Denied,
    /// The code went stale before it was entered.
    Expired,
}

/// A sign-in in progress.
#[derive(Debug, Clone)]
struct DeviceFlow {
    device_code: String,
    retry_in_seconds: u64,
}

fn github_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("Could not start an HTTPS client: {e}"))
}

fn require_client_id() -> Result<&'static str, String> {
    if GITHUB_CLIENT_ID.trim().is_empty() {
        return Err(
            "This build of MineCommit has no GitHub application configured, so it cannot sign in."
                .into(),
        );
    }
    Ok(GITHUB_CLIENT_ID)
}

/// Ask GitHub who a token belongs to. Doubles as the check that it is valid.
async fn github_user(token: &str) -> Result<(String, Option<String>), String> {
    let response = github_client()?
        .get("https://api.github.com/user")
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("GitHub did not accept the sign-in. Try signing in again.".into());
    }
    if !response.status().is_success() {
        return Err(format!("GitHub refused the sign-in ({})", response.status()));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("GitHub sent something unexpected: {e}"))?;
    let login = body
        .get("login")
        .and_then(|v| v.as_str())
        .ok_or("GitHub did not say which account this is")?
        .to_string();
    let avatar = body
        .get("avatar_url")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok((login, avatar))
}

/// Git needs somewhere to keep the token, or every push would ask for it again.
///
/// An existing choice is always respected. Otherwise the best available store
/// is picked: Windows has the Credential Manager through `manager`, desktop
/// Linux usually has libsecret, and `store` -- a plain file in the home
/// directory -- is the last resort.
fn ensure_credential_helper() -> Result<(), String> {
    let configured = git_command()
        .args(["config", "--get", "credential.helper"])
        .output()
        .map_err(|e| format!("Could not read your Git configuration: {e}"))?;
    if configured.status.success() && !String::from_utf8_lossy(&configured.stdout).trim().is_empty()
    {
        return Ok(());
    }

    let helper = if cfg!(windows) {
        "manager"
    } else if libsecret_available() {
        "libsecret"
    } else {
        "store"
    };

    let set = git_command()
        .args(["config", "--global", "credential.helper", helper])
        .output()
        .map_err(|e| format!("Could not update your Git configuration: {e}"))?;
    ensure_command_succeeded(set, "Could not tell Git where to keep the sign-in")
}

fn libsecret_available() -> bool {
    let Ok(output) = git_command().arg("--exec-path").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let exec_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Path::new(&exec_path)
        .join("git-credential-libsecret")
        .exists()
}

fn ensure_command_succeeded(output: std::process::Output, context: &str) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{context}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Read the token back out of Git's credential store.
///
/// Only attempted when MineCommit believes it is signed in, so a credential
/// helper with a user interface is not made to ask about an account that was
/// never stored.
fn read_git_credential(login: &str) -> Option<String> {
    use std::io::Write;

    let mut child = git_command()
        .args(["credential", "fill"])
        // A helper that cannot answer silently must fail rather than block on a
        // prompt no one is watching.
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    child
        .stdin
        .as_mut()?
        .write_all(credential_description(login, None).as_bytes())
        .ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("password="))
        .map(str::to_string)
        .filter(|token| !token.is_empty())
}

/// Hand the token to Git's credential store so pushes and fetches authenticate
/// without asking. MineCommit never writes it to its own files.
fn store_git_credential(login: &str, token: &str) -> Result<(), String> {
    ensure_credential_helper()?;
    run_credential("approve", login, Some(token))
}

fn forget_git_credential(login: &str) -> Result<(), String> {
    run_credential("reject", login, None)
}

/// The description Git reads on stdin: `key=value` lines, terminated by a
/// blank one. Without the terminator Git waits for more input forever.
fn credential_description(login: &str, token: Option<&str>) -> String {
    let mut input = format!("protocol=https\nhost=github.com\nusername={login}\n");
    if let Some(token) = token {
        input.push_str(&format!("password={token}\n"));
    }
    input.push('\n');
    input
}

fn run_credential(action: &str, login: &str, token: Option<&str>) -> Result<(), String> {
    use std::io::Write;

    let input = credential_description(login, token);

    let mut child = git_command()
        .args(["credential", action])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run Git: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("Could not talk to Git")?
        .write_all(input.as_bytes())
        .map_err(|e| format!("Could not talk to Git: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Git did not finish: {e}"))?;
    ensure_command_succeeded(output, "Git could not store the sign-in")
}

/// Begin signing in: ask GitHub for a code for the player to enter.
#[tauri::command]
async fn github_start_sign_in(
    state: tauri::State<'_, AppState>,
) -> Result<SignInRequest, String> {
    let client_id = require_client_id()?;

    let response = github_client()?
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id)])
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = body
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("GitHub would not start the sign-in");
        return Err(format!("{message} ({status})"));
    }

    let text = |key: &str| body.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let device_code = text("device_code").ok_or("GitHub did not send a sign-in code")?;
    let user_code = text("user_code").ok_or("GitHub did not send a sign-in code")?;
    let verification_uri =
        text("verification_uri").unwrap_or_else(|| "https://github.com/login/device".to_string());
    // GitHub's documented default is five seconds when it says nothing.
    let retry_in_seconds = body.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);
    let expires_in_seconds = body.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);

    *state.device_flow.lock().unwrap() = Some(DeviceFlow {
        device_code,
        retry_in_seconds,
    });

    Ok(SignInRequest {
        user_code,
        verification_uri,
        expires_in_seconds,
        retry_in_seconds,
    })
}

/// Ask once whether the player has finished on GitHub's page.
#[tauri::command]
async fn github_poll_sign_in(
    state: tauri::State<'_, AppState>,
) -> Result<SignInProgress, String> {
    let client_id = require_client_id()?;
    let flow = state
        .device_flow
        .lock()
        .unwrap()
        .clone()
        .ok_or("No sign-in is in progress")?;

    let response = github_client()?
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", flow.device_code.as_str()),
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
        ])
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("GitHub sent something unexpected: {e}"))?;

    if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
        let (login, avatar_url) = github_user(token).await?;
        store_git_credential(&login, token)?;

        let mut settings = read_settings(&state.data_dir);
        settings.github_login = login.clone();
        settings.github_avatar = avatar_url.clone().unwrap_or_default();
        write_settings(&state.data_dir, &settings)?;

        // Kept in memory only, and only so "create a repository for me" works
        // during the session it was granted in.
        *state.github_token.lock().unwrap() = Some(token.to_string());
        *state.device_flow.lock().unwrap() = None;

        return Ok(SignInProgress::Authorized {
            account: GitHubAccount {
                login,
                avatar_url,
            },
        });
    }

    match body.get("error").and_then(|v| v.as_str()) {
        Some("authorization_pending") => Ok(SignInProgress::Waiting {
            retry_in_seconds: flow.retry_in_seconds,
        }),
        // GitHub asks us to back off; it expects the extra delay to stick.
        Some("slow_down") => {
            let slower = body
                .get("interval")
                .and_then(|v| v.as_u64())
                .unwrap_or(flow.retry_in_seconds + 5);
            state
                .device_flow
                .lock()
                .unwrap()
                .as_mut()
                .map(|flow| flow.retry_in_seconds = slower);
            Ok(SignInProgress::Waiting {
                retry_in_seconds: slower,
            })
        }
        Some("expired_token") => {
            *state.device_flow.lock().unwrap() = None;
            Ok(SignInProgress::Expired)
        }
        Some("access_denied") => {
            *state.device_flow.lock().unwrap() = None;
            Ok(SignInProgress::Denied)
        }
        Some(other) => Err(format!("GitHub refused the sign-in: {other}")),
        None => Err("GitHub sent an answer MineCommit did not understand".into()),
    }
}

/// Abandon a sign-in the player backed out of.
#[tauri::command]
fn github_cancel_sign_in(state: tauri::State<AppState>) {
    *state.device_flow.lock().unwrap() = None;
}

#[tauri::command]
fn github_account(state: tauri::State<AppState>) -> Option<GitHubAccount> {
    let settings = read_settings(&state.data_dir);
    if settings.github_login.trim().is_empty() {
        return None;
    }
    Some(GitHubAccount {
        login: settings.github_login,
        avatar_url: Some(settings.github_avatar).filter(|url| !url.is_empty()),
    })
}

#[tauri::command]
fn github_sign_out(state: tauri::State<AppState>) -> Result<(), String> {
    let mut settings = read_settings(&state.data_dir);
    let login = std::mem::take(&mut settings.github_login);
    settings.github_avatar = String::new();
    write_settings(&state.data_dir, &settings)?;
    *state.github_token.lock().unwrap() = None;

    if !login.trim().is_empty() {
        // Best effort: the account is signed out of MineCommit either way, and
        // a credential store that refuses to forget should not look like a
        // failed sign-out.
        if let Err(error) = forget_git_credential(&login) {
            log::warn!("Could not remove the saved GitHub sign-in: {error}");
        }
    }
    Ok(())
}

/// The token for the signed-in account, from this session or from Git's store.
fn current_token(state: &AppState) -> Option<String> {
    if let Some(token) = state.github_token.lock().unwrap().clone() {
        return Some(token);
    }
    let login = read_settings(&state.data_dir).github_login;
    if login.trim().is_empty() {
        return None;
    }
    let token = read_git_credential(&login)?;
    *state.github_token.lock().unwrap() = Some(token.clone());
    Some(token)
}

/// Where the player chooses which repositories MineCommit may use.
#[tauri::command]
fn github_install_url() -> Result<String, String> {
    if GITHUB_APP_SLUG.trim().is_empty() {
        return Err("This build of MineCommit has no GitHub application configured.".into());
    }
    Ok(format!(
        "https://github.com/apps/{GITHUB_APP_SLUG}/installations/new"
    ))
}

/// The repositories the player has granted MineCommit access to.
///
/// This is the whole of what MineCommit can reach: repositories they picked,
/// nothing else. Listing them means a world is connected by choosing from that
/// list rather than by pasting an address.
#[tauri::command]
async fn github_repositories(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GrantedRepository>, String> {
    let token = current_token(&state).ok_or("Sign in to GitHub first")?;
    let client = github_client()?;

    let installations: serde_json::Value = client
        .get("https://api.github.com/user/installations")
        .bearer_auth(&token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GitHub sent something unexpected: {e}"))?;

    let ids: Vec<u64> = installations
        .get("installations")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|entry| entry.get("id").and_then(|v| v.as_u64()))
                .collect()
        })
        .unwrap_or_default();

    let mut granted = Vec::new();
    for id in ids {
        let page: serde_json::Value = client
            .get(format!(
                "https://api.github.com/user/installations/{id}/repositories?per_page=100"
            ))
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| format!("Could not reach GitHub: {e}"))?
            .json()
            .await
            .map_err(|e| format!("GitHub sent something unexpected: {e}"))?;

        let Some(list) = page.get("repositories").and_then(|v| v.as_array()) else {
            continue;
        };
        for repo in list {
            let text = |key: &str| repo.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let (Some(full_name), Some(clone_url)) = (text("full_name"), text("clone_url")) else {
                continue;
            };
            granted.push(GrantedRepository {
                full_name,
                clone_url,
                private: repo
                    .get("private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
    }
    granted.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(granted)
}


pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");

            let saves = load_saves(&data_dir)?;

            app.manage(AppState {
                saves: Mutex::new(saves),
                data_dir,
                github_token: Mutex::new(None),
                device_flow: Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_saves,
            add_save,
            configure_save_cloud,
            delete_save,
            derive_save_info,
            access_save,
            check_repo_exists,
            init_bare_repo,
            list_branches,
            get_head_ref,
            get_git_author,
            set_git_author,
            perform_commit,
            perform_restore,
            perform_push,
            perform_pull,
            get_cloud_status,
            list_remote_branches,
            list_local_branches,
            clone_save_from_cloud,
            default_saves_folder,
            list_worlds_in_folder,
            list_old_copies,
            tidy_old_copies,
            delete_old_copies,
            list_history,
            device_name,
            world_state,
            world_details,
            get_saves_folder,
            set_saves_folder,
            backup_and_upload,
            repair_world_repository,
            github_start_sign_in,
            github_poll_sign_in,
            github_cancel_sign_in,
            github_account,
            github_install_url,
            github_repositories,
            github_sign_out,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One record exactly as `git log --format=%H%x1f%cI%x1f%s%x1f%b%x1e`
    /// writes it, including the newline Git puts before the next record.
    fn record(id: &str, timestamp: &str, subject: &str, body: &str) -> String {
        format!("{id}\x1f{timestamp}\x1f{subject}\x1f{body}\x1e\n")
    }

    #[test]
    fn reads_the_device_a_backup_was_made_on() {
        let stdout = record(
            "abc123",
            "2026-08-22T11:45:59+08:00",
            "Backup",
            "MineCommit-Device: Juny's PC\n",
        );

        let entries = parse_history(&stdout);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "abc123");
        assert_eq!(entries[0].timestamp, "2026-08-22T11:45:59+08:00");
        assert_eq!(entries[0].note, "Backup");
        assert_eq!(entries[0].device.as_deref(), Some("Juny's PC"));
    }

    #[test]
    fn a_note_containing_blank_lines_does_not_split_the_entry() {
        // The reason records are separated by \x1e rather than by lines: a
        // player's note is free text and may contain anything.
        let stdout = format!(
            "{}{}",
            record(
                "aaa",
                "2026-08-22T10:00:00+08:00",
                "Built the nether portal",
                "first line\n\nsecond line\nMineCommit-Device: Laptop\n",
            ),
            record("bbb", "2026-08-21T10:00:00+08:00", "Backup", ""),
        );

        let entries = parse_history(&stdout);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].note, "Built the nether portal");
        assert_eq!(entries[0].device.as_deref(), Some("Laptop"));
        assert_eq!(entries[1].id, "bbb");
        assert_eq!(entries[1].device, None);
    }

    #[test]
    fn a_repository_with_no_backups_yields_no_entries() {
        assert!(parse_history("").is_empty());
        assert!(parse_history("\n").is_empty());
    }

    #[test]
    fn only_folders_holding_level_dat_count_as_worlds() {
        // The "add a world" list is built from this, so anything else in the
        // saves folder -- a screenshot, a leftover zip, an empty directory --
        // must not appear as something to back up.
        let saves = tempfile::tempdir().expect("tempdir");
        fs::create_dir(saves.path().join("My World")).unwrap();
        fs::write(saves.path().join("My World/level.dat"), b"nbt").unwrap();
        fs::create_dir(saves.path().join("not-a-world")).unwrap();
        fs::write(saves.path().join("loose-file.zip"), b"zip").unwrap();

        let worlds =
            list_worlds_in_folder(saves.path().to_string_lossy().to_string()).expect("scan");

        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].name, "My World");
        assert!(worlds[0].last_played.is_some());
        // That level.dat is not readable NBT, and the world is offered anyway.
        // Backing a world up does not depend on it describing itself, so a
        // world whose format this build cannot parse must never drop out of the
        // list -- that would be a world silently left unprotected.
        assert_eq!(worlds[0].version, None);
        assert_eq!(worlds[0].level_name, None);
    }

    #[test]
    fn the_copies_a_restore_leaves_behind_are_not_offered_as_worlds() {
        // Restoring renames the old world to "<name>.<millis>.snapshot" and
        // leaves it in the saves folder. It still holds a level.dat, so without
        // this it shows up in "Add a world" looking like a world of its own.
        let saves = tempfile::tempdir().expect("tempdir");
        for name in ["My World", "My World.1787384127276.snapshot"] {
            fs::create_dir(saves.path().join(name)).unwrap();
            fs::write(saves.path().join(name).join("level.dat"), b"nbt").unwrap();
        }

        let worlds =
            list_worlds_in_folder(saves.path().to_string_lossy().to_string()).expect("scan");

        assert_eq!(
            worlds.iter().map(|w| w.name.as_str()).collect::<Vec<_>>(),
            vec!["My World"]
        );
    }

    /// Acquiring session.lock is right when a backup is about to hold it
    /// anyway, and wrong for anything that merely reports state: while
    /// MineCommit holds the lock, Minecraft cannot take it, and Minecraft drops
    /// a world it cannot lock from the list it shows in the game. `world_state`
    /// runs on a timer for every world, so this must stay a question, not a
    /// claim.
    #[test]
    fn reporting_a_world_state_never_takes_its_lock() {
        let source = include_str!("lib.rs");
        let start = source
            .find("fn world_state(")
            .expect("world_state must exist");
        let body = &source[start..];
        let end = body.find("\n}\n").expect("world_state must end");
        let body = &body[..end];

        assert!(
            !body.contains("lock_inactive_world"),
            "world_state acquires the session lock again:\n{body}"
        );
        assert!(
            body.contains("world_is_busy"),
            "world_state should ask with world_is_busy"
        );
    }

    #[test]
    fn only_names_this_program_produced_count_as_copies() {
        // Ours, and the world behind each one.
        assert_eq!(
            snapshot_world("My World.1787384127276.snapshot").as_deref(),
            Some("My World")
        );
        // A second restore in the same millisecond gets a counter as well.
        assert_eq!(
            snapshot_world("My World.1787384127276.1.snapshot").as_deref(),
            Some("My World")
        );
        // Worlds are allowed dots of their own.
        assert_eq!(
            snapshot_world("v1.20 survival.1787384127276.snapshot").as_deref(),
            Some("v1.20 survival")
        );

        // Not ours. Every one of these is somebody's world, and matching any of
        // them would hide it from MineCommit and then offer it up for deletion.
        for impostor in [
            "Backup.snapshot",
            "snapshot",
            ".snapshot",
            "My World.snapshot",
            "Level.2.snapshot",
            "1787384127276.snapshot",
            "My World.1787384127276",
            "My World.1787384127276.snapshot.backup",
        ] {
            assert_eq!(
                snapshot_world(impostor),
                None,
                "{impostor:?} is not a copy MineCommit made"
            );
        }
    }

    /// Everything `tidy_old_copies` and `delete_old_copies` are allowed to
    /// touch passes through `checked_snapshot` first. These are the ways a real
    /// world could reach it, and every one of them has to be turned away: a
    /// mistake here does not corrupt a save, it erases one.
    #[test]
    fn nothing_but_our_own_copies_can_be_moved_or_deleted() {
        let saves = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("tempdir");
        let world = |dir: &Path, name: &str| {
            let path = dir.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("level.dat"), b"nbt").unwrap();
            path
        };

        let copy = world(saves.path(), "My World.1787384127276.snapshot");
        let real = world(saves.path(), "My World");
        let outside = world(elsewhere.path(), "Other.1787384127276.snapshot");
        let named_like_one = world(saves.path(), "Backup.snapshot");
        let empty = saves.path().join("Hollow.1787384127276.snapshot");
        fs::create_dir(&empty).unwrap();

        let tracked = vec![real.clone()];
        let ok = |p: &Path| checked_snapshot(saves.path(), &tracked, p.to_str().unwrap());

        assert!(ok(&copy).is_ok(), "our own copy is the one thing allowed");

        for (path, why) in [
            (&real, "a tracked world must never be touched"),
            (&outside, "a path outside the saves folder must be refused"),
            (&named_like_one, "a world merely named like a copy must be refused"),
            (&empty, "a folder holding no world is not a copy of one"),
            (&saves.path().join("gone.1787384127276.snapshot"), "a copy already removed"),
        ] {
            assert!(ok(path).is_err(), "{why}: {}", path.display());
        }

        // A copy that is really a shortcut to a world elsewhere: following it
        // would erase the world at the far end rather than the link.
        #[cfg(unix)]
        {
            let link = saves.path().join("Sneaky.1787384127276.snapshot");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(
                ok(&link).is_err(),
                "a shortcut must not be followed to somebody's world"
            );
            assert!(real.join("level.dat").is_file(), "and the world is untouched");
        }
    }

    /// Two commands erase a directory tree by a path read back from settings,
    /// which is a file on disk that anything could have written.
    #[test]
    fn only_a_real_backup_repository_can_be_erased() {
        let dir = tempfile::tempdir().expect("tempdir");

        let repo = dir.path().join("world.git");
        fs::create_dir_all(repo.join("objects")).unwrap();
        fs::write(repo.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
        assert!(assert_is_backup_repository(&repo).is_ok());

        let world = dir.path().join("My World");
        fs::create_dir_all(world.join("objects")).unwrap();
        fs::write(world.join("HEAD"), b"decoy").unwrap();
        fs::write(world.join("level.dat"), b"nbt").unwrap();
        assert!(
            assert_is_backup_repository(&world).is_err(),
            "a level.dat means a world, whatever else the folder contains"
        );

        let neither = dir.path().join("photos");
        fs::create_dir(&neither).unwrap();
        assert!(assert_is_backup_repository(&neither).is_err());
    }

    #[test]
    fn a_folder_that_is_not_there_is_reported_rather_than_read_as_empty() {
        let missing = tempfile::tempdir().expect("tempdir");
        let path = missing.path().join("gone").to_string_lossy().to_string();
        assert!(list_worlds_in_folder(path).is_err());
    }

    #[test]
    fn a_world_nobody_has_open_is_idle_and_dated() {
        let save = tempfile::tempdir().expect("tempdir");
        fs::write(save.path().join("level.dat"), b"nbt").unwrap();

        let state = world_state(save.path().to_string_lossy().to_string());
        assert_eq!(
            state.idle,
            Some(true),
            "a world with no session.lock is not in use"
        );
        assert!(state.last_played.is_some());
    }

    /// Reaches api.github.com, so it is not part of the normal suite. Run with
    /// `cargo test -p minecommit-gui -- --ignored` to check that the HTTPS
    /// stack works: a rustls build with no crypto provider installed panics at
    /// the first request rather than failing to compile.
    #[tokio::test]
    #[ignore = "requires network access"]
    async fn github_rejects_a_token_that_is_not_one() {
        let error = github_user("not-a-real-token")
            .await
            .expect_err("GitHub accepted a nonsense token");
        assert!(
            error.contains("did not accept"),
            "expected a rejected-token message, got: {error}"
        );
    }

    fn git_in(repo: &str, args: &[&str]) -> String {
        let out = git_command()
            .args(["--git-dir", repo])
            .args(args)
            .output()
            .expect("run git");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[test]
    fn a_world_added_without_a_repository_gets_one() {
        // Adding a world used to record where its backups would live without
        // ever creating it, so everything afterwards failed with "is not a
        // bare Git repository" and no screen offered to fix it.
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir
            .path()
            .join("minecommit")
            .join("my world.git")
            .to_string_lossy()
            .to_string();

        ensure_bare_repo(&repo, "my-world").expect("create the repository");

        assert_eq!(git_in(&repo, &["rev-parse", "--is-bare-repository"]), "true");
        assert_eq!(
            git_in(&repo, &["symbolic-ref", "HEAD"]),
            "refs/heads/my-world",
            "the world's branch should be the one it was added with"
        );
    }

    #[test]
    fn a_repository_that_already_exists_is_left_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("world.git").to_string_lossy().to_string();
        ensure_bare_repo(&repo, "original").expect("create");

        // Asking again, with a different branch, must not disturb the backups
        // already in there.
        ensure_bare_repo(&repo, "something-else").expect("second call");

        assert_eq!(git_in(&repo, &["symbolic-ref", "HEAD"]), "refs/heads/original");
    }

    /// The client ID and slug are compiled in, so a release built without them
    /// looks completely normal until someone tries to sign in. This asserts the
    /// two states are the ones intended rather than an accident.
    #[test]
    fn the_github_app_is_either_configured_or_says_it_is_not() {
        match (GITHUB_CLIENT_ID.is_empty(), GITHUB_APP_SLUG.is_empty()) {
            (true, true) => {
                assert!(
                    github_install_url().is_err(),
                    "an unconfigured build must refuse rather than send players to a broken URL"
                );
                assert!(require_client_id().is_err());
            }
            (false, false) => {
                assert_eq!(
                    github_install_url().expect("a configured build has an install URL"),
                    format!("https://github.com/apps/{GITHUB_APP_SLUG}/installations/new")
                );
                assert!(
                    GITHUB_CLIENT_ID.starts_with("Iv23"),
                    "GitHub App client IDs start with Iv23; {GITHUB_CLIENT_ID} looks like an OAuth app, \
                     which would ask for access to every repository"
                );
            }
            _ => panic!(
                "half-configured build: client id {:?}, slug {:?}",
                GITHUB_CLIENT_ID, GITHUB_APP_SLUG
            ),
        }
    }

    #[test]
    fn a_credential_description_ends_with_a_blank_line() {
        let approve = credential_description("octocat", Some("ghp_secret"));
        assert_eq!(
            approve,
            "protocol=https\nhost=github.com\nusername=octocat\npassword=ghp_secret\n\n"
        );

        // Rejecting a credential carries no password.
        let reject = credential_description("octocat", None);
        assert_eq!(reject, "protocol=https\nhost=github.com\nusername=octocat\n\n");
        assert!(reject.ends_with("\n\n"), "Git would wait for more input");
    }

    #[test]
    fn settings_written_before_sign_in_existed_still_load() {
        // Upgrading must not lose the saves folder someone already chose.
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(
            settings_file_path(dir.path()),
            r#"{"saves_folder": "/home/player/.minecraft/saves"}"#,
        )
        .unwrap();

        let settings = read_settings(dir.path());
        assert_eq!(settings.saves_folder, "/home/player/.minecraft/saves");
        assert!(settings.github_login.is_empty());
    }

    #[test]
    fn an_account_survives_a_round_trip_but_the_token_is_never_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings = AppSettings {
            saves_folder: "/saves".into(),
            github_login: "octocat".into(),
            github_avatar: "https://example.invalid/a.png".into(),
        };
        write_settings(dir.path(), &settings).expect("write");

        let read_back = read_settings(dir.path());
        assert_eq!(read_back.github_login, "octocat");
        assert_eq!(read_back.saves_folder, "/saves");

        // The token belongs to Git's credential store, not to MineCommit's files.
        let raw = fs::read_to_string(settings_file_path(dir.path())).unwrap();
        assert!(!raw.contains("token"), "settings.json must not carry a token");
        assert!(!raw.contains("password"));
    }

    #[test]
    fn the_saves_folder_default_ends_at_the_saves_directory() {
        // The dashboard opens on this path, so a wrong one means an empty
        // "add a world" list on a fresh install.
        let folder = default_saves_folder();
        assert!(
            folder.is_empty() || folder.ends_with("saves"),
            "unexpected default saves folder: {folder}"
        );
    }
}
