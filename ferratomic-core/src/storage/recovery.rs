//! Cold-start recovery cascade (INV-FERR-014, INV-FERR-028).
//!
//! Two entry points:
//! - [`cold_start_with_backend`]: Generic, backend-agnostic recovery.
//! - [`cold_start`]: Filesystem-specific convenience function.

use std::path::Path;

use ferratom::FerraError;

use super::{ColdStartResult, RecoveryLevel, StorageBackend, CHECKPOINT_FILENAME, WAL_FILENAME};
use crate::db::Database;

// ---------------------------------------------------------------------------
// Generic cold start (INV-FERR-024)
// ---------------------------------------------------------------------------

/// Open or create storage via a backend and recover the database.
///
/// INV-FERR-024: Backend-agnostic cold start.
/// INV-FERR-028: Tries the fastest recovery path first.
///
/// # Errors
///
/// Returns `FerraError` if `create_dirs` fails or recovery encounters
/// a non-corruption I/O error (HI-008).
pub fn cold_start_with_backend<B: StorageBackend>(
    backend: &B,
) -> Result<ColdStartResult, FerraError> {
    backend.create_dirs()?;

    let has_checkpoint = backend.checkpoint_exists();
    let has_wal = backend.wal_exists();

    // Level 1: checkpoint + WAL (fastest path).
    if has_checkpoint && has_wal {
        match recover_checkpoint_plus_wal(backend) {
            Ok(result) => return Ok(result),
            Err(FerraError::CheckpointCorrupted { .. } | FerraError::WalRead(_)) => {}
            Err(e) => return Err(e),
        }
    }

    // Level 1b: checkpoint only.
    if has_checkpoint && !has_wal {
        match recover_checkpoint_only(backend) {
            Ok(result) => return Ok(result),
            Err(FerraError::CheckpointCorrupted { .. }) => {}
            Err(e) => return Err(e),
        }
    }

    // Level 2: WAL-only.
    if has_wal {
        match recover_wal_only(backend) {
            Ok(result) => return Ok(result),
            Err(FerraError::WalRead(_)) => {}
            Err(e) => return Err(e),
        }
    }

    // Level 3: genesis.
    Ok(ColdStartResult {
        database: Database::genesis(),
        level: RecoveryLevel::Genesis,
    })
}

/// Level 1: checkpoint + WAL delta (INV-FERR-013, INV-FERR-014).
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
            let wire_datoms: Vec<ferratom::wire::WireDatom> = bincode::deserialize(&entry.payload)
                .map_err(|e| FerraError::WalRead(e.to_string()))?;
            let datoms: Vec<ferratom::Datom> = wire_datoms
                .into_iter()
                .map(ferratom::wire::WireDatom::into_trusted)
                .collect();
            db_store.replay_entry(entry.epoch, &datoms)?;
        }
    }

    Ok(ColdStartResult {
        database: Database::from_store(db_store),
        level: RecoveryLevel::CheckpointPlusWal,
    })
}

/// Level 1b: checkpoint only (INV-FERR-013).
fn recover_checkpoint_only<B: StorageBackend>(backend: &B) -> Result<ColdStartResult, FerraError> {
    let mut reader = backend.open_checkpoint_reader()?;
    let store = crate::checkpoint::load_checkpoint_from_reader(&mut reader)?;

    Ok(ColdStartResult {
        database: Database::from_store(store),
        level: RecoveryLevel::CheckpointOnly,
    })
}

/// Level 2: WAL-only (INV-FERR-014).
fn recover_wal_only<B: StorageBackend>(backend: &B) -> Result<ColdStartResult, FerraError> {
    let mut wal_reader = backend.open_wal_reader()?;
    let entries = crate::wal::recover_wal_from_reader(&mut wal_reader)?;

    let mut store = crate::store::Store::genesis();
    for entry in &entries {
        let wire_datoms: Vec<ferratom::wire::WireDatom> =
            bincode::deserialize(&entry.payload).map_err(|e| FerraError::WalRead(e.to_string()))?;
        let datoms: Vec<ferratom::Datom> = wire_datoms
            .into_iter()
            .map(ferratom::wire::WireDatom::into_trusted)
            .collect();
        store.replay_entry(entry.epoch, &datoms)?;
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
/// INV-FERR-028: Filesystem-specific entry point for cold start.
/// Creates an [`FsBackend`](super::FsBackend) and uses the filesystem-aware
/// recovery path that preserves WAL attachment for post-recovery durability.
///
/// # Errors
///
/// Returns `FerraError` if the data directory cannot be created or
/// recovery encounters a non-corruption I/O error.
pub fn cold_start(data_dir: &Path) -> Result<ColdStartResult, FerraError> {
    std::fs::create_dir_all(data_dir).map_err(|e| FerraError::Io {
        kind: format!("{:?}", e.kind()),
        message: format!("cannot create data dir: {e}"),
    })?;

    let checkpoint_path = data_dir.join(CHECKPOINT_FILENAME);
    let wal_path = data_dir.join(WAL_FILENAME);

    let has_checkpoint = checkpoint_path.exists();
    let has_wal = wal_path.exists();

    if has_checkpoint && has_wal {
        if let Some(result) = try_checkpoint_plus_wal(&checkpoint_path, &wal_path) {
            return Ok(result);
        }
    }

    if has_checkpoint && !has_wal {
        if let Some(result) = try_checkpoint_only(&checkpoint_path, &wal_path) {
            return Ok(result);
        }
    }

    if has_wal {
        if let Some(result) = try_wal_only(&wal_path) {
            return Ok(result);
        }
    }

    let db = Database::genesis_with_wal(&wal_path)?;

    Ok(ColdStartResult {
        database: db,
        level: RecoveryLevel::Genesis,
    })
}

/// Level 1 filesystem recovery (INV-FERR-013, INV-FERR-014).
fn try_checkpoint_plus_wal(checkpoint_path: &Path, wal_path: &Path) -> Option<ColdStartResult> {
    let db = Database::recover(checkpoint_path, wal_path).ok()?;
    Some(ColdStartResult {
        database: db,
        level: RecoveryLevel::CheckpointPlusWal,
    })
}

/// Level 1b filesystem recovery (INV-FERR-013, INV-FERR-008).
fn try_checkpoint_only(checkpoint_path: &Path, wal_path: &Path) -> Option<ColdStartResult> {
    let store = crate::checkpoint::load_checkpoint(checkpoint_path).ok()?;
    let db = match Database::from_store_with_wal(store.clone(), wal_path) {
        Ok(db) => db,
        Err(_) => Database::from_store(store),
    };
    Some(ColdStartResult {
        database: db,
        level: RecoveryLevel::CheckpointOnly,
    })
}

/// Level 2 filesystem recovery (INV-FERR-014).
fn try_wal_only(wal_path: &Path) -> Option<ColdStartResult> {
    let db = Database::recover_from_wal(wal_path).ok()?;
    Some(ColdStartResult {
        database: db,
        level: RecoveryLevel::WalOnly,
    })
}
