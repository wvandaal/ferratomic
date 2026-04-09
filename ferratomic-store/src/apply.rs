//! Transaction application, WAL replay, and merge construction.
//!
//! INV-FERR-004: Strict monotonic growth -- the store only grows.
//! INV-FERR-005: Secondary indexes updated in lockstep with primary set.
//! INV-FERR-007: Epoch strictly monotonically increasing on each transact.
//! INV-FERR-009: Schema evolution via schema-defining datoms.
//! INV-FERR-010: Merge convergence -- `from_merge` constructs SEC-convergent state.
//! INV-FERR-014: WAL replay restores last committed state.
//!
//! This module contains all mutating operations on the [`Store`]:
//! - [`Store::insert`] -- single-datom insertion (used by convergence tests).
//! - [`Store::replay_entry`] -- WAL replay during crash recovery.
//! - [`Store::from_merge`] -- construct a merged store from two inputs.
//! - [`Store::transact`] -- apply a committed transaction (epoch advance,
//!   `TxId` stamping, schema evolution, index maintenance).
//! - [`Store::batch_splice_transact`] -- apply multiple committed transactions
//!   in a single merge-sort splice (bd-ks5d).
//!
//! Private helpers [`stamp_datoms`] and [`create_tx_metadata`] live here
//! because they are only called by [`Store::transact`] and
//! [`Store::batch_splice_transact`].

use std::sync::Arc;

use ferratom::{Attribute, Datom, FerraError, NodeId};
use ferratomic_positional::{merge_sort_dedup, PositionalStore};
use ferratomic_tx::{Committed, Transaction};

use crate::{
    repr::StoreRepr,
    store::{Store, TxReceipt},
};

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
    /// bd-h2fz: promotes from Positional to `OrdMap` on first write, then
    /// inserts into the `OrdMap` variant.
    ///
    /// Used by convergence tests that build stores by individual insertion.
    pub fn insert(&mut self, datom: &Datom) {
        // bd-h2fz: lazy promotion -- Positional -> OrdMap on first write.
        self.promote();
        // INV-FERR-005: primary first, then indexes. If a panic occurs
        // between the two operations, the datom is in primary but missing
        // from indexes (recoverable by rebuild) rather than a phantom
        // index entry (no primary counterpart).
        if let StoreRepr::OrdMap { datoms, indexes } = &mut self.repr {
            datoms.insert(datom.clone());
            indexes.insert(datom);
        }
        // INV-FERR-029: maintain LIVE set incrementally.
        self.live_apply(datom);
    }

    /// Replay a WAL entry during crash recovery.
    ///
    /// INV-FERR-014: restores committed state by inserting datoms and
    /// advancing the epoch. Used by `Database::open` during WAL replay.
    /// INV-FERR-009: evolves schema from schema-defining datoms in the
    /// replayed entry, preventing schema loss across crash recovery.
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
    /// `TxId::with_node(epoch, 0, node)` which used epoch-as-physical --
    /// breaking INV-FERR-015 (HLC monotonicity) and INV-FERR-016 (causality).
    pub fn transact(
        &mut self,
        transaction: Transaction<Committed>,
        tx_id: ferratom::TxId,
    ) -> Result<TxReceipt, FerraError> {
        // INV-FERR-020: read node FIRST, then consume the transaction via
        // into_datoms(). Ownership transfer enforces single-application.
        let node = transaction.node();
        let datoms = transaction.into_datoms();
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
        all_datoms.extend(create_tx_metadata(self.epoch, node, tx_id));

        // INV-FERR-009: evolve schema from schema-defining datoms.
        crate::schema_evolution::evolve_schema(&mut self.schema, &all_datoms)?;

        // bd-886d: dispatch based on representation.
        // Positional → splice (O(N+K), no promote/demote cycle).
        // OrdMap → fallback to direct insertion (rare: only during batch_replay).
        match &self.repr {
            StoreRepr::Positional(_) => self.splice_transact(&all_datoms),
            StoreRepr::OrdMap { .. } => {
                for datom in &all_datoms {
                    if let StoreRepr::OrdMap { datoms, indexes } = &mut self.repr {
                        datoms.insert(datom.clone());
                        indexes.insert(datom);
                    }
                    self.live_apply(datom);
                }
                self.demote();
            }
        }

        Ok(TxReceipt {
            epoch: self.epoch,
            datoms: all_datoms,
        })
    }

    /// Batch WAL replay: promote once, replay all entries, demote once (INV-FERR-014).
    ///
    /// Replaces N individual `replay_entry()` calls in `Database::open` recovery.
    /// Cost: 1 promote + N x insert + 1 demote, vs N x (promote + insert + demote).
    ///
    /// INV-FERR-009: schema evolution is applied per-entry to maintain
    /// correct schema state at each epoch boundary.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if schema evolution fails for any entry.
    pub fn batch_replay(&mut self, entries: &[(u64, Vec<Datom>)]) -> Result<(), FerraError> {
        if entries.is_empty() {
            return Ok(());
        }
        self.promote();
        for (epoch, datoms) in entries {
            for datom in datoms {
                if let StoreRepr::OrdMap {
                    datoms: d,
                    indexes: idx,
                } = &mut self.repr
                {
                    d.insert(datom.clone());
                    idx.insert(datom);
                }
                self.live_apply(datom);
            }
            self.epoch = *epoch;
            crate::schema_evolution::evolve_schema(&mut self.schema, datoms)?;
        }
        self.demote();
        Ok(())
    }

    /// Test-only convenience: applies a transaction with a synthetic
    /// epoch-based `TxId` derived from checked epoch increment.
    ///
    /// Bypasses the HLC clock that `Database::transact` provides, making
    /// it suitable for tests that operate on `Store` directly without
    /// constructing a full `Database`.
    ///
    /// # Errors
    ///
    /// Delegates to [`Store::transact`]; returns the same error variants.
    /// Returns `FerraError::InvariantViolation` on epoch overflow.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn transact_test(
        &mut self,
        transaction: Transaction<Committed>,
    ) -> Result<TxReceipt, FerraError> {
        let node = transaction.node();
        let next_epoch =
            self.epoch
                .checked_add(1)
                .ok_or_else(|| FerraError::InvariantViolation {
                    invariant: "INV-FERR-007".to_string(),
                    details: "epoch overflow in transact_test".to_string(),
                })?;
        let tx_id = ferratom::TxId::with_node(next_epoch, 0, node);
        self.transact(transaction, tx_id)
    }

    /// Batch merge-sort splice: stamp all datoms, merge once into canonical.
    ///
    /// Each transaction gets a distinct epoch (INV-FERR-007) and schema
    /// evolution happens per-transaction (INV-FERR-009), but the expensive
    /// merge into the canonical array happens ONCE, amortizing the O(N)
    /// copy across M transactions.
    ///
    /// INV-FERR-072: batch equivalence -- produces the same datom set as
    /// M individual `transact` calls applied sequentially.
    ///
    /// Cost: O(N + K log K) where K = sum of all transaction sizes.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::EmptyTransaction` if any batch entry has zero
    /// datoms. Returns `FerraError::InvariantViolation` on epoch overflow.
    /// Returns other `FerraError` variants if schema evolution fails.
    pub fn batch_splice_transact(
        &mut self,
        batches: Vec<(Vec<Datom>, ferratom::TxId)>,
    ) -> Result<Vec<TxReceipt>, FerraError> {
        if batches.is_empty() {
            return Ok(Vec::new());
        }

        let mut receipts = Vec::with_capacity(batches.len());

        for (datoms, tx_id) in batches {
            if datoms.is_empty() {
                return Err(FerraError::EmptyTransaction);
            }

            // INV-FERR-007: advance epoch strictly per transaction.
            self.epoch =
                self.epoch
                    .checked_add(1)
                    .ok_or_else(|| FerraError::InvariantViolation {
                        invariant: "INV-FERR-007".to_string(),
                        details: "epoch counter overflow in batch".to_string(),
                    })?;

            // INV-FERR-020: read node from TxId for metadata.
            let node = tx_id.node();

            // INV-FERR-015: stamp datoms with HLC-derived TxId.
            let mut tx_datoms = stamp_datoms(datoms, tx_id);
            tx_datoms.extend(create_tx_metadata(self.epoch, node, tx_id));

            // INV-FERR-009: schema evolution per transaction boundary.
            crate::schema_evolution::evolve_schema(&mut self.schema, &tx_datoms)?;

            receipts.push(TxReceipt {
                epoch: self.epoch,
                datoms: tx_datoms,
            });
        }

        // Clone all datoms from receipts for the single merge pass.
        // O(K_total) clones — the receipts own the datoms for return to caller.
        let all_new_datoms: Vec<Datom> = receipts
            .iter()
            .flat_map(|r| r.datoms.iter().cloned())
            .collect();

        // Single merge into canonical (amortized across all transactions).
        match &self.repr {
            StoreRepr::Positional(_) => {
                self.splice_transact_batch(&all_new_datoms);
            }
            StoreRepr::OrdMap { .. } => {
                for datom in &all_new_datoms {
                    if let StoreRepr::OrdMap { datoms, indexes } = &mut self.repr {
                        datoms.insert(datom.clone());
                        indexes.insert(datom);
                    }
                    self.live_apply(datom);
                }
                self.demote();
            }
        }

        Ok(receipts)
    }

    /// Merge-sort splice for a single transaction: INV-FERR-072 Path A (bd-886d).
    ///
    /// Inserts datoms into Positional without `OrdMap` detour. Produces
    /// identical datom set to promote+insert+demote (batch equivalence
    /// theorem). Takes `&[Datom]` (not `Vec<Datom>` as in spec Level 2)
    /// because the caller owns the stamped datoms for the receipt.
    ///
    /// Cost: O(N + K log K) — N = store size, K = transaction size.
    /// Fingerprint is recomputed in O(N+K) via `from_sorted_canonical`
    /// (spec Level 2 describes O(K) incremental XOR — deferred to Phase 4b).
    ///
    /// 1. Sort + dedup new datoms into canonical EAVT order: O(K log K)
    /// 2. Merge into existing canonical array: O(N + K)
    /// 3. Build new `PositionalStore` (fingerprint + LIVE in parallel): O(N + K)
    /// 4. Update `live_causal`/`live_set` for new datoms only: O(K log M)
    fn splice_transact(&mut self, new_datoms: &[Datom]) {
        self.splice_transact_batch(new_datoms);
    }

    /// Shared splice implementation for both single and batch transact paths.
    ///
    /// Sorts, deduplicates, and merges new datoms into the Positional canonical
    /// array. No-op if the store is not in `Positional` representation.
    fn splice_transact_batch(&mut self, new_datoms: &[Datom]) {
        if let StoreRepr::Positional(ps) = &self.repr {
            // 1. Sort + dedup new datoms into canonical EAVT order.
            // dedup() required: merge_sort_dedup precondition is strictly sorted input.
            let mut sorted_new: Vec<Datom> = new_datoms.to_vec();
            sorted_new.sort_unstable();
            sorted_new.dedup();

            // 2. Merge into existing canonical: O(N + K), cache-sequential.
            let merged = merge_sort_dedup(ps.datoms(), &sorted_new);

            // 3. Build new PositionalStore (fingerprint + LIVE in parallel).
            let new_ps = PositionalStore::from_sorted_canonical(merged);
            self.repr = StoreRepr::Positional(Arc::new(new_ps));

            // 4. Update live_causal/live_set incrementally for new datoms.
            for datom in new_datoms {
                self.live_apply(datom);
            }
        }
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

/// INV-FERR-004: Create :tx/time and :tx/origin metadata datoms that
/// guarantee strict growth (every transaction adds at least 2 datoms).
fn create_tx_metadata(epoch: u64, node: NodeId, tx_id: ferratom::TxId) -> Vec<Datom> {
    // P1-003: Use full node bytes for tx_entity derivation, not just
    // first byte. Prevents collision when two nodes share the same
    // first byte but differ in subsequent bytes.
    let mut tx_content = format!("tx-{epoch}-").into_bytes();
    tx_content.extend_from_slice(node.as_bytes());
    let tx_entity = ferratom::EntityId::from_content(&tx_content);
    // Derive tx wall-clock from HLC physical component (deterministic,
    // no SystemTime dependency).  Overflow from u64->i64 is safe: the
    // fallback i64::MAX is ~292 billion years after epoch.
    let now_ms = i64::try_from(tx_id.physical()).unwrap_or(i64::MAX);

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
            Attribute::from("tx/origin"),
            ferratom::Value::Ref(ferratom::EntityId::from_content(node.as_bytes())),
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

    use ferratom::{Attribute, EntityId, NodeId, Op, TxId, Value};
    use ferratomic_tx::Transaction;

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
        // bd-5zc4: SortedVecBackend defers sorting; sort before querying.
        store.ensure_indexes_sorted();

        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let idx = store.indexes().unwrap();
        let eavt: BTreeSet<&Datom> = idx.eavt_datoms().collect();
        let aevt: BTreeSet<&Datom> = idx.aevt_datoms().collect();
        let vaet: BTreeSet<&Datom> = idx.vaet_datoms().collect();
        let avet: BTreeSet<&Datom> = idx.avet_datoms().collect();
        assert_eq!(
            primary, eavt,
            "INV-FERR-005: EAVT must match primary after insert"
        );
        assert_eq!(
            primary, aevt,
            "INV-FERR-005: AEVT must match primary after insert"
        );
        assert_eq!(
            primary, vaet,
            "INV-FERR-005: VAET must match primary after insert"
        );
        assert_eq!(
            primary, avet,
            "INV-FERR-005: AVET must match primary after insert"
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
        use crate::merge::merge;

        let mut a = Store::genesis();
        // Transact to advance epoch to 1
        let tx = Transaction::new(NodeId::from_bytes([1u8; 16]))
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
        let mut store = Store::genesis();
        let node = NodeId::from_bytes([42u8; 16]);
        let tx = Transaction::new(node)
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

        // The tx should carry the node we specified.
        let last_datom = store.datoms().last().expect("store not empty");
        assert_eq!(
            last_datom.tx().node(),
            node,
            "bd-1n6: TxId node must match transaction node"
        );

        // Epoch should be in the TxId physical component.
        assert_eq!(
            last_datom.tx().physical(),
            1, // epoch 1 after first transact
            "bd-1n6: TxId physical must equal epoch"
        );
    }

    /// Helper: evolve a store's schema by adding a new attribute with the given
    /// ident, value-type keyword, and node byte.
    fn evolve_schema(
        store: &mut Store,
        content_seed: &[u8],
        ident: &str,
        value_type: &str,
        node_byte: u8,
    ) {
        let tx = Transaction::new(NodeId::from_bytes([node_byte; 16]))
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
