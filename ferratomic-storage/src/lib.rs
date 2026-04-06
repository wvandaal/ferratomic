//! Storage backend trait and implementations (INV-FERR-024).
//!
//! The [`StorageBackend`] trait decouples cold-start recovery from any
//! specific storage substrate. Two implementations are provided:
//!
//! - [`FsBackend`] — filesystem-backed (production)
//! - [`InMemoryBackend`] — in-memory (testing)
//!
//! This crate is a LEAF in the dependency graph: it depends only on
//! `ferratom` for error types. Recovery orchestration lives in
//! `ferratomic-core`.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod backend;

pub use backend::{FsBackend, InMemoryBackend, ReadSeek, StorageBackend, WriteSeek};

/// Well-known checkpoint filename within the data directory (INV-FERR-013).
pub const CHECKPOINT_FILENAME: &str = "checkpoint.chkp";
/// Well-known WAL filename within the data directory (INV-FERR-008).
pub const WAL_FILENAME: &str = "wal.log";
