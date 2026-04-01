//! `storage` — data directory management and cold-start recovery.
//!
//! INV-FERR-024: Substrate agnosticism. The storage backend is swappable
//! via the [`StorageBackend`] trait. Implementations provide durable
//! storage for checkpoints and WAL files.
//!
//! INV-FERR-028: Cold start < 5s at 100M datoms.
//! INV-FERR-013: Checkpoint round-trip identity.
//! INV-FERR-014: Recovery produces the last committed state.
//!
//! # Three-Level Recovery Cascade
//!
//! 1. **Fast (checkpoint + WAL)**: Load latest checkpoint, replay WAL
//!    entries after checkpoint epoch. This is the normal path.
//! 2. **Medium (WAL-only)**: No valid checkpoint. Replay WAL from
//!    genesis. Slower but correct for small stores.
//! 3. **Full rebuild (genesis)**: No checkpoint, no WAL. Start fresh.
//!
//! # Data Directory Layout (for [`FsBackend`])
//!
//! ```text
//! data_dir/
//! ├── checkpoint.chkp   # Latest checkpoint file
//! └── wal.log           # Append-only write-ahead log
//! ```
//!
//! # Module Structure
//!
//! - [`backend`]: `StorageBackend` trait + `FsBackend` + `InMemoryBackend`
//! - [`recovery`]: Cold-start recovery cascade (generic + filesystem)

mod backend;
mod recovery;

use std::path::{Path, PathBuf};

pub use backend::{FsBackend, InMemoryBackend, ReadSeek, StorageBackend, WriteSeek};
pub use recovery::{cold_start, cold_start_with_backend};

use crate::db::Database;

/// Well-known filenames within the data directory.
pub(crate) const CHECKPOINT_FILENAME: &str = "checkpoint.chkp";
/// Well-known WAL filename within the data directory.
pub(crate) const WAL_FILENAME: &str = "wal.log";

/// Result of a cold start, including which recovery path was used (INV-FERR-014).
///
/// INV-FERR-028: Cold start selects the fastest available recovery path.
pub struct ColdStartResult {
    /// The recovered (or freshly created) database, ready for reads and writes.
    pub database: Database,
    /// Which recovery level was used to produce this database.
    pub level: RecoveryLevel,
}

impl std::fmt::Debug for ColdStartResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColdStartResult")
            .field("level", &self.level)
            .field("epoch", &self.database.epoch())
            .finish()
    }
}

/// Which recovery path was taken during cold start (INV-FERR-014, INV-FERR-028).
///
/// Levels are tried in order from fastest to slowest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryLevel {
    /// Level 1: checkpoint loaded + WAL delta replayed (INV-FERR-013, INV-FERR-014).
    CheckpointPlusWal,
    /// Level 1b: checkpoint loaded, no WAL file present (INV-FERR-013).
    CheckpointOnly,
    /// Level 2: no checkpoint, WAL replayed from genesis (INV-FERR-014).
    WalOnly,
    /// Level 3: no checkpoint, no WAL. Fresh genesis (INV-FERR-031).
    Genesis,
}

/// Return the checkpoint file path for a data directory (INV-FERR-013).
#[must_use]
pub fn checkpoint_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CHECKPOINT_FILENAME)
}

/// Return the WAL file path for a data directory (INV-FERR-008).
#[must_use]
pub fn wal_path(data_dir: &Path) -> PathBuf {
    data_dir.join(WAL_FILENAME)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{AgentId, Attribute, EntityId, Value};

    use super::*;
    use crate::{checkpoint::write_checkpoint, writer::Transaction};

    #[test]
    fn test_inv_ferr_028_cold_start_genesis() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = cold_start(dir.path()).unwrap();

        assert_eq!(result.level, RecoveryLevel::Genesis);
        assert_eq!(result.database.epoch(), 0);
    }

    #[test]
    fn test_inv_ferr_028_cold_start_wal_only() {
        let dir = tempfile::TempDir::new().unwrap();

        // Create a WAL with some transactions.
        {
            let db = Database::genesis_with_wal(&dir.path().join(WAL_FILENAME)).unwrap();
            let agent = AgentId::from_bytes([1u8; 16]);
            let schema = db.schema();

            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(b"cold-start-test"),
                    Attribute::from("db/doc"),
                    Value::String(Arc::from("hello")),
                )
                .commit(&schema)
                .unwrap();
            db.transact(tx).unwrap();
        }

        // Cold start should find the WAL.
        let result = cold_start(dir.path()).unwrap();
        assert_eq!(result.level, RecoveryLevel::WalOnly);
        assert!(
            result.database.snapshot().datoms().count() > 0,
            "WAL-only recovery must restore datoms"
        );
    }

    /// Helper: create a WAL + checkpoint + post-checkpoint WAL delta in the given dir.
    /// Returns the `TempDir` path for cold-start testing.
    fn setup_checkpoint_plus_wal(dir: &std::path::Path) {
        let wal_file = dir.join(WAL_FILENAME);
        let chkp_file = dir.join(CHECKPOINT_FILENAME);

        let db = Database::genesis_with_wal(&wal_file).unwrap();
        let agent = AgentId::from_bytes([2u8; 16]);
        let schema = db.schema();

        // Transaction 1 (will be in checkpoint).
        let tx1 = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"before-chkp"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("pre-checkpoint")),
            )
            .commit(&schema)
            .unwrap();
        db.transact(tx1).unwrap();

        // Checkpoint after tx1.
        let store = {
            let snap = db.snapshot();
            let mut s = crate::store::Store::genesis();
            for d in snap.datoms() {
                s.insert(d);
            }
            s
        };
        write_checkpoint(&store, &chkp_file).unwrap();

        // Transaction 2 (will be WAL delta after checkpoint).
        let tx2 = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(b"after-chkp"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("post-checkpoint")),
            )
            .commit(&schema)
            .unwrap();
        db.transact(tx2).unwrap();
    }

    #[test]
    fn test_inv_ferr_028_cold_start_checkpoint_plus_wal() {
        let dir = tempfile::TempDir::new().unwrap();
        setup_checkpoint_plus_wal(dir.path());

        let result = cold_start(dir.path()).unwrap();
        assert_eq!(result.level, RecoveryLevel::CheckpointPlusWal);

        let snap = result.database.snapshot();
        let datom_count = snap.datoms().count();
        assert!(
            datom_count > 2,
            "INV-FERR-014: recovered db must have datoms from both pre and post checkpoint"
        );
    }

    #[test]
    fn test_inv_ferr_028_cold_start_creates_missing_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let nested = dir.path().join("nested").join("data");

        let result = cold_start(&nested).unwrap();
        assert_eq!(result.level, RecoveryLevel::Genesis);
        assert!(nested.exists(), "cold_start must create the data directory");
    }

    // -- InMemoryBackend tests (INV-FERR-024) ---------------------------------

    #[test]
    fn test_inv_ferr_024_in_memory_backend_genesis() {
        let backend = InMemoryBackend::new();
        let result = cold_start_with_backend(&backend).unwrap();

        assert_eq!(
            result.level,
            RecoveryLevel::Genesis,
            "INV-FERR-024: empty in-memory backend must produce genesis"
        );
        assert_eq!(result.database.epoch(), 0);
    }

    #[test]
    fn test_inv_ferr_024_in_memory_backend_checkpoint_roundtrip() {
        let backend = InMemoryBackend::new();

        // Write a checkpoint into the backend.
        {
            let mut store = crate::store::Store::genesis();
            let tx = Transaction::new(AgentId::from_bytes([3u8; 16]))
                .assert_datom(
                    EntityId::from_content(b"mem-test"),
                    Attribute::from("db/doc"),
                    Value::String(Arc::from("in-memory")),
                )
                .commit_unchecked();
            store.transact_test(tx).unwrap();

            let mut writer = backend.open_checkpoint_writer().unwrap();
            crate::checkpoint::write_checkpoint_to_writer(&store, &mut writer).unwrap();
        }

        assert!(
            backend.checkpoint_exists(),
            "INV-FERR-024: checkpoint must exist after write"
        );

        let result = cold_start_with_backend(&backend).unwrap();
        assert_eq!(
            result.level,
            RecoveryLevel::CheckpointOnly,
            "INV-FERR-024: in-memory backend with checkpoint must recover from checkpoint"
        );
        assert!(
            result.database.snapshot().datoms().count() > 0,
            "INV-FERR-024: recovered database must contain datoms"
        );
    }

    #[test]
    fn test_inv_ferr_024_fs_backend_delegates_correctly() {
        let dir = tempfile::TempDir::new().unwrap();
        let backend = FsBackend::new(dir.path());

        backend.create_dirs().unwrap();
        assert!(!backend.checkpoint_exists());
        assert!(!backend.wal_exists());

        let result = cold_start_with_backend(&backend).unwrap();
        assert_eq!(
            result.level,
            RecoveryLevel::Genesis,
            "INV-FERR-024: FsBackend with empty dir must produce genesis"
        );
    }

    #[test]
    fn test_inv_ferr_024_in_memory_default() {
        let backend = InMemoryBackend::default();
        assert!(
            !backend.checkpoint_exists(),
            "INV-FERR-024: default backend must have no checkpoint"
        );
        assert!(
            !backend.wal_exists(),
            "INV-FERR-024: default backend must have no WAL"
        );
    }
}
