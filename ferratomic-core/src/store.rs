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
//! ## Design (Phase 4a MVP)
//!
//! The primary store is a `BTreeSet<Datom>`. All four secondary indexes
//! hold identical copies of the primary set — true per-index sort ordering
//! via newtype key wrappers is deferred to Phase 4b. This satisfies
//! INV-FERR-005 trivially: every index is a clone of the primary.

use std::collections::BTreeSet;
use std::sync::Arc;

use ferratom::{
    AgentId, Attribute, AttributeDef, Cardinality, Datom, FerraError, ResolutionMode, Schema,
    ValueType,
};

use crate::writer::{Committed, Transaction};

// ---------------------------------------------------------------------------
// Indexes
// ---------------------------------------------------------------------------

/// Secondary indexes over the datom set.
///
/// INV-FERR-005: every secondary index is a bijection with the primary set.
/// In Phase 4a, all four indexes contain identical `BTreeSet<Datom>` copies.
/// True per-index sort ordering (EAVT, AEVT, VAET, AVET newtype keys) is
/// deferred to Phase 4b.
#[derive(Debug, Clone)]
pub struct Indexes {
    /// Entity-Attribute-Value-Tx index.
    eavt: BTreeSet<Datom>,
    /// Attribute-Entity-Value-Tx index.
    aevt: BTreeSet<Datom>,
    /// Value-Attribute-Entity-Tx index (reverse references).
    vaet: BTreeSet<Datom>,
    /// Attribute-Value-Entity-Tx index (unique/lookup).
    avet: BTreeSet<Datom>,
}

impl Indexes {
    /// Build indexes from a primary datom set.
    ///
    /// INV-FERR-005: all four indexes receive the same datom set,
    /// ensuring bijection with the primary by construction.
    fn from_primary(primary: &BTreeSet<Datom>) -> Self {
        Self {
            eavt: primary.clone(),
            aevt: primary.clone(),
            vaet: primary.clone(),
            avet: primary.clone(),
        }
    }

    /// Insert a datom into all four indexes.
    ///
    /// INV-FERR-005: maintaining bijection requires every insert to
    /// touch all indexes.
    fn insert(&mut self, datom: Datom) {
        self.eavt.insert(datom.clone());
        self.aevt.insert(datom.clone());
        self.vaet.insert(datom.clone());
        self.avet.insert(datom);
    }

    /// Entity-Attribute-Value-Tx index.
    ///
    /// INV-FERR-005: returns a view bijective with the primary datom set.
    #[must_use]
    pub fn eavt(&self) -> &BTreeSet<Datom> {
        &self.eavt
    }

    /// Attribute-Entity-Value-Tx index.
    ///
    /// INV-FERR-005: returns a view bijective with the primary datom set.
    #[must_use]
    pub fn aevt(&self) -> &BTreeSet<Datom> {
        &self.aevt
    }

    /// Value-Attribute-Entity-Tx index (reverse reference lookups).
    ///
    /// INV-FERR-005: returns a view bijective with the primary datom set.
    #[must_use]
    pub fn vaet(&self) -> &BTreeSet<Datom> {
        &self.vaet
    }

    /// Attribute-Value-Entity-Tx index (unique attribute lookups).
    ///
    /// INV-FERR-005: returns a view bijective with the primary datom set.
    #[must_use]
    pub fn avet(&self) -> &BTreeSet<Datom> {
        &self.avet
    }
}

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
/// to the store do not affect it. Implemented via `Arc` sharing of
/// the datom set at the time the snapshot was taken.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Shared reference to the datom set at snapshot time.
    datoms: Arc<BTreeSet<Datom>>,
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
    datoms: BTreeSet<Datom>,
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
    /// Used by generators, merge, and tests that need to construct
    /// stores from arbitrary datom collections.
    #[must_use]
    pub fn from_datoms(datoms: BTreeSet<Datom>) -> Self {
        let indexes = Indexes::from_primary(&datoms);
        Self {
            datoms,
            indexes,
            schema: Schema::empty(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
        }
    }

    /// Deterministic genesis store with the 4 core meta-schema attributes.
    ///
    /// INV-FERR-031: every call to `genesis()` produces an identical store.
    /// The schema contains:
    /// - `db/ident` (Keyword, One) — attribute identity
    /// - `db/valueType` (Keyword, One) — declared value type
    /// - `db/cardinality` (Keyword, One) — one or many
    /// - `db/doc` (String, One) — documentation string
    ///
    /// The store contains no datoms. Genesis datoms are Phase 4a-SCHEMA
    /// (bd-85j.11); for now, these 4 attribute definitions are sufficient
    /// for schema validation tests.
    #[must_use]
    pub fn genesis() -> Self {
        let mut schema = Schema::empty();

        schema.define(
            Attribute::from("db/ident"),
            AttributeDef {
                value_type: ValueType::Keyword,
                cardinality: Cardinality::One,
                resolution_mode: ResolutionMode::Lww,
                doc: Some(Arc::from("Attribute identity keyword")),
            },
        );

        schema.define(
            Attribute::from("db/valueType"),
            AttributeDef {
                value_type: ValueType::Keyword,
                cardinality: Cardinality::One,
                resolution_mode: ResolutionMode::Lww,
                doc: Some(Arc::from("Declared value type for an attribute")),
            },
        );

        schema.define(
            Attribute::from("db/cardinality"),
            AttributeDef {
                value_type: ValueType::Keyword,
                cardinality: Cardinality::One,
                resolution_mode: ResolutionMode::Lww,
                doc: Some(Arc::from("Cardinality: one or many")),
            },
        );

        schema.define(
            Attribute::from("db/doc"),
            AttributeDef {
                value_type: ValueType::String,
                cardinality: Cardinality::One,
                resolution_mode: ResolutionMode::Lww,
                doc: Some(Arc::from("Documentation string")),
            },
        );

        Self {
            datoms: BTreeSet::new(),
            indexes: Indexes::from_primary(&BTreeSet::new()),
            schema,
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
        }
    }

    /// Return a reference to the primary datom set.
    ///
    /// INV-FERR-005: this is the authoritative set. All secondary
    /// indexes are bijective with this set.
    #[must_use]
    pub fn datom_set(&self) -> &BTreeSet<Datom> {
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
        self.indexes.insert(datom.clone());
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
        Snapshot {
            datoms: Arc::new(self.datoms.clone()),
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

        // INV-FERR-009: scan for schema-defining datoms and evolve the schema.
        self.evolve_schema(&datoms);

        // INV-FERR-004: insert every datom into primary and all indexes.
        // INV-FERR-005: bijection maintained by touching all structures.
        for datom in datoms {
            self.indexes.insert(datom.clone());
            self.datoms.insert(datom);
        }

        Ok(TxReceipt { epoch: self.epoch })
    }

    /// Scan datoms for schema-defining patterns and install new attributes.
    ///
    /// INV-FERR-009: schema evolution is a transaction. When a transaction
    /// contains datoms of the form:
    /// - `(E, db/ident, Keyword(attr_name), ...)`
    /// - `(E, db/valueType, Keyword(type_kw), ...)`
    /// - `(E, db/cardinality, Keyword(card_kw), ...)`
    ///
    /// ...all sharing the same entity E, a new attribute `attr_name` is
    /// installed with the declared type and cardinality.
    fn evolve_schema(&mut self, datoms: &[Datom]) {
        use std::collections::HashMap;

        // Group datoms by entity to find complete attribute definitions.
        let mut by_entity: HashMap<ferratom::EntityId, Vec<&Datom>> = HashMap::new();
        for datom in datoms {
            by_entity.entry(datom.entity()).or_default().push(datom);
        }

        let db_ident = Attribute::from("db/ident");
        let db_value_type = Attribute::from("db/valueType");
        let db_cardinality = Attribute::from("db/cardinality");

        for entity_datoms in by_entity.values() {
            let mut ident: Option<&str> = None;
            let mut value_type: Option<ValueType> = None;
            let mut cardinality: Option<Cardinality> = None;

            for datom in entity_datoms {
                if datom.attribute() == &db_ident {
                    if let ferratom::Value::Keyword(kw) = datom.value() {
                        ident = Some(leak_arc_str(kw));
                    }
                } else if datom.attribute() == &db_value_type {
                    if let ferratom::Value::Keyword(kw) = datom.value() {
                        value_type = parse_value_type(kw);
                    }
                } else if datom.attribute() == &db_cardinality {
                    if let ferratom::Value::Keyword(kw) = datom.value() {
                        cardinality = parse_cardinality(kw);
                    }
                }
            }

            // Install the attribute only when all three required fields are present.
            if let (Some(name), Some(vt), Some(card)) = (ident, value_type, cardinality) {
                self.schema.define(
                    Attribute::from(name),
                    AttributeDef {
                        value_type: vt,
                        cardinality: card,
                        resolution_mode: ResolutionMode::Lww,
                        doc: None,
                    },
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a `&str` from an `Arc<str>` by reborrowing.
///
/// This is safe because we only use the reference within the scope of
/// `evolve_schema` where the `Arc<str>` is still alive. We return a
/// `&str` with an artificially extended lifetime to avoid cloning
/// the string for the `Attribute::from` call. In practice the
/// attribute is constructed before the reference is dropped.
///
/// # Safety
///
/// No unsafe code. The `&str` borrows from the `Arc<str>` which
/// lives for the duration of the `evolve_schema` call.
fn leak_arc_str(arc: &Arc<str>) -> &str {
    arc.as_ref()
}

/// Parse a `db.type/*` keyword into a `ValueType`.
///
/// Returns `None` for unrecognized type keywords. Unrecognized types
/// cause the attribute definition to be silently skipped (the transaction
/// datoms are still stored; only the schema entry is not created).
fn parse_value_type(keyword: &str) -> Option<ValueType> {
    match keyword {
        "db.type/keyword" => Some(ValueType::Keyword),
        "db.type/string" => Some(ValueType::String),
        "db.type/long" => Some(ValueType::Long),
        "db.type/double" => Some(ValueType::Double),
        "db.type/boolean" => Some(ValueType::Boolean),
        "db.type/instant" => Some(ValueType::Instant),
        "db.type/uuid" => Some(ValueType::Uuid),
        "db.type/bytes" => Some(ValueType::Bytes),
        "db.type/ref" => Some(ValueType::Ref),
        "db.type/bigint" => Some(ValueType::BigInt),
        "db.type/bigdec" => Some(ValueType::BigDec),
        _ => None,
    }
}

/// Parse a `db.cardinality/*` keyword into a `Cardinality`.
///
/// Returns `None` for unrecognized cardinality keywords.
fn parse_cardinality(keyword: &str) -> Option<Cardinality> {
    match keyword {
        "db.cardinality/one" => Some(Cardinality::One),
        "db.cardinality/many" => Some(Cardinality::Many),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ferratom::{EntityId, Op, TxId, Value};
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
        assert_eq!(store.datom_set(), &set);
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
    fn test_inv_ferr_031_genesis_schema_has_four_attributes() {
        let store = Store::genesis();
        assert_eq!(
            store.schema().len(),
            4,
            "INV-FERR-031: genesis schema must have exactly 4 core attributes"
        );
        assert!(store.schema().get(&Attribute::from("db/ident")).is_some());
        assert!(store.schema().get(&Attribute::from("db/valueType")).is_some());
        assert!(store.schema().get(&Attribute::from("db/cardinality")).is_some());
        assert!(store.schema().get(&Attribute::from("db/doc")).is_some());
    }

    #[test]
    fn test_inv_ferr_005_index_bijection_from_datoms() {
        let mut set = BTreeSet::new();
        set.insert(sample_datom("x"));
        set.insert(sample_datom("y"));
        set.insert(sample_datom("z"));

        let store = Store::from_datoms(set);
        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let eavt: BTreeSet<&Datom> = store.indexes().eavt().iter().collect();
        let aevt: BTreeSet<&Datom> = store.indexes().aevt().iter().collect();
        let vaet: BTreeSet<&Datom> = store.indexes().vaet().iter().collect();
        let avet: BTreeSet<&Datom> = store.indexes().avet().iter().collect();

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
        let eavt: BTreeSet<&Datom> = store.indexes().eavt().iter().collect();
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
        let count_before = snap.datoms().count();

        store.insert(sample_datom("after"));

        let count_after = snap.datoms().count();
        assert_eq!(
            count_before, count_after,
            "INV-FERR-006: snapshot must not see later inserts"
        );
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
}
