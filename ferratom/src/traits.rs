//! Algebraic traits for Ferratomic.
//!
//! INV-FERR-001 (commutativity), INV-FERR-002 (associativity),
//! INV-FERR-003 (idempotency): [`Semilattice`] trait encodes the
//! G-Set CRDT merge laws.
//! INV-FERR-012: [`ContentAddressed`] trait encodes content-addressed identity.
//!
//! These traits encode the algebraic structure at the type level
//! via the Curry-Howard correspondence.

/// A join-semilattice with idempotent, commutative, associative merge.
/// INV-FERR-001 (commutativity), INV-FERR-002 (associativity), INV-FERR-003 (idempotency).
///
/// Laws (verified by Lean proofs in Store.lean):
/// - `merge(a, b) = merge(b, a)` (commutativity)
/// - `merge(merge(a, b), c) = merge(a, merge(b, c))` (associativity)
/// - `merge(a, a) = a` (idempotency)
pub trait Semilattice: Sized {
    /// Merge two values. The result is the least upper bound.
    ///
    /// # Errors
    ///
    /// Returns an error if the merge precondition is not met (e.g.,
    /// schema incompatibility for stores — INV-FERR-043).
    fn merge(&self, other: &Self) -> Result<Self, crate::FerraError>;
}

/// Content-addressed identity: identity determined by content hash (INV-FERR-012).
///
/// Two values with identical content produce identical identity hashes.
/// This is the foundation of deduplication and content-addressed storage.
pub trait ContentAddressed {
    /// Compute the BLAKE3 content-addressed hash of this value (INV-FERR-012).
    fn content_hash(&self) -> [u8; 32];
}
