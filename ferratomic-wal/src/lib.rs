//! Write-ahead log for Ferratomic.
//!
//! Frame-based durability with CRC32 integrity and epoch monotonicity.
//! The WAL operates at the frame/byte level — it does not know about
//! Transactions, Stores, or Datoms. Serialization is the caller's
//! responsibility.
//!
//! INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`.
//! INV-FERR-007: Epoch monotonicity within a single WAL file.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

pub mod wal;

pub use wal::{crc32_ieee, recover_wal_from_reader, Wal, WalEntry};
