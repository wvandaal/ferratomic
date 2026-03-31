//! Anti-entropy protocol trait boundary (INV-FERR-022).
//!
//! Phase 4a defines the trait; Phase 4c provides real implementations.
//! The `NullAntiEntropy` no-op implementation satisfies the trait for
//! single-node operation.

use ferratom::FerraError;

use crate::store::Store;

/// Anti-entropy synchronization interface (INV-FERR-022).
///
/// Implementations must guarantee eventual convergence: after
/// finite message exchanges, all replicas hold identical datom sets.
///
/// # Errors
///
/// Methods return `FerraError` on transport or serialization failures.
pub trait AntiEntropy {
    /// Compute the diff between local store and a remote root hash.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` on serialization or transport failure.
    fn diff(&self, local: &Store) -> Result<Vec<u8>, FerraError>;

    /// Apply a received diff to the local store.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` on deserialization or merge failure.
    fn apply_diff(&self, store: &mut Store, diff: &[u8]) -> Result<(), FerraError>;
}

/// No-op anti-entropy for single-node operation (INV-FERR-022).
///
/// Returns empty diffs and ignores applied diffs. This is the default
/// when no federation is configured. Phase 4c replaces this with a
/// real implementation backed by the prolly tree block store.
#[derive(Debug, Default, Clone)]
pub struct NullAntiEntropy;

impl AntiEntropy for NullAntiEntropy {
    fn diff(&self, _local: &Store) -> Result<Vec<u8>, FerraError> {
        Ok(Vec::new())
    }

    fn apply_diff(&self, _store: &mut Store, _diff: &[u8]) -> Result<(), FerraError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn test_null_anti_entropy_round_trip() {
        let mut store = Store::genesis();
        let ae = NullAntiEntropy;

        let diff = ae.diff(&store).expect("diff should succeed");
        assert!(diff.is_empty(), "NullAntiEntropy diff must be empty");

        ae.apply_diff(&mut store, &diff)
            .expect("apply_diff should succeed on empty diff");
    }

    #[test]
    fn test_null_anti_entropy_apply_nonempty_diff_is_noop() {
        let mut store = Store::genesis();
        let ae = NullAntiEntropy;
        let epoch_before = store.epoch();

        ae.apply_diff(&mut store, &[0xDE, 0xAD])
            .expect("apply_diff should ignore arbitrary bytes");

        assert_eq!(
            store.epoch(),
            epoch_before,
            "NullAntiEntropy must not mutate the store"
        );
    }
}
