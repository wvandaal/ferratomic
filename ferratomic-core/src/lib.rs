//! # ferratomic-core — Storage and concurrency engine
//!
//! This is the **core crate**: business logic, state machines, concurrency control.
//! It implements the algebras (operations) over the types defined in `ferratom`.
//! The central abstraction is `Store = (P(D), union)` — a G-Set CRDT semilattice
//! where writes are commutative, associative, and idempotent by construction.
//!
//! ## Module Reference
//!
//! ### `store` — G-Set CRDT semilattice (Phase 4a, active)
//!
//! The core data structure: an append-only, content-addressed datom set with four
//! secondary indexes maintained in bijection with the primary set.
//! **INV-FERR-001** (merge commutativity), **INV-FERR-002** (merge associativity),
//! **INV-FERR-003** (merge idempotency), **INV-FERR-004** (monotonic growth),
//! **INV-FERR-005** (index bijection), **INV-FERR-007** (epoch monotonicity),
//! **INV-FERR-031** (genesis determinism).
//!
//! ### `indexes` — Secondary index key types and ordering (Phase 4a, active)
//!
//! Per-index key types (EAVT, AEVT, VAET, AVET) whose `Ord` implementations
//! produce the correct sort order for each access pattern.
//! **INV-FERR-005** (secondary indexes in bijection with primary set).
//!
//! ### `db` — MVCC database with lock-free reads (Phase 4a, active)
//!
//! Provides snapshot isolation via `ArcSwap` and write linearizability via
//! single-threaded `Mutex` serialization (ADR-FERR-003). Uses typestate
//! (`Database<Opening>` -> `Database<Ready>`) to restrict reads and writes
//! to fully initialized databases. All constructors return `Database<Ready>`.
//! **INV-FERR-006** (snapshot isolation), **INV-FERR-007** (write linearizability),
//! **INV-FERR-008** (WAL-before-visible ordering), **INV-FERR-011** (observer delivery).
//!
//! ### `writer` — Transaction typestate builder (Phase 4a, active)
//!
//! `Transaction<Building>` accumulates datoms; `Transaction<Committed>` is sealed
//! and read-only. Invalid state transitions are compile errors.
//! **INV-FERR-009** (schema validation at transact boundary),
//! **INV-FERR-006** (transaction atomicity), **INV-FERR-018** (committed immutability).
//!
//! ### `schema_evolution` — Genesis meta-schema and schema evolution (Phase 4a, active)
//!
//! Builds the deterministic 19-attribute genesis meta-schema and handles
//! transact-time attribute installation for schema-as-data.
//! **INV-FERR-009** (schema validation), **INV-FERR-031** (genesis determinism).
//!
//! ### `wal` — Write-ahead log with frame-based durability (Phase 4a, active)
//!
//! Frame-format WAL with CRC32 integrity. Every transaction is written and
//! fsynced before its epoch becomes visible to readers.
//! **INV-FERR-008** (durable before visible: `durable(WAL(T)) BEFORE visible(SNAP(e))`).
//!
//! ### `checkpoint` — BLAKE3-verified durable snapshots (Phase 4a, active)
//!
//! Serializes the full store state to a file with BLAKE3 integrity hash.
//! Supports WAL-delta recovery from the checkpoint epoch forward.
//! **INV-FERR-013** (round-trip identity: `load(checkpoint(S)) = S`).
//!
//! ### `storage` — Data directory management and cold-start recovery (Phase 4a, active)
//!
//! Three-level recovery cascade: checkpoint+WAL, WAL-only, or fresh genesis.
//! Manages the on-disk data directory layout.
//! **INV-FERR-014** (recovery produces last committed state),
//! **INV-FERR-028** (cold start < 5s at 100M datoms),
//! **INV-FERR-013** (checkpoint round-trip identity).
//!
//! ### `observer` — Monotonic snapshot observation (Phase 4a, active)
//!
//! Push-based `DatomObserver` trait with bounded replay history for catch-up.
//! Observers never see a snapshot older than their previous observation.
//! **INV-FERR-011** (observer epoch monotonically non-decreasing).
//!
//! ### `merge` — CRDT merge via set union (Phase 4a, active)
//!
//! Pure set union of two datom stores. No schema validation required (C4).
//! **INV-FERR-001** (commutativity), **INV-FERR-002** (associativity),
//! **INV-FERR-003** (idempotency), **INV-FERR-004** (monotonic growth).
//!
//! ### `backpressure` — Write queue depth limiting (Phase 4a, active)
//!
//! Concurrency limiter preventing silent data loss and unbounded memory growth
//! under write saturation. Rejects excess transactions immediately.
//! **INV-FERR-021** (no silent data loss under backpressure),
//! **NEG-FERR-005** (no unbounded memory growth).
//!
//! ### `anti_entropy` — Anti-entropy protocol trait boundary (Phase 4a, active)
//!
//! Trait for eventual convergence between replicas. `NullAntiEntropy` provides
//! the no-op default for single-node operation.
//! **INV-FERR-022** (anti-entropy convergence).
//!
//! ### `snapshot` — Dedicated snapshot types (Phase 4b, planned)
//!
//! Reserved for prolly-tree-backed storage with lazy index materialization.
//! Phase 4a snapshots are handled directly by `db` and `store`.
//! **INV-FERR-006** (snapshot isolation).
//!
//! ### `transport` — Federation transport layer (Phase 4c, planned)
//!
//! Chunk-level sync between federated peers. Depends on the prolly tree block
//! store (Phase 4b) for content-addressed chunk identification and O(|delta|) transfer.
//! **INV-FERR-037..044** (federation invariants), **INV-FERR-051..055** (VKN).
//!
//! ### `topology` — Topology and replica filtering (Phase 4a trait, Phase 4c impl)
//!
//! `ReplicaFilter` trait for selective replication. `AcceptAll` provides
//! full-replica behavior for single-node operation. Phase 4c adds real
//! topology management and peer discovery.
//! **INV-FERR-030** (replica filtering), **INV-FERR-037..044** (federation).
//!
//! ## Architecture — Phase 4a (current)
//!
//! - `Store`: `im::OrdMap` persistent indexes (EAVT, entity, attribute, VAET, AVET, LIVE)
//! - `Database`: typestate (`Opening` -> `Ready`), MVCC snapshots via `ArcSwap`, `Mutex`-serialized single writer (ADR-FERR-003)
//! - `Snapshot`: lock-free read handle (~1ns load, zero contention)
//! - `Transaction`: typestate builder (`Building` -> `Committed`), schema validation
//! - `Wal`: write-ahead log with chain-hash integrity
//! - `Checkpoint`: BLAKE3-verified durable snapshots, WAL-delta recovery
//! - `DatomObserver`: trait for at-least-once snapshot notifications
//! - `WriteLimiter`: backpressure via try-lock semantics (INV-FERR-021)
//!
//! ## Planned — Phase 4b+
//!
//! - `WriterActor`: mpsc channel replacing Mutex, group commit, two-fsync barrier (Phase 4b)
//! - `CheckpointActor`: supervised WAL compaction with restart (Phase 4b)
//! - Prolly tree block store for O(d) diff and on-disk structural sharing (Phase 4b)
//! - `transport`: federation transport layer (Phase 4c)
//! - `topology`: cluster topology and peer discovery (Phase 4c)
//!
//! ## Algebraic Role
//!
//! Core crate. ALGEBRAS — operations over types from ferratom.
//! Implements the G-Set CRDT semilattice (INV-FERR-001..003).

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![deny(clippy::unwrap_used)]
#![warn(clippy::pedantic)]

pub mod indexes;
pub mod schema_evolution;
pub mod store;
pub mod db;
pub mod snapshot;
pub mod writer;
pub mod wal;
pub mod checkpoint;
pub mod storage;
pub mod observer;
pub mod merge;
pub mod transport;
pub mod topology;
pub mod backpressure;
pub mod anti_entropy;

// Phase 4b+
// pub mod federation;
// pub mod shard;
