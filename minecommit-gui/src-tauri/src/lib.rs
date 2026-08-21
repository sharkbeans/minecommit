use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record};
use minecommit::{
    sync::{
        backup_message_with_device, lock_inactive_world, restore_commit, RemoteStatus, RemoteSync,
    },
    utils::cmd::{git_cmd, git_count_objects, git_repack},
    Config,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
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
            let _ = Command::new("git")
                .args(["config", "--global", "user.name", &author_name])
                .output();
        }
        if !author_email.is_empty() {
            let _ = Command::new("git")
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

        let commit = format!("refs/heads/{branch}");
        log::info!("Restoring save from '{branch}' using staged checkout...");

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
    let output = Command::new("git")
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

    let output = Command::new("git")
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
    let name = Command::new("git")
        .args(["config", "--global", "user.name"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let email = Command::new("git")
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
    Command::new("git")
        .args(["config", "--global", "user.name", &name])
        .output()
        .map_err(|e| AppError::Io(e))?;
    Command::new("git")
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
    let output = Command::new("git")
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
