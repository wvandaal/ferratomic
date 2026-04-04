//! Write-Ahead Log with frame-based durability.
//!
//! INV-FERR-008: WAL fsync ordering. Durable(WAL(T)) BEFORE visible(SNAP(e)).
//! Every transaction is written to the WAL and fsynced before the epoch
//! advances and the snapshot becomes visible to readers.
//!
//! # Frame Format (22-byte overhead per entry)
//!
//! ```text
//! +------------------+
//! | Magic (4B)       | 0x46455252 ("FERR")
//! +------------------+
//! | Version (2B)     | 0x0001 (little-endian)
//! +------------------+
//! | Epoch (8B)       | u64 little-endian, strictly monotonically increasing
//! +------------------+
//! | Length (4B)       | u32 byte count of payload
//! +------------------+
//! | Payload (N)      | Committed transaction datoms serialized as bincode
//! +------------------+
//! | CRC32 (4B)       | CRC32 of [Magic..Payload]
//! +------------------+
//! ```
//!
//! # Module structure
//!
//! - [`mod.rs`](self): Core types, constants, constructors, CRC32.
//! - [`writer`]: Append and fsync — the write path.
//! - [`recover`]: Recovery and frame parsing — the read path.

mod recover;
mod writer;

use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use ferratom::FerraError;
pub(crate) use recover::recover_wal_from_reader;

/// WAL frame magic bytes: ASCII "FERR" (0x46455252).
pub(crate) const WAL_MAGIC: [u8; 4] = *b"FERR";

/// WAL frame format version. Little-endian u16.
pub(crate) const WAL_VERSION: u16 = 1;

/// Fixed header size: magic(4) + version(2) + epoch(8) + length(4) = 18 bytes.
pub(crate) const HEADER_SIZE: usize = 18;

/// CRC32 trailer size: 4 bytes.
pub(crate) const CRC_SIZE: usize = 4;

/// Minimum frame size: header + CRC (no payload).
pub(crate) const MIN_FRAME_SIZE: usize = HEADER_SIZE + CRC_SIZE;

/// A single recovered WAL entry (INV-FERR-008).
///
/// Each entry represents one committed transaction that was durably
/// written before its epoch became visible to readers. Recovery replays
/// entries in epoch order to reconstruct the last committed store state
/// (INV-FERR-014). The payload is deserialized through the ADR-FERR-010
/// trust boundary (`WireDatom` -> `into_trusted()` -> `Datom`).
///
/// Fields are `pub` because the recovery cascade in `storage::recovery`
/// and `db::recover` must inspect epoch ordering and deserialize payloads.
#[derive(Debug)]
pub struct WalEntry {
    /// The epoch at which this transaction was committed.
    ///
    /// INV-FERR-007: epochs are strictly monotonically increasing within
    /// a single WAL file. Recovery uses this to skip entries already
    /// covered by a checkpoint (INV-FERR-014).
    pub epoch: u64,
    /// The serialized transaction payload: bincode-encoded `Vec<WireDatom>`.
    ///
    /// INV-FERR-008: contains the post-stamp datoms (with real `TxId`s)
    /// so that recovery produces identical state to the pre-crash store.
    /// ADR-FERR-010: deserialized as `Vec<WireDatom>`, then converted
    /// through the trust boundary via `into_trusted()`.
    pub payload: Vec<u8>,
}

/// Write-ahead log for durable transaction storage.
///
/// INV-FERR-008: `durable(WAL(T)) BEFORE visible(SNAP(e))`. Transactions
/// are appended to the WAL and fsynced before the in-memory store advances
/// and the epoch becomes visible to readers.
///
/// INV-FERR-014: Recovery replays complete entries (verified by CRC32) and
/// truncates incomplete ones at the first invalid frame boundary.
///
/// The WAL uses a frame-based format with 22 bytes of overhead per entry
/// (magic + version + epoch + length + CRC32). See the module-level
/// documentation for the wire format.
///
/// # Visibility
///
/// Fields are `pub(crate)` because `db::recover` and `db::transact`
/// interact directly with the WAL handle. The public API surface is
/// limited to `create`, `open`, `append`, `fsync`, `recover`,
/// `last_synced_epoch`, and `path`.
pub struct Wal {
    /// Path to the WAL file on disk (INV-FERR-008).
    pub(crate) path: PathBuf,
    /// Open file handle for both reading and writing (INV-FERR-008).
    pub(crate) file: File,
    /// The epoch of the last entry confirmed durable via fsync (INV-FERR-007).
    pub(crate) last_synced_epoch: u64,
    /// ME-012: The highest epoch written since last fsync. Updated to
    /// `last_synced_epoch` on successful `fsync()` (INV-FERR-007).
    pub(crate) pending_epoch: u64,
}

impl Wal {
    /// Create a new WAL file at `path`. Fails if the file already exists.
    ///
    /// INV-FERR-008: A fresh WAL starts empty with no durable entries.
    /// HI-002: Parent directory is fsynced after file creation to ensure
    /// the directory entry is durable on ext4/XFS.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the file cannot be created.
    pub fn create(path: &Path) -> Result<Self, FerraError> {
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(path)
            .map_err(|e| FerraError::Io {
                kind: format!("{:?}", e.kind()),
                message: e.to_string(),
            })?;

        // HI-002: fsync parent directory so the WAL file's directory entry
        // is durable. Without this, a crash after create but before the first
        // append could lose the WAL file entirely on ext4/XFS.
        if let Some(parent) = path.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            file,
            last_synced_epoch: 0,
            pending_epoch: 0,
        })
    }

    /// Open an existing WAL file for append and recovery.
    ///
    /// INV-FERR-008: The file must already exist. Use [`recover`](Self::recover)
    /// to replay durable entries after opening.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the file cannot be opened.
    pub fn open(path: &Path) -> Result<Self, FerraError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| FerraError::Io {
                kind: format!("{:?}", e.kind()),
                message: e.to_string(),
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            last_synced_epoch: 0,
            pending_epoch: 0,
        })
    }

    /// The epoch of the last entry confirmed during recovery.
    ///
    /// INV-FERR-008: Returns 0 if no entries have been recovered or if the
    /// WAL is empty.
    #[must_use]
    pub fn last_synced_epoch(&self) -> u64 {
        self.last_synced_epoch
    }

    /// The filesystem path of this WAL file.
    ///
    /// INV-FERR-008: Useful for diagnostics and checkpoint coordination.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ---------------------------------------------------------------------------
// CRC32 (IEEE 802.3)
// ---------------------------------------------------------------------------

/// Compute CRC32 using the IEEE 802.3 polynomial (0xEDB88320 reflected).
///
/// This is a table-less bit-by-bit implementation. It is correct and
/// deterministic; performance is acceptable for WAL frames which are
/// small relative to the payload serialization cost.
pub(crate) fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_deterministic() {
        let data = b"hello world";
        assert_eq!(crc32_ieee(data), crc32_ieee(data));
    }

    #[test]
    fn test_crc32_known_value() {
        // "hello world" CRC32 IEEE = 0x0D4A1185
        let crc = crc32_ieee(b"hello world");
        assert_eq!(
            crc, 0x0D4A_1185,
            "CRC32 of 'hello world' must match known value"
        );
    }

    #[test]
    fn test_crc32_empty() {
        // CRC32 of empty input = 0x00000000
        let crc = crc32_ieee(b"");
        assert_eq!(crc, 0x0000_0000, "CRC32 of empty input must be 0");
    }
}
