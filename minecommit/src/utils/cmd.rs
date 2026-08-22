use std::{
    ffi::OsStr,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use anyhow::{Context, Result};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// `git`, configured so it never flashes a console window.
///
/// `git.exe` is a console application, so on Windows every invocation from a
/// GUI process opens a black window for as long as it runs. A single backup
/// runs hundreds of them, which makes the app look like it has been taken over
/// by malware. On other platforms this is plain `Command::new("git")`.
pub fn git_command() -> Command {
    #[allow(unused_mut)] // `creation_flags` is the only mutation, and only on Windows.
    let mut cmd = Command::new("git");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

pub fn exec(mut cmd: Command, stdin: Option<String>) -> Result<String> {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    log::debug!("command: {:?}", cmd);
    let out = if let Some(stdin) = stdin {
        for line in stdin.lines() {
            log::trace!("stdin: {line:?}");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to run command {cmd:?}"))?;
        child
            .stdin
            .as_mut()
            .with_context(|| format!("failed to get stdin handle to command {cmd:?}"))?
            .write_all(stdin.as_bytes())
            .with_context(|| format!("failed to write stdin to command {cmd:?}"))?;
        child
            .wait_with_output()
            .with_context(|| format!("failed to wait command {cmd:?}"))?
    } else {
        cmd.output()
            .with_context(|| format!("failed to read stdout from command {cmd:?}"))?
    };
    let stderr = String::from_utf8(out.stderr)
        .with_context(|| format!("failed to encoding stderr by UTF-8"))?;
    for line in stderr.lines() {
        log::debug!("stderr: {line:?}");
    }
    let stdout = String::from_utf8(out.stdout)
        .with_context(|| format!("failed to encoding stdout by UTF-8"))?;
    for line in stdout.lines() {
        log::trace!("stdout: {line:?}");
    }
    anyhow::ensure!(
        out.status.success(),
        "command {cmd:?} failed: {}",
        stderr.trim()
    );
    Ok(stdout)
}

pub fn git_cmd(
    git_dir: impl AsRef<OsStr>,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Command {
    let mut cmd = git_command();
    cmd.arg("--git-dir").arg(git_dir);
    for arg in args {
        cmd.arg(arg);
    }
    cmd
}

pub fn git_repo_exists(git_dir: &str) -> Result<PathBuf> {
    let git_dir = PathBuf::from(git_dir);
    let cmd = git_cmd(&git_dir, ["rev-parse", "--is-bare-repository"]);
    let _ = exec(cmd, None)?;
    Ok(git_dir)
}

pub struct RepoStats {
    pub count: u64,
    pub size_mib: f64,
    pub in_pack: u64,
    pub packs: u64,
    pub size_pack_mib: f64,
    pub prune_packable: u64,
    pub garbage: u64,
    pub size_garbage_mib: f64,
}

impl RepoStats {
    pub fn total_size_mib(&self) -> f64 {
        self.size_mib + self.size_pack_mib + self.size_garbage_mib
    }
}

pub fn git_count_objects(git_dir: impl AsRef<OsStr>) -> Result<RepoStats> {
    let cmd = git_cmd(git_dir, ["count-objects", "-v"]);
    let result = exec(cmd, None)?;

    let mut stats = RepoStats {
        count: 0,
        size_mib: 0.0,
        in_pack: 0,
        packs: 0,
        size_pack_mib: 0.0,
        prune_packable: 0,
        garbage: 0,
        size_garbage_mib: 0.0,
    };

    for line in result.lines() {
        if let Some((key, val)) = line.split_once(": ") {
            let val = val.trim();
            match key {
                "count" => {
                    stats.count = val.parse().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'count': {e}");
                        0
                    })
                }
                "size" => {
                    stats.size_mib = val.parse::<f64>().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'size': {e}");
                        0.0
                    }) / 1024.0
                }
                "in-pack" => {
                    stats.in_pack = val.parse().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'in-pack': {e}");
                        0
                    })
                }
                "packs" => {
                    stats.packs = val.parse().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'packs': {e}");
                        0
                    })
                }
                "size-pack" => {
                    stats.size_pack_mib = val.parse::<f64>().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'size-pack': {e}");
                        0.0
                    }) / 1024.0
                }
                "prune-packable" => {
                    stats.prune_packable = val.parse().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'prune-packable': {e}");
                        0
                    })
                }
                "garbage" => {
                    stats.garbage = val.parse().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'garbage': {e}");
                        0
                    })
                }
                "size-garbage" => {
                    stats.size_garbage_mib = val.parse::<f64>().unwrap_or_else(|e| {
                        log::warn!("Failed to parse git count-objects field 'size-garbage': {e}");
                        0.0
                    }) / 1024.0
                }
                _ => {}
            }
        }
    }

    Ok(stats)
}

/// How many loose objects justify paying for a repack.
///
/// A full repack of a multi-gigabyte world takes many minutes, and running one
/// after every backup spends all of it on almost nothing: an evening of play
/// adds a few hundred objects to a pack holding hundreds of thousands. This is
/// Git's own `gc.auto` threshold, so loose objects are still folded in
/// regularly without charging for it every time.
pub const LOOSE_OBJECTS_BEFORE_REPACK: u64 = 6700;

/// Whether repacking is worth its cost right now.
pub fn repack_is_worthwhile(stats: &RepoStats) -> bool {
    // Nothing packed yet: the first repack is what makes a world small, and
    // there is no cheaper moment to do it.
    stats.packs == 0 || stats.count >= LOOSE_OBJECTS_BEFORE_REPACK
}

pub fn git_repack(git_dir: impl AsRef<OsStr>) -> Result<()> {
    log::info!("Repacking");
    let git_dir = git_dir.as_ref();

    // `--path-walk` groups objects by path and produces far better deltas for
    // Minecraft region files, but it only exists in recent Git builds. Retry
    // without it so repacking still works on the Git shipped by most distros.
    match exec_with_heartbeat(repack_cmd(git_dir, true), git_dir) {
        Ok(()) => Ok(()),
        Err(error) if rejected_unknown_option(&error, "path-walk") => {
            log::info!("This Git build does not support `--path-walk`; repacking without it");
            exec_with_heartbeat(repack_cmd(git_dir, false), git_dir)
        }
        Err(error) => Err(error),
    }
}

/// Run a repack, reporting that it is still going.
///
/// Git only draws progress when its stderr is a terminal, and MineCommit pipes
/// it, so a repack of a large world printed nothing at all for however many
/// minutes it took -- indistinguishable from a hang. Git's own progress cannot
/// be turned back on from here, so the growing pack it is writing is reported
/// instead, which is the thing worth watching anyway.
fn exec_with_heartbeat(mut cmd: Command, git_dir: &OsStr) -> Result<()> {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    log::debug!("command: {:?}", cmd);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run command {cmd:?}"))?;

    let pack_dir = PathBuf::from(git_dir).join("objects").join("pack");
    let started = std::time::Instant::now();
    let mut next_report = std::time::Duration::from_secs(10);

    loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("failed to wait command {cmd:?}"))?
        {
            let output = child
                .wait_with_output()
                .with_context(|| format!("failed to read output of {cmd:?}"))?;
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            for line in stderr.lines() {
                log::debug!("stderr: {line:?}");
            }
            anyhow::ensure!(status.success(), "command {cmd:?} failed: {stderr}");
            log::info!("Repacking finished in {}s", started.elapsed().as_secs());
            return Ok(());
        }

        std::thread::sleep(std::time::Duration::from_millis(250));
        if started.elapsed() >= next_report {
            next_report += std::time::Duration::from_secs(10);
            match in_progress_pack_mib(&pack_dir) {
                Some(written) => log::info!(
                    "Still repacking after {}s, {written:.0} MiB written so far",
                    started.elapsed().as_secs()
                ),
                None => log::info!("Still repacking after {}s", started.elapsed().as_secs()),
            }
        }
    }
}

/// Total size of the temporary packs Git is currently writing, in MiB.
fn in_progress_pack_mib(pack_dir: &Path) -> Option<f64> {
    let entries = std::fs::read_dir(pack_dir).ok()?;
    let bytes: u64 = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("tmp_pack_"))
        })
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();
    (bytes > 0).then(|| bytes as f64 / 1024.0 / 1024.0)
}

fn repack_cmd(git_dir: &OsStr, path_walk: bool) -> Command {
    let mut args = vec![
        "-c",
        "pack.deltaCacheLimit=65535",
        "-c",
        "pack.deltaCacheSize=1073741824", // 1GiB
        "repack",
        "--depth=4095",
        "--window=2",
        "-a",
        "-d",
        "-f",
    ];
    if path_walk {
        args.push("--path-walk");
    }
    git_cmd(git_dir, args)
}

/// Detect the `error: unknown option \`<name>'` that older Git versions print
/// when they are given a flag they do not know about.
fn rejected_unknown_option(error: &anyhow::Error, option: &str) -> bool {
    let message = format!("{error:#}");
    message.contains("unknown option") && message.contains(option)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    /// Every Git invocation must be built by [`git_command`], directly or via
    /// [`git_cmd`], or Windows opens a console window for it. A GUI running
    /// hundreds of them per backup then looks like it is being taken over by
    /// malware.
    ///
    /// There is no way to observe a console window from a test, and the bug is
    /// invisible on Linux, so guard the source instead. Test code may spawn
    /// Git however it likes, so scanning stops at the file's `mod tests`, which
    /// is the last item in every file in this workspace.
    #[test]
    fn no_git_is_spawned_outside_git_command() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate directory has a parent");

        // This file defines `git_command`, so it is the one place allowed to
        // name the raw constructor.
        let definition = workspace.join("minecommit/src/utils/cmd.rs");

        let mut offenders = Vec::new();
        visit(workspace, &mut |file| {
            if file == definition {
                return;
            }
            let source = fs::read_to_string(file).unwrap_or_default();
            for (index, line) in source.lines().enumerate() {
                if line.trim_start().starts_with("mod tests") {
                    break;
                }
                if line.contains(r#"Command::new("git")"#) {
                    offenders.push(format!("{}:{}", file.display(), index + 1));
                }
            }
        });

        assert!(
            offenders.is_empty(),
            "these spawn Git directly instead of through `git_command()`, \
             which flashes a console window on Windows:\n  {}",
            offenders.join("\n  ")
        );
    }

    fn visit(dir: &Path, found: &mut impl FnMut(&Path)) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !matches!(path.file_name().and_then(|n| n.to_str()), Some("target" | "node_modules" | ".git")) {
                    visit(&path, found);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found(&path);
            }
        }
    }
}

#[cfg(test)]
mod repack_policy_tests {
    use super::*;

    fn stats(loose: u64, packs: u64) -> RepoStats {
        RepoStats {
            count: loose,
            size_mib: 0.0,
            in_pack: 0,
            packs,
            size_pack_mib: 0.0,
            prune_packable: 0,
            garbage: 0,
            size_garbage_mib: 0.0,
        }
    }

    #[test]
    fn the_first_backup_always_repacks() {
        // Nothing is packed yet, and this is the repack that makes a world
        // small enough to be worth uploading.
        assert!(repack_is_worthwhile(&stats(0, 0)));
        assert!(repack_is_worthwhile(&stats(500_000, 0)));
    }

    #[test]
    fn an_evening_of_play_does_not_pay_for_a_full_repack() {
        // A few hundred new objects against a pack holding hundreds of
        // thousands: repacking here spent minutes to save almost nothing, and
        // looked like a hang while it did.
        assert!(!repack_is_worthwhile(&stats(400, 1)));
        assert!(!repack_is_worthwhile(&stats(LOOSE_OBJECTS_BEFORE_REPACK - 1, 1)));
    }

    #[test]
    fn enough_loose_objects_earn_a_repack() {
        assert!(repack_is_worthwhile(&stats(LOOSE_OBJECTS_BEFORE_REPACK, 1)));
    }
}
