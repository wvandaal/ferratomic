//! Anti-entropy protocol trait boundary (INV-FERR-022).
//!
//! Phase 4a defines the trait; Phase 4c provides real implementations.
//! The `NullAntiEntropy` no-op implementation satisfies the trait for
//! single-node operation.

use ferratom::FerraError;

use crate::store::Store;

/// Anti-entropy synchronization interface for store reconciliation.
///
/// INV-FERR-022: the anti-entropy protocol guarantees
/// `apply_diff(B, diff(A, B)) ⊇ A ∪ B`. That is, applying the diff
/// computed from store A against store B to store B produces a store
/// that contains at least the union of both stores' datom sets.
///
/// The protocol operates in two phases: `diff` serializes the datoms
/// present in the local store but missing from the remote (as inferred
/// from root-hash comparison), and `apply_diff` merges those datoms
/// into the receiving store. Because the underlying store is a G-Set
/// CRDT semilattice (INV-FERR-001 through INV-FERR-003), merge is
/// commutative, associative, and idempotent — repeated or reordered
/// diff/apply exchanges converge to the same state.
///
/// # Contract
///
/// Implementors guarantee:
/// - **Convergence**: repeated `diff`/`apply_diff` exchanges between
///   any two stores eventually produce identical datom sets.
/// - **Idempotency**: applying the same diff twice leaves the store
///   unchanged (inherited from INV-FERR-003).
/// - **Monotonicity**: `apply_diff` never removes datoms from the
///   receiving store (inherited from INV-FERR-004).
///
/// # Errors
///
/// Methods return [`FerraError`] on transport or serialization failures.
/// Transport errors are retryable; the protocol is idempotent, so
/// retrying a failed exchange is always safe.
pub trait AntiEntropy {
    /// Serialize the datoms in the local store as a diff payload.
    ///
    /// INV-FERR-022: the returned byte vector encodes the datoms
    /// that the remote store needs in order to reach a state that
    /// is a superset of `local ∪ remote`. Phase 4c implementations
    /// compare prolly-tree root hashes to compute a minimal diff;
    /// the `NullAntiEntropy` stub returns an empty vector (no datoms
    /// to send in single-node mode).
    ///
    /// # Errors
    ///
    /// Returns `FerraError` on serialization or transport failure.
    fn diff(&self, local: &Store) -> Result<Vec<u8>, FerraError>;

    /// Merge a received diff payload into the local store.
    ///
    /// INV-FERR-022: after `apply_diff(store, diff)` succeeds, `store`
    /// contains at least the union of its previous datom set and the
    /// datoms encoded in `diff`. Because store merge is idempotent
    /// (INV-FERR-003), applying the same diff twice is a no-op.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` on deserialization or merge failure.
    fn apply_diff(&self, store: &mut Store, diff: &[u8]) -> Result<(), FerraError>;
}

/// No-op anti-entropy for single-node operation.
///
/// INV-FERR-022: vacuously satisfies the anti-entropy contract because
/// `diff` returns an empty payload (no datoms to reconcile) and
/// `apply_diff` leaves the store unchanged. In a single-node
/// configuration there is no remote store, so the union identity
/// `apply_diff(B, diff(A, B)) >= A u B` holds trivially.
///
/// Phase 4c will replace this with a real implementation backed by the
/// prolly tree block store.
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// anti-entropy round-trip properties (INV-FERR-022 conformance testing).
/// Phase 4c will add prolly-tree-backed implementations.
#[derive(Debug, Default, Clone)]
pub struct NullAntiEntropy;

impl AntiEntropy for NullAntiEntropy {
    fn diff(&self, _local: &Store) -> Result<Vec<u8>, FerraError> {
        Ok(Vec::new())
    }

    /// Intentionally accepts any payload (including non-empty) as a no-op.
    /// In single-node mode, receiving a non-empty diff is harmless — there
    /// is no remote store to reconcile with. Phase 4c real implementations
    /// will validate payload structure.
    ///
    /// NOTE(Phase 4c): This silently discards non-empty payloads. A real
    /// anti-entropy implementation must deserialize and merge the diff
    /// contents. Revisit when implementing prolly-tree-backed sync
    /// (INV-FERR-022, Phase 4c federation transport).
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
