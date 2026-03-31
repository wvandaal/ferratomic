//! WAL recovery path: frame parsing and truncation.
//!
//! INV-FERR-008: Recovery reads frames sequentially from the beginning.
//! A frame is accepted only if magic matches, version matches, payload
//! bytes are present, and CRC32 verifies. The first frame that fails
//! any check terminates recovery.

use std::io::{Read as IoRead, Seek, SeekFrom};

use ferratom::FerraError;

use super::{
    crc32_ieee, Wal, WalEntry, CRC_SIZE, HEADER_SIZE, MIN_FRAME_SIZE, WAL_MAGIC, WAL_VERSION,
};

/// HI-006: Maximum WAL payload size (256 MiB). Prevents OOM on crafted
/// frames with spoofed u32 length fields. A single transaction producing
/// >256 MiB of bincode-serialized datoms is pathological.
const MAX_PAYLOAD_SIZE: usize = 256 * 1024 * 1024;

impl Wal {
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
    pub fn recover(&mut self) -> Result<Vec<WalEntry>, FerraError> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| FerraError::WalRead(e.to_string()))?;

        let mut buf = Vec::new();
        self.file
            .read_to_end(&mut buf)
            .map_err(|e| FerraError::WalRead(e.to_string()))?;

        let (entries, last_valid_pos) = parse_wal_frames_with_pos(&buf);

        // Track the last recovered epoch.
        if let Some(last) = entries.last() {
            self.last_synced_epoch = last.epoch;
        }

        // Truncate any garbage after the last valid frame so that future
        // appends start from a clean boundary.
        if last_valid_pos < buf.len() {
            self.file
                .set_len(last_valid_pos as u64)
                .map_err(|e| FerraError::WalWrite(e.to_string()))?;
            // ME-013: fsync after truncation to ensure the truncated state
            // is durable. Without this, a crash after set_len could leave
            // the WAL with inconsistent length.
            self.file
                .sync_all()
                .map_err(|e| FerraError::WalWrite(e.to_string()))?;
        }

        // ME-012: Update pending_epoch to match recovered state.
        self.pending_epoch = self.last_synced_epoch;

        // Seek to end so subsequent appends land after the last valid frame.
        self.file
            .seek(SeekFrom::End(0))
            .map_err(|e| FerraError::WalWrite(e.to_string()))?;

        Ok(entries)
    }
}

/// Recover WAL entries from an arbitrary reader.
///
/// INV-FERR-008, INV-FERR-024: Backend-agnostic WAL recovery. Reads all
/// bytes from the reader and parses valid frames exactly as
/// [`Wal::recover`] does, but without truncation (the reader may not
/// support truncation). Returns valid entries up to the first invalid frame.
///
/// # Errors
///
/// Returns `FerraError::WalRead` on I/O errors during the read.
pub fn recover_wal_from_reader<R: IoRead>(reader: &mut R) -> Result<Vec<WalEntry>, FerraError> {
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| FerraError::WalRead(e.to_string()))?;

    let (entries, _pos) = parse_wal_frames_with_pos(&buf);
    Ok(entries)
}

/// Parse WAL frames from a byte buffer, returning all valid entries and
/// the byte offset past the last valid frame.
///
/// INV-FERR-008: Shared frame-parsing logic. A frame is accepted only if
/// magic matches, version matches, payload bytes are present, and CRC32
/// verifies. The first frame that fails any check terminates parsing.
fn parse_wal_frames_with_pos(buf: &[u8]) -> (Vec<WalEntry>, usize) {
    let mut entries = Vec::new();
    let mut pos = 0;
    let mut last_valid_pos: usize = 0;

    while pos + MIN_FRAME_SIZE <= buf.len() {
        match try_parse_frame(buf, pos) {
            Some((entry, frame_end)) => {
                entries.push(entry);
                last_valid_pos = frame_end;
                pos = frame_end;
            }
            None => break,
        }
    }

    (entries, last_valid_pos)
}

/// Try to parse a single WAL frame starting at `pos` in the buffer.
///
/// Returns `Some((entry, frame_end))` if the frame is valid, where
/// `frame_end` is the byte offset past the frame. Returns `None` if
/// magic, version, completeness, or CRC validation fails.
fn try_parse_frame(buf: &[u8], pos: usize) -> Option<(WalEntry, usize)> {
    // Magic (4 bytes)
    if buf[pos..pos + 4] != WAL_MAGIC {
        return None;
    }

    // Version (2 bytes)
    let version = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]);
    if version != WAL_VERSION {
        return None;
    }

    // Epoch (8 bytes)
    let epoch_bytes: [u8; 8] = buf[pos + 6..pos + 14].try_into().ok()?;
    let epoch = u64::from_le_bytes(epoch_bytes);

    // Payload length (4 bytes)
    let len_bytes: [u8; 4] = buf[pos + 14..pos + 18].try_into().ok()?;
    let payload_len = u32::from_le_bytes(len_bytes) as usize;

    // HI-006: Reject frames with payload exceeding MAX_PAYLOAD_SIZE to
    // prevent OOM on crafted frames with spoofed length fields.
    if payload_len > MAX_PAYLOAD_SIZE {
        return None;
    }

    // Frame completeness
    let frame_end = pos + HEADER_SIZE + payload_len + CRC_SIZE;
    if frame_end > buf.len() {
        return None; // Incomplete frame (crash mid-write)
    }

    // CRC32 verification
    let frame_data = &buf[pos..pos + HEADER_SIZE + payload_len];
    let stored_crc_bytes: [u8; 4] = buf[pos + HEADER_SIZE + payload_len..frame_end]
        .try_into()
        .ok()?;
    let stored_crc = u32::from_le_bytes(stored_crc_bytes);
    let computed_crc = crc32_ieee(frame_data);

    if stored_crc != computed_crc {
        return None; // CRC mismatch -- corrupt frame
    }

    // Frame is valid
    let payload = buf[pos + HEADER_SIZE..pos + HEADER_SIZE + payload_len].to_vec();
    Some((WalEntry { epoch, payload }, frame_end))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{fs::OpenOptions as StdOpenOptions, io::Write as StdWrite};

    use ferratom::{AgentId, Attribute, EntityId, Value};
    use tempfile::TempDir;

    use crate::{
        wal::{Wal, WAL_MAGIC, WAL_VERSION},
        writer::Transaction,
    };

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
}
