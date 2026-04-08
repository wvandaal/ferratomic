//! Storage backend trait and implementations (INV-FERR-024).
//!
//! Abstracts raw byte I/O so that checkpoint and WAL persistence work
//! identically on real filesystems, in-memory buffers, or any future
//! substrate (e.g., object stores, block devices).
//!
//! ## Dependency DAG
//!
//! Leaf crate. Depends only on `ferratom` for error types. Consumed by
//! `ferratomic-checkpoint` (for checkpoint I/O), `ferratomic-wal` callers,
//! and `ferratomic-core` (cold-start recovery orchestration).
//!
//! ## Key Types
//!
//! - [`StorageBackend`] — trait providing `open_read` / `open_write` /
//!   `exists` over named files within a data directory. Implementations
//!   must be deterministic: the same sequence of writes produces the same
//!   byte-identical files.
//! - [`FsBackend`] — production backend. Opens real files under a
//!   configurable root directory.
//! - [`InMemoryBackend`] — test backend. Stores files as `Vec<u8>` in a
//!   `HashMap`. Deterministic and zero-I/O.
//! - [`ReadSeek`] / [`WriteSeek`] — trait aliases for `Read + Seek` and
//!   `Write + Seek`, returned by the backend trait methods.
//!
//! ## Well-Known Files
//!
//! - [`CHECKPOINT_FILENAME`] (`"checkpoint.chkp"`) — INV-FERR-013
//! - [`WAL_FILENAME`] (`"wal.log"`) — INV-FERR-008
//!
//! ## Invariants
//!
//! - INV-FERR-024: Substrate agnosticism — the database engine never
//!   touches `std::fs` directly; all I/O flows through [`StorageBackend`].
//!   Swapping backends does not change observable behavior.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod backend;

pub use backend::{FsBackend, InMemoryBackend, ReadSeek, StorageBackend, WriteSeek};

/// Well-known checkpoint filename within the data directory (INV-FERR-013).
pub const CHECKPOINT_FILENAME: &str = "checkpoint.chkp";
/// Well-known WAL filename within the data directory (INV-FERR-008).
pub const WAL_FILENAME: &str = "wal.log";
