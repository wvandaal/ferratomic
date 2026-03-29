//! Algebraic traits that encode the formal properties of Ferratomic types.
//!
//! These traits ARE propositions (Curry-Howard). Implementing them IS
//! proving the property holds for the implementing type.
//!
//! - `Semilattice`: join-semilattice with idempotent, commutative, associative merge
//! - `ContentAddressed`: identity determined by content hash
//! - `CausalOrder`: partial order via happens-before relation

// TODO(Phase 3): Define Semilattice, ContentAddressed, CausalOrder traits
// See spec/23-ferratomic.md §23.0.4 for the algebraic foundation.
