//! How far along a long operation is.
//!
//! A first backup of a large world reads and rewrites hundreds of thousands of
//! files, and until now the only sign of life was a log line scrolling past.
//! That is enough to know something is happening and not enough to know whether
//! to wait or give up, which is the question a player actually has.
//!
//! Both a file count and a byte count are kept. Files are what the work is made
//! of, but they are wildly uneven -- a world is a few hundred region files of
//! several megabytes each next to a few thousand tiny ones -- so a bar driven by
//! the file count races and stalls. Bytes move smoothly, and "351 / 1000 MB" is
//! a size a player can compare against their own idea of how big the world is.
//!
//! The count lives in globals rather than being threaded through every handler.
//! MineCommit runs one backup at a time -- the GUI serialises them behind a
//! single button, and the CLI is one command -- and the alternative is a
//! reporter parameter on every `flatten` and `unflatten` signature purely so a
//! progress bar can move. Log capture in the GUI is already global for the same
//! reason.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// What the running work is currently doing.
///
/// The four phases read very differently to someone watching: reading a world
/// off the disk is fast and local, uploading is slow and depends on their
/// connection. Naming the phase is most of what makes a long wait bearable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing is being counted, which is the caller's cue to show no bar.
    Idle,
    /// Reading the world folder, on the way to a backup.
    Reading,
    /// Writing the world folder back out, during a restore.
    Writing,
    /// Receiving a backup from the cloud.
    Downloading,
    /// Sending a backup to the cloud.
    Uploading,
}

impl Phase {
    /// The name the GUI switches on. Kept in step with `Phase` in `cloud.ts`.
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Reading => "reading",
            Phase::Writing => "writing",
            Phase::Downloading => "downloading",
            Phase::Uploading => "uploading",
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Phase::Reading,
            2 => Phase::Writing,
            3 => Phase::Downloading,
            4 => Phase::Uploading,
            _ => Phase::Idle,
        }
    }

    fn code(self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::Reading => 1,
            Phase::Writing => 2,
            Phase::Downloading => 3,
            Phase::Uploading => 4,
        }
    }
}

/// Which phase is running, or `Idle` when nobody asked to be told about
/// progress.
///
/// Without this gate every file the library touches would be counted, including
/// in unrelated work and in tests that never started a run, and the count would
/// be whatever the last thing to run happened to leave behind.
static PHASE: AtomicU8 = AtomicU8::new(0);
static FILES_TOTAL: AtomicU64 = AtomicU64::new(0);
static FILES_DONE: AtomicU64 = AtomicU64::new(0);
static BYTES_TOTAL: AtomicU64 = AtomicU64::new(0);
static BYTES_DONE: AtomicU64 = AtomicU64::new(0);

/// Everything known about the running operation at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub phase: Phase,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    /// Zero when the size of the job cannot be known ahead of time, which is
    /// the case for a network transfer: Git says how much has arrived but never
    /// how much is coming.
    pub bytes_total: u64,
}

impl Report {
    /// Whether there is anything worth drawing.
    pub fn running(&self) -> bool {
        self.phase != Phase::Idle
    }
}

/// Start counting a run of `files` files totalling `bytes`. Either total may be
/// zero, for work whose size is not known ahead of time.
pub fn begin(phase: Phase, files: u64, bytes: u64) {
    FILES_DONE.store(0, Ordering::Relaxed);
    BYTES_DONE.store(0, Ordering::Relaxed);
    FILES_TOTAL.store(files, Ordering::Relaxed);
    BYTES_TOTAL.store(bytes, Ordering::Relaxed);
    // Last, so a reader never sees a live phase against the previous run's
    // totals.
    PHASE.store(phase.code(), Ordering::Relaxed);
}

/// Stop counting, so a finished run does not leave a stale bar behind.
pub fn end() {
    PHASE.store(Phase::Idle.code(), Ordering::Relaxed);
    FILES_TOTAL.store(0, Ordering::Relaxed);
    FILES_DONE.store(0, Ordering::Relaxed);
    BYTES_TOTAL.store(0, Ordering::Relaxed);
    BYTES_DONE.store(0, Ordering::Relaxed);
}

/// Record that `files` more files, holding `bytes` between them, have been
/// handled. Ignored when no run is counting.
pub fn advance(files: u64, bytes: u64) {
    if PHASE.load(Ordering::Relaxed) != 0 {
        FILES_DONE.fetch_add(files, Ordering::Relaxed);
        BYTES_DONE.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Record an absolute position rather than an increment.
///
/// Git reports a transfer as a running total rather than as deltas, and it
/// revises the total object count as it goes, so both numbers are replaced
/// wholesale. Ignored when no run is counting.
pub fn set(files_done: u64, files_total: u64, bytes_done: u64) {
    if PHASE.load(Ordering::Relaxed) != 0 {
        FILES_DONE.store(files_done, Ordering::Relaxed);
        FILES_TOTAL.store(files_total, Ordering::Relaxed);
        BYTES_DONE.store(bytes_done, Ordering::Relaxed);
    }
}

/// Where the running operation has got to.
pub fn report() -> Report {
    Report {
        phase: Phase::from_code(PHASE.load(Ordering::Relaxed)),
        files_done: FILES_DONE.load(Ordering::Relaxed),
        files_total: FILES_TOTAL.load(Ordering::Relaxed),
        bytes_done: BYTES_DONE.load(Ordering::Relaxed),
        bytes_total: BYTES_TOTAL.load(Ordering::Relaxed),
    }
}

/// Held by any test that runs, or inspects, a counted operation.
///
/// The counters are process-wide by design, and Rust runs tests in parallel, so
/// a test asserting on them and a test backing up a world would otherwise read
/// each other's numbers.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// Counting is process-wide, and Rust runs tests in parallel, so while this
    /// test has counting switched on any other test writing a file is counted
    /// too. Totals are asserted exactly -- only `begin`, `end` and `set` move
    /// those, and every test that calls them takes `TEST_LOCK` -- while the
    /// counts of work done are asserted as "at least", which is the part
    /// parallel work can only ever inflate.
    #[test]
    fn counting_runs_only_between_begin_and_end() {
        use crate::odb::{LocalFsOdb, OdbReader, OdbWriter};

        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let dir = tempfile::tempdir().expect("tempdir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());

        // Nothing has begun, so nothing is counted. This is what stops the rest
        // of the suite from dragging the counter around between runs.
        advance(7, 700);
        odb.put("before.dat", b"x".to_vec()).expect("write");
        assert!(!report().running(), "no run, no counting");
        assert_eq!(report().files_done, 0);
        assert_eq!(report().bytes_done, 0);

        begin(Phase::Reading, 10, 1_000);
        let started = report();
        assert_eq!(started.phase, Phase::Reading);
        assert_eq!((started.files_done, started.bytes_done), (0, 0));
        assert_eq!((started.files_total, started.bytes_total), (10, 1_000));

        advance(3, 300);
        assert!(report().files_done >= 3);
        assert!(report().bytes_done >= 300);

        // A second run must not inherit the first one's progress.
        begin(Phase::Writing, 4, 40);
        let fresh = report();
        assert_eq!((fresh.files_total, fresh.bytes_total), (4, 40));
        assert!(fresh.files_done < 4, "a fresh run starts near zero");
        assert!(fresh.bytes_done < 40);

        // Reading and writing a save file is what the bar counts, and it has to
        // happen here rather than in each handler.
        let before = report();
        odb.put("a.dat", b"one".to_vec()).expect("write a");
        odb.put("b.dat", b"two".to_vec()).expect("write b");
        assert!(report().files_done >= before.files_done + 2, "each write counts");
        assert!(
            report().bytes_done >= before.bytes_done + 6,
            "and the bytes it wrote count with it"
        );

        let before = report();
        odb.get("a.dat").expect("read a");
        // get_par reads through get, so the parallel readers are covered too.
        odb.get_par(&["a.dat", "b.dat"]).expect("read both");
        assert!(report().files_done >= before.files_done + 3, "each read counts");
        assert!(report().bytes_done >= before.bytes_done + 9);

        end();
        advance(4, 400);
        odb.put("after.dat", b"y".to_vec()).expect("write");
        let stopped = report();
        assert!(
            !stopped.running(),
            "an idle phase is what tells the GUI to stop drawing a bar"
        );
        assert_eq!((stopped.files_done, stopped.bytes_done), (0, 0));
        assert_eq!((stopped.files_total, stopped.bytes_total), (0, 0));
    }

    /// Git counts a transfer up from zero and revises its own object total as
    /// it discovers more, so those numbers are set outright rather than added.
    #[test]
    fn a_transfer_reports_where_it_is_rather_than_how_far_it_moved() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        set(5, 10, 500);
        assert!(!report().running(), "nothing began, so nothing is recorded");

        begin(Phase::Downloading, 0, 0);
        set(450, 1_000, 12_500_000);
        let mid = report();
        assert_eq!(mid.phase, Phase::Downloading);
        assert_eq!((mid.files_done, mid.files_total), (450, 1_000));
        assert_eq!(mid.bytes_done, 12_500_000);
        assert_eq!(
            mid.bytes_total, 0,
            "Git never says how much is still coming, and guessing is the GUI's business"
        );

        // A later reading replaces the earlier one instead of stacking on it.
        set(900, 1_000, 25_000_000);
        assert_eq!(report().files_done, 900);
        assert_eq!(report().bytes_done, 25_000_000);

        end();
    }
}
