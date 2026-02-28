//! Event debouncer for filesystem events.
//!
//! This module provides a pure, time-agnostic event debouncer that accumulates
//! filesystem events and emits them as batches after a period of silence.
//! The debouncer is a state machine with no side effects: callers provide
//! the current time (`Instant`), making it fully testable without real timers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::types::EventType;

/// Duration of silence (no new events) required before emitting a batch.
const SILENCE_WINDOW: Duration = Duration::from_secs(3);

/// A pure filesystem event debouncer.
///
/// Accumulates events for multiple files and emits them as a batch once
/// the silence window (3 seconds) has elapsed without new events.
///
/// For the same path, only the last event type is retained (coalescing).
/// For example, if a file is created then modified, only `Modify` is recorded.
///
/// This debouncer is a pure state machine: all timestamps are provided by the caller,
/// allowing deterministic testing without real timers.
#[derive(Debug)]
pub struct Debouncer {
    /// Pending events, keyed by file path. Last event type wins for duplicates.
    pending: HashMap<PathBuf, EventType>,
    /// Timestamp of the most recent event received.
    /// Used to determine when the silence window has elapsed.
    last_event: Option<Instant>,
}

impl Debouncer {
    /// Creates a new, empty debouncer.
    ///
    /// # Example
    /// ```
    /// # use unfudged::watcher::debounce::Debouncer;
    /// let debouncer = Debouncer::new();
    /// assert!(!debouncer.has_pending());
    /// ```
    pub fn new() -> Self {
        Debouncer {
            pending: HashMap::new(),
            last_event: None,
        }
    }

    /// Records a new filesystem event.
    ///
    /// If the same path has been recorded before, the new event type overwrites
    /// the old one (coalescing). The timestamp is updated to `now`, which resets
    /// the silence timer.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path that changed.
    /// * `event_type` - The type of event (`Create`, `Modify`, or `Delete`).
    /// * `now` - The current time, used to track the silence window.
    ///
    /// # Example
    /// ```
    /// # use unfudged::watcher::debounce::Debouncer;
    /// # use unfudged::types::EventType;
    /// # use std::path::PathBuf;
    /// # use std::time::Instant;
    /// let mut debouncer = Debouncer::new();
    /// let now = Instant::now();
    /// debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
    /// assert!(debouncer.has_pending());
    /// ```
    pub fn push(&mut self, path: PathBuf, event_type: EventType, now: Instant) {
        self.pending.insert(path, event_type);
        self.last_event = Some(now);
    }

    /// Checks if the silence window has elapsed and returns pending events if ready.
    ///
    /// Returns `Some(events)` if:
    /// - There are pending events AND
    /// - At least `SILENCE_WINDOW` (3 seconds) have passed since the last event.
    ///
    /// Returns `None` if:
    /// - There are no pending events, OR
    /// - The silence window has not yet elapsed.
    ///
    /// When returning `Some`, the debouncer is reset to an empty state.
    /// Subsequent calls to `drain_if_ready` will return `None` until new events
    /// are pushed.
    ///
    /// # Arguments
    ///
    /// * `now` - The current time. Compared against the last event timestamp
    ///   to determine if the silence window has passed.
    ///
    /// # Example
    /// ```
    /// # use unfudged::watcher::debounce::Debouncer;
    /// # use unfudged::types::EventType;
    /// # use std::path::PathBuf;
    /// # use std::time::{Instant, Duration};
    /// let mut debouncer = Debouncer::new();
    /// let now = Instant::now();
    /// debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Modify, now);
    ///
    /// // Not ready yet (0 seconds elapsed)
    /// assert!(debouncer.drain_if_ready(now).is_none());
    ///
    /// // After 3+ seconds
    /// let later = now + Duration::from_secs(3);
    /// let events = debouncer.drain_if_ready(later);
    /// assert!(events.is_some());
    /// ```
    pub fn drain_if_ready(&mut self, now: Instant) -> Option<Vec<(PathBuf, EventType)>> {
        if self.pending.is_empty() {
            return None;
        }

        match self.last_event {
            Some(last) => {
                if now.duration_since(last) >= SILENCE_WINDOW {
                    let events: Vec<(PathBuf, EventType)> = self.pending.drain().collect();
                    self.last_event = None;
                    Some(events)
                } else {
                    None
                }
            }
            None => {
                // Safety: if last_event is None but pending is not empty,
                // we're in an inconsistent state. This should never happen
                // in correct usage, but we treat it as "not ready".
                None
            }
        }
    }

    /// Returns true if there are pending events waiting to be drained.
    ///
    /// # Example
    /// ```
    /// # use unfudged::watcher::debounce::Debouncer;
    /// # use unfudged::types::EventType;
    /// # use std::path::PathBuf;
    /// # use std::time::Instant;
    /// let mut debouncer = Debouncer::new();
    /// assert!(!debouncer.has_pending());
    ///
    /// debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, Instant::now());
    /// assert!(debouncer.has_pending());
    /// ```
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Unconditionally drains all pending events.
    ///
    /// Used during graceful shutdown to flush queued changes
    /// without waiting for the silence window to expire.
    ///
    /// Returns `None` if there are no pending events.
    ///
    /// # Example
    /// ```
    /// # use unfudged::watcher::debounce::Debouncer;
    /// # use unfudged::types::EventType;
    /// # use std::path::PathBuf;
    /// # use std::time::Instant;
    /// let mut debouncer = Debouncer::new();
    /// let now = Instant::now();
    /// debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Modify, now);
    /// debouncer.push(PathBuf::from("/tmp/test2.rs"), EventType::Create, now);
    ///
    /// let batch = debouncer.force_drain().expect("should have events");
    /// assert_eq!(batch.len(), 2);
    /// assert!(!debouncer.has_pending());
    /// ```
    pub fn force_drain(&mut self) -> Option<Vec<(PathBuf, EventType)>> {
        if self.pending.is_empty() {
            return None;
        }
        let events: Vec<(PathBuf, EventType)> = self.pending.drain().collect();
        self.last_event = None;
        Some(events)
    }
}

impl Default for Debouncer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_debouncer_is_empty() {
        let mut debouncer = Debouncer::new();
        assert!(!debouncer.has_pending());
        let now = Instant::now();
        assert!(debouncer.drain_if_ready(now).is_none());
    }

    #[test]
    fn push_single_event_not_ready_immediately() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);

        assert!(debouncer.has_pending());
        assert!(debouncer.drain_if_ready(now).is_none());
    }

    #[test]
    fn push_single_event_ready_after_3_seconds() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later);

        assert!(events.is_some());
        let batch = events.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].0, PathBuf::from("/tmp/test.rs"));
        assert_eq!(batch[0].1, EventType::Create);
    }

    #[test]
    fn multiple_events_different_files_in_batch() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/file1.rs"), EventType::Create, now);
        debouncer.push(PathBuf::from("/tmp/file2.rs"), EventType::Modify, now);
        debouncer.push(PathBuf::from("/tmp/file3.rs"), EventType::Delete, now);

        assert!(debouncer.has_pending());
        assert_eq!(debouncer.pending.len(), 3);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later);

        assert!(events.is_some());
        let batch = events.unwrap();
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn same_path_multiple_events_last_wins() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Modify, now);
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Delete, now);

        assert_eq!(debouncer.pending.len(), 1);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later);

        assert!(events.is_some());
        let batch = events.unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].1, EventType::Delete);
    }

    #[test]
    fn drain_resets_state() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
        let later = now + Duration::from_secs(3);
        let first_drain = debouncer.drain_if_ready(later);
        assert!(first_drain.is_some());

        assert!(!debouncer.has_pending());
        let second_drain = debouncer.drain_if_ready(later);
        assert!(second_drain.is_none());
    }

    #[test]
    fn second_drain_after_reset_returns_none() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Modify, now);
        let later = now + Duration::from_secs(3);

        let first = debouncer.drain_if_ready(later);
        assert!(first.is_some());

        let second = debouncer.drain_if_ready(later);
        assert!(second.is_none());
    }

    #[test]
    fn has_pending_reflects_state() {
        let mut debouncer = Debouncer::new();
        assert!(!debouncer.has_pending());

        let now = Instant::now();
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
        assert!(debouncer.has_pending());

        let later = now + Duration::from_secs(3);
        debouncer.drain_if_ready(later);
        assert!(!debouncer.has_pending());
    }

    #[test]
    fn empty_debouncer_drain_returns_none() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        let later = now + Duration::from_secs(10);
        assert!(debouncer.drain_if_ready(later).is_none());
    }

    #[test]
    fn event_at_exactly_3_seconds_boundary_is_ready() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);

        // Exactly at 3 seconds (duration_since returns exactly 3s)
        let exactly_3s = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(exactly_3s);

        assert!(events.is_some());
    }

    #[test]
    fn new_event_resets_timer() {
        let mut debouncer = Debouncer::new();
        let t0 = Instant::now();

        debouncer.push(PathBuf::from("/tmp/file1.rs"), EventType::Create, t0);

        // Check at t0 + 2.5 seconds (not yet ready)
        let t1 = t0 + Duration::from_millis(2500);
        assert!(debouncer.drain_if_ready(t1).is_none());

        // Push a new event at t0 + 2.5 seconds (resets timer)
        debouncer.push(PathBuf::from("/tmp/file2.rs"), EventType::Modify, t1);

        // Check at t0 + 5 seconds (only 2.5 seconds since last event)
        let t2 = t0 + Duration::from_secs(5);
        assert!(debouncer.drain_if_ready(t2).is_none());

        // Check at t0 + 5.5 seconds (3+ seconds since last event)
        let t3 = t0 + Duration::from_millis(5500);
        let events = debouncer.drain_if_ready(t3);
        assert!(events.is_some());
        let batch = events.unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn default_creates_empty_debouncer() {
        let debouncer = Debouncer::default();
        assert!(!debouncer.has_pending());
    }

    #[test]
    fn coalesce_create_then_modify() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Modify, now);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, EventType::Modify);
    }

    #[test]
    fn coalesce_create_then_delete() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Delete, now);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, EventType::Delete);
    }

    #[test]
    fn near_silence_window_not_ready() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        debouncer.push(PathBuf::from("/tmp/test.rs"), EventType::Create, now);

        // Just under 3 seconds
        let almost_ready = now + Duration::from_secs(3) - Duration::from_millis(1);
        assert!(debouncer.drain_if_ready(almost_ready).is_none());
    }

    #[test]
    fn drain_clears_all_events() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();

        debouncer.push(PathBuf::from("/tmp/file1.rs"), EventType::Create, now);
        debouncer.push(PathBuf::from("/tmp/file2.rs"), EventType::Modify, now);

        let later = now + Duration::from_secs(3);
        let events = debouncer.drain_if_ready(later).unwrap();

        assert_eq!(events.len(), 2);
        assert!(!debouncer.has_pending());
    }

    #[test]
    fn force_drain_empty() {
        let mut debouncer = Debouncer::new();
        assert!(debouncer.force_drain().is_none());
    }

    #[test]
    fn force_drain_returns_all_pending() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        debouncer.push(PathBuf::from("a.txt"), EventType::Create, now);
        debouncer.push(PathBuf::from("b.txt"), EventType::Modify, now);

        let events = debouncer.force_drain().expect("should have events");
        assert_eq!(events.len(), 2);
        assert!(!debouncer.has_pending());
    }

    #[test]
    fn force_drain_ignores_silence_window() {
        let mut debouncer = Debouncer::new();
        let now = Instant::now();
        // Push event just now (silence window hasn't expired)
        debouncer.push(PathBuf::from("test.txt"), EventType::Modify, now);

        // drain_if_ready would return None (too soon)
        assert!(debouncer.drain_if_ready(now).is_none());

        // But force_drain returns events regardless
        let events = debouncer.force_drain().expect("should have events");
        assert_eq!(events.len(), 1);
    }
}
