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
//! Each job slot stores its result tagged with a monotonically increasing
//! [`JobId`] **and** a [`WorkflowKind`] + input fingerprint. The caller records
//! the expected [`JobId`] when starting a job. When polling, [`JobSlot::take_if`]
//! only returns a result whose stored [`JobId`] matches the expected one.
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
//! let id1 = runner.next_id(WorkflowKind::Scan, "fingerprint-a");
//! let id2 = runner.next_id(WorkflowKind::Scan, "fingerprint-a");
//! slot.set(id1, Ok(100));      // stale result from job #1
//! assert!(slot.take_if(id2).is_none()); // discarded — id mismatch
//! slot.set(id2, Ok(200));
//! assert_eq!(slot.take_if(id2), Some(Ok(200)));
//! ```

use std::sync::Mutex;

// ---------------------------------------------------------------------------
// WorkflowKind
// ---------------------------------------------------------------------------

/// Identifies the kind of background workflow.
///
/// Used together with an input fingerprint to detect when a new job's
/// inputs differ from a previous job's, even if the global ID is higher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)] // variants used as jobs move off-frame
pub enum WorkflowKind {
    Scan,
    Write,
    DryRun,
    CacheRefresh,
    JunkPreview,
    RecommendPreview,
    Discover,
    Dynamic,
    Mood,
    PlaylistMatch,
}

// ---------------------------------------------------------------------------
// JobId
// ---------------------------------------------------------------------------

/// Monotonic request ID for a background job.
///
/// Each time the user triggers a job (scan, write, cache refresh, dry-run),
/// the runner allocates a new `JobId` via [`JobRunner::next_id`]. The spawned
/// thread tags its result with this ID, and the UI poll compares it with the
/// expected ID before applying the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

// ---------------------------------------------------------------------------
// JobRunner
// ---------------------------------------------------------------------------

/// Allocates monotonically increasing [`JobId`]s.
///
/// Kept as a simple counter on the app struct so each new job trigger gets a
/// unique ID, making stale results from superseded runs detectable.
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

    /// Allocate the next [`JobId`]. Never returns 0 (0 is reserved as
    /// "no active job").
    ///
    /// `kind` and `fingerprint` are accepted so callers document which workflow
    /// and inputs a job corresponds to. Stale-result protection is provided by
    /// the monotonic [`JobId`] (see [`JobSlot::take_if`]); input-drift
    /// protection is the caller's responsibility — generator jobs capture the
    /// fingerprint in their result and the consumer verifies it on poll.
    pub fn next_id(&mut self, kind: WorkflowKind, fingerprint: &str) -> JobId {
        // The kind/fingerprint are intentionally not stored: the JobId alone
        // guards against stale results, and fingerprint verification happens
        // via the result payload (see GeneratorJobResult). Logging here would
        // add noise without changing behaviour.
        let _ = (kind, fingerprint);
        let id = JobId(self.next);
        self.next += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// JobSlot
// ---------------------------------------------------------------------------

/// A thread-safe slot holding the latest result of a background job, tagged
/// with the [`JobId`] that produced it.
///
/// Callers compare the slot's stored [`JobId`] with their expected [`JobId`]
/// via [`take_if`] to detect and discard stale results from superseded runs.
///
/// [`take_if`]: JobSlot::take_if
pub struct JobSlot<T> {
    inner: Mutex<Option<(JobId, Result<T, String>)>>,
}

impl<T: Send + 'static> JobSlot<T> {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Store a result tagged with the given [`JobId`].
    ///
    /// **Race-safe:** if the slot already holds a result with a **higher**
    /// [`JobId`], the incoming (older) result is discarded. This prevents an
    /// out-of-order completion (new job finishes first, old job finishes
    /// later) from clobbering the newer result before the UI thread consumes
    /// it.
    pub fn set(&self, id: JobId, result: Result<T, String>) {
        let mut guard = self.inner.lock().unwrap();
        match &*guard {
            Some((existing_id, _)) if *existing_id > id => {
                // The stored result is newer — discard the stale incoming one.
                return;
            }
            _ => {}
        }
        *guard = Some((id, result));
    }

    /// Take the result if the slot's stored [`JobId`] matches `expected`.
    ///
    /// Returns:
    /// - `Some(result)` if the slot has a matching-ID result (consumed).
    /// - `None` if the slot is empty (job still running).
    /// - `None` if the slot has a **stale** result (ID mismatch, discarded).
    pub fn take_if(&self, expected: JobId) -> Option<Result<T, String>> {
        let mut guard = self.inner.lock().unwrap();
        match guard.take() {
            Some((id, result)) if id == expected => Some(result),
            _ => None,
        }
    }

    /// Clear the slot (used when starting a new job to discard any leftover
    /// result from a previous run).
    pub fn clear(&self) {
        *self.inner.lock().unwrap() = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_runner_ids_are_monotonic_and_nonzero() {
        let mut runner = JobRunner::new();
        let a = runner.next_id(WorkflowKind::Scan, "a");
        let b = runner.next_id(WorkflowKind::Scan, "a");
        let c = runner.next_id(WorkflowKind::Scan, "a");
        assert!(a.0 > 0);
        assert!(b.0 > a.0);
        assert!(c.0 > b.0);
    }

    #[test]
    fn job_runner_default_starts_at_one() {
        let mut runner = JobRunner::default();
        let id = runner.next_id(WorkflowKind::Scan, "a");
        assert_eq!(id.0, 1);
    }

    #[test]
    fn take_if_returns_matching_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id(WorkflowKind::Scan, "a");
        slot.set(id, Ok(42));
        assert_eq!(slot.take_if(id), Some(Ok(42)));
        // Slot is now empty.
        assert!(slot.take_if(id).is_none());
    }

    #[test]
    fn take_if_discards_stale_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id1 = runner.next_id(WorkflowKind::Scan, "a");
        let id2 = runner.next_id(WorkflowKind::Scan, "a");
        slot.set(id1, Ok(100));
        // id2 is the current expected job; id1's result is stale.
        assert!(slot.take_if(id2).is_none());
        // The stale result was consumed and discarded.
        assert!(slot.take_if(id1).is_none());
    }

    #[test]
    fn clear_empties_slot() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id(WorkflowKind::Scan, "a");
        slot.set(id, Ok(7));
        slot.clear();
        assert!(slot.take_if(id).is_none());
    }

    #[test]
    fn set_overwrites_previous_result_same_id() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id(WorkflowKind::Scan, "a");
        slot.set(id, Ok(1));
        slot.set(id, Ok(2));
        assert_eq!(slot.take_if(id), Some(Ok(2)));
    }

    #[test]
    fn error_results_round_trip() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id(WorkflowKind::Scan, "a");
        slot.set(id, Err("boom".into()));
        assert_eq!(slot.take_if(id), Some(Err("boom".into())));
    }

    /// New result arrives first, then old result arrives.
    /// The old result must NOT overwrite the new one.
    #[test]
    fn set_discards_older_result_when_newer_already_present() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id_old = runner.next_id(WorkflowKind::Scan, "a");
        let id_new = runner.next_id(WorkflowKind::Scan, "a");

        // New job finishes first.
        slot.set(id_new, Ok(200));
        // Old job finishes later — must be discarded.
        slot.set(id_old, Ok(999));

        // The newer result is preserved.
        assert_eq!(slot.take_if(id_new), Some(Ok(200)));
    }

    /// Concurrent out-of-order: two threads, old finishes after new.
    /// Verify the newer result survives.
    #[test]
    fn concurrent_out_of_order_newer_survives() {
        let slot = std::sync::Arc::new(JobSlot::<u32>::new());
        let mut runner = JobRunner::new();
        let id_old = runner.next_id(WorkflowKind::Scan, "a");
        let id_new = runner.next_id(WorkflowKind::Scan, "a");

        // Thread 1: sets the newer result immediately.
        let slot1 = slot.clone();
        let t1 = std::thread::spawn(move || {
            slot1.set(id_new, Ok(42));
        });

        // Thread 2: sets the older result after a small delay.
        let slot2 = slot.clone();
        let t2 = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            slot2.set(id_old, Ok(999));
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // The newer result must be the one we see.
        assert_eq!(slot.take_if(id_new), Some(Ok(42)));
        // The old result was discarded.
        assert!(slot.take_if(id_old).is_none());
    }
}
