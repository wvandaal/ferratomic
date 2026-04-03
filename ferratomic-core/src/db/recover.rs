//! WAL and checkpoint recovery constructors for `Database`.
//!
//! INV-FERR-014: Recovery produces the last committed state.
//! INV-FERR-013: Checkpoint round-trip identity (`load(checkpoint(S)) = S`).
//! INV-FERR-008: Post-recovery WAL ensures durable-before-visible ordering.
//!
//! All constructors in this module attach a WAL for post-recovery durability.
//! They return `Database<Ready>` directly.

use std::path::Path;

use ferratom::{wire::WireDatom, FerraError};

use super::{Database, Ready};
use crate::{store::Store, wal::Wal};

impl Database<Ready> {
    /// Create a genesis database backed by a WAL file.
    ///
    /// INV-FERR-008: All subsequent `transact()` calls write and fsync
    /// the WAL before advancing the epoch.
    /// INV-FERR-031: The initial store is deterministic.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the WAL file cannot be created.
    pub fn genesis_with_wal(wal_path: &Path) -> Result<Self, FerraError> {
        let wal = Wal::create(wal_path)?;
        Ok(Self::build(Store::genesis(), Some(wal)))
    }

    /// Recover a database from a WAL file.
    ///
    /// INV-FERR-014: Recovery replays all complete WAL entries into a
    /// genesis store, producing the last committed state.
    ///
    /// TODO(bd-nwva): migrate to `Store::batch_replay()` for single
    /// promote/demote cycle instead of N individual `replay_entry` calls.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the WAL cannot be opened or recovery fails.
    pub fn recover_from_wal(wal_path: &Path) -> Result<Self, FerraError> {
        let mut wal = Wal::open(wal_path)?;
        let entries = wal.recover()?;

        let mut store = Store::genesis();
        for entry in &entries {
            // ADR-FERR-010: Deserialize as wire types, then convert through
            // trust boundary. CRC was verified by Wal::recover().
            let wire_datoms: Vec<WireDatom> = bincode::deserialize(&entry.payload)
                .map_err(|e| FerraError::WalRead(e.to_string()))?;
            let datoms: Vec<ferratom::Datom> = wire_datoms
                .into_iter()
                .map(ferratom::wire::WireDatom::into_trusted)
                .collect();
            // INV-FERR-014: replay restores full state (datoms + schema + epoch),
            // not just raw datom insertion.
            store.replay_entry(entry.epoch, &datoms)?;
        }

        Ok(Self::build(store, Some(wal)))
    }

    /// Recover a database from a checkpoint file plus WAL delta.
    ///
    /// INV-FERR-013: Loads the checkpoint as the base state.
    /// INV-FERR-014: Replays only WAL entries with epoch > checkpoint epoch.
    /// This is the full three-level recovery path: checkpoint -> WAL delta -> ready.
    ///
    /// TODO(bd-nwva): migrate to `Store::batch_replay()` for single
    /// promote/demote cycle instead of N individual `replay_entry` calls.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::CheckpointCorrupted` if the checkpoint is invalid.
    /// Returns `FerraError::WalRead` if WAL recovery fails.
    pub fn recover(checkpoint_path: &Path, wal_path: &Path) -> Result<Self, FerraError> {
        // Step 1: Load checkpoint (verified by BLAKE3).
        let mut store = crate::checkpoint::load_checkpoint(checkpoint_path)?;
        let checkpoint_epoch = store.epoch();

        // Step 2: Replay WAL entries after the checkpoint epoch.
        let mut wal = Wal::open(wal_path)?;
        let entries = wal.recover()?;

        for entry in &entries {
            if entry.epoch > checkpoint_epoch {
                // ADR-FERR-010: Deserialize as wire types, convert through trust boundary.
                let wire_datoms: Vec<WireDatom> = bincode::deserialize(&entry.payload)
                    .map_err(|e| FerraError::WalRead(e.to_string()))?;
                let datoms: Vec<ferratom::Datom> = wire_datoms
                    .into_iter()
                    .map(ferratom::wire::WireDatom::into_trusted)
                    .collect();
                // INV-FERR-014: replay restores full state (datoms + schema + epoch).
                store.replay_entry(entry.epoch, &datoms)?;
            }
        }

        Ok(Self::build(store, Some(wal)))
    }

    /// Create a database from an existing store with a new WAL file.
    ///
    /// INV-FERR-006: the provided store becomes the initial snapshot state.
    /// INV-FERR-008: subsequent transacts are durable via the WAL.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the WAL file cannot be created.
    pub fn from_store_with_wal(store: Store, wal_path: &Path) -> Result<Self, FerraError> {
        let wal = Wal::create(wal_path)?;
        Ok(Self::build(store, Some(wal)))
    }
}
