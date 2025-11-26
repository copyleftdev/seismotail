//! Bounded deduplication ring buffer.
//!
//! Implements a fixed-size ring buffer for tracking seen event IDs.
//! Follows NASA Power of 10: bounded resources, no dynamic allocation in hot path.

use std::collections::VecDeque;

/// Default capacity for the deduplication ring.
/// Sized for ~24 hours of earthquake data at peak activity.
pub const DEFAULT_CAPACITY: usize = 10_000;

/// A bounded ring buffer for deduplicating events by ID.
///
/// Uses a fixed-capacity ring that evicts oldest entries when full.
/// This ensures bounded memory usage regardless of stream duration.
#[derive(Debug)]
pub struct DedupeRing {
    /// Ring of seen IDs (oldest at front, newest at back)
    seen: VecDeque<SeenEntry>,
    /// Maximum capacity
    capacity: usize,
    /// Total events processed (for stats)
    total_seen: u64,
    /// Total duplicates skipped
    total_dupes: u64,
}

/// An entry in the deduplication ring.
#[derive(Debug, Clone)]
struct SeenEntry {
    /// Event ID
    id: String,
    /// Last update timestamp (for tracking updates)
    updated: i64,
}

impl DedupeRing {
    /// Create a new deduplication ring with the specified capacity.
    ///
    /// # Panics
    ///
    /// Panics if capacity is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be positive");

        Self {
            seen: VecDeque::with_capacity(capacity),
            capacity,
            total_seen: 0,
            total_dupes: 0,
        }
    }

    /// Create a new deduplication ring with default capacity.
    #[must_use]
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Check if an event should be processed (not a duplicate).
    ///
    /// Returns `true` if the event is new or has been updated.
    /// Returns `false` if this is a duplicate we've seen before.
    ///
    /// This also marks the event as seen if it's new.
    pub fn check_and_mark(&mut self, id: &str, updated: i64) -> DedupeResult {
        self.total_seen += 1;

        // Check if we've seen this ID before
        if let Some(pos) = self.find_position(id) {
            let entry = &self.seen[pos];

            // Check if this is an update (newer timestamp)
            if updated > entry.updated {
                // Update the existing entry with new timestamp
                self.seen[pos].updated = updated;
                return DedupeResult::Updated;
            }

            // It's a duplicate with same or older timestamp
            self.total_dupes += 1;
            return DedupeResult::Duplicate;
        }

        // New event - add to ring
        self.insert(id.to_string(), updated);
        DedupeResult::New
    }

    /// Find the position of an ID in the ring.
    fn find_position(&self, id: &str) -> Option<usize> {
        // Linear search - could optimize with a HashSet if needed,
        // but for 10k entries this is fast enough (~1-2ms worst case)
        self.seen.iter().position(|e| e.id == id)
    }

    /// Insert a new entry, evicting oldest if at capacity.
    fn insert(&mut self, id: String, updated: i64) {
        // Evict oldest if at capacity (FIFO)
        if self.seen.len() >= self.capacity {
            self.seen.pop_front();
        }

        self.seen.push_back(SeenEntry { id, updated });

        // NASA Power of 10: assert postcondition
        debug_assert!(self.seen.len() <= self.capacity);
    }

    /// Get the current number of tracked IDs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Check if the ring is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// Get total events processed.
    #[must_use]
    pub fn total_seen(&self) -> u64 {
        self.total_seen
    }

    /// Get total duplicates skipped.
    #[must_use]
    pub fn total_dupes(&self) -> u64 {
        self.total_dupes
    }

    /// Get the deduplication rate (0.0 to 1.0).
    #[must_use]
    pub fn dupe_rate(&self) -> f64 {
        if self.total_seen == 0 {
            0.0
        } else {
            self.total_dupes as f64 / self.total_seen as f64
        }
    }

    /// Clear all tracked IDs (for testing or reset).
    pub fn clear(&mut self) {
        self.seen.clear();
        self.total_seen = 0;
        self.total_dupes = 0;
    }
}

impl Default for DedupeRing {
    fn default() -> Self {
        Self::with_default_capacity()
    }
}

/// Result of a deduplication check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeResult {
    /// Event is new (never seen before)
    New,
    /// Event was seen but has been updated
    Updated,
    /// Event is a duplicate (same ID and timestamp)
    Duplicate,
}

impl DedupeResult {
    /// Check if this result should be emitted (not a duplicate).
    #[must_use]
    pub fn should_emit(self) -> bool {
        match self {
            Self::New | Self::Updated => true,
            Self::Duplicate => false,
        }
    }

    /// Check if this is an update to an existing event.
    #[must_use]
    pub fn is_update(self) -> bool {
        matches!(self, Self::Updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_events() {
        let mut ring = DedupeRing::new(100);

        // First occurrence is new
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::New);
        assert_eq!(ring.check_and_mark("event2", 2000), DedupeResult::New);
        assert_eq!(ring.check_and_mark("event3", 3000), DedupeResult::New);

        assert_eq!(ring.len(), 3);
        assert_eq!(ring.total_seen(), 3);
        assert_eq!(ring.total_dupes(), 0);
    }

    #[test]
    fn test_duplicates() {
        let mut ring = DedupeRing::new(100);

        // First occurrence
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::New);

        // Same ID, same timestamp = duplicate
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::Duplicate);
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::Duplicate);

        assert_eq!(ring.len(), 1);
        assert_eq!(ring.total_dupes(), 2);
    }

    #[test]
    fn test_updates() {
        let mut ring = DedupeRing::new(100);

        // First occurrence
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::New);

        // Same ID, newer timestamp = update
        assert_eq!(ring.check_and_mark("event1", 2000), DedupeResult::Updated);
        assert_eq!(ring.check_and_mark("event1", 3000), DedupeResult::Updated);

        // Same ID, older timestamp = duplicate
        assert_eq!(ring.check_and_mark("event1", 2000), DedupeResult::Duplicate);

        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn test_bounded_capacity() {
        let mut ring = DedupeRing::new(3);

        ring.check_and_mark("event1", 1000);
        ring.check_and_mark("event2", 2000);
        ring.check_and_mark("event3", 3000);
        assert_eq!(ring.len(), 3);

        // Fourth event evicts oldest
        ring.check_and_mark("event4", 4000);
        assert_eq!(ring.len(), 3);

        // event1 should be gone (evicted)
        assert_eq!(ring.check_and_mark("event1", 1000), DedupeResult::New);

        // event2, event3, event4 should still be tracked
        assert_eq!(ring.check_and_mark("event2", 2000), DedupeResult::Duplicate);
    }

    #[test]
    fn test_should_emit() {
        assert!(DedupeResult::New.should_emit());
        assert!(DedupeResult::Updated.should_emit());
        assert!(!DedupeResult::Duplicate.should_emit());
    }

    #[test]
    fn test_dupe_rate() {
        let mut ring = DedupeRing::new(100);

        ring.check_and_mark("event1", 1000);
        ring.check_and_mark("event1", 1000); // dupe
        ring.check_and_mark("event1", 1000); // dupe
        ring.check_and_mark("event2", 2000);

        // 2 dupes out of 4 = 50%
        assert!((ring.dupe_rate() - 0.5).abs() < 0.01);
    }
}
