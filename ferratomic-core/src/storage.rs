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

use std::{
    io::{Cursor, Read as IoRead, Seek, Write as IoWrite},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ferratom::FerraError;

use crate::db::Database;

/// Well-known filenames within the data directory.
const CHECKPOINT_FILENAME: &str = "checkpoint.chkp";
const WAL_FILENAME: &str = "wal.log";

// ---------------------------------------------------------------------------
// StorageBackend trait (INV-FERR-024)
// ---------------------------------------------------------------------------

/// Storage backend abstraction (INV-FERR-024).
///
/// Implementations provide durable storage for checkpoints and WAL.
/// The trait decouples cold-start recovery from any specific storage
/// substrate (filesystem, in-memory, object store, etc.).
pub trait StorageBackend {
    /// Writer handle for checkpoint data.
    type CheckpointWriter: IoWrite;
    /// Reader handle for checkpoint data.
    type CheckpointReader: IoRead;
    /// Writer handle for WAL data. Must support seek for frame-level access.
    type WalWriter: IoWrite + Seek;
    /// Reader handle for WAL data. Must support seek for recovery scanning.
    type WalReader: IoRead + Seek;

    /// Open a writer for the checkpoint file, truncating any existing data.
    ///
    /// INV-FERR-024: The returned writer receives the serialized checkpoint.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created.
    fn open_checkpoint_writer(&self) -> Result<Self::CheckpointWriter, FerraError>;

    /// Open a reader for the existing checkpoint file.
    ///
    /// INV-FERR-024: The returned reader provides the serialized checkpoint bytes.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened.
    fn open_checkpoint_reader(&self) -> Result<Self::CheckpointReader, FerraError>;

    /// Check whether a checkpoint file exists.
    ///
    /// INV-FERR-024: Used by cold-start to select the recovery level.
    fn checkpoint_exists(&self) -> bool;

    /// Open a writer for the WAL file.
    ///
    /// INV-FERR-024: The returned writer receives WAL frame data.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created.
    fn open_wal_writer(&self) -> Result<Self::WalWriter, FerraError>;

    /// Open a reader for the existing WAL file.
    ///
    /// INV-FERR-024: The returned reader provides WAL frame bytes for recovery.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened.
    fn open_wal_reader(&self) -> Result<Self::WalReader, FerraError>;

    /// Check whether a WAL file exists.
    ///
    /// INV-FERR-024: Used by cold-start to select the recovery level.
    fn wal_exists(&self) -> bool;

    /// Ensure the storage directory structure exists.
    ///
    /// INV-FERR-024: For filesystem backends, this creates the data directory.
    /// For in-memory backends, this is typically a no-op.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if directories cannot be created.
    fn create_dirs(&self) -> Result<(), FerraError>;
}

// ---------------------------------------------------------------------------
// FsBackend
// ---------------------------------------------------------------------------

/// Filesystem-backed storage (INV-FERR-024).
///
/// Wraps the current filesystem storage logic: checkpoint and WAL files
/// live in a data directory on the local filesystem.
pub struct FsBackend {
    /// Root data directory containing checkpoint and WAL files.
    data_dir: PathBuf,
}

impl FsBackend {
    /// Create a new filesystem backend rooted at the given directory.
    ///
    /// INV-FERR-024: The directory need not exist yet; call
    /// [`create_dirs`](StorageBackend::create_dirs) before use.
    #[must_use]
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Return the checkpoint file path within the data directory (INV-FERR-013).
    #[must_use]
    pub fn checkpoint_path(&self) -> PathBuf {
        self.data_dir.join(CHECKPOINT_FILENAME)
    }

    /// Return the WAL file path within the data directory (INV-FERR-008).
    #[must_use]
    pub fn wal_path(&self) -> PathBuf {
        self.data_dir.join(WAL_FILENAME)
    }

    /// Return the root data directory path (INV-FERR-024).
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

impl StorageBackend for FsBackend {
    type CheckpointWriter = std::io::BufWriter<std::fs::File>;
    type CheckpointReader = std::io::BufReader<std::fs::File>;
    type WalWriter = std::fs::File;
    type WalReader = std::fs::File;

    fn open_checkpoint_writer(&self) -> Result<Self::CheckpointWriter, FerraError> {
        let file = std::fs::File::create(self.checkpoint_path())
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        Ok(std::io::BufWriter::new(file))
    }

    fn open_checkpoint_reader(&self) -> Result<Self::CheckpointReader, FerraError> {
        let file = std::fs::File::open(self.checkpoint_path())
            .map_err(|e| FerraError::Io(e.to_string()))?;
        Ok(std::io::BufReader::new(file))
    }

    fn checkpoint_exists(&self) -> bool {
        self.checkpoint_path().exists()
    }

    fn open_wal_writer(&self) -> Result<Self::WalWriter, FerraError> {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
            .open(self.wal_path())
            .map_err(|e| FerraError::Io(e.to_string()))
    }

    fn open_wal_reader(&self) -> Result<Self::WalReader, FerraError> {
        std::fs::File::open(self.wal_path()).map_err(|e| FerraError::Io(e.to_string()))
    }

    fn wal_exists(&self) -> bool {
        self.wal_path().exists()
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| FerraError::Io(format!("cannot create data dir: {e}")))
    }
}

// ---------------------------------------------------------------------------
// InMemoryBackend
// ---------------------------------------------------------------------------

/// Shared mutable byte buffer for in-memory storage.
type SharedBuffer = Arc<Mutex<Vec<u8>>>;

/// In-memory storage backend for testing (INV-FERR-024).
///
/// Checkpoint and WAL data are stored in `Arc<Mutex<Vec<u8>>>` buffers.
/// No filesystem access is performed. Thread-safe for use in concurrent
/// test scenarios.
pub struct InMemoryBackend {
    /// Checkpoint data buffer.
    checkpoint: SharedBuffer,
    /// WAL data buffer.
    wal: SharedBuffer,
}

impl InMemoryBackend {
    /// Create a new empty in-memory backend.
    ///
    /// INV-FERR-024: Both checkpoint and WAL start empty. Use the
    /// `StorageBackend` methods to write data before reading.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checkpoint: Arc::new(Mutex::new(Vec::new())),
            wal: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Writer that overwrites a shared buffer on each write.
///
/// INV-FERR-024: Used by [`InMemoryBackend`] to collect checkpoint writes.
pub struct SharedBufferWriter {
    /// Reference to the shared buffer that will receive the data.
    target: SharedBuffer,
    /// Local accumulation buffer; flushed to `target` on `flush()`.
    local: Vec<u8>,
}

impl IoWrite for SharedBufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.local.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut guard = self
            .target
            .lock()
            .map_err(|_| std::io::Error::other("mutex poisoned"))?;
        guard.clone_from(&self.local);
        Ok(())
    }
}

impl Drop for SharedBufferWriter {
    fn drop(&mut self) {
        // Ensure data is committed even if flush was not called explicitly.
        if let Ok(mut guard) = self.target.lock() {
            *guard = std::mem::take(&mut self.local);
        }
    }
}

/// Seekable writer backed by a shared buffer.
///
/// INV-FERR-024: Used by [`InMemoryBackend`] for WAL writes.
/// Supports `Write` and `Seek` via `Cursor<Vec<u8>>` with
/// synchronization back to the shared buffer on drop.
pub struct SharedBufferSeekWriter {
    /// Reference to the shared buffer.
    target: SharedBuffer,
    /// Cursor providing write + seek over a local copy.
    cursor: Cursor<Vec<u8>>,
}

impl IoWrite for SharedBufferSeekWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.cursor.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let data = self.cursor.get_ref().clone();
        let mut guard = self
            .target
            .lock()
            .map_err(|_| std::io::Error::other("mutex poisoned"))?;
        *guard = data;
        Ok(())
    }
}

impl Seek for SharedBufferSeekWriter {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl Drop for SharedBufferSeekWriter {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.target.lock() {
            guard.clone_from(self.cursor.get_ref());
        }
    }
}

impl StorageBackend for InMemoryBackend {
    type CheckpointWriter = SharedBufferWriter;
    type CheckpointReader = Cursor<Vec<u8>>;
    type WalWriter = SharedBufferSeekWriter;
    type WalReader = Cursor<Vec<u8>>;

    fn open_checkpoint_writer(&self) -> Result<Self::CheckpointWriter, FerraError> {
        Ok(SharedBufferWriter {
            target: Arc::clone(&self.checkpoint),
            local: Vec::new(),
        })
    }

    fn open_checkpoint_reader(&self) -> Result<Self::CheckpointReader, FerraError> {
        let guard = self
            .checkpoint
            .lock()
            .map_err(|_| FerraError::Io("checkpoint mutex poisoned".to_string()))?;
        Ok(Cursor::new(guard.clone()))
    }

    fn checkpoint_exists(&self) -> bool {
        self.checkpoint
            .lock()
            .is_ok_and(|guard| !guard.is_empty())
    }

    fn open_wal_writer(&self) -> Result<Self::WalWriter, FerraError> {
        let guard = self
            .wal
            .lock()
            .map_err(|_| FerraError::Io("WAL mutex poisoned".to_string()))?;
        let existing = guard.clone();
        let mut cursor = Cursor::new(existing);
        // Seek to end so appends land after existing data.
        let _ = cursor.seek(std::io::SeekFrom::End(0));
        Ok(SharedBufferSeekWriter {
            target: Arc::clone(&self.wal),
            cursor,
        })
    }

    fn open_wal_reader(&self) -> Result<Self::WalReader, FerraError> {
        let guard = self
            .wal
            .lock()
            .map_err(|_| FerraError::Io("WAL mutex poisoned".to_string()))?;
        Ok(Cursor::new(guard.clone()))
    }

    fn wal_exists(&self) -> bool {
        self.wal
            .lock()
            .is_ok_and(|guard| !guard.is_empty())
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        // No-op for in-memory backend.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ColdStartResult + RecoveryLevel
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Generic cold start (INV-FERR-024)
// ---------------------------------------------------------------------------

/// Open or create storage via a backend and recover the database.
///
/// INV-FERR-024: Backend-agnostic cold start. This is the generic entry
/// point that works with any [`StorageBackend`] implementation.
///
/// INV-FERR-028: Tries the fastest recovery path first (checkpoint + WAL)
/// and falls back to slower paths if earlier ones fail.
///
/// # Errors
///
/// Returns `FerraError` if:
/// - The backend's `create_dirs` fails.
/// - Both checkpoint and WAL are present but recovery fails (data corruption).
///
/// Note: Missing checkpoint or WAL are NOT errors — they trigger
/// fallback to a lower recovery level.
pub fn cold_start_with_backend<B: StorageBackend>(
    backend: &B,
) -> Result<ColdStartResult, FerraError> {
    backend.create_dirs()?;

    let has_checkpoint = backend.checkpoint_exists();
    let has_wal = backend.wal_exists();

    // Level 1: checkpoint + WAL (fastest path).
    if has_checkpoint && has_wal {
        if let Ok(result) = recover_checkpoint_plus_wal(backend) {
            return Ok(result);
        }
    }

    // Level 1b: checkpoint only (no WAL file).
    if has_checkpoint && !has_wal {
        if let Ok(result) = recover_checkpoint_only(backend) {
            return Ok(result);
        }
    }

    // Level 2: WAL-only (no checkpoint).
    if has_wal {
        if let Ok(result) = recover_wal_only(backend) {
            return Ok(result);
        }
    }

    // Level 3: genesis (no data, or all corrupted).
    Ok(ColdStartResult {
        database: Database::genesis(),
        level: RecoveryLevel::Genesis,
    })
}

/// Level 1 recovery: checkpoint + WAL delta.
///
/// INV-FERR-013: Load checkpoint as base state.
/// INV-FERR-014: Replay WAL entries with epoch > checkpoint epoch.
fn recover_checkpoint_plus_wal<B: StorageBackend>(
    backend: &B,
) -> Result<ColdStartResult, FerraError> {
    let mut reader = backend.open_checkpoint_reader()?;
    let store = crate::checkpoint::load_checkpoint_from_reader(&mut reader)?;
    let checkpoint_epoch = store.epoch();

    let mut wal_reader = backend.open_wal_reader()?;
    let entries = crate::wal::recover_wal_from_reader(&mut wal_reader)?;

    let mut db_store = store;
    for entry in &entries {
        if entry.epoch > checkpoint_epoch {
            let datoms: Vec<ferratom::Datom> = bincode::deserialize(&entry.payload)
                .map_err(|e| FerraError::WalRead(e.to_string()))?;
            for datom in datoms {
                db_store.insert(&datom);
            }
        }
    }

    Ok(ColdStartResult {
        database: Database::from_store(db_store),
        level: RecoveryLevel::CheckpointPlusWal,
    })
}

/// Level 1b recovery: checkpoint only, no WAL.
///
/// INV-FERR-013: Load checkpoint as the complete state.
fn recover_checkpoint_only<B: StorageBackend>(backend: &B) -> Result<ColdStartResult, FerraError> {
    let mut reader = backend.open_checkpoint_reader()?;
    let store = crate::checkpoint::load_checkpoint_from_reader(&mut reader)?;

    Ok(ColdStartResult {
        database: Database::from_store(store),
        level: RecoveryLevel::CheckpointOnly,
    })
}

/// Level 2 recovery: WAL-only, no checkpoint.
///
/// INV-FERR-014: Replay all WAL entries from genesis.
fn recover_wal_only<B: StorageBackend>(backend: &B) -> Result<ColdStartResult, FerraError> {
    let mut wal_reader = backend.open_wal_reader()?;
    let entries = crate::wal::recover_wal_from_reader(&mut wal_reader)?;

    let mut store = crate::store::Store::genesis();
    for entry in &entries {
        let datoms: Vec<ferratom::Datom> =
            bincode::deserialize(&entry.payload).map_err(|e| FerraError::WalRead(e.to_string()))?;
        for datom in datoms {
            store.insert(&datom);
        }
    }

    Ok(ColdStartResult {
        database: Database::from_store(store),
        level: RecoveryLevel::WalOnly,
    })
}

// ---------------------------------------------------------------------------
// Filesystem convenience functions
// ---------------------------------------------------------------------------

/// Open or create a data directory and recover the database.
///
/// INV-FERR-028: This is the filesystem-specific entry point for cold start.
/// It creates an [`FsBackend`] and delegates to the filesystem-aware
/// [`cold_start`] implementation that preserves WAL attachment for durability.
///
/// The data directory is created if it does not exist. After cold start,
/// the database has an attached WAL for durability (when a WAL file is
/// present or created).
///
/// # Errors
///
/// Returns `FerraError` if:
/// - The data directory cannot be created.
/// - Both checkpoint and WAL are present but recovery fails (data corruption).
///
/// Note: Missing checkpoint or WAL files are NOT errors — they trigger
/// fallback to a lower recovery level.
pub fn cold_start(data_dir: &Path) -> Result<ColdStartResult, FerraError> {
    // Ensure data directory exists.
    std::fs::create_dir_all(data_dir)
        .map_err(|e| FerraError::Io(format!("cannot create data dir: {e}")))?;

    let checkpoint_path = data_dir.join(CHECKPOINT_FILENAME);
    let wal_path = data_dir.join(WAL_FILENAME);

    let has_checkpoint = checkpoint_path.exists();
    let has_wal = wal_path.exists();

    // Level 1: checkpoint + WAL (fastest path).
    if has_checkpoint && has_wal {
        if let Ok(db) = Database::recover(&checkpoint_path, &wal_path) {
            return Ok(ColdStartResult {
                database: db,
                level: RecoveryLevel::CheckpointPlusWal,
            });
        }
    }

    // Level 1b: checkpoint only (no WAL file).
    // INV-FERR-008: attach a WAL so post-recovery transactions are durable.
    if has_checkpoint && !has_wal {
        if let Ok(store) = crate::checkpoint::load_checkpoint(&checkpoint_path) {
            let db = match Database::from_store_with_wal(store.clone(), &wal_path) {
                Ok(db) => db,
                Err(_) => Database::from_store(store),
            };
            return Ok(ColdStartResult {
                database: db,
                level: RecoveryLevel::CheckpointOnly,
            });
        }
    }

    // Level 2: WAL-only (no checkpoint).
    if has_wal {
        if let Ok(db) = Database::recover_from_wal(&wal_path) {
            return Ok(ColdStartResult {
                database: db,
                level: RecoveryLevel::WalOnly,
            });
        }
    }

    // Level 3: genesis (no checkpoint, no WAL, or all corrupted).
    let db = Database::genesis_with_wal(&wal_path).unwrap_or_else(|_| Database::genesis());

    Ok(ColdStartResult {
        database: db,
        level: RecoveryLevel::Genesis,
    })
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

    #[test]
    fn test_inv_ferr_028_cold_start_checkpoint_plus_wal() {
        let dir = tempfile::TempDir::new().unwrap();
        let wal_file = dir.path().join(WAL_FILENAME);
        let chkp_file = dir.path().join(CHECKPOINT_FILENAME);

        // Write some data, then checkpoint, then write more.
        {
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
                // Build a store from snapshot datoms for checkpointing.
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

        // Cold start: should use checkpoint + WAL.
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
            store.transact(tx).unwrap();

            let mut writer = backend.open_checkpoint_writer().unwrap();
            crate::checkpoint::write_checkpoint_to_writer(&store, &mut writer).unwrap();
        }

        // Cold start should find the checkpoint.
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
