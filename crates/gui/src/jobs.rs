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
//! [`JobId`]. The caller records the expected [`JobId`] when starting a job.
//! When polling, [`JobSlot::take_if`] only returns a result whose stored
//! [`JobId`] matches the expected one; stale results are silently discarded.
//!
//! ```
//! use vapourfly_gui::jobs::{JobRunner, JobSlot};
//!
//! let slot = JobSlot::<u32>::new();
//! let mut runner = JobRunner::new();
//!
//! // Start job #1, then supersede it with job #2.
//! let id1 = runner.next_id();
//! let id2 = runner.next_id();
//! slot.set(id1, Ok(100));      // stale result from job #1
//! assert!(slot.take_if(id2).is_none()); // discarded — id mismatch
//! slot.set(id2, Ok(200));
//! assert_eq!(slot.take_if(id2), Some(Ok(200)));
//! ```

use std::sync::Mutex;

// ---------------------------------------------------------------------------
// JobId
// ---------------------------------------------------------------------------

/// Monotonic request ID for a background job.
///
/// Each time the user triggers a job (scan, write, cache refresh, dry-run),
/// the runner allocates a new `JobId` via [`JobRunner::next_id`]. The spawned
/// thread tags its result with this ID, and the UI poll compares it with the
/// expected ID before applying the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

// ---------------------------------------------------------------------------
// JobRunner
// ---------------------------------------------------------------------------

/// Allocates monotonically increasing [`JobId`]s.
///
/// Kept as a simple counter on the app struct so each new job trigger gets a
/// unique ID, making stale results from superseded runs detectable.
#[derive(Default)]
pub struct JobRunner {
    next: u64,
}

impl JobRunner {
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    /// Allocate the next [`JobId`]. Never returns 0 (0 is reserved as
    /// "no active job").
    pub fn next_id(&mut self) -> JobId {
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
    /// Overwrites any previous content (the latest result wins if two threads
    /// race; the [`take_if`] call on the UI thread filters by ID).
    pub fn set(&self, id: JobId, result: Result<T, String>) {
        *self.inner.lock().unwrap() = Some((id, result));
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
        let a = runner.next_id();
        let b = runner.next_id();
        let c = runner.next_id();
        assert!(a.0 > 0);
        assert!(b.0 > a.0);
        assert!(c.0 > b.0);
    }

    #[test]
    fn take_if_returns_matching_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id();
        slot.set(id, Ok(42));
        assert_eq!(slot.take_if(id), Some(Ok(42)));
        // Slot is now empty.
        assert!(slot.take_if(id).is_none());
    }

    #[test]
    fn take_if_discards_stale_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id1 = runner.next_id();
        let id2 = runner.next_id();
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
        let id = runner.next_id();
        slot.set(id, Ok(7));
        slot.clear();
        assert!(slot.take_if(id).is_none());
    }

    #[test]
    fn set_overwrites_previous_result() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id();
        slot.set(id, Ok(1));
        slot.set(id, Ok(2));
        assert_eq!(slot.take_if(id), Some(Ok(2)));
    }

    #[test]
    fn error_results_round_trip() {
        let slot = JobSlot::<u32>::new();
        let mut runner = JobRunner::new();
        let id = runner.next_id();
        slot.set(id, Err("boom".into()));
        assert_eq!(slot.take_if(id), Some(Err("boom".into())));
    }
}
