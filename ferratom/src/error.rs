//! Error taxonomy for Ferratomic.
//!
//! INV-FERR-019: Error exhaustiveness. Every API function returns
//! typed errors, never panics. No unwrap(), no expect() in production code.
//!
//! NEG-FERR-001: No panics in production code.

// TODO(Phase 3): Implement FerraError enum
// Categories: Storage, Validation, Concurrency, Federation, InvariantViolation
// See spec/23-ferratomic.md §23.2 INV-FERR-019 and §23.5 NEG-FERR-001.
