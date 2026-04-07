//! WAL write path: append and fsync.
//!
//! INV-FERR-008: Frames are appended to the WAL file, then fsynced
//! to guarantee durability before the epoch advances.

use std::io::Write as IoWrite;

use ferratom::FerraError;

use super::{crc32_ieee, Wal, CRC_SIZE, HEADER_SIZE, WAL_MAGIC, WAL_VERSION};

impl Wal {
    /// Append a pre-serialized payload to the WAL at the given epoch.
    ///
    /// INV-FERR-008: The frame is written to the OS page cache but NOT
    /// fsynced. Call [`fsync`](Self::fsync) to guarantee durability before
    /// advancing the epoch.
    ///
    /// The WAL does not interpret the payload — serialization format is
    /// the caller's responsibility (typically `bincode::serialize(datoms)`
    /// at the Database layer).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::WalWrite` if the write fails.
    pub fn append_raw(&mut self, epoch: u64, payload: &[u8]) -> Result<(), FerraError> {
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
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;
        // ME-012: Update last_synced_epoch after successful fsync so that
        // epoch monotonicity enforcement (ME-011) reflects actual durable state.
        // The pending_epoch tracks the highest epoch written since last fsync.
        self.last_synced_epoch = self.last_synced_epoch.max(self.pending_epoch);
        Ok(())
    }

    /// Build a WAL frame and write it to the file.
    ///
    /// ME-011: Enforces epoch monotonicity — each frame's epoch must be
    /// strictly greater than the previous synced epoch.
    ///
    /// Shared by [`append_raw`](Self::append_raw).
    fn write_frame(&mut self, epoch: u64, payload: &[u8]) -> Result<(), FerraError> {
        // ME-011: Enforce WAL epoch monotonicity. Appending an epoch <=
        // the highest pending (written but not yet fsynced) epoch violates
        // INV-FERR-007 (strict monotonicity). Using pending_epoch instead
        // of last_synced_epoch closes the gap where duplicate epochs could
        // slip in between writes before the first fsync (bd-y286).
        if epoch <= self.pending_epoch && self.pending_epoch > 0 {
            return Err(FerraError::InvariantViolation {
                invariant: "INV-FERR-007".to_string(),
                details: format!(
                    "WAL epoch monotonicity violated: epoch {epoch} <= pending {}",
                    self.pending_epoch
                ),
            });
        }
        let payload_len = payload.len();
        // HI-006: Reject oversized payloads on the write path too.
        if payload_len > 256 * 1024 * 1024 {
            return Err(FerraError::WalWrite(format!(
                "payload too large: {payload_len} bytes exceeds 256 MiB limit"
            )));
        }
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

        // ME-012: Track highest epoch written for fsync bookkeeping.
        self.pending_epoch = self.pending_epoch.max(epoch);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::wal::Wal;

    /// Sample payload bytes for testing WAL frame mechanics.
    /// The WAL is payload-agnostic; any bytes suffice for frame tests.
    fn sample_payload() -> Vec<u8> {
        b"test payload for WAL frame verification".to_vec()
    }

    #[test]
    fn test_inv_ferr_008_create_append_recover() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");

        // Write two entries.
        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append_raw(1, &sample_payload()).unwrap();
            wal.append_raw(2, &sample_payload()).unwrap();
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
            wal.append_raw(1, &sample_payload()).unwrap();
            wal.fsync().unwrap();
        }

        // Recover, then append more.
        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 1);

            wal.append_raw(2, &sample_payload()).unwrap();
            wal.append_raw(3, &sample_payload()).unwrap();
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
    fn test_inv_ferr_007_epoch_monotonicity_rejection() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");
        let mut wal = Wal::create(&path).unwrap();
        wal.append_raw(1, &sample_payload()).unwrap();

        // Duplicate epoch must be rejected (INV-FERR-007).
        let err = wal.append_raw(1, &sample_payload());
        assert!(
            err.is_err(),
            "INV-FERR-007: duplicate epoch must be rejected"
        );

        // Decreasing epoch must be rejected (INV-FERR-007).
        let err = wal.append_raw(0, &sample_payload());
        assert!(
            err.is_err(),
            "INV-FERR-007: decreasing epoch must be rejected"
        );

        // Next epoch must still succeed.
        wal.append_raw(2, &sample_payload()).unwrap();
    }

    #[test]
    fn test_inv_ferr_008_payload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.wal");
        let payload = b"roundtrip content verification";

        {
            let mut wal = Wal::create(&path).unwrap();
            wal.append_raw(1, payload).unwrap();
            wal.fsync().unwrap();
        }

        {
            let mut wal = Wal::open(&path).unwrap();
            let entries = wal.recover().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0].payload, payload,
                "INV-FERR-008: WAL payload must survive write-recover roundtrip"
            );
        }
    }
}
