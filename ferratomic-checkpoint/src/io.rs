//! File I/O helpers for checkpoint persistence.
//!
//! Atomic write (HI-001), parent directory fsync (HI-003), and
//! backend-agnostic reader/writer functions (INV-FERR-024).

use std::{
    fs::File,
    io::{BufWriter, Write as IoWrite},
    path::Path,
};

use ferratom::FerraError;

use crate::{
    deserialize_checkpoint_bytes, serialize_checkpoint_bytes, serialize_live_first_bytes,
    CheckpointData,
};

/// Write checkpoint data to a file (V3 format).
///
/// INV-FERR-013: The checkpoint contains the full store state in a format
/// that `load_checkpoint` can reconstruct exactly. A trailing BLAKE3 hash
/// covers all preceding bytes for tamper detection.
///
/// HI-001: Write is atomic via write-to-temp-then-rename. A crash during
/// write leaves the old checkpoint intact (the temp file is discarded).
/// HI-003: Parent directory is fsynced after rename to ensure the new
/// directory entry is durable on ext4/XFS.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if file creation, serialization,
/// or fsync fails.
pub fn write_checkpoint(data: &CheckpointData, path: &Path) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(data)?;

    // HI-001: Atomic write via temp file + rename. A crash between
    // temp creation and rename leaves the original checkpoint intact.
    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(format!(".checkpoint.{}.tmp", std::process::id()));

    // Write to temp file and fsync the data.
    {
        let file =
            File::create(&tmp_path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&buf)
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    }

    // Atomic rename (POSIX guarantees atomicity for same-filesystem rename).
    std::fs::rename(&tmp_path, path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    // HI-003: fsync parent directory to ensure the new directory entry
    // is durable. Required on ext4/XFS for metadata durability.
    fsync_parent_dir(parent)?;

    Ok(())
}

/// Write a LIVE-first V3 checkpoint to a file (INV-FERR-075).
///
/// Same atomic write pattern as `write_checkpoint` (HI-001, HI-003).
/// LIVE datoms are stored first for partial cold start.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization, write, or
/// fsync fails.
pub fn write_checkpoint_live_first(data: &CheckpointData, path: &Path) -> Result<(), FerraError> {
    let buf = serialize_live_first_bytes(data)?;

    let parent = path
        .parent()
        .ok_or_else(|| FerraError::CheckpointWrite("path has no parent directory".to_string()))?;
    let tmp_path = parent.join(format!(".checkpoint_lf.{}.tmp", std::process::id()));

    {
        let file =
            File::create(&tmp_path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&buf)
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    }

    std::fs::rename(&tmp_path, path).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    fsync_parent_dir(parent)?;

    Ok(())
}

/// Fsync a parent directory to ensure directory entry durability (HI-002, HI-003).
///
/// Required on ext4, XFS, and other journaling filesystems where file
/// data may be durable but directory entries are not until the parent
/// directory is fsynced.
fn fsync_parent_dir(dir: &Path) -> Result<(), FerraError> {
    let dir_file = File::open(dir).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    dir_file
        .sync_all()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    Ok(())
}

/// Load checkpoint data from a file.
///
/// INV-FERR-013: Verifies the BLAKE3 checksum before reconstructing
/// checkpoint data. Returns an error if the file is truncated, the magic
/// is wrong, or the checksum fails.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// `FerraError::Io` on read failure, or `FerraError::CheckpointWrite`
/// on deserialization failure.
pub fn load_checkpoint(path: &Path) -> Result<CheckpointData, FerraError> {
    let data = std::fs::read(path).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;

    deserialize_checkpoint_bytes(&data)
}

/// Load checkpoint data from an arbitrary reader (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint loading for `StorageBackend` implementations.
///
/// # Errors
///
/// Returns `FerraError::Io` on read failure or `FerraError::CheckpointCorrupted`
/// on checksum/format errors.
pub fn load_checkpoint_from_reader<R: std::io::Read>(
    reader: &mut R,
) -> Result<CheckpointData, FerraError> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: e.to_string(),
    })?;
    deserialize_checkpoint_bytes(&data)
}

/// Write checkpoint data to an arbitrary writer (INV-FERR-013, INV-FERR-024).
///
/// Backend-agnostic checkpoint writing for `StorageBackend` implementations.
///
/// INV-FERR-013: `load(checkpoint(S)) = S` -- round-trip identity.
/// INV-FERR-024: substrate agnosticism -- writes through any `std::io::Write`
/// implementor, decoupling the checkpoint protocol from filesystem specifics.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization or the write fails.
pub fn write_checkpoint_to_writer<W: std::io::Write>(
    data: &CheckpointData,
    writer: &mut W,
) -> Result<(), FerraError> {
    let buf = serialize_checkpoint_bytes(data)?;
    writer
        .write_all(&buf)
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    writer
        .flush()
        .map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;
    Ok(())
}
