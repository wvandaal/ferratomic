//! Transaction application, WAL replay, and merge construction.
//!
//! INV-FERR-004: Strict monotonic growth -- the store only grows.
//! INV-FERR-005: Secondary indexes updated in lockstep with primary set.
//! INV-FERR-007: Epoch strictly monotonically increasing on each transact.
//! INV-FERR-009: Schema evolution via schema-defining datoms.
//! INV-FERR-010: Merge convergence — `from_merge` constructs SEC-convergent state.
//! INV-FERR-014: WAL replay restores last committed state.
//!
//! This module contains all mutating operations on the [`Store`]:
//! - [`Store::insert`] — single-datom insertion (used by convergence tests).
//! - [`Store::replay_entry`] — WAL replay during crash recovery.
//! - [`Store::from_merge`] — construct a merged store from two inputs.
//! - [`Store::transact`] — apply a committed transaction (epoch advance,
//!   `TxId` stamping, schema evolution, index maintenance).
//!
//! Private helpers [`stamp_datoms`] and [`create_tx_metadata`] live here
//! because they are only called by [`Store::transact`].

use ferratom::{AgentId, Attribute, Datom, FerraError};

use super::{Store, TxReceipt};
use crate::writer::{Committed, Transaction};

// ---------------------------------------------------------------------------
// Mutating methods on Store
// ---------------------------------------------------------------------------

impl Store {
    /// Insert a single datom into the store and all indexes.
    ///
    /// INV-FERR-003: inserting a duplicate datom is a no-op (set semantics).
    /// INV-FERR-004: the store never shrinks.
    /// INV-FERR-005: the datom is inserted into all four secondary indexes.
    ///
    /// Used by convergence tests that build stores by individual insertion.
    pub fn insert(&mut self, datom: &Datom) {
        // INV-FERR-005: primary first, then indexes. If a panic occurs
        // between the two operations, the datom is in primary but missing
        // from indexes (recoverable by rebuild) rather than a phantom
        // index entry (no primary counterpart).
        self.datoms.insert(datom.clone());
        self.indexes.insert(datom);
        // INV-FERR-029: maintain LIVE set incrementally.
        self.live_apply(datom);
    }

    /// Replay a WAL entry during crash recovery.
    ///
    /// INV-FERR-014: restores committed state by inserting datoms and
    /// advancing the epoch. Used by `Database::open` during WAL replay.
    ///
    /// # Errors
    ///
    /// Currently infallible but returns `Result` for forward compatibility.
    pub fn replay_entry(&mut self, epoch: u64, datoms: &[Datom]) -> Result<(), FerraError> {
        for datom in datoms {
            self.insert(datom);
        }
        self.epoch = epoch;
        // CR-001: Schema-defining datoms in the WAL must be installed into the
        // schema during recovery, otherwise the schema is lost after crash.
        // INV-FERR-009: evolve_schema scans for db/ident + db/valueType +
        // db/cardinality triples and installs new attributes.
        crate::schema_evolution::evolve_schema(&mut self.schema, datoms)?;
        Ok(())
    }

    /// Apply a committed transaction to the store.
    ///
    /// INV-FERR-004: strict growth -- the store gains at least one datom.
    /// INV-FERR-005: all indexes are updated in lockstep.
    /// INV-FERR-007: epoch is incremented, producing a strictly greater
    /// epoch than any previous transaction on this store.
    /// INV-FERR-009: schema evolution -- if the transaction defines new
    /// attributes (via `db/ident`, `db/valueType`, `db/cardinality`),
    /// they are installed into the schema for future validation.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::EmptyTransaction` if the committed transaction
    /// carries no datoms (should not happen for validly committed
    /// transactions, but defended against per NEG-FERR-001).
    /// HI-011: `tx_id` is provided by the caller (`Database::transact` ticks
    /// the `HybridClock` under the write lock). This replaces the previous
    /// `TxId::with_agent(epoch, 0, agent)` which used epoch-as-physical —
    /// breaking INV-FERR-015 (HLC monotonicity) and INV-FERR-016 (causality).
    pub fn transact(
        &mut self,
        transaction: Transaction<Committed>,
        tx_id: ferratom::TxId,
    ) -> Result<TxReceipt, FerraError> {
        // INV-FERR-020: extract datoms and agent, then consume the transaction.
        // Ownership transfer enforces single-application: a committed transaction
        // cannot be applied twice.
        let datoms = transaction.datoms().to_vec();
        let agent = transaction.agent();
        drop(transaction);
        if datoms.is_empty() {
            return Err(FerraError::EmptyTransaction);
        }

        // INV-FERR-007: advance epoch strictly.
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| FerraError::InvariantViolation {
                invariant: "INV-FERR-007".to_string(),
                details: "epoch counter overflow".to_string(),
            })?;

        // INV-FERR-015: stamp datoms with HLC-derived TxId + append tx metadata.
        let mut all_datoms = stamp_datoms(datoms, tx_id);
        all_datoms.extend(create_tx_metadata(self.epoch, agent, tx_id));

        // INV-FERR-009: evolve schema from schema-defining datoms.
        crate::schema_evolution::evolve_schema(&mut self.schema, &all_datoms)?;

        // INV-FERR-004/005: insert into primary then indexes (bd-4pg).
        // MI-007: Insert from references, then move vec into receipt
        // (eliminates receipt_datoms.clone() double-clone).
        for datom in &all_datoms {
            self.datoms.insert(datom.clone());
            self.indexes.insert(datom);
            self.live_apply(datom);
        }

        Ok(TxReceipt {
            epoch: self.epoch,
            datoms: all_datoms,
        })
    }

    /// Test-only convenience: transact with a synthetic epoch-based `TxId`.
    ///
    /// Production code MUST use `transact(tx, hlc_tx_id)` with an HLC-derived
    /// `TxId` from `Database::transact`. This method exists only so that tests
    /// calling `Store::transact` directly (without a `Database`) don't need
    /// to construct an HLC.
    ///
    /// # Errors
    ///
    /// Delegates to [`Store::transact`]; returns the same error variants.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn transact_test(
        &mut self,
        transaction: Transaction<Committed>,
    ) -> Result<TxReceipt, FerraError> {
        let agent = transaction.agent();
        let tx_id = ferratom::TxId::with_agent(self.epoch.wrapping_add(1), 0, agent);
        self.transact(transaction, tx_id)
    }
}

// ---------------------------------------------------------------------------
// Transaction helpers (private)
// ---------------------------------------------------------------------------

/// INV-FERR-015: Re-stamp datoms with a real `TxId`, replacing the
/// placeholder `TxId(0,0,0)` from the Transaction builder.
fn stamp_datoms(datoms: Vec<Datom>, tx_id: ferratom::TxId) -> Vec<Datom> {
    datoms
        .into_iter()
        .map(|d| {
            Datom::new(
                d.entity(),
                d.attribute().clone(),
                d.value().clone(),
                tx_id,
                d.op(),
            )
        })
        .collect()
}

/// INV-FERR-004: Create :tx/time and :tx/agent metadata datoms that
/// guarantee strict growth (every transaction adds at least 2 datoms).
fn create_tx_metadata(epoch: u64, agent: AgentId, tx_id: ferratom::TxId) -> Vec<Datom> {
    // P1-003: Use full agent bytes for tx_entity derivation, not just
    // first byte. Prevents collision when two agents share the same
    // first byte but differ in subsequent bytes.
    let mut tx_content = format!("tx-{epoch}-").into_bytes();
    tx_content.extend_from_slice(agent.as_bytes());
    let tx_entity = ferratom::EntityId::from_content(&tx_content);
    let now_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX);

    vec![
        Datom::new(
            tx_entity,
            Attribute::from("tx/time"),
            ferratom::Value::Instant(now_ms),
            tx_id,
            ferratom::Op::Assert,
        ),
        Datom::new(
            tx_entity,
            Attribute::from("tx/agent"),
            ferratom::Value::Ref(ferratom::EntityId::from_content(agent.as_bytes())),
            tx_id,
            ferratom::Op::Assert,
        ),
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use ferratom::{AgentId, Attribute, EntityId, Op, TxId, Value};

    use super::*;

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
    fn test_inv_ferr_005_index_bijection_after_insert() {
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(&sample_datom("inserted"));

        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
        assert_eq!(
            primary, eavt,
            "INV-FERR-005: EAVT must match primary after insert"
        );
        assert_eq!(primary.len(), 1);
    }

    #[test]
    fn test_inv_ferr_003_insert_duplicate_is_noop() {
        let d = sample_datom("dup");
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(&d);
        store.insert(&d);
        assert_eq!(
            store.len(),
            1,
            "INV-FERR-003: duplicate insert must be idempotent"
        );
    }

    /// Regression: bd-10p -- `merge()` must preserve schema from both stores.
    #[test]
    fn test_bug_bd_10p_merge_preserves_schema() {
        use crate::merge::merge;

        let a = Store::genesis(); // has 19 schema attributes
        let b = Store::genesis();

        let merged = merge(&a, &b).expect("merge genesis stores");
        assert_eq!(
            merged.schema().len(),
            19,
            "bd-10p: merge must preserve schema -- expected 19 genesis attributes, got {}",
            merged.schema().len()
        );
        assert!(
            merged.schema().get(&Attribute::from("db/ident")).is_some(),
            "bd-10p: merge lost db/ident"
        );
    }

    /// Regression: bd-10p -- `merge()` must take max epoch.
    #[test]
    fn test_bug_bd_10p_merge_preserves_epoch() {
        use crate::{merge::merge, writer::Transaction};

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
        a.transact_test(tx).expect("transact ok");
        assert_eq!(a.epoch(), 1);

        let b = Store::genesis(); // epoch 0

        let merged = merge(&a, &b).expect("merge stores");
        assert_eq!(
            merged.epoch(),
            1,
            "bd-10p: merge must take max(epoch_a, epoch_b) = max(1, 0) = 1"
        );
    }

    /// Regression: bd-1n6 -- `transact()` must stamp real `TxId`, not placeholder.
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

        store.transact_test(tx).expect("transact ok");

        // Every datom in the store should have a non-placeholder TxId.
        let placeholder = TxId::new(0, 0, 0);
        for datom in store.datoms() {
            assert_ne!(
                datom.tx(),
                placeholder,
                "bd-1n6: datom has placeholder TxId(0,0,0) -- transact must stamp real TxId. \
                 datom={datom:?}"
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

    /// Helper: evolve a store's schema by adding a new attribute with the given
    /// ident, value-type keyword, and agent byte.
    fn evolve_schema(
        store: &mut Store,
        content_seed: &[u8],
        ident: &str,
        value_type: &str,
        agent_byte: u8,
    ) {
        let tx = crate::writer::Transaction::new(AgentId::from_bytes([agent_byte; 16]))
            .assert_datom(
                EntityId::from_content(content_seed),
                Attribute::from("db/ident"),
                Value::Keyword(ident.into()),
            )
            .assert_datom(
                EntityId::from_content(content_seed),
                Attribute::from("db/valueType"),
                Value::Keyword(value_type.into()),
            )
            .assert_datom(
                EntityId::from_content(content_seed),
                Attribute::from("db/cardinality"),
                Value::Keyword("db.cardinality/one".into()),
            )
            .commit(store.schema())
            .expect("valid schema tx");
        store.transact_test(tx).expect("transact ok");
    }

    /// Regression: bd-3n6 -- merge stores with disjoint schemas unions all attributes.
    #[test]
    fn test_bug_bd_3n6_merge_disjoint_schemas() {
        use crate::merge::merge;

        let mut a = Store::genesis();
        let mut b = Store::genesis();

        evolve_schema(&mut a, b"attr-user-name", "user/name", "db.type/string", 1);
        evolve_schema(&mut b, b"attr-user-age", "user/age", "db.type/long", 2);

        assert!(a.schema().get(&Attribute::from("user/name")).is_some());
        assert!(b.schema().get(&Attribute::from("user/age")).is_some());

        let merged = merge(&a, &b).expect("merge disjoint schema stores");

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
        let merged_ba = merge(&b, &a).expect("merge reverse direction");
        assert_eq!(
            merged.schema().len(),
            merged_ba.schema().len(),
            "bd-3n6: merge schema must be commutative"
        );
    }

    /// Regression: bd-3n6 -- merge is commutative even for schema.
    #[test]
    fn test_bug_bd_3n6_merge_schema_commutativity() {
        use crate::merge::merge;

        let a = Store::genesis();
        let b = Store::genesis();

        let ab = merge(&a, &b).expect("merge A,B");
        let ba = merge(&b, &a).expect("merge B,A");

        // Schema must be identical regardless of merge order.
        assert_eq!(
            ab.schema(),
            ba.schema(),
            "bd-3n6: merge(A,B).schema must equal merge(B,A).schema"
        );
    }
}
