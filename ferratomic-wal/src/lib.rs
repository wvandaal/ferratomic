//! Write-ahead log for Ferratomic.
//!
//! Frame-based durability with CRC32 integrity and epoch monotonicity.
//! The WAL operates at the frame/byte level — it does not know about
//! Transactions, Stores, or Datoms. Serialization is the caller's
//! responsibility (callers use bincode).
//!
//! ## Dependency DAG
//!
//! Leaf crate. Depends only on `ferratom` for error types. WAL replay
//! orchestration lives in `ferratomic-core`; this crate provides the
//! low-level frame I/O primitives.
//!
//! ## Key Types
//!
//! - [`Wal`] — append-only writer. Accepts opaque byte frames, prepends
//!   a CRC32 + length header, and fsyncs to the underlying `WriteSeek`.
//! - [`WalEntry`] — a single recovered frame: CRC-verified payload bytes
//!   plus the entry's byte offset within the WAL file.
//! - [`recover_wal_from_reader`] — crash-recovery replay. Reads frames
//!   sequentially, discards any trailing torn write (CRC mismatch), and
//!   returns the valid prefix.
//!
//! ## Invariants
//!
//! - INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))` — fsync
//!   ordering guarantees that a transaction is on disk before any snapshot
//!   can observe it.
//! - INV-FERR-007: Epoch monotonicity within a single WAL file.
//! - INV-FERR-014: Recovery correctness — `recover_wal_from_reader`
//!   returns exactly the durable prefix; no phantom or lost frames.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

pub mod wal;

pub use wal::{crc32_ieee, recover_wal_from_reader, Wal, WalEntry};
