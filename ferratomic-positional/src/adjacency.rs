//! Graph adjacency index (INV-FERR-083, bd-ewma).
//!
//! Precomputed from Ref-valued datoms: `EntityId -> Vec<(AttributeId, EntityId)>`.
//! Enables O(1) neighbor lookup for graph traversal in both forward and
//! reverse directions.
//!
//! The index is built from the canonical datom array by scanning for
//! `Value::Ref` datoms with `Op::Assert`. Retracted edges are excluded.
//! Attributes are resolved to `AttributeId` via `AttributeIntern` for
//! compact, integer-comparable storage.

use std::collections::BTreeMap;

use ferratom::{AttributeId, AttributeIntern, Datom, EntityId, Op, Value};

// ---------------------------------------------------------------------------
// AdjacencyIndex (INV-FERR-083)
// ---------------------------------------------------------------------------

/// Graph adjacency index for O(1) neighbor traversal (INV-FERR-083, bd-ewma).
///
/// Built from the canonical datom array by extracting all `Value::Ref`
/// datoms with `Op::Assert`. Each such datom `(source, attr, Ref(target))`
/// produces a forward edge `source -> (attr_id, target)` and a reverse
/// edge `target -> (attr_id, source)`.
///
/// Both forward and reverse maps use `BTreeMap` for deterministic
/// iteration order (consistent with the project's avoidance of `HashMap`
/// in content-addressed structures).
pub struct AdjacencyIndex {
    /// Forward edges: source entity -> [(attribute, target entity)].
    forward: BTreeMap<EntityId, Vec<(AttributeId, EntityId)>>,
    /// Reverse edges: target entity -> [(attribute, source entity)].
    reverse: BTreeMap<EntityId, Vec<(AttributeId, EntityId)>>,
}

/// Sentinel empty slice returned when an entity has no neighbors.
const EMPTY_EDGES: &[(AttributeId, EntityId)] = &[];

impl AdjacencyIndex {
    /// Build from canonical datom array (INV-FERR-083).
    ///
    /// Scans for `Value::Ref` datoms with `Op::Assert`. Requires
    /// `AttributeIntern` for `Attribute -> AttributeId` resolution.
    /// Datoms whose attribute is not in the intern table are skipped
    /// (should not happen for schema-validated stores).
    #[must_use]
    pub fn from_canonical(datoms: &[Datom], intern: &AttributeIntern) -> Self {
        let mut forward: BTreeMap<EntityId, Vec<(AttributeId, EntityId)>> = BTreeMap::new();
        let mut reverse: BTreeMap<EntityId, Vec<(AttributeId, EntityId)>> = BTreeMap::new();

        for datom in datoms {
            // Only asserted Ref datoms produce edges.
            if datom.op() != Op::Assert {
                continue;
            }
            let target = match datom.value() {
                Value::Ref(eid) => *eid,
                _ => continue,
            };
            let Some(attr_id) = intern.id_of(datom.attribute()) else {
                continue;
            };

            let source = datom.entity();
            forward.entry(source).or_default().push((attr_id, target));
            reverse.entry(target).or_default().push((attr_id, source));
        }

        Self { forward, reverse }
    }

    /// Forward neighbors: entities reachable from `entity` via Ref edges.
    ///
    /// Returns `(AttributeId, EntityId)` pairs for each outgoing edge.
    /// O(1) lookup (`BTreeMap` point query). Returns an empty slice if the
    /// entity has no outgoing Ref edges.
    #[must_use]
    pub fn neighbors(&self, entity: &EntityId) -> &[(AttributeId, EntityId)] {
        self.forward.get(entity).map_or(EMPTY_EDGES, Vec::as_slice)
    }

    /// Reverse neighbors: entities that point TO `entity` via Ref edges.
    ///
    /// Returns `(AttributeId, EntityId)` pairs for each incoming edge.
    /// O(1) lookup. Returns an empty slice if no entity points to this one.
    #[must_use]
    pub fn reverse_neighbors(&self, entity: &EntityId) -> &[(AttributeId, EntityId)] {
        self.reverse.get(entity).map_or(EMPTY_EDGES, Vec::as_slice)
    }

    /// Number of entities with at least one outgoing edge.
    #[must_use]
    pub fn forward_entity_count(&self) -> usize {
        self.forward.len()
    }

    /// Number of entities with at least one incoming edge.
    #[must_use]
    pub fn reverse_entity_count(&self) -> usize {
        self.reverse.len()
    }

    /// Total number of forward edges across all entities.
    #[must_use]
    pub fn total_edges(&self) -> usize {
        self.forward.values().map(Vec::len).sum()
    }
}

// ---------------------------------------------------------------------------
// Tests (INV-FERR-083)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    use super::*;

    /// Build an `AttributeIntern` from a list of string attribute names.
    fn make_intern(names: &[&str]) -> AttributeIntern {
        AttributeIntern::from_attributes(names.iter().map(|s| Attribute::from(*s)))
            .expect("intern table construction must succeed in tests")
    }

    /// Helper: create a Ref datom (Assert).
    fn ref_datom(source: &[u8], attr: &str, target: &[u8], tx_phys: u64) -> Datom {
        Datom::new(
            EntityId::from_content(source),
            Attribute::from(attr),
            Value::Ref(EntityId::from_content(target)),
            TxId::new(tx_phys, 0, 1),
            Op::Assert,
        )
    }

    /// Helper: create a non-Ref datom (Long value, Assert).
    fn long_datom(entity: &[u8], attr: &str, val: i64, tx_phys: u64) -> Datom {
        Datom::new(
            EntityId::from_content(entity),
            Attribute::from(attr),
            Value::Long(val),
            TxId::new(tx_phys, 0, 1),
            Op::Assert,
        )
    }

    /// Helper: create a Ref datom with `Op::Retract`.
    fn retracted_ref_datom(source: &[u8], attr: &str, target: &[u8], tx_phys: u64) -> Datom {
        Datom::new(
            EntityId::from_content(source),
            Attribute::from(attr),
            Value::Ref(EntityId::from_content(target)),
            TxId::new(tx_phys, 0, 1),
            Op::Retract,
        )
    }

    // -----------------------------------------------------------------------
    // Test 1: Empty store -> empty adjacency
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_empty_store_empty_adjacency() {
        let intern = make_intern(&[]);
        let index = AdjacencyIndex::from_canonical(&[], &intern);

        assert_eq!(index.forward_entity_count(), 0);
        assert_eq!(index.reverse_entity_count(), 0);
        assert_eq!(index.total_edges(), 0);

        let any_entity = EntityId::from_content(b"nonexistent");
        assert!(
            index.neighbors(&any_entity).is_empty(),
            "INV-FERR-083: empty store must have no forward neighbors"
        );
        assert!(
            index.reverse_neighbors(&any_entity).is_empty(),
            "INV-FERR-083: empty store must have no reverse neighbors"
        );
    }

    // -----------------------------------------------------------------------
    // Test 2: Single Ref datom -> one forward + one reverse edge
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_single_ref_one_forward_one_reverse() {
        let intern = make_intern(&["knows"]);
        let datoms = [ref_datom(b"alice", "knows", b"bob", 1)];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        let alice = EntityId::from_content(b"alice");
        let bob = EntityId::from_content(b"bob");
        let knows_id = intern
            .id_of(&Attribute::from("knows"))
            .expect("knows must be interned");

        // Forward: alice -> [(knows, bob)]
        let fwd = index.neighbors(&alice);
        assert_eq!(fwd.len(), 1, "INV-FERR-083: one forward edge expected");
        assert_eq!(fwd[0], (knows_id, bob));

        // Reverse: bob -> [(knows, alice)]
        let rev = index.reverse_neighbors(&bob);
        assert_eq!(rev.len(), 1, "INV-FERR-083: one reverse edge expected");
        assert_eq!(rev[0], (knows_id, alice));

        // No edges for unrelated entities.
        assert!(index.neighbors(&bob).is_empty());
        assert!(index.reverse_neighbors(&alice).is_empty());

        assert_eq!(index.total_edges(), 1);
    }

    // -----------------------------------------------------------------------
    // Test 3: Non-Ref datoms are ignored
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_non_ref_datoms_ignored() {
        let intern = make_intern(&["name", "age"]);
        let datoms = [
            long_datom(b"alice", "age", 30, 1),
            Datom::new(
                EntityId::from_content(b"alice"),
                Attribute::from("name"),
                Value::String(Arc::from("Alice")),
                TxId::new(1, 0, 1),
                Op::Assert,
            ),
        ];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        assert_eq!(
            index.total_edges(),
            0,
            "INV-FERR-083: non-Ref datoms must not produce edges"
        );
        assert_eq!(index.forward_entity_count(), 0);
        assert_eq!(index.reverse_entity_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Test 4: Multiple edges from same entity
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_multiple_edges_from_same_entity() {
        let intern = make_intern(&["knows", "works-with"]);
        let datoms = [
            ref_datom(b"alice", "knows", b"bob", 1),
            ref_datom(b"alice", "knows", b"carol", 2),
            ref_datom(b"alice", "works-with", b"dave", 3),
        ];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        let alice = EntityId::from_content(b"alice");
        let fwd = index.neighbors(&alice);
        assert_eq!(
            fwd.len(),
            3,
            "INV-FERR-083: alice should have 3 outgoing edges"
        );
        assert_eq!(index.total_edges(), 3);
        assert_eq!(index.forward_entity_count(), 1);
        // Three distinct targets.
        assert_eq!(index.reverse_entity_count(), 3);
    }

    // -----------------------------------------------------------------------
    // Test 5: Bidirectional consistency
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_bidirectional_consistency() {
        let intern = make_intern(&["follows", "likes"]);
        let datoms = [
            ref_datom(b"alice", "follows", b"bob", 1),
            ref_datom(b"bob", "follows", b"carol", 2),
            ref_datom(b"carol", "likes", b"alice", 3),
        ];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        // For every forward edge (source -> target), there must be a
        // corresponding reverse edge (target <- source).
        for datom in &datoms {
            let source = datom.entity();
            let target = match datom.value() {
                Value::Ref(eid) => *eid,
                _ => continue,
            };
            let attr_id = intern
                .id_of(datom.attribute())
                .expect("attribute must be interned");

            // Forward: source has edge (attr, target).
            let fwd = index.neighbors(&source);
            assert!(
                fwd.contains(&(attr_id, target)),
                "INV-FERR-083: forward index must contain ({source:?} -> {target:?})"
            );

            // Reverse: target has edge (attr, source).
            let rev = index.reverse_neighbors(&target);
            assert!(
                rev.contains(&(attr_id, source)),
                "INV-FERR-083: reverse index must contain ({target:?} <- {source:?})"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Test 6: Retracted Ref datoms are excluded
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_retracted_refs_excluded() {
        let intern = make_intern(&["knows"]);
        let datoms = [
            ref_datom(b"alice", "knows", b"bob", 1),
            retracted_ref_datom(b"carol", "knows", b"dave", 2),
        ];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        // Only the asserted edge should be present.
        assert_eq!(
            index.total_edges(),
            1,
            "INV-FERR-083: retracted Ref datoms must not produce edges"
        );

        let carol = EntityId::from_content(b"carol");
        assert!(
            index.neighbors(&carol).is_empty(),
            "INV-FERR-083: retracted edge must not appear in forward index"
        );

        let dave = EntityId::from_content(b"dave");
        assert!(
            index.reverse_neighbors(&dave).is_empty(),
            "INV-FERR-083: retracted edge must not appear in reverse index"
        );
    }

    // -----------------------------------------------------------------------
    // Test 7: Unknown attributes are skipped
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_083_unknown_attribute_skipped() {
        // Intern table does NOT contain "secret-link".
        let intern = make_intern(&["knows"]);
        let datoms = [
            ref_datom(b"alice", "knows", b"bob", 1),
            ref_datom(b"alice", "secret-link", b"carol", 2),
        ];

        let index = AdjacencyIndex::from_canonical(&datoms, &intern);

        // Only "knows" edge should be present; "secret-link" is skipped.
        assert_eq!(
            index.total_edges(),
            1,
            "INV-FERR-083: datoms with un-interned attributes must be skipped"
        );
    }
}
