//! Deterministic fault injection for storage backends (ADR-FERR-011).
//!
//! INV-FERR-056: Crash recovery under adversarial fault model.
//! The [`FaultInjectingBackend`] wraps any [`StorageBackend`] and intercepts
//! operations, injecting faults per [`FaultSpec`] configuration.
//!
//! All faults are deterministic: same spec + same operation sequence = same
//! injection points. This enables reproducible regression testing.

use std::{
    io::{Read as IoRead, Write as IoWrite},
    sync::{Arc, Mutex},
};

use ferratom::FerraError;
use ferratomic_core::storage::{ReadSeek, StorageBackend, WriteSeek};

// ---------------------------------------------------------------------------
// Fault specification types
// ---------------------------------------------------------------------------

/// A single fault to inject during storage operations (ADR-FERR-011).
///
/// Each variant models a real-world storage failure mode. The spec
/// is deterministic: same variant + same operation count = same failure.
#[derive(Debug, Clone)]
pub enum FaultSpec {
    /// A write where only partial data reaches storage.
    /// Models: crash during write, partial page flush, sector tearing.
    TornWrite {
        /// Which write operation to tear (1-indexed).
        nth_write: usize,
        /// How many bytes survive (0 < valid_bytes < intended).
        valid_bytes: usize,
    },
    /// Simulated power loss after the nth sync.
    /// All subsequent operations fail with IO error.
    PowerCut {
        /// Which sync triggers the cut (1-indexed).
        after_nth_sync: usize,
    },
    /// Transient IO error on the nth occurrence of any read.
    IoError {
        /// Which read fails (1-indexed).
        nth_read: usize,
    },
    /// Disk full after the nth write operation.
    DiskFull {
        /// Which write returns ENOSPC (1-indexed).
        after_nth_write: usize,
    },
    /// A single bit flip at the specified offset in read data.
    /// Detected by BLAKE3/CRC32 — tests detection, not tolerance.
    BitFlip {
        /// Byte offset in the read buffer to flip.
        offset: usize,
        /// Which bit within the byte (0-7).
        bit_position: u8,
    },
}

/// Mutable fault state tracking operation counts (ADR-FERR-011).
///
/// Deterministic: same `FaultSpec` + same operation sequence = same
/// injection points regardless of wall-clock time or thread scheduling.
#[derive(Debug, Default)]
pub struct FaultState {
    /// Total write operations observed.
    pub write_count: usize,
    /// Total sync/flush operations observed.
    pub sync_count: usize,
    /// Total read operations observed.
    pub read_count: usize,
    /// Whether a power cut has been triggered (all subsequent ops fail).
    pub power_cut_active: bool,
}

// ---------------------------------------------------------------------------
// FaultInjectingBackend
// ---------------------------------------------------------------------------

/// A storage backend decorator that injects faults per spec (ADR-FERR-011).
///
/// Wraps any `StorageBackend` and intercepts the readers/writers it produces,
/// injecting faults according to the configured [`FaultSpec`] list.
///
/// INV-FERR-056: Used by crash recovery proptests and Kani harnesses to
/// verify recovery correctness under adversarial conditions.
pub struct FaultInjectingBackend<B: StorageBackend> {
    /// The inner backend being decorated.
    inner: B,
    /// Fault specifications to apply.
    specs: Vec<FaultSpec>,
    /// Shared mutable state tracking operation counts.
    state: Arc<Mutex<FaultState>>,
}

impl<B: StorageBackend> FaultInjectingBackend<B> {
    /// Create a new fault-injecting backend wrapping `inner`.
    pub fn new(inner: B, specs: Vec<FaultSpec>) -> Self {
        Self {
            inner,
            specs,
            state: Arc::new(Mutex::new(FaultState::default())),
        }
    }

    /// Access the current fault state for inspection in tests.
    #[must_use]
    pub fn state(&self) -> Arc<Mutex<FaultState>> {
        Arc::clone(&self.state)
    }

    /// Check whether a power cut is active.
    fn is_power_cut(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.power_cut_active)
            .unwrap_or(false)
    }

    /// Return an IO error if power cut is active.
    fn check_power_cut(&self) -> Result<(), FerraError> {
        if self.is_power_cut() {
            return Err(FerraError::Io {
                kind: "Other".to_string(),
                message: "power cut: all operations failed".to_string(),
            });
        }
        Ok(())
    }
}

impl<B: StorageBackend> StorageBackend for FaultInjectingBackend<B> {
    fn open_checkpoint_writer(&self) -> Result<Box<dyn IoWrite>, FerraError> {
        self.check_power_cut()?;
        let inner_writer = self.inner.open_checkpoint_writer()?;
        Ok(Box::new(FaultWriter {
            inner: inner_writer,
            specs: self.specs.clone(),
            state: Arc::clone(&self.state),
        }))
    }

    fn open_checkpoint_reader(&self) -> Result<Box<dyn IoRead>, FerraError> {
        self.check_power_cut()?;
        let inner_reader = self.inner.open_checkpoint_reader()?;
        Ok(Box::new(FaultReader {
            inner: inner_reader,
            specs: self.specs.clone(),
            state: Arc::clone(&self.state),
        }))
    }

    fn checkpoint_exists(&self) -> bool {
        if self.is_power_cut() {
            return false;
        }
        self.inner.checkpoint_exists()
    }

    fn open_wal_writer(&self) -> Result<Box<dyn WriteSeek>, FerraError> {
        self.check_power_cut()?;
        let inner_writer = self.inner.open_wal_writer()?;
        Ok(Box::new(FaultSeekWriter {
            inner: inner_writer,
            specs: self.specs.clone(),
            state: Arc::clone(&self.state),
        }))
    }

    fn open_wal_reader(&self) -> Result<Box<dyn ReadSeek>, FerraError> {
        self.check_power_cut()?;
        let inner_reader = self.inner.open_wal_reader()?;
        Ok(Box::new(FaultSeekReader {
            inner: inner_reader,
            specs: self.specs.clone(),
            state: Arc::clone(&self.state),
        }))
    }

    fn wal_exists(&self) -> bool {
        if self.is_power_cut() {
            return false;
        }
        self.inner.wal_exists()
    }

    fn create_dirs(&self) -> Result<(), FerraError> {
        self.check_power_cut()?;
        self.inner.create_dirs()
    }
}

// ---------------------------------------------------------------------------
// Fault-injecting writer (non-seekable)
// ---------------------------------------------------------------------------

/// Writer that injects faults on write and flush operations.
struct FaultWriter {
    /// The real writer from the inner backend.
    inner: Box<dyn IoWrite>,
    /// Fault specs to check against.
    specs: Vec<FaultSpec>,
    /// Shared operation counters.
    state: Arc<Mutex<FaultState>>,
}

impl IoWrite for FaultWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.write_count += 1;
        let count = state.write_count;

        for spec in &self.specs {
            match spec {
                FaultSpec::TornWrite {
                    nth_write,
                    valid_bytes,
                } if count == *nth_write => {
                    let truncated = buf.len().min(*valid_bytes);
                    return self.inner.write(&buf[..truncated]);
                }
                FaultSpec::DiskFull { after_nth_write } if count == *after_nth_write => {
                    return Err(std::io::Error::from_raw_os_error(28)); // ENOSPC
                }
                _ => {}
            }
        }
        drop(state);
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.sync_count += 1;
        let count = state.sync_count;

        for spec in &self.specs {
            if let FaultSpec::PowerCut { after_nth_sync } = spec {
                if count == *after_nth_sync {
                    state.power_cut_active = true;
                    return Err(std::io::Error::other("power cut triggered"));
                }
            }
        }
        drop(state);
        self.inner.flush()
    }
}

// ---------------------------------------------------------------------------
// Fault-injecting reader (non-seekable)
// ---------------------------------------------------------------------------

/// Reader that injects faults on read operations.
struct FaultReader {
    /// The real reader from the inner backend.
    inner: Box<dyn IoRead>,
    /// Fault specs to check against.
    specs: Vec<FaultSpec>,
    /// Shared operation counters.
    state: Arc<Mutex<FaultState>>,
}

impl IoRead for FaultReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.read_count += 1;
        let count = state.read_count;

        for spec in &self.specs {
            if let FaultSpec::IoError { nth_read } = spec {
                if count == *nth_read {
                    return Err(std::io::Error::other("injected IO error"));
                }
            }
        }
        drop(state);

        let n = self.inner.read(buf)?;

        // Apply bit flips post-read.
        let state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        for spec in &self.specs {
            if let FaultSpec::BitFlip {
                offset,
                bit_position,
            } = spec
            {
                if *offset < n {
                    buf[*offset] ^= 1 << bit_position;
                }
            }
        }
        drop(state);
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Fault-injecting seekable writer (for WAL)
// ---------------------------------------------------------------------------

/// Seekable writer that injects faults (WAL path).
struct FaultSeekWriter {
    /// The real seekable writer from the inner backend.
    inner: Box<dyn WriteSeek>,
    /// Fault specs to check against.
    specs: Vec<FaultSpec>,
    /// Shared operation counters.
    state: Arc<Mutex<FaultState>>,
}

impl IoWrite for FaultSeekWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.write_count += 1;
        let count = state.write_count;

        for spec in &self.specs {
            match spec {
                FaultSpec::TornWrite {
                    nth_write,
                    valid_bytes,
                } if count == *nth_write => {
                    let truncated = buf.len().min(*valid_bytes);
                    return self.inner.write(&buf[..truncated]);
                }
                FaultSpec::DiskFull { after_nth_write } if count == *after_nth_write => {
                    return Err(std::io::Error::from_raw_os_error(28));
                }
                _ => {}
            }
        }
        drop(state);
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.sync_count += 1;
        let count = state.sync_count;

        for spec in &self.specs {
            if let FaultSpec::PowerCut { after_nth_sync } = spec {
                if count == *after_nth_sync {
                    state.power_cut_active = true;
                    return Err(std::io::Error::other("power cut triggered"));
                }
            }
        }
        drop(state);
        self.inner.flush()
    }
}

impl std::io::Seek for FaultSeekWriter {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        if self.is_power_cut() {
            return Err(std::io::Error::other("power cut active"));
        }
        self.inner.seek(pos)
    }
}

impl FaultSeekWriter {
    fn is_power_cut(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.power_cut_active)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Fault-injecting seekable reader (for WAL)
// ---------------------------------------------------------------------------

/// Seekable reader that injects faults (WAL path).
struct FaultSeekReader {
    /// The real seekable reader from the inner backend.
    inner: Box<dyn ReadSeek>,
    /// Fault specs to check against.
    specs: Vec<FaultSpec>,
    /// Shared operation counters.
    state: Arc<Mutex<FaultState>>,
}

impl IoRead for FaultSeekReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        if state.power_cut_active {
            return Err(std::io::Error::other("power cut active"));
        }
        state.read_count += 1;
        let count = state.read_count;

        for spec in &self.specs {
            if let FaultSpec::IoError { nth_read } = spec {
                if count == *nth_read {
                    return Err(std::io::Error::other("injected IO error"));
                }
            }
        }
        drop(state);

        let n = self.inner.read(buf)?;

        let state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
        for spec in &self.specs {
            if let FaultSpec::BitFlip {
                offset,
                bit_position,
            } = spec
            {
                if *offset < n {
                    buf[*offset] ^= 1 << bit_position;
                }
            }
        }
        drop(state);
        Ok(n)
    }
}

impl std::io::Seek for FaultSeekReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        if self.is_power_cut() {
            return Err(std::io::Error::other("power cut active"));
        }
        self.inner.seek(pos)
    }
}

impl FaultSeekReader {
    fn is_power_cut(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.power_cut_active)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ferratomic_core::storage::InMemoryBackend;

    use super::*;

    #[test]
    fn test_fault_state_determinism() {
        let backend1 = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::DiskFull { after_nth_write: 3 }],
        );
        let backend2 = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::DiskFull { after_nth_write: 3 }],
        );

        // Same spec → same state after same operations.
        backend1.create_dirs().unwrap();
        backend2.create_dirs().unwrap();

        let s1 = backend1.state.lock().unwrap();
        let s2 = backend2.state.lock().unwrap();
        assert_eq!(s1.write_count, s2.write_count);
        assert_eq!(s1.read_count, s2.read_count);
    }

    #[test]
    fn test_disk_full_injection() {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::DiskFull { after_nth_write: 2 }],
        );
        backend.create_dirs().unwrap();

        let mut writer = backend.open_checkpoint_writer().unwrap();
        // First write succeeds.
        writer.write_all(b"first").unwrap();
        // Second write triggers ENOSPC.
        let result = writer.write_all(b"second");
        assert!(
            result.is_err(),
            "ADR-FERR-011: DiskFull must fail on 2nd write"
        );
    }

    #[test]
    fn test_power_cut_injection() {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::PowerCut { after_nth_sync: 1 }],
        );
        backend.create_dirs().unwrap();

        let mut writer = backend.open_checkpoint_writer().unwrap();
        writer.write_all(b"data").unwrap();
        // First flush triggers power cut.
        let result = writer.flush();
        assert!(
            result.is_err(),
            "ADR-FERR-011: PowerCut must fail on 1st sync"
        );

        // Subsequent operations fail.
        assert!(
            backend.open_checkpoint_reader().is_err(),
            "ADR-FERR-011: all ops must fail after power cut"
        );
    }

    #[test]
    fn test_io_error_injection() {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::IoError { nth_read: 1 }],
        );
        backend.create_dirs().unwrap();

        // Write some data first.
        {
            let mut w = backend.open_checkpoint_writer().unwrap();
            w.write_all(b"test data").unwrap();
            w.flush().unwrap();
        }

        let mut reader = backend.open_checkpoint_reader().unwrap();
        let mut buf = [0u8; 16];
        let result = reader.read(&mut buf);
        assert!(
            result.is_err(),
            "ADR-FERR-011: IoError must fail on 1st read"
        );
    }

    #[test]
    fn test_torn_write_injection() {
        let backend = FaultInjectingBackend::new(
            InMemoryBackend::new(),
            vec![FaultSpec::TornWrite {
                nth_write: 1,
                valid_bytes: 3,
            }],
        );
        backend.create_dirs().unwrap();

        let mut writer = backend.open_checkpoint_writer().unwrap();
        let n = writer.write(b"hello world").unwrap();
        // Only 3 bytes should have been written.
        assert_eq!(n, 3, "ADR-FERR-011: TornWrite must truncate to valid_bytes");
    }

    #[test]
    fn test_no_faults_passes_through() {
        let backend = FaultInjectingBackend::new(InMemoryBackend::new(), vec![]);
        backend.create_dirs().unwrap();

        let mut writer = backend.open_checkpoint_writer().unwrap();
        writer.write_all(b"clean data").unwrap();
        writer.flush().unwrap();

        assert!(backend.checkpoint_exists());
    }
}
