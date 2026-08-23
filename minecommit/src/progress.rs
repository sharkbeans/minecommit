//! How far along a long operation is.
//!
//! A first backup of a large world reads and rewrites hundreds of thousands of
//! files, and until now the only sign of life was a log line scrolling past.
//! That is enough to know something is happening and not enough to know whether
//! to wait or give up, which is the question a player actually has.
//!
//! The count lives in globals rather than being threaded through every handler.
//! MineCommit runs one backup at a time -- the GUI serialises them behind a
//! single button, and the CLI is one command -- and the alternative is a
//! reporter parameter on every `flatten` and `unflatten` signature purely so a
//! progress bar can move. Log capture in the GUI is already global for the same
//! reason.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether anyone asked to be told about progress.
///
/// Without this every file the library touches would be counted, including in
/// unrelated work and in tests that never started a run, and the count would be
/// whatever the last thing to run happened to leave behind.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TOTAL: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);

/// Start counting a run of `total` files. A total of zero still counts, for
/// work whose size is not known ahead of time.
pub fn begin(total: u64) {
    DONE.store(0, Ordering::Relaxed);
    TOTAL.store(total, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Stop counting, so a finished run does not leave a stale bar behind.
pub fn end() {
    ACTIVE.store(false, Ordering::Relaxed);
    TOTAL.store(0, Ordering::Relaxed);
    DONE.store(0, Ordering::Relaxed);
}

/// Record that `files` more have been handled. Ignored when no run is counting.
pub fn advance(files: u64) {
    if ACTIVE.load(Ordering::Relaxed) {
        DONE.fetch_add(files, Ordering::Relaxed);
    }
}

/// `(done, total)`. A total of zero means nothing is being counted, which is
/// the caller's cue to show no bar rather than an empty one.
pub fn snapshot() -> (u64, u64) {
    (DONE.load(Ordering::Relaxed), TOTAL.load(Ordering::Relaxed))
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
    /// too. Totals are asserted exactly -- only `begin` and `end` move those,
    /// and every test that calls them takes `TEST_LOCK` -- while the count of
    /// files is asserted as "at least", which is the part parallel work can
    /// only ever inflate.
    #[test]
    fn counting_runs_only_between_begin_and_end() {
        use crate::odb::{LocalFsOdb, OdbReader, OdbWriter};

        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let dir = tempfile::tempdir().expect("tempdir");
        let mut odb = LocalFsOdb::from_dir(dir.path().to_path_buf());

        // Nothing has begun, so nothing is counted. This is what stops the rest
        // of the suite from dragging the counter around between runs.
        advance(7);
        odb.put("before.dat", b"x".to_vec()).expect("write");
        assert_eq!(snapshot(), (0, 0), "no run, no counting");

        begin(10);
        assert_eq!(snapshot(), (0, 10), "a run starts from nothing");

        advance(3);
        assert!(snapshot().0 >= 3);

        // A second run must not inherit the first one's progress.
        begin(4);
        assert_eq!(snapshot().1, 4);
        assert!(snapshot().0 < 4, "a fresh run starts near zero");

        // Reading and writing a save file is what the bar counts, and it has to
        // happen here rather than in each handler.
        let before = snapshot().0;
        odb.put("a.dat", b"one".to_vec()).expect("write a");
        odb.put("b.dat", b"two".to_vec()).expect("write b");
        assert!(snapshot().0 >= before + 2, "each write counts");

        let before = snapshot().0;
        odb.get("a.dat").expect("read a");
        // get_par reads through get, so the parallel readers are covered too.
        odb.get_par(&["a.dat", "b.dat"]).expect("read both");
        assert!(snapshot().0 >= before + 3, "each read counts");

        end();
        advance(4);
        odb.put("after.dat", b"y".to_vec()).expect("write");
        assert_eq!(
            snapshot(),
            (0, 0),
            "a total of zero is what tells the GUI to stop drawing a bar"
        );
    }

}
