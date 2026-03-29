//! # ferratomic-core — Storage and concurrency engine
//!
//! This is the **core crate**: business logic, state machines, concurrency control.
//!
//! ## Architecture
//!
//! - `Store`: im::OrdMap persistent indexes (EAVT, entity, attribute, VAET, AVET, LIVE)
//! - `Database`: MVCC snapshots via ArcSwap, single writer actor, group commit
//! - `Snapshot`: lock-free read handle (~1ns load, zero contention)
//! - `WriterActor`: mpsc channel, WAL append, two-fsync barrier, batch commit
//! - `Wal`: write-ahead log with chain-hash integrity
//! - `CheckpointActor`: WAL → durable storage, supervised restart
//! - `DatomObserver`: async trait for at-least-once snapshot notifications
//!
//! ## Algebraic Role
//!
//! Core crate. ALGEBRAS — operations over types from ferratom.
//! Implements the G-Set CRDT semilattice (INV-FERR-001..003).

#![forbid(unsafe_code)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]

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

// Phase 4b+
// pub mod federation;
// pub mod shard;
