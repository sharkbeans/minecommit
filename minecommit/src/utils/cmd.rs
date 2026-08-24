use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::progress::{self, Phase};

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


// ─── Watching a transfer ────────────────────────────────────────────────────

/// What a finished command left behind.
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// One reading of Git's own transfer progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Transfer {
    phase: Phase,
    objects_done: u64,
    objects_total: u64,
    /// Bytes moved so far. Git does not print this until the transfer starts.
    bytes: u64,
}

/// Run a Git command that moves data over the network, reporting how far it
/// has got.
///
/// Uploading a first backup of a large world is the longest single wait
/// MineCommit imposes, and it happens on somebody's home upload speed. Git
/// knows exactly how it is going, but only draws progress when its stderr is a
/// terminal, and MineCommit pipes it -- hence `--progress` at the call sites
/// and this reader, which follows the carriage returns Git redraws its counter
/// with.
///
/// Git reports how much has moved but never how much is still to come, so only
/// the object counts give a fraction; the byte figure is a running total.
pub fn exec_watching_transfer(mut cmd: Command) -> Result<CommandOutput> {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    log::debug!("command: {:?}", cmd);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run command {cmd:?}"))?;

    // Drained on its own thread: a command whose stdout fills the pipe buffer
    // blocks forever while this thread waits on stderr.
    let mut out = child
        .stdout
        .take()
        .with_context(|| format!("failed to get stdout handle to command {cmd:?}"))?;
    let collect_stdout = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = out.read_to_end(&mut buffer);
        buffer
    });

    let mut err = child
        .stderr
        .take()
        .with_context(|| format!("failed to get stderr handle to command {cmd:?}"))?;

    let mut stderr = Vec::new();
    let mut line = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = match err.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("failed to read Git's progress output"),
        };
        stderr.extend_from_slice(&chunk[..read]);
        for byte in &chunk[..read] {
            // Git redraws its counter in place with a carriage return, so
            // waiting for newlines would report nothing until each phase ended.
            if *byte == b'\r' || *byte == b'\n' {
                report_transfer(&String::from_utf8_lossy(&line));
                line.clear();
            } else {
                line.push(*byte);
            }
        }
    }
    report_transfer(&String::from_utf8_lossy(&line));

    let status = child
        .wait()
        .with_context(|| format!("failed to wait command {cmd:?}"))?;
    let stdout = collect_stdout
        .join()
        .map_err(|_| anyhow::anyhow!("failed to read stdout from command {cmd:?}"))?;

    // The phase this command opened closes with it. Nothing else sets these two,
    // so a transfer phase still standing here is one this call started -- and
    // leaving it standing would keep a finished transfer on screen as a live
    // one, and hand the next thing to run a bar it never asked for.
    if matches!(progress::report().phase, Phase::Downloading | Phase::Uploading) {
        progress::end();
    }

    let stderr = String::from_utf8(stderr).context("Git emitted non-UTF-8 stderr")?;
    for text in stderr.split(['\r', '\n']).filter(|text| !text.is_empty()) {
        log::debug!("stderr: {text:?}");
    }

    Ok(CommandOutput {
        success: status.success(),
        stdout: String::from_utf8(stdout).context("Git emitted non-UTF-8 stdout")?,
        stderr,
    })
}

/// Feed one redrawn progress line into the counters.
fn report_transfer(line: &str) {
    // Not every fetch is one the player is waiting on: checking whether the
    // cloud has anything new is a fetch as well, and so is the one a push does
    // before it uploads. Only a job somebody started draws a bar.
    if !progress::job_running() {
        return;
    }
    let Some(transfer) = parse_transfer(line) else {
        return;
    };
    // Entering a phase resets the counters, so only do it on the first line of
    // one: a push fetches, uploads and fetches again, and restarting the bar on
    // every reading would leave it stuck at zero.
    if progress::report().phase != transfer.phase {
        progress::begin(transfer.phase, transfer.objects_total, 0);
    }
    progress::set(transfer.objects_done, transfer.objects_total, transfer.bytes);
}

/// Read one of Git's transfer counters.
///
/// The lines look like `Receiving objects:  45% (450/1000), 12.50 MiB | 3.21
/// MiB/s`. Only the two that move data over the network are of interest: the
/// server's own `remote: Compressing objects` lines describe work MineCommit
/// is only waiting on, and counting them would run the bar to full twice.
fn parse_transfer(line: &str) -> Option<Transfer> {
    let line = line.trim();
    let (phase, rest) = if let Some(rest) = line.strip_prefix("Receiving objects:") {
        (Phase::Downloading, rest)
    } else if let Some(rest) = line.strip_prefix("Writing objects:") {
        (Phase::Uploading, rest)
    } else {
        return None;
    };

    let counts = rest.split_once('(')?.1;
    let (counts, tail) = counts.split_once(')')?;
    let (done, total) = counts.split_once('/')?;

    Some(Transfer {
        phase,
        objects_done: done.trim().parse().ok()?,
        objects_total: total.trim().parse().ok()?,
        // Absent until the transfer proper starts, and followed by a rate after
        // a pipe that must not be mistaken for the amount moved.
        bytes: parse_size(tail.split('|').next().unwrap_or_default()).unwrap_or(0),
    })
}

/// Read `12.50 MiB` as a number of bytes.
fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim().trim_start_matches(',').trim();
    let (amount, unit) = text.split_once(' ')?;
    let amount: f64 = amount.trim().parse().ok()?;
    let scale: f64 = match unit.trim() {
        "B" | "bytes" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((amount * scale) as u64)
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

#[cfg(test)]
mod transfer_tests {
    use super::*;

    /// The real thing, copied from `git push --progress` and `git fetch
    /// --progress` against GitHub. Everything the bar shows is read out of
    /// these lines, so an unnoticed change in their shape would leave the bar
    /// at zero for the whole upload.
    #[test]
    fn gits_own_counters_are_read_back_as_objects_and_bytes() {
        let receiving =
            parse_transfer("Receiving objects:  45% (450/1000), 12.50 MiB | 3.21 MiB/s")
                .expect("a download reading");
        assert_eq!(receiving.phase, Phase::Downloading);
        assert_eq!((receiving.objects_done, receiving.objects_total), (450, 1000));
        assert_eq!(receiving.bytes, (12.5 * 1024.0 * 1024.0) as u64);

        let writing = parse_transfer("Writing objects:  33% (33/100), 900.00 KiB | 1.10 MiB/s")
            .expect("an upload reading");
        assert_eq!(writing.phase, Phase::Uploading);
        assert_eq!((writing.objects_done, writing.objects_total), (33, 100));
        assert_eq!(writing.bytes, 900 * 1024);

        // The last line of a phase carries a trailing ", done." after the rate.
        let finished =
            parse_transfer("Receiving objects: 100% (1000/1000), 2.50 GiB | 8.00 MiB/s, done.")
                .expect("the closing reading");
        assert_eq!(finished.objects_done, 1000);
        assert_eq!(finished.bytes, (2.5 * 1024.0 * 1024.0 * 1024.0) as u64);

        // Before any data moves Git prints the counts alone.
        let starting = parse_transfer("Receiving objects:   0% (1/1000)").expect("a first reading");
        assert_eq!(starting.bytes, 0, "no figure yet is zero, not a guess");
    }

    /// A transfer bar that also counted the server's compression would fill up,
    /// reset and fill again, which reads as the upload having restarted.
    #[test]
    fn only_the_two_counters_that_move_data_are_followed() {
        for line in [
            "remote: Counting objects: 100% (100/100), done.",
            "remote: Compressing objects:  50% (25/50)",
            "Resolving deltas:  75% (15/20)",
            "Enumerating objects: 100, done.",
            "Delta compression using up to 8 threads",
            "",
            "To https://github.com/someone/world.git",
        ] {
            assert!(
                parse_transfer(line).is_none(),
                "{line:?} is not data crossing the network"
            );
        }
    }

    #[test]
    fn a_rate_is_never_mistaken_for_an_amount() {
        // "3.21 MiB/s" sits right after the amount, and reading it instead
        // would show a download of a few megabytes as finished immediately.
        let reading = parse_transfer("Receiving objects:  10% (100/1000), 1.00 KiB | 3.21 MiB/s")
            .expect("a reading");
        assert_eq!(reading.bytes, 1024);
        assert_eq!(parse_size("3.21 MiB/s"), None, "a rate has no place in a total");
    }

    /// Not every fetch is one somebody is waiting on. Checking whether the
    /// cloud has anything new is a fetch, and so is the one a push runs before
    /// it uploads, and neither should put a bar on screen.
    ///
    /// The gate cannot be seen from outside: whatever it lets through is ended
    /// again when the command finishes, so a caller watching the counters
    /// afterwards sees the same stillness either way. It can only be caught
    /// mid-transfer, which is a race, so the source is checked instead -- the
    /// same way this file already guards how Git is spawned.
    #[test]
    fn a_transfer_nobody_asked_about_moves_nothing() {
        // Normalised first: Windows checks this repository out with CRLF line
        // endings, and a scan looking for "\n}\n" finds nothing there. A guard
        // that quietly matches nothing on one platform is worse than no guard.
        let source = include_str!("cmd.rs").replace("\r\n", "\n");
        let start = source
            .find("fn report_transfer(")
            .expect("report_transfer must exist");
        let body = &source[start..];
        let end = body.find("\n}\n").expect("report_transfer must end");
        let body = &body[..end];

        let gate = body
            .find("job_running()")
            .expect("report_transfer must ask whether anyone is watching");
        for touches in ["progress::begin", "progress::set"] {
            let at = body
                .find(touches)
                .unwrap_or_else(|| panic!("report_transfer no longer calls {touches}"));
            assert!(
                gate < at,
                "{touches} runs before the check for whether anyone asked to watch, \
                 so an ordinary background fetch would draw a bar:\n{body}"
            );
        }
    }

    #[test]
    fn sizes_are_read_in_the_units_git_prints() {
        assert_eq!(parse_size("512 B"), Some(512));
        assert_eq!(parse_size(", 4.00 KiB "), Some(4096));
        assert_eq!(parse_size("1.50 MiB"), Some(1024 * 1024 + 512 * 1024));
        assert_eq!(parse_size("done."), None);
        assert_eq!(parse_size(""), None);
    }
}
