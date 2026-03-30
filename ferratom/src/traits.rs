//! Algebraic traits for Ferratomic.
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
pub trait Semilattice {
    /// Merge two values. The result is the least upper bound.
    #[must_use]
    fn merge(&self, other: &Self) -> Self;
}

/// Content-addressed identity: identity determined by content hash.
/// INV-FERR-012: Two values with identical content have identical identity.
pub trait ContentAddressed {
    /// Compute the content-addressed hash of this value.
    fn content_hash(&self) -> [u8; 32];
}
