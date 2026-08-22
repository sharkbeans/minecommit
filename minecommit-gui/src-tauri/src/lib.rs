use chrono::{DateTime, Local};
use log::{LevelFilter, Log, Metadata, Record};
use minecommit::{
    sync::{
        backup_message_with_device, current_device_name, lock_inactive_world, restore_commit,
        RemoteStatus, RemoteSync, DEFAULT_BRANCH,
    },
    utils::cmd::{git_cmd, git_command, git_count_objects, git_repack},
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
        if let Err(e) = git_repack(&git_dir_path) {
            log::warn!("Repack failed: {e}");
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

    // Spawn a blocking thread to periodically drain and emit captured logs
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            let logs = take_logs();
            for entry in &logs {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // Drain remaining logs after commit finishes
        let logs = take_logs();
        for entry in &logs {
            let _ = app_clone.emit("commit-log", entry);
        }
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

    // Spawn a blocking thread to periodically drain and emit captured logs
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            let logs = take_logs();
            for entry in &logs {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let logs = take_logs();
        for entry in &logs {
            let _ = app_clone.emit("commit-log", entry);
        }
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

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            let logs = take_logs();
            for entry in &logs {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let logs = take_logs();
        for entry in &logs {
            let _ = app_clone.emit("commit-log", entry);
        }
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

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            let logs = take_logs();
            for entry in &logs {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let logs = take_logs();
        for entry in &logs {
            let _ = app_clone.emit("commit-log", entry);
        }
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
    if state.saves.lock().unwrap().iter().any(|s| s.name == name) {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!("A save named \"{name}\" already exists")),
            save: None,
        });
    }

    let save_dir = PathBuf::from(&save_path);
    if save_dir.exists() {
        return Ok(CloneFromCloudResult {
            success: false,
            error: Some(format!(
                "{save_path} already exists. Choose a folder that does not exist yet, so an \
                 existing world cannot be overwritten."
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

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();
    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            for entry in &take_logs() {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for entry in &take_logs() {
            let _ = app_clone.emit("commit-log", entry);
        }
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
            let _ = fs::remove_dir_all(&repo_path);
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
        let last_played = fs::metadata(&level)
            .and_then(|meta| meta.modified())
            .ok()
            .map(|time| DateTime::<Local>::from(time).to_rfc3339());
        worlds.push(FoundWorld {
            name: entry.file_name().to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
            last_played,
        });
    }
    worlds.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    Ok(worlds)
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
    /// False while Minecraft still has the world open. This is the same
    /// session.lock check the backup performs, so the dashboard cannot promise
    /// something the button will then refuse to do.
    pub idle: bool,
    /// When level.dat was last written, ISO 8601. Compared against the newest
    /// backup to tell whether the world has been played since.
    pub last_played: Option<String>,
}

#[tauri::command]
fn world_state(save_dir: String) -> WorldState {
    let dir = Path::new(&save_dir);
    WorldState {
        idle: lock_inactive_world(dir).is_ok(),
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
}

fn settings_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

#[tauri::command]
fn get_saves_folder(state: tauri::State<AppState>) -> String {
    let stored = fs::read_to_string(settings_file_path(&state.data_dir))
        .ok()
        .and_then(|text| serde_json::from_str::<AppSettings>(&text).ok())
        .map(|settings| settings.saves_folder)
        .unwrap_or_default();

    if stored.trim().is_empty() {
        default_saves_folder()
    } else {
        stored
    }
}

#[tauri::command]
fn set_saves_folder(state: tauri::State<AppState>, folder: String) -> Result<(), AppError> {
    fs::create_dir_all(&state.data_dir)?;
    let settings = AppSettings {
        saves_folder: folder,
    };
    fs::write(
        settings_file_path(&state.data_dir),
        serde_json::to_string_pretty(&settings)?,
    )?;
    Ok(())
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

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let app_clone = app.clone();

    let log_task = tauri::async_runtime::spawn_blocking(move || {
        while running_clone.load(Ordering::Relaxed) {
            for entry in &take_logs() {
                let _ = app_clone.emit("commit-log", entry);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for entry in &take_logs() {
            let _ = app_clone.emit("commit-log", entry);
        }
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
            list_history,
            device_name,
            world_state,
            get_saves_folder,
            set_saves_folder,
            backup_and_upload,
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
