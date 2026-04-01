//! Storage backend trait and implementations (INV-FERR-024).
//!
//! The [`StorageBackend`] trait decouples cold-start recovery from any
//! specific storage substrate. Two implementations are provided:
//!
//! - [`FsBackend`] — filesystem-backed (production)
//! - [`InMemoryBackend`] — in-memory (testing)

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

/// Storage backend abstraction (INV-FERR-024).
///
/// Implementations provide durable storage for checkpoints and WAL.
/// The trait decouples cold-start recovery from any specific storage
/// substrate (filesystem, in-memory, object store, etc.).
pub trait StorageBackend {
    /// Open a writer for the checkpoint file, truncating any existing data.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created.
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError>;

    /// Open a reader for the existing checkpoint file.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened.
    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError>;

    /// Check whether a checkpoint file exists.
    fn checkpoint_exists(&self) -> bool;

    /// Open a writer for the WAL file.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the writer cannot be created.
    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError>;

    /// Open a reader for the existing WAL file.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the reader cannot be opened.
    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError>;

    /// Check whether a WAL file exists.
    fn wal_exists(&self) -> bool;

    /// Ensure the storage directory structure exists.
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
        self.data_dir.join(super::CHECKPOINT_FILENAME)
    }

    /// Return the WAL file path within the data directory (INV-FERR-008).
    #[must_use]
    pub fn wal_path(&self) -> PathBuf {
        self.data_dir.join(super::WAL_FILENAME)
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
        let file = std::fs::File::open(self.checkpoint_path())
            .map_err(|e| FerraError::Io(e.to_string()))?;
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
            .map_err(|e| FerraError::Io(e.to_string()))?;
        Ok(Box::new(file))
    }

    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError> {
        let file =
            std::fs::File::open(self.wal_path()).map_err(|e| FerraError::Io(e.to_string()))?;
        Ok(Box::new(file))
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
        guard.clone_from(&self.local);
        Ok(())
    }
}

impl Drop for SharedBufferWriter {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.target.lock() {
            *guard = std::mem::take(&mut self.local);
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
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError> {
        Ok(Box::new(SharedBufferWriter {
            target: Arc::clone(&self.checkpoint),
            local: Vec::new(),
        }))
    }

    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError> {
        let guard = self
            .checkpoint
            .lock()
            .map_err(|_| FerraError::Io("checkpoint mutex poisoned".to_string()))?;
        Ok(Box::new(Cursor::new(guard.clone())))
    }

    fn checkpoint_exists(&self) -> bool {
        self.checkpoint.lock().is_ok_and(|guard| !guard.is_empty())
    }

    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError> {
        let guard = self
            .wal
            .lock()
            .map_err(|_| FerraError::Io("WAL mutex poisoned".to_string()))?;
        let existing = guard.clone();
        let mut cursor = Cursor::new(existing);
        cursor
            .seek(std::io::SeekFrom::End(0))
            .map_err(|e| FerraError::Io(format!("WAL seek to end failed: {e}")))?;
        Ok(Box::new(SharedBufferSeekWriter {
            target: Arc::clone(&self.wal),
            cursor,
        }))
    }

    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError> {
        let guard = self
            .wal
            .lock()
            .map_err(|_| FerraError::Io("WAL mutex poisoned".to_string()))?;
        Ok(Box::new(Cursor::new(guard.clone())))
    }

    fn wal_exists(&self) -> bool {
        self.wal.lock().is_ok_and(|guard| !guard.is_empty())
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        Ok(())
    }
}
