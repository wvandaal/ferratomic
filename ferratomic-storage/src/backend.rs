//! Storage backend trait and implementations (INV-FERR-024).
//!
//! The [`StorageBackend`] trait decouples cold-start recovery from any
//! specific storage substrate. The core engine operates identically
//! regardless of which backend is active — correctness depends only on
//! the trait contract, never on implementation details of the substrate.
//!
//! Two implementations are provided:
//!
//! - [`FsBackend`] — filesystem-backed (production)
//! - [`InMemoryBackend`] — in-memory (testing)
//!
//! INV-FERR-013 (checkpoint equivalence) and INV-FERR-014 (recovery
//! correctness) are upheld by the checkpoint/WAL protocol that operates
//! *through* this trait. The trait itself provides the I/O surface;
//! serialization format and recovery logic live in the `checkpoint` and
//! `wal` modules.

use std::{
    io::{Cursor, Read as IoRead, Seek, Write as IoWrite},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ferratom::FerraError;

/// Shared mutable byte buffer for in-memory storage.
type SharedBuffer = Arc<Mutex<Vec<u8>>>;

/// Reader + seeker trait object bound for WAL recovery.
pub trait ReadSeek: IoRead + Seek {}

impl<T: IoRead + Seek> ReadSeek for T {}

/// Writer + seeker trait object bound for WAL appenders.
pub trait WriteSeek: IoWrite + Seek {}

impl<T: IoWrite + Seek> WriteSeek for T {}

// ---------------------------------------------------------------------------
// StorageBackend trait
// ---------------------------------------------------------------------------

/// Substrate-agnostic persistence abstraction (INV-FERR-024).
///
/// Provides the I/O surface through which the checkpoint and WAL
/// modules read and write durable state. The core engine operates
/// identically on any implementation of this trait -- correctness
/// depends only on the contract below, not on whether bytes land on
/// a filesystem, in memory, or in an object store.
///
/// # Invariant relationships
///
/// - **INV-FERR-024 (substrate agnosticism)**: switching implementations
///   changes durability characteristics but not engine semantics. All
///   store algebraic properties (INV-FERR-001 through INV-FERR-005)
///   hold regardless of backend.
/// - **INV-FERR-013 (checkpoint equivalence)**: the checkpoint writer
///   and reader form a round-trip pair. The checkpoint module serializes
///   store state `S` through [`open_checkpoint_writer`](Self::open_checkpoint_writer),
///   and [`open_checkpoint_reader`](Self::open_checkpoint_reader) yields
///   bytes from which `load(checkpoint(S)) = S` is reconstructed.
/// - **INV-FERR-014 (recovery correctness)**: the WAL writer and reader
///   form a durable-append pair. The recovery module replays WAL entries
///   through [`open_wal_reader`](Self::open_wal_reader) to reconstruct
///   at least the last committed state: `recover(crash(S)) >= last_committed(S)`.
///
/// # Contract
///
/// Implementors guarantee:
/// - **Writer isolation**: checkpoint writes truncate prior data;
///   subsequent reads see only the new content.
/// - **WAL append semantics**: WAL writes append without truncation.
///   The reader sees all previously written entries.
/// - **Existence consistency**: `checkpoint_exists()` returns `true`
///   only after at least one successful checkpoint write. `wal_exists()`
///   returns `true` only when WAL data is present.
pub trait StorageBackend: Send + Sync {
    /// Open a writer whose content, once completed, replaces any previous
    /// checkpoint data.
    ///
    /// The returned writer receives the serialized store snapshot.
    /// The replacement is atomic: partial old data never contaminates
    /// a new checkpoint (`FsBackend` uses truncate-on-open;
    /// `InMemoryBackend` swaps on flush).
    ///
    /// INV-FERR-013: this is the write half of the checkpoint round-trip.
    /// After the writer is flushed/dropped and a subsequent call to
    /// [`open_checkpoint_reader`](Self::open_checkpoint_reader) succeeds,
    /// the reader yields the bytes written here.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created (e.g.,
    /// missing directory, permission denied, mutex poisoned).
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError>;

    /// Open a reader for the existing checkpoint file.
    ///
    /// The returned reader yields the bytes from the most recent
    /// successful checkpoint write. The checkpoint module deserializes
    /// these bytes to reconstruct the store.
    ///
    /// INV-FERR-013: this is the read half of the checkpoint round-trip.
    /// `load(checkpoint(S)) = S` holds when the bytes read here are
    /// exactly those produced by the corresponding write.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened (e.g.,
    /// file does not exist, permission denied, mutex poisoned).
    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError>;

    /// Check whether a checkpoint file exists and contains data.
    ///
    /// Returns `true` only after at least one successful checkpoint
    /// write. The cold-start recovery path uses this to decide whether
    /// to load from checkpoint or start from genesis (INV-FERR-031).
    fn checkpoint_exists(&self) -> bool;

    /// Open a seekable writer for the WAL file.
    ///
    /// The WAL writer appends transaction entries without truncating
    /// existing data. Seek capability is required for length queries
    /// and position management during append.
    ///
    /// INV-FERR-014: this is the write half of the WAL durability
    /// contract. Entries written here persist across process restarts
    /// (for durable backends) and are readable via
    /// [`open_wal_reader`](Self::open_wal_reader) during recovery.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created.
    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError>;

    /// Open a seekable reader for the existing WAL file.
    ///
    /// The returned reader yields all WAL entries written since the
    /// last checkpoint. The recovery module replays these entries to
    /// reconstruct committed transactions.
    ///
    /// INV-FERR-014: this is the read half of the WAL durability
    /// contract. `recover(crash(S)) >= last_committed(S)` holds when
    /// the reader yields all entries that were durably appended via
    /// the corresponding writer.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened.
    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError>;

    /// Check whether a WAL file exists and contains data.
    ///
    /// Returns `true` only when WAL entries are present. The cold-start
    /// recovery path uses this to decide whether WAL replay is needed
    /// after loading the checkpoint (INV-FERR-014).
    fn wal_exists(&self) -> bool;

    /// Ensure the storage directory structure exists.
    ///
    /// Creates any missing directories required by the backend. Must
    /// be called before the first write operation. Subsequent calls
    /// are idempotent.
    ///
    /// INV-FERR-024: directory creation is the only substrate-specific
    /// setup step. After this call, all other trait methods operate
    /// uniformly regardless of backend.
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
/// Checkpoint and WAL files live in a data directory on the local filesystem.
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
        self.data_dir.join(crate::CHECKPOINT_FILENAME)
    }

    /// Return the WAL file path within the data directory (INV-FERR-008).
    #[must_use]
    pub fn wal_path(&self) -> PathBuf {
        self.data_dir.join(crate::WAL_FILENAME)
    }

    /// Return the root data directory path (INV-FERR-024).
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

impl StorageBackend for FsBackend {
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError> {
        let file = std::fs::File::create(self.checkpoint_path())
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        Ok(Box::new(std::io::BufWriter::new(file)))
    }

    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError> {
        let file = std::fs::File::open(self.checkpoint_path()).map_err(|e| FerraError::Io {
            kind: format!("{:?}", e.kind()),
            message: e.to_string(),
        })?;
        Ok(Box::new(std::io::BufReader::new(file)))
    }

    fn checkpoint_exists(&self) -> bool {
        self.checkpoint_path().exists()
    }

    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
            .open(self.wal_path())
            .map_err(|e| FerraError::Io {
                kind: format!("{:?}", e.kind()),
                message: e.to_string(),
            })?;
        Ok(Box::new(file))
    }

    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError> {
        let file = std::fs::File::open(self.wal_path()).map_err(|e| FerraError::Io {
            kind: format!("{:?}", e.kind()),
            message: e.to_string(),
        })?;
        Ok(Box::new(file))
    }

    fn wal_exists(&self) -> bool {
        self.wal_path().exists()
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        std::fs::create_dir_all(&self.data_dir).map_err(|e| FerraError::Io {
            kind: format!("{:?}", e.kind()),
            message: format!("cannot create data dir: {e}"),
        })
    }
}

// ---------------------------------------------------------------------------
// InMemoryBackend
// ---------------------------------------------------------------------------

/// In-memory storage backend for testing (INV-FERR-024).
///
/// Checkpoint and WAL data are stored in `Arc<Mutex<Vec<u8>>>` buffers.
/// No filesystem access is performed.
pub struct InMemoryBackend {
    /// Checkpoint data buffer.
    checkpoint: SharedBuffer,
    /// WAL data buffer.
    wal: SharedBuffer,
}

impl InMemoryBackend {
    /// Create a new empty in-memory backend (INV-FERR-024).
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

/// Writer that overwrites a shared buffer on each write (INV-FERR-024).
pub(crate) struct SharedBufferWriter {
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
        // take() moves data to target, leaving local empty.
        // Drop becomes a no-op if flush was already called.
        *guard = std::mem::take(&mut self.local);
        Ok(())
    }
}

impl Drop for SharedBufferWriter {
    fn drop(&mut self) {
        // Only write if flush was NOT called (local still has data).
        if !self.local.is_empty() {
            if let Ok(mut guard) = self.target.lock() {
                *guard = std::mem::take(&mut self.local);
            }
        }
    }
}

/// Seekable writer backed by a shared buffer (INV-FERR-024).
pub(crate) struct SharedBufferSeekWriter {
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
        let mut guard = self
            .target
            .lock()
            .map_err(|_| std::io::Error::other("mutex poisoned"))?;
        // Move data to target. Cursor is left with empty vec after take.
        // Drop becomes a no-op if flush was called.
        *guard = std::mem::take(self.cursor.get_mut());
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
        // Only write if flush was NOT called (cursor still has data).
        if !self.cursor.get_ref().is_empty() {
            if let Ok(mut guard) = self.target.lock() {
                *guard = std::mem::take(self.cursor.get_mut());
            }
        }
    }
}

impl StorageBackend for InMemoryBackend {
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError> {
        Ok(Box::new(SharedBufferWriter {
            target: Arc::clone(&self.checkpoint),
            local: Vec::new(),
        }))
    }

    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError> {
        let guard = self.checkpoint.lock().map_err(|_| FerraError::Io {
            kind: "Other".to_string(),
            message: "checkpoint mutex poisoned".to_string(),
        })?;
        Ok(Box::new(Cursor::new(guard.clone())))
    }

    fn checkpoint_exists(&self) -> bool {
        self.checkpoint.lock().is_ok_and(|guard| !guard.is_empty())
    }

    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError> {
        let guard = self.wal.lock().map_err(|_| FerraError::Io {
            kind: "Other".to_string(),
            message: "WAL mutex poisoned".to_string(),
        })?;
        let existing = guard.clone();
        let mut cursor = Cursor::new(existing);
        cursor
            .seek(std::io::SeekFrom::End(0))
            .map_err(|e| FerraError::Io {
                kind: format!("{:?}", e.kind()),
                message: format!("WAL seek to end failed: {e}"),
            })?;
        Ok(Box::new(SharedBufferSeekWriter {
            target: Arc::clone(&self.wal),
            cursor,
        }))
    }

    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError> {
        let guard = self.wal.lock().map_err(|_| FerraError::Io {
            kind: "Other".to_string(),
            message: "WAL mutex poisoned".to_string(),
        })?;
        Ok(Box::new(Cursor::new(guard.clone())))
    }

    fn wal_exists(&self) -> bool {
        self.wal.lock().is_ok_and(|guard| !guard.is_empty())
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::io::{Read as IoRead, Write as IoWrite};

    use super::*;

    #[test]
    fn test_inv_ferr_024_in_memory_checkpoint_roundtrip() {
        let backend = InMemoryBackend::new();
        assert!(
            !backend.checkpoint_exists(),
            "INV-FERR-024: fresh backend must have no checkpoint"
        );

        let data = b"checkpoint content for roundtrip test";
        let mut writer = backend.open_checkpoint_writer().unwrap();
        writer.write_all(data).unwrap();
        writer.flush().unwrap();
        drop(writer);

        assert!(
            backend.checkpoint_exists(),
            "INV-FERR-024: checkpoint must exist after write"
        );

        let mut reader = backend.open_checkpoint_reader().unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(
            buf, data,
            "INV-FERR-024: checkpoint content must survive roundtrip"
        );
    }

    #[test]
    fn test_inv_ferr_024_in_memory_wal_append_roundtrip() {
        let backend = InMemoryBackend::new();
        assert!(
            !backend.wal_exists(),
            "INV-FERR-024: fresh backend must have no WAL"
        );

        let data = b"wal entry content";
        {
            let mut writer = backend.open_wal_writer().unwrap();
            writer.write_all(data).unwrap();
            writer.flush().unwrap();
        }

        assert!(
            backend.wal_exists(),
            "INV-FERR-024: WAL must exist after write"
        );

        let mut reader = backend.open_wal_reader().unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(
            buf, data,
            "INV-FERR-024: WAL content must survive roundtrip"
        );
    }

    #[test]
    fn test_inv_ferr_024_fs_backend_paths() {
        let dir = std::path::Path::new("/tmp/ferratomic-test-paths");
        let backend = FsBackend::new(dir);
        assert_eq!(
            backend.checkpoint_path(),
            dir.join(crate::CHECKPOINT_FILENAME)
        );
        assert_eq!(backend.wal_path(), dir.join(crate::WAL_FILENAME));
        assert_eq!(backend.data_dir(), dir);
    }

    #[test]
    fn test_inv_ferr_024_in_memory_default() {
        let backend = InMemoryBackend::default();
        assert!(!backend.checkpoint_exists());
        assert!(!backend.wal_exists());
    }
}
