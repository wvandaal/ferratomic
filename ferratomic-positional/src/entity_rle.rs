//! Entity run-length encoding for the entity column (INV-FERR-082, bd-mdfq).
//!
//! EAVT sort order guarantees that all datoms for the same entity are
//! contiguous. The entity column is a series of runs:
//! `[e1, e1, e1, e2, e2, e3, e3, e3, e3]` encodes as
//! `[(e1, 3), (e2, 2), (e3, 4)]`.
//!
//! The prefix-sum array enables O(log G) position-to-group, O(1) group boundary
//! lookup:
//! - `group_of(position)` -- which entity group contains canonical position p
//! - `group_range(group_idx)` -- (start, end) position range for an entity group

use ferratom::EntityId;

/// Run-length encoded entity column with prefix-sum index (INV-FERR-082, bd-mdfq).
///
/// Compresses the flat entity column into `(EntityId, u32)`
/// run pairs plus a prefix-sum array for O(log G) position-to-group,
/// O(1) group boundary lookup. EAVT sort guarantees entity contiguity
/// (INV-FERR-076).
///
/// Memory: for `G` distinct entity groups, stores `G * 36` bytes (runs)
/// plus `(G + 1) * 4` bytes (prefix sums) instead of `N * 32` bytes
/// (flat column). Compression ratio depends on datoms-per-entity.
#[derive(Clone, Debug)]
pub struct EntityRle {
    /// `(entity, run_length)` pairs in canonical EAVT order.
    runs: Vec<(EntityId, u32)>,
    /// `prefix_sums[i]` = sum of `run_lengths[0..i]`.
    /// `prefix_sums[0] = 0`. `prefix_sums[G] = N` (total datom count).
    /// Length = `runs.len() + 1`.
    prefix_sums: Vec<u32>,
}

impl EntityRle {
    /// Build from the entity column (INV-FERR-082, bd-mdfq).
    ///
    /// Scans `entities` left-to-right, collapsing consecutive identical
    /// `EntityId` values into `(entity, run_length)` pairs. Simultaneously
    /// builds the prefix-sum array for O(1) group boundary queries.
    ///
    /// O(n) time, single pass. The input must reflect canonical EAVT order
    /// (entity contiguity guaranteed by INV-FERR-076).
    #[must_use]
    pub fn from_entities(entities: &[EntityId]) -> Self {
        debug_assert!(
            u32::try_from(entities.len()).is_ok(),
            "INV-FERR-076: entity column exceeds u32 position space"
        );
        if entities.is_empty() {
            return Self {
                runs: Vec::new(),
                prefix_sums: vec![0],
            };
        }

        // Pre-allocate with a reasonable estimate. Worst case (every datom
        // is a different entity) allocates exactly N; typical case is much
        // smaller. We start with a conservative estimate and let Vec grow.
        let mut runs: Vec<(EntityId, u32)> = Vec::new();
        let mut prefix_sums: Vec<u32> = vec![0];

        let mut current_entity = entities[0];
        let mut run_len: u32 = 1;

        for entity in &entities[1..] {
            if *entity == current_entity {
                run_len = run_len.saturating_add(1);
            } else {
                let cumulative = prefix_sums
                    .last()
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(run_len);
                runs.push((current_entity, run_len));
                prefix_sums.push(cumulative);
                current_entity = *entity;
                run_len = 1;
            }
        }

        // Flush the final run.
        let cumulative = prefix_sums
            .last()
            .copied()
            .unwrap_or(0)
            .saturating_add(run_len);
        runs.push((current_entity, run_len));
        prefix_sums.push(cumulative);

        Self { runs, prefix_sums }
    }

    /// Number of distinct entity groups (INV-FERR-082, bd-mdfq).
    #[must_use]
    pub fn group_count(&self) -> usize {
        self.runs.len()
    }

    /// Total datom count across all groups.
    #[must_use]
    pub fn total_datoms(&self) -> u32 {
        self.prefix_sums.last().copied().unwrap_or(0)
    }

    /// Which entity group contains canonical position `position` (INV-FERR-082, bd-mdfq).
    ///
    /// Binary search on the prefix-sum array: O(log G) where G is the
    /// number of distinct entity groups. Returns `None` if `position`
    /// is out of bounds (>= total datom count).
    #[must_use]
    pub fn group_of(&self, position: usize) -> Option<usize> {
        let pos = u32::try_from(position).ok()?;
        let total = self.total_datoms();
        if pos >= total {
            return None;
        }

        // Binary search: find the largest i such that prefix_sums[i] <= pos.
        // The group index is that i, since prefix_sums[i] is the start of
        // group i and prefix_sums[i+1] is the start of group i+1.
        //
        // partition_point returns the first index where prefix_sums[idx] > pos,
        // so the group is partition_point - 1.
        let pp = self.prefix_sums.partition_point(|&s| s <= pos);
        // pp >= 1 because prefix_sums[0] = 0 <= pos (pos < total implies
        // total > 0 implies at least one group).
        Some(pp.saturating_sub(1))
    }

    /// Start and end canonical positions for entity group `group_idx` (INV-FERR-082, bd-mdfq).
    ///
    /// Returns `(start, end)` where datoms for this group occupy positions
    /// `[start, end)`. Returns `None` if `group_idx >= group_count()`.
    #[must_use]
    pub fn group_range(&self, group_idx: usize) -> Option<(u32, u32)> {
        if group_idx >= self.runs.len() {
            return None;
        }
        let start = self.prefix_sums[group_idx];
        let end = self.prefix_sums[group_idx + 1];
        Some((start, end))
    }

    /// Entity for group `group_idx` (INV-FERR-082).
    ///
    /// Returns `None` if `group_idx >= group_count()`.
    #[must_use]
    pub fn entity_at_group(&self, group_idx: usize) -> Option<EntityId> {
        self.runs.get(group_idx).map(|(eid, _)| *eid)
    }

    /// Entity at a canonical position. O(log G) via `group_of` + O(1) lookup.
    #[must_use]
    pub fn entity_at_position(&self, pos: usize) -> Option<EntityId> {
        self.entity_at_group(self.group_of(pos)?)
    }

    /// Borrow the run pairs slice.
    #[must_use]
    pub fn runs(&self) -> &[(EntityId, u32)] {
        &self.runs
    }

    /// Borrow the prefix-sum array.
    #[must_use]
    pub fn prefix_sums(&self) -> &[u32] {
        &self.prefix_sums
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a deterministic `EntityId` from a single byte.
    fn test_entity(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    // -- Test 1: Empty input -> 0 groups --

    #[test]
    fn test_bd_mdfq_empty_entities_yields_zero_groups() {
        let rle = EntityRle::from_entities(&[]);
        assert_eq!(
            rle.group_count(),
            0,
            "bd-mdfq: empty input must produce 0 groups"
        );
        assert_eq!(
            rle.total_datoms(),
            0,
            "bd-mdfq: empty input must have 0 total datoms"
        );
        assert_eq!(
            rle.group_of(0),
            None,
            "bd-mdfq: group_of on empty must return None"
        );
        assert_eq!(
            rle.group_range(0),
            None,
            "bd-mdfq: group_range on empty must return None"
        );
        assert_eq!(
            rle.entity_at_group(0),
            None,
            "bd-mdfq: entity_at_group on empty must return None"
        );
    }

    // -- Test 2: Single entity run -> 1 group --

    #[test]
    fn test_bd_mdfq_single_entity_run() {
        let e1 = test_entity(1);
        let entities = vec![e1, e1, e1, e1, e1];
        let rle = EntityRle::from_entities(&entities);

        assert_eq!(rle.group_count(), 1, "bd-mdfq: single run = 1 group");
        assert_eq!(rle.total_datoms(), 5);

        // All positions map to group 0.
        for pos in 0..5 {
            assert_eq!(
                rle.group_of(pos),
                Some(0),
                "bd-mdfq: position {pos} must be in group 0"
            );
        }
        assert_eq!(
            rle.group_of(5),
            None,
            "bd-mdfq: position 5 is out of bounds"
        );

        assert_eq!(
            rle.group_range(0),
            Some((0, 5)),
            "bd-mdfq: group 0 spans [0, 5)"
        );
        assert_eq!(
            rle.entity_at_group(0),
            Some(e1),
            "bd-mdfq: group 0 entity must be e1"
        );
    }

    // -- Test 3: Multiple runs -> correct group_of and group_range --

    #[test]
    fn test_bd_mdfq_multiple_runs() {
        let e1 = test_entity(1);
        let e2 = test_entity(2);
        let e3 = test_entity(3);

        // [e1, e1, e1, e2, e2, e3, e3, e3, e3]
        // Runs: (e1, 3), (e2, 2), (e3, 4)
        // Prefix sums: [0, 3, 5, 9]
        let entities = vec![e1, e1, e1, e2, e2, e3, e3, e3, e3];
        let rle = EntityRle::from_entities(&entities);

        assert_eq!(rle.group_count(), 3, "bd-mdfq: 3 distinct entity groups");
        assert_eq!(rle.total_datoms(), 9);

        // Verify prefix sums.
        assert_eq!(rle.prefix_sums(), &[0, 3, 5, 9]);

        // group_of checks.
        assert_eq!(rle.group_of(0), Some(0), "bd-mdfq: pos 0 in group 0");
        assert_eq!(rle.group_of(1), Some(0), "bd-mdfq: pos 1 in group 0");
        assert_eq!(rle.group_of(2), Some(0), "bd-mdfq: pos 2 in group 0");
        assert_eq!(rle.group_of(3), Some(1), "bd-mdfq: pos 3 in group 1");
        assert_eq!(rle.group_of(4), Some(1), "bd-mdfq: pos 4 in group 1");
        assert_eq!(rle.group_of(5), Some(2), "bd-mdfq: pos 5 in group 2");
        assert_eq!(rle.group_of(6), Some(2), "bd-mdfq: pos 6 in group 2");
        assert_eq!(rle.group_of(7), Some(2), "bd-mdfq: pos 7 in group 2");
        assert_eq!(rle.group_of(8), Some(2), "bd-mdfq: pos 8 in group 2");
        assert_eq!(rle.group_of(9), None, "bd-mdfq: pos 9 out of bounds");

        // group_range checks.
        assert_eq!(rle.group_range(0), Some((0, 3)));
        assert_eq!(rle.group_range(1), Some((3, 5)));
        assert_eq!(rle.group_range(2), Some((5, 9)));
        assert_eq!(rle.group_range(3), None);

        // entity_at_group checks.
        assert_eq!(rle.entity_at_group(0), Some(e1));
        assert_eq!(rle.entity_at_group(1), Some(e2));
        assert_eq!(rle.entity_at_group(2), Some(e3));
        assert_eq!(rle.entity_at_group(3), None);
    }

    // -- Test 4: Degenerate case -- every datom is a different entity --

    #[test]
    fn test_bd_mdfq_all_distinct_entities() {
        let entities: Vec<EntityId> = (0..10u8).map(test_entity).collect();
        let rle = EntityRle::from_entities(&entities);

        assert_eq!(
            rle.group_count(),
            10,
            "bd-mdfq: N distinct entities = N groups of size 1"
        );
        assert_eq!(rle.total_datoms(), 10);

        // Each position maps to its own group.
        for i in 0..10 {
            assert_eq!(
                rle.group_of(i),
                Some(i),
                "bd-mdfq: position {i} must be in group {i}"
            );
            let start = u32::try_from(i).unwrap_or(0);
            assert_eq!(
                rle.group_range(i),
                Some((start, start + 1)),
                "bd-mdfq: group {i} spans [{start}, {})",
                start + 1
            );
            assert_eq!(
                rle.entity_at_group(i),
                Some(test_entity(u8::try_from(i).unwrap_or(0))),
                "bd-mdfq: group {i} entity check"
            );
        }
    }

    // -- Test 5: Single datom (boundary case for prefix-sum logic) --

    #[test]
    fn test_bd_mdfq_single_datom() {
        let e = test_entity(42);
        let rle = EntityRle::from_entities(&[e]);

        assert_eq!(rle.group_count(), 1);
        assert_eq!(rle.total_datoms(), 1);
        assert_eq!(rle.group_of(0), Some(0));
        assert_eq!(rle.group_of(1), None);
        assert_eq!(rle.group_range(0), Some((0, 1)));
        assert_eq!(rle.entity_at_group(0), Some(e));
    }

    // -- Test 6: runs() and prefix_sums() accessors --

    #[test]
    fn test_bd_mdfq_accessors() {
        let e1 = test_entity(1);
        let e2 = test_entity(2);
        let entities = vec![e1, e1, e2, e2, e2];
        let rle = EntityRle::from_entities(&entities);

        assert_eq!(rle.runs(), &[(e1, 2), (e2, 3)]);
        assert_eq!(rle.prefix_sums(), &[0, 2, 5]);
    }
}
