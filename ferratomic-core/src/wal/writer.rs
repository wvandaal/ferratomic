//! WAL write path: append and fsync.
//!
//! INV-FERR-008: Frames are appended to the WAL file, then fsynced
//! to guarantee durability before the epoch advances.

use std::io::Write as IoWrite;

use ferratom::FerraError;

use crate::writer::{Committed, Transaction};

use super::{crc32_ieee, Wal, CRC_SIZE, HEADER_SIZE, WAL_MAGIC, WAL_VERSION};

impl Wal {
    /// Append a committed transaction to the WAL at the given epoch.
    ///
    /// INV-FERR-008: The frame is written to the OS page cache but NOT
    /// fsynced. Call [`fsync`](Self::fsync) to guarantee durability before
    /// advancing the epoch.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` if bincode serialization or the write fails.
    pub fn append(&mut self, epoch: u64, tx: &Transaction<Committed>) -> Result<(), FerraError> {
        let payload =
            bincode::serialize(tx.datoms()).map_err(|e| FerraError::WalWrite(e.to_string()))?;

        self.write_frame(epoch, &payload)
    }

    /// Append a pre-serialized payload to the WAL at the given epoch.
    ///
    /// INV-FERR-008: Like [`append`](Self::append), the frame is written but NOT
    /// fsynced. Used by [`Database::transact`](crate::db::Database::transact) to
    /// write post-stamp datoms (with real `TxId`s and tx metadata).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` if the write fails.
    pub(crate) fn append_raw(&mut self, epoch: u64, payload: &[u8]) -> Result<(), FerraError> {
        self.write_frame(epoch, payload)
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

    /// Build a WAL frame and write it to the file.
    ///
    /// Shared by [`append`](Self::append) and [`append_raw`](Self::append_raw).
    fn write_frame(&mut self, epoch: u64, payload: &[u8]) -> Result<(), FerraError> {
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
        let len_u32 = u32::try_from(payload_len).map_err(|_| {
            FerraError::WalWrite(format!(
                "payload too large: {payload_len} bytes exceeds u32::MAX"
            ))
        })?;
        frame.extend_from_slice(&len_u32.to_le_bytes());
        // Payload (N bytes)
        frame.extend_from_slice(payload);
        // CRC32 (4 bytes, little-endian) over [Magic..Payload]
        let crc = crc32_ieee(&frame);
        frame.extend_from_slice(&crc.to_le_bytes());

        self.file
            .write_all(&frame)
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ferratom::{AgentId, Attribute, EntityId, Value};
    use tempfile::TempDir;

    use crate::wal::Wal;
    use crate::writer::Transaction;

    /// Build a minimal committed transaction for testing.
    fn sample_tx() -> Transaction<crate::writer::Committed> {
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
            assert_eq!(
                entries.len(),
                2,
                "INV-FERR-008: expected 2 recovered entries"
            );
            assert_eq!(entries[0].epoch, 1);
            assert_eq!(entries[1].epoch, 2);
            assert_eq!(wal.last_synced_epoch(), 2);
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
    fn test_inv_ferr_008_payload_is_valid_bincode() {
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

            // The payload must be valid bincode (Vec<Datom>).
            let datoms: Vec<ferratom::Datom> = bincode::deserialize(&entries[0].payload).unwrap();
            assert!(
                !datoms.is_empty(),
                "INV-FERR-008: WAL payload must contain datoms"
            );
        }
    }

    /// Regression: bd-32t -- WAL payload roundtrip preserves datom content.
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
            let datoms: Vec<ferratom::Datom> = bincode::deserialize(&entries[0].payload).unwrap();
            assert!(!datoms.is_empty(), "bd-32t: payload must contain datoms");
            assert_eq!(
                datoms[0].attribute().as_str(),
                "db/doc",
                "bd-32t: datom attribute must survive WAL roundtrip"
            );
        }
    }
}
