//! `store` — the G-Set CRDT semilattice: `Store = (P(D), union)`.
//!
//! The `Store` is the core data structure of Ferratomic. It holds an
//! append-only, content-addressed set of datoms with four secondary
//! indexes (EAVT, AEVT, VAET, AVET) maintained in bijection with
//! the primary set.
//!
//! ## Algebraic properties
//!
//! - **INV-FERR-001**: merge is commutative (set union).
//! - **INV-FERR-002**: merge is associative (set union).
//! - **INV-FERR-003**: merge is idempotent (set union).
//! - **INV-FERR-004**: transact is strictly monotonic — the store only grows.
//! - **INV-FERR-005**: secondary indexes are in bijection with the primary set.
//! - **INV-FERR-007**: epochs are strictly monotonically increasing.
//! - **INV-FERR-031**: genesis produces a deterministic store.
//!
//! ## Design (Phase 4a)
//!
//! The primary store uses `im::OrdSet<Datom>` (ADR-FERR-001). Snapshots
//! are O(1) via structural sharing — `clone()` shares the tree spine.
//! All four secondary indexes hold identical copies of the primary set —
//! true per-index sort ordering via newtype key wrappers is deferred to
//! Phase 4b. This satisfies INV-FERR-005 trivially: every index is a
//! clone of the primary.

use std::collections::BTreeSet;

use im::OrdSet;

use ferratom::{AgentId, Attribute, AttributeDef, Datom, FerraError, Schema};

use crate::indexes::Indexes;
use crate::writer::{Committed, Transaction};

// ---------------------------------------------------------------------------
// TxReceipt
// ---------------------------------------------------------------------------

/// Receipt returned by a successful [`Store::transact`] call.
///
/// INV-FERR-007: the epoch field is strictly monotonically increasing
/// across successive transactions on the same store.
#[derive(Debug, Clone)]
pub struct TxReceipt {
    /// The epoch at which this transaction was committed.
    epoch: u64,
}

impl TxReceipt {
    /// The epoch at which this transaction was committed.
    ///
    /// INV-FERR-007: each receipt's epoch is strictly greater than
    /// the epoch of the immediately preceding transaction.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// An immutable point-in-time view of the store.
///
/// INV-FERR-006: a snapshot is frozen at creation time. Later writes
/// to the store do not affect it. `im::OrdSet` clone is O(1) via
/// structural sharing (ADR-FERR-001), so snapshot creation is O(1).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Structurally-shared copy of the datom set at snapshot time.
    /// O(1) clone via `im::OrdSet` (ADR-FERR-001).
    datoms: OrdSet<Datom>,
    /// Epoch at the time the snapshot was taken.
    epoch: u64,
}

impl Snapshot {
    /// Iterate over all datoms visible in this snapshot.
    ///
    /// INV-FERR-006: the iterator yields exactly the datoms that
    /// were present when the snapshot was created — no more, no fewer.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        self.datoms.iter()
    }

    /// The epoch at which this snapshot was taken.
    ///
    /// INV-FERR-011: observer epochs derived from snapshots are
    /// monotonically non-decreasing.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// The G-Set CRDT semilattice: an append-only set of datoms with
/// secondary indexes and schema.
///
/// `Store = (P(D), union)` where `P(D)` is the powerset of all datoms
/// and `union` is the join (least upper bound) operation. Writes are
/// commutative, associative, and idempotent by construction
/// (INV-FERR-001, INV-FERR-002, INV-FERR-003).
///
/// INV-FERR-004: the store only grows. No datom is ever removed.
/// Retractions are new datoms with `Op::Retract`.
#[derive(Debug, Clone)]
pub struct Store {
    /// Primary datom set. The single source of truth.
    /// `im::OrdSet` provides O(1) clone via structural sharing (ADR-FERR-001).
    datoms: OrdSet<Datom>,
    /// Secondary indexes maintained in bijection with the primary set.
    indexes: Indexes,
    /// Attribute definitions governing transact validation.
    schema: Schema,
    /// Monotonically increasing transaction epoch counter.
    /// INV-FERR-007: incremented on every successful transact.
    epoch: u64,
    /// The agent identity used for genesis transactions.
    /// Stored so callers can create transactions against this store.
    genesis_agent: AgentId,
}

impl Store {
    /// Construct a store from a `BTreeSet` of datoms.
    ///
    /// INV-FERR-005: indexes are built from the provided datom set,
    /// ensuring bijection by construction. The schema is empty and
    /// epoch starts at 0.
    ///
    /// Accepts `BTreeSet` for generator/test compatibility and converts
    /// to `im::OrdSet` internally (ADR-FERR-001). For merge, use
    /// [`from_merge`] which preserves schema and epoch.
    #[must_use]
    pub fn from_datoms(datoms: BTreeSet<Datom>) -> Self {
        let ord_set: OrdSet<Datom> = datoms.into_iter().collect();
        let indexes = Indexes::from_datoms(ord_set.iter());
        Self {
            datoms: ord_set,
            indexes,
            schema: Schema::empty(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
        }
    }

    /// Reconstruct a store from checkpoint data.
    ///
    /// INV-FERR-013: Used by `load_checkpoint` to rebuild the store from
    /// serialized epoch, genesis agent, schema attributes, and datoms.
    /// INV-FERR-005: indexes are rebuilt from the datom set by construction.
    #[must_use]
    pub fn from_checkpoint(
        epoch: u64,
        genesis_agent: AgentId,
        schema_attrs: Vec<(String, AttributeDef)>,
        datoms: Vec<Datom>,
    ) -> Self {
        let mut schema = Schema::empty();
        for (name, def) in schema_attrs {
            schema.define(Attribute::from(name.as_str()), def);
        }
        let ord_set: OrdSet<Datom> = datoms.into_iter().collect();
        let indexes = Indexes::from_datoms(ord_set.iter());
        Self {
            datoms: ord_set,
            indexes,
            schema,
            epoch,
            genesis_agent,
        }
    }

    /// Construct a store from merging two stores: union datoms, union schemas,
    /// take max epoch.
    ///
    /// INV-FERR-001..003: datoms are the set union.
    /// INV-FERR-009: schema is the union of both schemas (all attributes from both).
    /// INV-FERR-007: epoch is `max(a.epoch, b.epoch)` — the merged store is at
    /// least as current as either input.
    #[must_use]
    pub fn from_merge(a: &Store, b: &Store) -> Self {
        // im::OrdSet::union is O(n+m) with structural sharing.
        let datoms = a.datoms.clone().union(b.datoms.clone());
        let indexes = Indexes::from_datoms(datoms.iter());

        // Union schemas: all attributes from both stores.
        // INV-FERR-043: shared attributes must have identical definitions.
        // INV-FERR-001: schema merge must be commutative. When both stores
        // define the same attribute with different definitions, we keep the
        // one that sorts first (by Debug representation) for deterministic
        // symmetry. A debug_assert flags the conflict for diagnosis.
        let mut schema = Schema::empty();
        for (attr, def) in a.schema.iter().chain(b.schema.iter()) {
            match schema.get(attr) {
                None => {
                    schema.define(attr.clone(), def.clone());
                }
                Some(existing) => {
                    if existing != def {
                        // INV-FERR-043 violation: conflicting definitions.
                        // Deterministic resolution: keep whichever sorts first.
                        debug_assert!(
                            false,
                            "INV-FERR-043: merge found conflicting schema for {attr:?}: \
                             {existing:?} vs {def:?}. Keeping first in sort order.",
                        );
                        if format!("{def:?}") < format!("{existing:?}") {
                            schema.define(attr.clone(), def.clone());
                        }
                    }
                    // If equal, no-op — already installed.
                }
            }
        }

        let epoch = a.epoch.max(b.epoch);

        Self {
            datoms,
            indexes,
            schema,
            epoch,
            genesis_agent: a.genesis_agent,
        }
    }

    /// Deterministic genesis store with the 19 axiomatic meta-schema attributes.
    ///
    /// INV-FERR-031: every call to `genesis()` produces an identical store.
    /// The 19 attributes are the ONLY hardcoded elements in the engine.
    /// Every other attribute is defined by transacting datoms that reference
    /// these 19. This is the schema-as-data bootstrap (C3, C7).
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            datoms: OrdSet::new(),
            indexes: Indexes::from_datoms(std::iter::empty()),
            schema: crate::schema_evolution::genesis_schema(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
        }
    }

    /// Return a reference to the primary datom set.
    ///
    /// INV-FERR-005: this is the authoritative set. All secondary
    /// indexes are bijective with this set.
    #[must_use]
    pub fn datom_set(&self) -> &OrdSet<Datom> {
        &self.datoms
    }

    /// Iterate over all datoms in the store.
    ///
    /// INV-FERR-004: the iterator yields every datom ever inserted.
    /// No datom is skipped or filtered.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        self.datoms.iter()
    }

    /// Number of datoms in the store.
    ///
    /// INV-FERR-004: this value only increases over the lifetime
    /// of a store (modulo cloning via `from_datoms`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.datoms.len()
    }

    /// Whether the store contains zero datoms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.datoms.is_empty()
    }

    /// Insert a single datom into the store and all indexes.
    ///
    /// INV-FERR-003: inserting a duplicate datom is a no-op (set semantics).
    /// INV-FERR-004: the store never shrinks.
    /// INV-FERR-005: the datom is inserted into all four secondary indexes.
    ///
    /// Used by convergence tests that build stores by individual insertion.
    pub fn insert(&mut self, datom: Datom) {
        self.indexes.insert(&datom);
        self.datoms.insert(datom);
    }

    /// Access the secondary indexes.
    ///
    /// INV-FERR-005: all four indexes are bijective with the primary set.
    #[must_use]
    pub fn indexes(&self) -> &Indexes {
        &self.indexes
    }

    /// Access the schema.
    ///
    /// INV-FERR-009: the schema governs transact-time validation.
    /// INV-FERR-031: for a genesis store, this returns the deterministic
    /// meta-schema.
    #[must_use]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The agent identity associated with this store's genesis.
    ///
    /// Callers use this to construct `Transaction::new(store.genesis_agent())`
    /// when they need to transact against a genesis store without
    /// manufacturing their own agent identity.
    #[must_use]
    pub fn genesis_agent(&self) -> AgentId {
        self.genesis_agent
    }

    /// The current epoch (transaction counter).
    ///
    /// INV-FERR-007: strictly monotonically increasing. Incremented
    /// by each successful `transact` call.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Take an immutable point-in-time snapshot of the store.
    ///
    /// INV-FERR-006: the returned snapshot is frozen. Subsequent
    /// calls to `transact` or `insert` do not affect it.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        // O(1) via im::OrdSet structural sharing (ADR-FERR-001).
        // No Arc wrapper needed — im::OrdSet clone shares the tree spine.
        Snapshot {
            datoms: self.datoms.clone(),
            epoch: self.epoch,
        }
    }

    /// Apply a committed transaction to the store.
    ///
    /// INV-FERR-004: strict growth — the store gains at least one datom.
    /// INV-FERR-005: all indexes are updated in lockstep.
    /// INV-FERR-007: epoch is incremented, producing a strictly greater
    /// epoch than any previous transaction on this store.
    /// INV-FERR-009: schema evolution — if the transaction defines new
    /// attributes (via `db/ident`, `db/valueType`, `db/cardinality`),
    /// they are installed into the schema for future validation.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::EmptyTransaction` if the committed transaction
    /// carries no datoms (should not happen for validly committed
    /// transactions, but defended against per NEG-FERR-001).
    #[allow(clippy::needless_pass_by_value)] // Transaction consumed semantically — applied once
    #[allow(clippy::too_many_lines)] // tx metadata datom creation adds necessary lines
    pub fn transact(&mut self, transaction: Transaction<Committed>) -> Result<TxReceipt, FerraError> {
        let datoms: Vec<Datom> = transaction.datoms().to_vec();

        if datoms.is_empty() {
            return Err(FerraError::EmptyTransaction);
        }

        // INV-FERR-007: advance the epoch before inserting datoms.
        self.epoch = self.epoch.checked_add(1).ok_or_else(|| {
            FerraError::InvariantViolation {
                invariant: "INV-FERR-007".to_string(),
                details: "epoch counter overflow".to_string(),
            }
        })?;

        // INV-FERR-015: Generate a real TxId from the new epoch and
        // the transaction's agent identity. Replaces the placeholder
        // TxId(0,0,0) that datoms carry from the Transaction builder.
        let real_tx_id = ferratom::TxId::with_agent(
            self.epoch,
            0,
            transaction.agent(),
        );

        // Re-stamp datoms with the real TxId.
        let stamped: Vec<Datom> = datoms
            .into_iter()
            .map(|d| {
                Datom::new(
                    d.entity(),
                    d.attribute().clone(),
                    d.value().clone(),
                    real_tx_id,
                    d.op(),
                )
            })
            .collect();

        // INV-FERR-004: Create tx metadata datoms to guarantee strict growth.
        // Every transaction adds at least :tx/time and :tx/agent datoms,
        // so |transact(S, T)| > |S| even if T contains only duplicates.
        let tx_entity = ferratom::EntityId::from_content(
            &format!("tx-{}-{}", self.epoch, transaction.agent().as_bytes()[0]).into_bytes(),
        );
        #[allow(clippy::cast_possible_truncation)] // i64 millis covers 292 million years
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let mut all_datoms = stamped;
        all_datoms.push(Datom::new(
            tx_entity,
            Attribute::from("tx/time"),
            ferratom::Value::Instant(now_ms),
            real_tx_id,
            ferratom::Op::Assert,
        ));
        all_datoms.push(Datom::new(
            tx_entity,
            Attribute::from("tx/agent"),
            ferratom::Value::Ref(ferratom::EntityId::from_content(
                transaction.agent().as_bytes(),
            )),
            real_tx_id,
            ferratom::Op::Assert,
        ));

        // INV-FERR-009: scan for schema-defining datoms and evolve the schema.
        crate::schema_evolution::evolve_schema(&mut self.schema, &all_datoms);

        // INV-FERR-004: insert every datom into primary and all indexes.
        // INV-FERR-005: bijection maintained by touching all structures.
        for datom in all_datoms {
            self.indexes.insert(&datom);
            self.datoms.insert(datom);
        }

        Ok(TxReceipt { epoch: self.epoch })
    }

}

// ---------------------------------------------------------------------------
// Trait implementations
// ---------------------------------------------------------------------------

/// INV-FERR-001..003: Store is a join-semilattice under set union.
/// The merge operation is commutative, associative, and idempotent.
impl ferratom::traits::Semilattice for Store {
    fn merge(&self, other: &Self) -> Self {
        Store::from_merge(self, other)
    }
}

// Note: ContentAddressed for Datom must be impl'd in ferratom crate
// (orphan rule). See ferratom/src/datom.rs — Datom::content_hash()
// already provides the INV-FERR-012 contract.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_evolution::{parse_cardinality, parse_value_type};
    use ferratom::{Cardinality, EntityId, Op, TxId, Value, ValueType};
    use std::sync::Arc;

    /// Helper: build a sample datom for testing.
    fn sample_datom(seed: &str) -> Datom {
        Datom::new(
            EntityId::from_content(seed.as_bytes()),
            Attribute::from("test/name"),
            Value::String(Arc::from(seed)),
            TxId::new(1, 0, 0),
            Op::Assert,
        )
    }

    #[test]
    fn test_from_datoms_preserves_set() {
        let mut set = BTreeSet::new();
        set.insert(sample_datom("a"));
        set.insert(sample_datom("b"));

        let store = Store::from_datoms(set.clone());
        // Compare by converting BTreeSet to OrdSet for type compatibility.
        let expected: im::OrdSet<Datom> = set.into_iter().collect();
        assert_eq!(*store.datom_set(), expected);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_from_datoms_empty() {
        let store = Store::from_datoms(BTreeSet::new());
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_inv_ferr_031_genesis_determinism() {
        let a = Store::genesis();
        let b = Store::genesis();
        assert_eq!(
            a.schema(),
            b.schema(),
            "INV-FERR-031: genesis() must produce identical schemas"
        );
        assert_eq!(a.datom_set(), b.datom_set());
        assert_eq!(a.epoch(), b.epoch());
    }

    #[test]
    fn test_inv_ferr_031_genesis_schema_has_19_attributes() {
        let store = Store::genesis();
        assert_eq!(
            store.schema().len(),
            19,
            "INV-FERR-031: genesis schema must have exactly 19 axiomatic attributes"
        );
        // Core meta-schema (1-9)
        assert!(store.schema().get(&Attribute::from("db/ident")).is_some());
        assert!(store.schema().get(&Attribute::from("db/valueType")).is_some());
        assert!(store.schema().get(&Attribute::from("db/cardinality")).is_some());
        assert!(store.schema().get(&Attribute::from("db/doc")).is_some());
        assert!(store.schema().get(&Attribute::from("db/unique")).is_some());
        assert!(store.schema().get(&Attribute::from("db/isComponent")).is_some());
        assert!(store.schema().get(&Attribute::from("db/resolutionMode")).is_some());
        assert!(store.schema().get(&Attribute::from("db/latticeOrder")).is_some());
        assert!(store.schema().get(&Attribute::from("db/lwwClock")).is_some());
        // Lattice (10-14)
        assert!(store.schema().get(&Attribute::from("lattice/ident")).is_some());
        assert!(store.schema().get(&Attribute::from("lattice/elements")).is_some());
        assert!(store.schema().get(&Attribute::from("lattice/comparator")).is_some());
        assert!(store.schema().get(&Attribute::from("lattice/bottom")).is_some());
        assert!(store.schema().get(&Attribute::from("lattice/top")).is_some());
        // Transaction metadata (15-19)
        assert!(store.schema().get(&Attribute::from("tx/time")).is_some());
        assert!(store.schema().get(&Attribute::from("tx/agent")).is_some());
        assert!(store.schema().get(&Attribute::from("tx/provenance")).is_some());
        assert!(store.schema().get(&Attribute::from("tx/rationale")).is_some());
        assert!(store.schema().get(&Attribute::from("tx/coherence-override")).is_some());
    }

    #[test]
    fn test_inv_ferr_005_index_bijection_from_datoms() {
        let mut set = BTreeSet::new();
        set.insert(sample_datom("x"));
        set.insert(sample_datom("y"));
        set.insert(sample_datom("z"));

        let store = Store::from_datoms(set);
        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
        let aevt: BTreeSet<&Datom> = store.indexes().aevt_datoms().collect();
        let vaet: BTreeSet<&Datom> = store.indexes().vaet_datoms().collect();
        let avet: BTreeSet<&Datom> = store.indexes().avet_datoms().collect();

        assert_eq!(primary, eavt, "INV-FERR-005: EAVT must match primary");
        assert_eq!(primary, aevt, "INV-FERR-005: AEVT must match primary");
        assert_eq!(primary, vaet, "INV-FERR-005: VAET must match primary");
        assert_eq!(primary, avet, "INV-FERR-005: AVET must match primary");
    }

    #[test]
    fn test_inv_ferr_005_index_bijection_after_insert() {
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(sample_datom("inserted"));

        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
        assert_eq!(primary, eavt, "INV-FERR-005: EAVT must match primary after insert");
        assert_eq!(primary.len(), 1);
    }

    #[test]
    fn test_inv_ferr_003_insert_duplicate_is_noop() {
        let d = sample_datom("dup");
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(d.clone());
        store.insert(d);
        assert_eq!(store.len(), 1, "INV-FERR-003: duplicate insert must be idempotent");
    }

    #[test]
    fn test_genesis_is_empty_of_datoms() {
        let store = Store::genesis();
        assert!(store.is_empty(), "genesis store must have zero datoms");
    }

    #[test]
    fn test_snapshot_is_frozen() {
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(sample_datom("before"));

        let snap = store.snapshot();
        let snap_set_before: BTreeSet<&Datom> = snap.datoms().collect();

        store.insert(sample_datom("after"));

        // bd-3bg regression: compare datom SETS, not counts.
        let snap_set_after: BTreeSet<&Datom> = snap.datoms().collect();
        assert_eq!(
            snap_set_before, snap_set_after,
            "INV-FERR-006: snapshot datom set must not change after later inserts"
        );
        assert_eq!(snap_set_before.len(), 1, "snapshot should have exactly 1 datom");
    }

    #[test]
    fn test_parse_value_type_all_variants() {
        assert_eq!(parse_value_type("db.type/keyword"), Some(ValueType::Keyword));
        assert_eq!(parse_value_type("db.type/string"), Some(ValueType::String));
        assert_eq!(parse_value_type("db.type/long"), Some(ValueType::Long));
        assert_eq!(parse_value_type("db.type/double"), Some(ValueType::Double));
        assert_eq!(parse_value_type("db.type/boolean"), Some(ValueType::Boolean));
        assert_eq!(parse_value_type("db.type/instant"), Some(ValueType::Instant));
        assert_eq!(parse_value_type("db.type/uuid"), Some(ValueType::Uuid));
        assert_eq!(parse_value_type("db.type/bytes"), Some(ValueType::Bytes));
        assert_eq!(parse_value_type("db.type/ref"), Some(ValueType::Ref));
        assert_eq!(parse_value_type("db.type/bigint"), Some(ValueType::BigInt));
        assert_eq!(parse_value_type("db.type/bigdec"), Some(ValueType::BigDec));
        assert_eq!(parse_value_type("db.type/unknown"), None);
    }

    #[test]
    fn test_parse_cardinality_variants() {
        assert_eq!(parse_cardinality("db.cardinality/one"), Some(Cardinality::One));
        assert_eq!(parse_cardinality("db.cardinality/many"), Some(Cardinality::Many));
        assert_eq!(parse_cardinality("db.cardinality/unknown"), None);
    }

    // -- Regression tests for cleanroom review defects -------------------------

    /// Regression: bd-10p — merge() must preserve schema from both stores.
    #[test]
    fn test_bug_bd_10p_merge_preserves_schema() {
        use crate::merge::merge;

        let a = Store::genesis(); // has 19 schema attributes
        let b = Store::genesis();

        let merged = merge(&a, &b);
        assert_eq!(
            merged.schema().len(),
            19,
            "bd-10p: merge must preserve schema — expected 19 genesis attributes, got {}",
            merged.schema().len()
        );
        assert!(
            merged.schema().get(&Attribute::from("db/ident")).is_some(),
            "bd-10p: merge lost db/ident"
        );
    }

    /// Regression: bd-10p — merge() must take max epoch.
    #[test]
    fn test_bug_bd_10p_merge_preserves_epoch() {
        use crate::merge::merge;
        use crate::writer::Transaction;

        let mut a = Store::genesis();
        // Transact to advance epoch to 1
        let tx = Transaction::new(AgentId::from_bytes([1u8; 16]))
            .assert_datom(
                EntityId::from_content(b"e1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("test")),
            )
            .commit(a.schema())
            .expect("valid tx");
        a.transact(tx).expect("transact ok");
        assert_eq!(a.epoch(), 1);

        let b = Store::genesis(); // epoch 0

        let merged = merge(&a, &b);
        assert_eq!(
            merged.epoch(),
            1,
            "bd-10p: merge must take max(epoch_a, epoch_b) = max(1, 0) = 1"
        );
    }

    /// Regression: bd-1n6 — transact() must stamp real TxId, not placeholder.
    #[test]
    fn test_bug_bd_1n6_transact_stamps_real_tx_id() {
        use crate::writer::Transaction;

        let mut store = Store::genesis();
        let agent = AgentId::from_bytes([42u8; 16]);
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"e1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("test")),
            )
            .commit(store.schema())
            .expect("valid tx");

        store.transact(tx).expect("transact ok");

        // Every datom in the store should have a non-placeholder TxId.
        let placeholder = TxId::new(0, 0, 0);
        for datom in store.datoms() {
            assert_ne!(
                datom.tx(),
                placeholder,
                "bd-1n6: datom has placeholder TxId(0,0,0) — transact must stamp real TxId. \
                 datom={:?}",
                datom
            );
        }

        // The tx should carry the agent we specified.
        let last_datom = store.datoms().last().expect("store not empty");
        assert_eq!(
            last_datom.tx().agent(),
            agent,
            "bd-1n6: TxId agent must match transaction agent"
        );

        // Epoch should be in the TxId physical component.
        assert_eq!(
            last_datom.tx().physical(),
            1, // epoch 1 after first transact
            "bd-1n6: TxId physical must equal epoch"
        );
    }

    /// Regression: bd-3n6 — merge stores with disjoint schemas unions all attributes.
    #[test]
    fn test_bug_bd_3n6_merge_disjoint_schemas() {
        use crate::merge::merge;

        let mut a = Store::genesis();
        let mut b = Store::genesis();

        // Evolve A's schema: add user/name (String)
        let tx_a = crate::writer::Transaction::new(AgentId::from_bytes([1u8; 16]))
            .assert_datom(
                EntityId::from_content(b"attr-user-name"),
                Attribute::from("db/ident"),
                Value::Keyword("user/name".into()),
            )
            .assert_datom(
                EntityId::from_content(b"attr-user-name"),
                Attribute::from("db/valueType"),
                Value::Keyword("db.type/string".into()),
            )
            .assert_datom(
                EntityId::from_content(b"attr-user-name"),
                Attribute::from("db/cardinality"),
                Value::Keyword("db.cardinality/one".into()),
            )
            .commit(a.schema())
            .expect("valid schema tx");
        a.transact(tx_a).expect("transact a ok");

        // Evolve B's schema: add user/age (Long)
        let tx_b = crate::writer::Transaction::new(AgentId::from_bytes([2u8; 16]))
            .assert_datom(
                EntityId::from_content(b"attr-user-age"),
                Attribute::from("db/ident"),
                Value::Keyword("user/age".into()),
            )
            .assert_datom(
                EntityId::from_content(b"attr-user-age"),
                Attribute::from("db/valueType"),
                Value::Keyword("db.type/long".into()),
            )
            .assert_datom(
                EntityId::from_content(b"attr-user-age"),
                Attribute::from("db/cardinality"),
                Value::Keyword("db.cardinality/one".into()),
            )
            .commit(b.schema())
            .expect("valid schema tx");
        b.transact(tx_b).expect("transact b ok");

        // A has: genesis 4 + user/name = 5 attrs
        // B has: genesis 4 + user/age = 5 attrs
        assert!(a.schema().get(&Attribute::from("user/name")).is_some());
        assert!(b.schema().get(&Attribute::from("user/age")).is_some());

        let merged = merge(&a, &b);

        // Merged must have genesis 19 + user/name + user/age = 21
        assert_eq!(
            merged.schema().len(),
            21,
            "bd-3n6: disjoint schema merge must union all attributes. \
             Expected 21 (19 genesis + user/name + user/age), got {}",
            merged.schema().len()
        );
        assert!(merged.schema().get(&Attribute::from("user/name")).is_some());
        assert!(merged.schema().get(&Attribute::from("user/age")).is_some());

        // Commutativity: merge(b, a) must produce identical schema
        let merged_ba = merge(&b, &a);
        assert_eq!(
            merged.schema().len(),
            merged_ba.schema().len(),
            "bd-3n6: merge schema must be commutative"
        );
    }

    /// Regression: bd-3n6 — merge is commutative even for schema.
    #[test]
    fn test_bug_bd_3n6_merge_schema_commutativity() {
        use crate::merge::merge;

        let a = Store::genesis();
        let b = Store::genesis();

        let ab = merge(&a, &b);
        let ba = merge(&b, &a);

        // Schema must be identical regardless of merge order.
        assert_eq!(
            ab.schema(),
            ba.schema(),
            "bd-3n6: merge(A,B).schema must equal merge(B,A).schema"
        );
    }

    /// bd-20j: Semilattice trait is usable via generic bounds.
    #[test]
    fn test_semilattice_trait_bound() {
        use ferratom::traits::Semilattice;

        fn requires_semilattice<T: Semilattice>(a: &T, b: &T) -> T {
            a.merge(b)
        }

        let a = Store::genesis();
        let b = Store::genesis();
        let merged = requires_semilattice(&a, &b);
        assert_eq!(merged.epoch(), 0, "bd-20j: Semilattice merge of genesis stores");
    }

    /// bd-20j: ContentAddressed trait is usable via generic bounds.
    #[test]
    fn test_content_addressed_trait_bound() {
        use ferratom::traits::ContentAddressed;

        fn requires_content_addressed<T: ContentAddressed>(x: &T) -> [u8; 32] {
            x.content_hash()
        }

        let datom = sample_datom("trait-test");
        let hash = requires_content_addressed(&datom);
        assert_ne!(hash, [0u8; 32], "bd-20j: ContentAddressed must produce non-zero hash");
    }
}
