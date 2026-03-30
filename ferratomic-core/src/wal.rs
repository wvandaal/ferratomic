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
//! | Payload (N)      | Committed transaction datoms serialized as JSON
//! +------------------+
//! | CRC32 (4B)       | CRC32 of [Magic..Payload]
//! +------------------+
//! ```

use std::fs::{File, OpenOptions};
use std::io::{Read as IoRead, Seek, SeekFrom, Write as IoWrite};
use std::path::{Path, PathBuf};

use ferratom::FerraError;

use crate::writer::{Committed, Transaction};

/// WAL frame magic bytes: ASCII "FERR" (0x46455252).
const WAL_MAGIC: [u8; 4] = *b"FERR";

/// WAL frame format version. Little-endian u16.
const WAL_VERSION: u16 = 1;

/// Fixed header size: magic(4) + version(2) + epoch(8) + length(4) = 18 bytes.
const HEADER_SIZE: usize = 18;

/// CRC32 trailer size: 4 bytes.
const CRC_SIZE: usize = 4;

/// Minimum frame size: header + CRC (no payload).
const MIN_FRAME_SIZE: usize = HEADER_SIZE + CRC_SIZE;

/// A single recovered WAL entry.
///
/// INV-FERR-008: Each entry represents one committed transaction
/// that was durably written before its epoch became visible.
#[derive(Debug)]
pub struct WalEntry {
    /// The epoch at which this transaction was committed.
    pub epoch: u64,
    /// The serialized transaction payload (JSON-encoded datoms).
    pub payload: Vec<u8>,
}

/// Write-ahead log for durable transaction storage.
///
/// INV-FERR-008: Transactions are appended to the WAL and fsynced
/// before the in-memory store advances. Recovery replays complete
/// entries and truncates incomplete ones.
pub struct Wal {
    /// Path to the WAL file on disk.
    path: PathBuf,
    /// Open file handle for both reading and writing.
    file: File,
    /// The epoch of the last entry confirmed durable via fsync.
    last_synced_epoch: u64,
}

impl Wal {
    /// Create a new WAL file at `path`. Fails if the file already exists.
    ///
    /// INV-FERR-008: A fresh WAL starts empty with no durable entries.
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
            .map_err(|e| FerraError::Io(e.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            last_synced_epoch: 0,
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
            .map_err(|e| FerraError::Io(e.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
            last_synced_epoch: 0,
        })
    }

    /// Append a committed transaction to the WAL at the given epoch.
    ///
    /// INV-FERR-008: The frame is written to the OS page cache but NOT
    /// fsynced. Call [`fsync`](Self::fsync) to guarantee durability before
    /// advancing the epoch.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` if JSON serialization or the write fails.
    pub fn append(&mut self, epoch: u64, tx: &Transaction<Committed>) -> Result<(), FerraError> {
        let payload = serde_json::to_vec(tx.datoms())
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;

        let payload_len = payload.len();
        let frame_size = HEADER_SIZE + payload_len + CRC_SIZE;
        let mut frame = Vec::with_capacity(frame_size);

        // Magic (4 bytes)
        frame.extend_from_slice(&WAL_MAGIC);
        // Version (2 bytes, little-endian)
        frame.extend_from_slice(&WAL_VERSION.to_le_bytes());
        // Epoch (8 bytes, little-endian)
        frame.extend_from_slice(&epoch.to_le_bytes());
        // Length (4 bytes, little-endian)
        let len_u32 = u32::try_from(payload_len)
            .map_err(|_| FerraError::WalWrite(
                format!("payload too large: {payload_len} bytes exceeds u32::MAX"),
            ))?;
        frame.extend_from_slice(&len_u32.to_le_bytes());
        // Payload (N bytes)
        frame.extend_from_slice(&payload);
        // CRC32 (4 bytes, little-endian) over [Magic..Payload]
        let crc = crc32_ieee(&frame);
        frame.extend_from_slice(&crc.to_le_bytes());

        self.file
            .write_all(&frame)
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;

        Ok(())
    }

    /// Flush the WAL to durable storage.
    ///
    /// INV-FERR-008: After this returns `Ok`, all previously appended entries
    /// are guaranteed to survive a power loss or crash. The epoch may now
    /// safely advance and the snapshot may become visible to readers.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` if the `fsync` syscall fails.
    pub fn fsync(&mut self) -> Result<(), FerraError> {
        self.file
            .sync_all()
            .map_err(|e| FerraError::WalWrite(e.to_string()))
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

    /// Recover all complete WAL entries, truncating incomplete or corrupt ones.
    ///
    /// INV-FERR-008: Recovery reads frames sequentially from the beginning.
    /// A frame is accepted only if all of: magic matches, version matches,
    /// payload bytes are present, and CRC32 verifies. The first frame that
    /// fails any check terminates recovery; the file is truncated at the
    /// end of the last valid frame to discard partial writes from crashes.
    ///
    /// INV-FERR-014: The recovered entries represent the last durable state
    /// that can be replayed into an in-memory store.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalRead` on I/O errors during the initial read,
    /// or `FerraError::WalWrite` if truncation of garbage bytes fails.
    #[allow(clippy::too_many_lines)] // sequential frame parsing is inherently step-heavy
    pub fn recover(&mut self) -> Result<Vec<WalEntry>, FerraError> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| FerraError::WalRead(e.to_string()))?;

        let mut buf = Vec::new();
        self.file
            .read_to_end(&mut buf)
            .map_err(|e| FerraError::WalRead(e.to_string()))?;

        let mut entries = Vec::new();
        let mut pos = 0;
        let mut last_valid_pos: usize = 0;

        while pos + MIN_FRAME_SIZE <= buf.len() {
            // --- Validate header ---

            // Magic (4 bytes)
            if buf[pos..pos + 4] != WAL_MAGIC {
                break;
            }

            // Version (2 bytes)
            let version = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]);
            if version != WAL_VERSION {
                break;
            }

            // Epoch (8 bytes)
            let epoch_bytes: [u8; 8] = match buf[pos + 6..pos + 14].try_into() {
                Ok(b) => b,
                Err(_) => break,
            };
            let epoch = u64::from_le_bytes(epoch_bytes);

            // Payload length (4 bytes)
            let len_bytes: [u8; 4] = match buf[pos + 14..pos + 18].try_into() {
                Ok(b) => b,
                Err(_) => break,
            };
            let payload_len = u32::from_le_bytes(len_bytes) as usize;

            // --- Validate frame completeness ---
            let frame_end = pos + HEADER_SIZE + payload_len + CRC_SIZE;
            if frame_end > buf.len() {
                break; // Incomplete frame (crash mid-write)
            }

            // --- Validate CRC32 ---
            let frame_data = &buf[pos..pos + HEADER_SIZE + payload_len];
            let stored_crc_bytes: [u8; 4] = match buf[pos + HEADER_SIZE + payload_len..frame_end]
                .try_into()
            {
                Ok(b) => b,
                Err(_) => break,
            };
            let stored_crc = u32::from_le_bytes(stored_crc_bytes);
            let computed_crc = crc32_ieee(frame_data);

            if stored_crc != computed_crc {
                break; // CRC mismatch -- corrupt frame
            }

            // --- Frame is valid ---
            let payload = buf[pos + HEADER_SIZE..pos + HEADER_SIZE + payload_len].to_vec();
            entries.push(WalEntry { epoch, payload });
            self.last_synced_epoch = epoch;
            last_valid_pos = frame_end;
            pos = frame_end;
        }

        // Truncate any garbage after the last valid frame so that future
        // appends start from a clean boundary.
        if last_valid_pos < buf.len() {
            self.file
                .set_len(last_valid_pos as u64)
                .map_err(|e| FerraError::WalWrite(e.to_string()))?;
        }

        // Seek to end so subsequent appends land after the last valid frame.
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;

        Ok(entries)
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
fn crc32_ieee(data: &[u8]) -> u32 {
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
    use ferratom::{AgentId, Attribute, EntityId, Value};
    use std::fs::OpenOptions as StdOpenOptions;
    use std::io::Write as StdWrite;
    use tempfile::TempDir;

    /// Build a minimal committed transaction for testing.
    fn sample_tx() -> Transaction<Committed> {
        Transaction::new(AgentId::from_bytes([1u8; 16]))
            .assert_datom(
                EntityId::from_content(b"test"),
                Attribute::from("db/doc"),
                Value::String("test value".into()),
            )
            .commit_unchecked()
    }

    #[test]
    fn test_inv_ferr_008_create_append_recover() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write two entries.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.append(2, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        // Recover and verify.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 2, "INV-FERR-008: expected 2 recovered entries");
            assert_eq!(entries[0].epoch, 1);
            assert_eq!(entries[1].epoch, 2);
            assert_eq!(wal.last_synced_epoch(), 2);
        }
    }

    #[test]
    fn test_inv_ferr_008_truncates_garbage() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write 2 valid entries, then append raw garbage.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.append(2, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }
        {
            let mut f = StdOpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&[0xFF; 20]).unwrap();
        }

        // Recovery must preserve the 2 valid entries and truncate garbage.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(
                entries.len(),
                2,
                "INV-FERR-008: garbage after valid frames must be truncated"
            );
        }
    }

    #[test]
    fn test_inv_ferr_008_truncates_partial_frame() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write 1 valid entry.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        // Append a partial frame: valid magic + version but nothing else.
        {
            let mut f = StdOpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&WAL_MAGIC).unwrap();
            f.write_all(&WAL_VERSION.to_le_bytes()).unwrap();
            // Missing epoch, length, payload, CRC.
        }

        // Recovery must preserve the 1 valid entry.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(
                entries.len(),
                1,
                "INV-FERR-008: partial frame must be truncated"
            );
        }
    }

    #[test]
    fn test_inv_ferr_008_empty_wal_recovers_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        {
            let _wal = Wal::create(&path).unwrap();
        }

        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert!(
                entries.is_empty(),
                "INV-FERR-008: empty WAL must recover zero entries"
            );
            assert_eq!(wal.last_synced_epoch(), 0);
        }
    }

    #[test]
    fn test_inv_ferr_008_recover_then_append() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write 1 entry.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        // Recover, then append more.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 1);

            wal.append(2, &sample_tx()).unwrap();
            wal.append(3, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        // Recover again: should see all 3.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(
                entries.len(),
                3,
                "INV-FERR-008: appends after recovery must be recoverable"
            );
            assert_eq!(entries[0].epoch, 1);
            assert_eq!(entries[1].epoch, 2);
            assert_eq!(entries[2].epoch, 3);
        }
    }

    #[test]
    fn test_inv_ferr_008_crc_corruption_detected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write 2 entries.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.append(2, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        // Corrupt 1 byte in the middle of the second frame's payload.
        {
            let mut data = std::fs::read(&path).unwrap();
            // The second frame starts somewhere after the first. Flip a byte
            // near the end of the file (inside the second frame's payload).
            let corrupt_pos = data.len() - 10;
            data[corrupt_pos] ^= 0xFF;
            std::fs::write(&path, &data).unwrap();
        }

        // Recovery must return only the first (uncorrupted) entry.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(
                entries.len(),
                1,
                "INV-FERR-008: CRC mismatch must stop recovery at the corrupt frame"
            );
            assert_eq!(entries[0].epoch, 1);
        }
    }

    #[test]
    fn test_crc32_deterministic() {
        let data = b"hello world";
        assert_eq!(crc32_ieee(data), crc32_ieee(data));
    }

    #[test]
    fn test_crc32_known_value() {
        // "hello world" CRC32 IEEE = 0x0D4A1185
        let crc = crc32_ieee(b"hello world");
        assert_eq!(crc, 0x0D4A_1185, "CRC32 of 'hello world' must match known value");
    }

    #[test]
    fn test_crc32_empty() {
        // CRC32 of empty input = 0x00000000
        let crc = crc32_ieee(b"");
        assert_eq!(crc, 0x0000_0000, "CRC32 of empty input must be 0");
    }

    #[test]
    fn test_inv_ferr_008_payload_is_valid_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 1);

            // The payload must be valid JSON (array of Datom).
            let parsed: serde_json::Value =
                serde_json::from_slice(&entries[0].payload).unwrap();
            assert!(
                parsed.is_array(),
                "INV-FERR-008: WAL payload must be a JSON array of datoms"
            );
        }
    }

    /// Regression: bd-32t — WAL payload roundtrip preserves datom content.
    #[test]
    fn test_bug_bd_32t_payload_content_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append(1, &sample_tx()).unwrap();
            wal.fsync().unwrap();
        }

        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 1);

            // Deserialize and verify datom content survives roundtrip.
            let datoms: Vec<ferratom::Datom> =
                serde_json::from_slice(&entries[0].payload).unwrap();
            assert!(!datoms.is_empty(), "bd-32t: payload must contain datoms");
            assert_eq!(
                datoms[0].attribute().as_str(),
                "db/doc",
                "bd-32t: datom attribute must survive WAL roundtrip"
            );
        }
    }
}
