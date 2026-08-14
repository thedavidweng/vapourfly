//! Background job runner abstraction.
//!
//! Provides typed job slots with request IDs and stale-result protection.
//!
//! ## Problem
//!
//! The previous design used bare `static Mutex<Option<Result<T, E>>>` channels
//! for each background job type. If a user triggered a new job while an old
//! one was still running, the old (stale) result could arrive first and
//! prematurely clear the loading flag, while the current result was never
//! consumed.
//!
//! ## Solution
//!
//! Each job slot stores its result tagged with a [`JobTicket`] — a monotonically
//! increasing [`JobId`] **plus** the [`WorkflowKind`] and a 64-bit input
//! fingerprint captured at job start. The caller records the expected ticket
//! when starting a job. When polling, [`JobSlot::take_if`] only returns a result
//! whose stored ticket matches the expected one on **all three fields** (id,
//! kind, fingerprint), so a result computed for different inputs (e.g. the user
//! changed Deck mode mid-job) is discarded even if its id is current.
//!
//! ### Race safety
//!
//! If two jobs race — the newer job completes first, then the older job
//! completes — [`JobSlot::set`] compares the incoming [`JobId`] with the
//! already-stored one and **keeps the larger ID**, discarding the stale
//! older result. This prevents an old result from overwriting a newer one
//! before the UI thread can consume it.
//!
//! ```
//! use vapourfly_gui::jobs::{JobRunner, JobSlot, WorkflowKind};
//!
//! let slot = JobSlot::<u32>::new();
//! let mut runner = JobRunner::new();
//!
//! // Start job #1, then supersede it with job #2.
//! let t1 = runner.next_ticket(WorkflowKind::Scan, "fingerprint-a");
//! let t2 = runner.next_ticket(WorkflowKind::Scan, "fingerprint-a");
//! slot.set(t1, Ok(100));      // stale result from job #1
//! assert!(slot.take_if(t2).is_none()); // discarded — ticket mismatch
//! slot.set(t2, Ok(200));
//! assert_eq!(slot.take_if(t2), Some(Ok(200)));
//! ```

use std::sync::{Arc, Condvar, Mutex};

/// Identifies the kind of background workflow.
///
/// Used together with an input fingerprint to detect when a new job's
/// inputs differ from a previous job's, even if the global ID is higher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkflowKind {
    Scan,
    Write,
    DryRun,
    CacheRefresh,
    Prepare,
    JunkPreview,
    RecommendPreview,
    Discover,
    Dynamic,
    Mood,
    PlaylistMatch,
}

/// Monotonic request ID for a background job.
///
/// Each time the user triggers a job (scan, write, cache refresh, dry-run),
/// the runner allocates a new id via [`JobRunner::next_ticket`]. The spawned
/// thread tags its result with the [`JobTicket`] carrying this id, and the UI
/// poll compares the full ticket before applying the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// The complete identity of a background job: monotonic [`JobId`] + workflow
/// [`WorkflowKind`] + input `fingerprint`.
///
/// All three fields are compared when polling a [`JobSlot`], so a result
/// computed for different inputs (e.g. the user changed Deck mode, Junk mode,
/// or Playlist content while the job was running) is discarded even if its
/// `JobId` is the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JobTicket {
    pub id: JobId,
    pub kind: WorkflowKind,
    pub fingerprint: u64,
}

/// Stable 64-bit fingerprint of a job's input string, for stale-input
/// detection. Two jobs with the same logical inputs produce the same
/// fingerprint; a changed chooser or parameter produces a different one.
/// Wakes the GPUI entity after a background [`JobSlot`] stores a result.
///
/// `signal` is `Send + Sync` so worker threads can call it. The UI task
/// blocks on [`JobWake::wait`] (off the frame) then `tick()` + `notify`.
#[derive(Clone)]
pub struct JobWake {
    inner: Arc<(Mutex<bool>, Condvar)>,
}

impl Default for JobWake {
    fn default() -> Self {
        Self::new()
    }
}

impl JobWake {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    pub fn signal(&self) {
        let (lock, cvar) = &*self.inner;
        let mut ready = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *ready = true;
        cvar.notify_one();
    }

    /// Block until [`JobWake::signal`] has been called at least once since
    /// the last wait. Extra signals coalesce.
    pub fn wait(&self) {
        let (lock, cvar) = &*self.inner;
        let mut ready = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*ready {
            ready = cvar
                .wait(ready)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *ready = false;
    }
}

pub fn fingerprint_u64(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Allocates monotonically increasing [`JobTicket`]s.
///
/// Kept as a simple counter on the app struct so each new job trigger gets a
/// unique id, making stale results from superseded runs detectable. The
/// `kind` and `fingerprint` are captured in the returned [`JobTicket`] so the
/// consumer can verify all three fields on poll.
///
/// `Default` starts at 1 (0 is reserved as "no active job"), matching
/// [`JobRunner::new`].
pub struct JobRunner {
    next: u64,
}

impl Default for JobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRunner {
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Allocate the next [`JobTicket`]. Never returns id 0 (0 is reserved as
    /// "no active job"). The `fingerprint` string is hashed via
    /// [`fingerprint_u64`] so the consumer can detect input drift on poll.
    pub fn next_ticket(&mut self, kind: WorkflowKind, fingerprint: &str) -> JobTicket {
        let id = JobId(self.next);
        self.next += 1;
        JobTicket {
            id,
            kind,
            fingerprint: fingerprint_u64(fingerprint),
        }
    }
}

/// A thread-safe slot holding the latest result of a background job, tagged
/// with the [`JobTicket`] that produced it.
///
/// Callers compare the slot's stored ticket with their expected ticket via
/// [`take_if`] to detect and discard stale results (id mismatch) **and**
/// input-drift results (kind/fingerprint mismatch) from superseded runs.
///
/// [`take_if`]: JobSlot::take_if
pub struct JobSlot<T> {
    inner: Mutex<Option<(JobTicket, Result<T, String>)>>,
}

impl<T> JobSlot<T> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl<T> Default for JobSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + 'static> JobSlot<T> {
    /// Store a result tagged with the given [`JobTicket`].
    ///
    /// **Race-safe:** if the slot already holds a result with a **higher**
    /// [`JobId`], the incoming (older) result is discarded. This prevents an
    /// out-of-order completion (new job finishes first, old job finishes
    /// later) from clobbering the newer result before the UI thread consumes
    /// it.
    pub fn set(&self, ticket: JobTicket, result: Result<T, String>) {
        let mut guard = self.inner.lock().unwrap();
        if matches!(&*guard, Some((existing, _)) if existing.id > ticket.id) {
            return;
        }
        *guard = Some((ticket, result));
    }

    /// Take the result if the slot's stored [`JobTicket`] matches `expected`
    /// on **all three fields** (id, kind, fingerprint).
    ///
    /// Returns:
    /// - `Some(result)` if the slot has a matching-ticket result (consumed).
    /// - `None` if the slot is empty (job still running).
    /// - `None` if the slot has a **stale** result (id mismatch, discarded) or
    ///   an **input-drift** result (kind/fingerprint mismatch, discarded).
    pub fn take_if(&self, expected: JobTicket) -> Option<Result<T, String>> {
        let mut guard = self.inner.lock().unwrap();
        match guard.take() {
            Some((ticket, result)) if ticket == expected => Some(result),
            _ => None,
        }
    }

    /// Clear the slot (used when starting a new job to discard any leftover
    /// result from a previous run).
    pub fn clear(&self) {
        *self.inner.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_runner_ids_are_monotonic_and_nonzero() {
        let mut runner = JobRunner::new();
        let a = runner.next_ticket(WorkflowKind::Scan, "a");
        let b = runner.next_ticket(WorkflowKind::Scan, "a");
        let c = runner.next_ticket(WorkflowKind::Scan, "a");
        assert!(a.id.0 > 0);
        assert!(b.id.0 > a.id.0);
        assert!(c.id.0 > b.id.0);
    }

    #[test]
    fn job_runner_default_starts_at_one() {
        let mut runner = JobRunner::default();
        let t = runner.next_ticket(WorkflowKind::Scan, "a");
        assert_eq!(t.id.0, 1);
    }

    #[test]
    fn take_if_returns_matching_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t = runner.next_ticket(WorkflowKind::Scan, "a");
        slot.set(t, Ok(42));
        assert_eq!(slot.take_if(t), Some(Ok(42)));
        // Slot is now empty.
        assert!(slot.take_if(t).is_none());
    }

    #[test]
    fn take_if_discards_stale_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t1 = runner.next_ticket(WorkflowKind::Scan, "a");
        let t2 = runner.next_ticket(WorkflowKind::Scan, "a");
        slot.set(t1, Ok(100));
        // t2 is the current expected job; t1's result is stale.
        assert!(slot.take_if(t2).is_none());
        // The stale result was consumed and discarded.
        assert!(slot.take_if(t1).is_none());
    }

    #[test]
    fn take_if_discards_input_drift_result() {
        // Same id is impossible (monotonic), but a higher-id job with a
        // *different fingerprint* must still be discarded when the expected
        // ticket has a different fingerprint — the consumer asked for different
        // inputs.
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let expected = runner.next_ticket(WorkflowKind::Scan, "inputs-a");
        // Simulate a result arriving with the same id but a different
        // fingerprint (e.g. the user changed inputs mid-job and the old job's
        // result leaked through with a recomputed fingerprint).
        let drifted = JobTicket {
            id: expected.id,
            kind: WorkflowKind::Scan,
            fingerprint: fingerprint_u64("inputs-b"),
        };
        slot.set(drifted, Ok(100));
        assert!(
            slot.take_if(expected).is_none(),
            "input-drift result must be discarded"
        );
    }

    #[test]
    fn clear_empties_slot() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t = runner.next_ticket(WorkflowKind::Scan, "a");
        slot.set(t, Ok(7));
        slot.clear();
        assert!(slot.take_if(t).is_none());
    }

    #[test]
    fn set_overwrites_previous_result_same_id() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t = runner.next_ticket(WorkflowKind::Scan, "a");
        slot.set(t, Ok(1));
        slot.set(t, Ok(2));
        assert_eq!(slot.take_if(t), Some(Ok(2)));
    }

    #[test]
    fn error_results_round_trip() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t = runner.next_ticket(WorkflowKind::Scan, "a");
        slot.set(t, Err("boom".into()));
        assert_eq!(slot.take_if(t), Some(Err("boom".into())));
    }

    /// New result arrives first, then old result arrives.
    /// The old result must NOT overwrite the new one.
    #[test]
    fn set_discards_older_result_when_newer_already_present() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let t_old = runner.next_ticket(WorkflowKind::Scan, "a");
        let t_new = runner.next_ticket(WorkflowKind::Scan, "a");

        // New job finishes first.
        slot.set(t_new, Ok(200));
        // Old job finishes later — must be discarded.
        slot.set(t_old, Ok(999));

        // The newer result is preserved.
        assert_eq!(slot.take_if(t_new), Some(Ok(200)));
    }

    /// Concurrent out-of-order: two threads, old finishes after new.
    /// Verify the newer result survives.
    #[test]
    fn concurrent_out_of_order_newer_survives() {
        let slot = std::sync::Arc::new(JobSlot::<u32>::new());
        let mut runner = JobRunner::new();
        let t_old = runner.next_ticket(WorkflowKind::Scan, "a");
        let t_new = runner.next_ticket(WorkflowKind::Scan, "a");

        // Thread 1: sets the newer result immediately.
        let slot1 = slot.clone();
        let t1 = std::thread::spawn(move || {
            slot1.set(t_new, Ok(42));
        });

        // Thread 2: sets the older result after a small delay.
        let slot2 = slot.clone();
        let t2 = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            slot2.set(t_old, Ok(999));
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // The newer result must be the one we see.
        assert_eq!(slot.take_if(t_new), Some(Ok(42)));
        // The old result was discarded.
        assert!(slot.take_if(t_old).is_none());
    }

    #[test]
    fn job_wake_signal_unblocks_waiter() {
        let wake = JobWake::new();
        let waiter = wake.clone();
        let done = std::thread::spawn(move || {
            waiter.wait();
            true
        });
        wake.signal();
        assert!(done.join().unwrap());
    }

    #[test]
    fn job_wake_signal_before_wait_does_not_deadlock() {
        let wake = JobWake::new();
        wake.signal();
        wake.wait();
    }

    #[test]
    fn job_wake_coalesces_extra_signals() {
        let wake = JobWake::new();
        wake.signal();
        wake.signal();
        wake.wait();
        // A second wait must block until another signal — spawn one after a tick.
        let waiter = wake.clone();
        let done = std::thread::spawn(move || {
            waiter.wait();
            1
        });
        wake.signal();
        assert_eq!(done.join().unwrap(), 1);
    }
}
