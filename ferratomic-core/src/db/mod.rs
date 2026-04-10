//! `db` -- MVCC database with lock-free reads and serialized writes.
//!
//! INV-FERR-006: Snapshot isolation via `ArcSwap`. Readers call
//! `db.snapshot()` which performs an atomic pointer load (~1ns, no lock).
//! INV-FERR-007: Write linearizability via single-threaded writer.
//! All writes go through `db.transact()` which holds a mutex for
//! the duration of the transaction application.
//!
//! ## Typestate
//!
//! `Database<Opening>` -- initialization in progress. Only `finish()` available.
//! `Database<Ready>` -- fully initialized; reads and writes available.
//!
//! Convenience constructors (`genesis`, `from_store`, etc.) return
//! `Database<Ready>` directly via internal `Opening` → `finish()` transition.
//! The default type parameter is `Ready`, so bare `Database` equals
//! `Database<Ready>`. Methods `transact()`, `snapshot()`, `epoch()`,
//! `schema()`, and `register_observer()` are only on `Database<Ready>`.
//!
//! ## Architecture (ADR-FERR-003)
//!
//! ```text
//! Readers --> ArcSwap::load() --> Arc<Store>  (lock-free, O(1))
//! Writer  --> Mutex::lock()   --> mutate Store --> ArcSwap::store()
//! ```
//!
//! ## Submodules
//!
//! - `transact`: transaction application (`Database::transact`).
//! - `recover`: WAL and checkpoint recovery constructors.

mod observe;
mod recover;
mod transact;

#[cfg(test)]
mod tests;

use std::{
    marker::PhantomData,
    sync::{atomic::AtomicU64, Mutex},
};

use arc_swap::ArcSwap;
use ferratom::{FerraError, HybridClock, Schema};

use crate::{
    observer::{ObserverBroadcast, DEFAULT_OBSERVER_BUFFER},
    store::{Snapshot, Store},
};

// ---------------------------------------------------------------------------
// Identity transaction helper (INV-FERR-060)
// ---------------------------------------------------------------------------

/// Tx 1: Define `store/public-key` and `store/created` schema attributes.
///
/// Uses only genesis-schema attributes (`db/ident`, `db/valueType`,
/// `db/cardinality`), so `commit()` against the genesis schema succeeds.
fn build_identity_schema_tx(
    node: ferratom::NodeId,
    schema: &Schema,
) -> Result<crate::writer::Transaction<crate::writer::Committed>, FerraError> {
    let tx = crate::writer::Transaction::new(node);
    let tx = define_schema_attr(tx, "store/public-key", "db.type/bytes");
    let tx = define_schema_attr(tx, "store/created", "db.type/instant");
    tx.commit(schema).map_err(FerraError::from)
}

/// Tx 2: Assert the store's public key and creation timestamp.
///
/// Requires `store/public-key` and `store/created` in the schema
/// (installed by schema evolution from tx 1).
fn build_identity_value_tx(
    node: ferratom::NodeId,
    pubkey: &ed25519_dalek::VerifyingKey,
    schema: &Schema,
) -> Result<crate::writer::Transaction<crate::writer::Committed>, FerraError> {
    let store_eid = ferratom::EntityId::from_content(pubkey.as_bytes());
    let now_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX);

    crate::writer::Transaction::new(node)
        .assert_datom(
            store_eid,
            ferratom::Attribute::from("store/public-key"),
            ferratom::Value::Bytes(pubkey.as_bytes().as_slice().into()),
        )
        .assert_datom(
            store_eid,
            ferratom::Attribute::from("store/created"),
            ferratom::Value::Instant(now_ms),
        )
        .commit(schema)
        .map_err(FerraError::from)
}

/// Add schema-defining datoms for a new attribute (db/ident + db/valueType + db/cardinality).
fn define_schema_attr(
    tx: crate::writer::Transaction<crate::writer::Building>,
    ident: &str,
    value_type: &str,
) -> crate::writer::Transaction<crate::writer::Building> {
    let entity = ferratom::EntityId::from_content(ident.as_bytes());
    tx.assert_datom(
        entity,
        ferratom::Attribute::from("db/ident"),
        ferratom::Value::Keyword(ident.into()),
    )
    .assert_datom(
        entity,
        ferratom::Attribute::from("db/valueType"),
        ferratom::Value::Keyword(value_type.into()),
    )
    .assert_datom(
        entity,
        ferratom::Attribute::from("db/cardinality"),
        ferratom::Value::Keyword("db.cardinality/one".into()),
    )
}

// ---------------------------------------------------------------------------
// Typestate markers
// ---------------------------------------------------------------------------

/// Marker: database initialization in progress. Only `finish()` is available.
///
/// `Database<Opening>` cannot call `snapshot()`, `transact()`, `schema()`, or
/// `epoch()`. These become available after `finish()` transitions to
/// `Database<Ready>`. Phase 4b will add validation and actor startup in
/// `finish()`.
///
/// `pub` because callers using phased initialization must name
/// `Database<Opening>` in type signatures.
#[derive(Debug)]
pub struct Opening;

/// Marker: database initialization is complete. Reads (`snapshot`, `schema`,
/// `epoch`) and writes (`transact`, `register_observer`) are available.
///
/// All convenience constructors (`genesis`, `recover`, `from_store`, etc.)
/// return `Database<Ready>` directly by internally going through
/// `Database<Opening>` -> `finish()`.
///
/// `pub` because `Database<Ready>` is the standard type parameter used
/// throughout the crate and by downstream consumers.
#[derive(Debug)]
pub struct Ready;

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// MVCC database providing lock-free snapshot reads and serialized writes.
///
/// # Typestate
///
/// `Database<Opening>` -- initialization in progress. Only `finish()` available.
/// `Database<Ready>` -- fully initialized; reads and writes are available.
///
/// Convenience constructors return `Database<Ready>` directly via internal
/// `Opening` → `finish()` transition. The `Opening` state is available for
/// phased-initialization flows where callers need to inspect or configure
/// the database before enabling reads and writes.
///
/// The default type parameter is `Ready`, so bare `Database` (without an
/// explicit parameter) is `Database<Ready>`. Existing code that does not
/// name the parameter continues to compile unchanged.
///
/// INV-FERR-006: snapshot isolation. Every `snapshot()` call returns an
/// immutable view that is never affected by concurrent or subsequent writes.
///
/// INV-FERR-007: write linearizability. The internal `Mutex` ensures that
/// exactly one writer operates at a time, producing strictly ordered epochs.
///
/// INV-FERR-008: when a WAL is attached, `transact()` writes and fsyncs the
/// WAL BEFORE advancing the epoch and swapping the pointer.
pub struct Database<S = Ready> {
    /// The current store state. Readers load atomically. Writers swap after mutation.
    /// `ArcSwap` provides wait-free reads (~1ns) per ADR-FERR-003.
    current: ArcSwap<Store>,

    /// Write serialization lock. Only one writer at a time.
    /// INV-FERR-007: ensures epoch ordering is strict.
    write_lock: Mutex<()>,

    /// Optional WAL for durability. When `Some`, `transact()` writes and
    /// fsyncs the WAL before applying the transaction to the store.
    /// INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`.
    wal: Mutex<Option<crate::wal::Wal>>,

    /// Registered observers plus bounded history for catch-up.
    /// INV-FERR-011: delivery is at-least-once with bounded replay.
    observers: Mutex<ObserverBroadcast>,

    /// INV-FERR-021: Concurrency limiter for write backpressure.
    /// Pre-checks concurrent write attempts before `try_lock()` to
    /// prevent thundering herd on the write Mutex.
    write_limiter: crate::backpressure::WriteLimiter,

    /// Monotonic transaction counter for release-mode bijection canary.
    /// Incremented after every successful `transact()`. When the
    /// `release_bijection_check` feature is enabled, every 100th
    /// transaction triggers `GenericIndexes::verify_bijection()`.
    transaction_count: AtomicU64,

    /// HI-011: HLC providing causally-ordered `TxId` values for transactions.
    /// `INV-FERR-015`: `tick()` produces strictly monotonic timestamps.
    /// `INV-FERR-016`: `receive()` merges remote timestamps for causality.
    /// Ticked under the write lock so `TxId` ordering matches commit order.
    clock: Mutex<HybridClock>,

    /// Typestate marker. Zero-size, erased at compile time.
    _state: PhantomData<S>,
}

// ---------------------------------------------------------------------------
// Opening state — initialization in progress
// ---------------------------------------------------------------------------

impl Database<Opening> {
    /// Assemble a `Database<Opening>` from a store and optional WAL.
    ///
    /// All constructors delegate here (bd-bgdt). The `Opening` state
    /// enforces that `snapshot()` and `transact()` are unavailable until
    /// `finish()` is called, making invalid state transitions compile errors.
    fn build_opening(store: Store, wal: Option<crate::wal::Wal>) -> Self {
        let node = store.genesis_node();
        Self {
            current: ArcSwap::from_pointee(store),
            write_lock: Mutex::new(()),
            wal: Mutex::new(wal),
            observers: Mutex::new(ObserverBroadcast::new(DEFAULT_OBSERVER_BUFFER)),
            write_limiter: crate::backpressure::WriteLimiter::new(
                &crate::backpressure::BackpressurePolicy::default(),
            ),
            transaction_count: AtomicU64::new(0),
            clock: Mutex::new(HybridClock::new(node)),
            _state: PhantomData,
        }
    }

    /// Transition from `Opening` to `Ready`, enabling reads and writes.
    ///
    /// INV-FERR-006: after this call, `snapshot()`, `transact()`, `schema()`,
    /// and `epoch()` become available. Phase 4b will add validation and
    /// actor startup logic here.
    #[must_use]
    pub fn finish(self) -> Database<Ready> {
        Database {
            current: self.current,
            write_lock: self.write_lock,
            wal: self.wal,
            observers: self.observers,
            write_limiter: self.write_limiter,
            transaction_count: self.transaction_count,
            clock: self.clock,
            _state: PhantomData,
        }
    }
}

// ---------------------------------------------------------------------------
// In-memory constructors -- no WAL
// ---------------------------------------------------------------------------

impl Database<Ready> {
    /// Assemble a `Database<Ready>` from a store and optional WAL.
    ///
    /// Convenience helper: goes through `Opening` → `finish()` internally.
    fn build(store: Store, wal: Option<crate::wal::Wal>) -> Self {
        Database::<Opening>::build_opening(store, wal).finish()
    }

    /// Create a new database from a genesis store (no WAL).
    ///
    /// INV-FERR-031: The initial store is deterministic -- identical on
    /// every call. The genesis store contains the 25 axiomatic meta-schema
    /// attributes and no datoms.
    ///
    /// Without a WAL, transactions are not durable across crashes. Use
    /// [`genesis_with_wal`](Self::genesis_with_wal) for durability.
    #[must_use]
    pub fn genesis() -> Self {
        Self::build(Store::genesis(), None)
    }

    /// Create a database with a cryptographic identity (INV-FERR-060).
    ///
    /// The first transaction is self-signed: it declares `store/public-key`
    /// and `store/created` attributes via schema-as-data evolution, then
    /// asserts the store's public key and creation time.
    ///
    /// ADR-FERR-027: Store identity = self-signed first transaction.
    /// D15: Signing at the Database layer via `transact_signed`.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the identity transaction fails.
    pub fn genesis_with_identity(
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<Self, FerraError> {
        let db = Self::genesis();
        let pubkey = signing_key.verifying_key();

        // D8: NodeId = BLAKE3(pubkey)[..16] for uniform distribution.
        let node_hash = blake3::hash(pubkey.as_bytes());
        let mut node_bytes = [0u8; 16];
        node_bytes.copy_from_slice(&node_hash.as_bytes()[..16]);
        let node = ferratom::NodeId::from_bytes(node_bytes);

        // Tx 1: define store/public-key and store/created attributes.
        // Uses only genesis-schema attributes (db/ident, db/valueType, etc.).
        let schema_tx = build_identity_schema_tx(node, &db.schema())?;
        db.transact_signed(schema_tx, signing_key)?;

        // Tx 2: assert identity values. Now store/public-key and
        // store/created are in the schema (installed by schema evolution).
        let value_tx = build_identity_value_tx(node, &pubkey, &db.schema())?;
        db.transact_signed(value_tx, signing_key)?;

        Ok(db)
    }

    /// Create a database from an existing store (no WAL).
    ///
    /// INV-FERR-006: the provided store becomes the initial snapshot state.
    /// INV-FERR-007: epoch ordering continues from the store's current epoch.
    #[must_use]
    pub fn from_store(store: Store) -> Self {
        Self::build(store, None)
    }
}

// ---------------------------------------------------------------------------
// Operational API -- only available on Database<Ready>
// ---------------------------------------------------------------------------

impl Database<Ready> {
    /// Take an immutable point-in-time snapshot.
    ///
    /// INV-FERR-006: The returned snapshot is frozen at the moment of the
    /// atomic pointer load. This call is lock-free (~1ns via `ArcSwap`).
    /// Multiple readers can hold different snapshots simultaneously
    /// without contention.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let store = self.current.load();
        store.snapshot()
    }

    /// Access the current store's schema for transaction building.
    ///
    /// INV-FERR-009: the returned schema governs transact-time validation.
    /// Returns a clone of the schema. The schema is small (tens of
    /// attributes) so cloning is cheap relative to the transaction
    /// validation it enables.
    #[must_use]
    pub fn schema(&self) -> Schema {
        let store = self.current.load();
        store.schema().clone()
    }

    /// Causal frontier (INV-FERR-061).
    ///
    /// Returns an empty frontier until frontier tracking is wired into
    /// the transact path. Phase 4a.5 uses this as a placeholder; Phase 4c
    /// will maintain the frontier incrementally via `Frontier::advance`.
    #[must_use]
    pub fn frontier(&self) -> ferratom::Frontier {
        ferratom::Frontier::new()
    }

    /// Access the current epoch.
    ///
    /// INV-FERR-007: the epoch is strictly monotonically increasing.
    /// The value returned reflects the epoch at the time of the atomic
    /// pointer load and may be stale by the time the caller uses it.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        let store = self.current.load();
        store.epoch()
    }

    /// Access the genesis node identity.
    ///
    /// HI-014: the genesis node is `min(a.genesis_node, b.genesis_node)`
    /// across all merge ancestors. For single-node databases, this is the
    /// node that created the genesis store.
    #[must_use]
    pub fn genesis_node(&self) -> ferratom::NodeId {
        let store = self.current.load();
        store.genesis_node()
    }

    /// Obtain a clone of the current Store suitable for checkpoint serialization.
    ///
    /// INV-FERR-013: the returned `Store` faithfully represents the database's
    /// current state — epoch, schema, `genesis_node`, datom set, and LIVE
    /// metadata. Callers pass this to `write_checkpoint` for durable
    /// persistence. The clone is O(n) for both representations
    /// (`Positional` clones contiguous arrays; `OrdMap` uses structural
    /// sharing making it fast in practice).
    ///
    /// This is the correct entry point for checkpoint writing. Reconstructing
    /// a Store from snapshot parts (epoch + datoms + schema) loses LIVE
    /// metadata and risks epoch mismatch.
    #[must_use]
    pub fn store_for_checkpoint(&self) -> Store {
        let guard = self.current.load();
        Store::clone(&guard)
    }
}

// Send + Sync safety: ArcSwap<T> is Send+Sync when T: Send+Sync.
// Store is Send+Sync (im::OrdSet is Send+Sync, Schema is Send+Sync).
// Mutex<()> is Send+Sync. PhantomData<S> is Send+Sync for any S.
// Therefore Database<S> is Send+Sync and can be shared across
// threads via Arc<Database>.
