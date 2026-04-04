//! Stateright protocol models for ferratomic verification.
//!
//! Imported by `ferratomic-verify/src/lib.rs` as `stateright_models`.
//! Contains bounded model-checking for CRDT merge convergence (INV-FERR-010),
//! monotonic growth (INV-FERR-004), append-only (INV-FERR-018),
//! crash-recovery correctness (INV-FERR-014), index bijection (INV-FERR-005),
//! WAL fsync ordering (INV-FERR-008), checkpoint equivalence (INV-FERR-013),
//! write linearizability (INV-FERR-007), snapshot isolation (INV-FERR-006),
//! observer monotonicity (INV-FERR-011), schema validation (INV-FERR-009),
//! HLC monotonicity (INV-FERR-015), transaction atomicity (INV-FERR-020),
//! and backpressure safety (INV-FERR-021).

/// CRDT merge protocol model for bounded model-checking (INV-FERR-004, INV-FERR-010, INV-FERR-018).
pub mod crdt_model;

/// Crash-recovery state machine model for bounded model-checking (INV-FERR-005, INV-FERR-008, INV-FERR-013, INV-FERR-014, INV-FERR-018).
pub mod crash_recovery_model;

/// Write linearizability model for bounded model-checking (INV-FERR-007).
pub mod write_linearizability_model;

/// Snapshot isolation model for bounded model-checking (INV-FERR-006, INV-FERR-011).
pub mod snapshot_isolation_model;

/// Schema validation model for bounded model-checking (INV-FERR-009).
pub mod schema_validation_model;

/// Hybrid Logical Clock model for bounded model-checking (INV-FERR-015).
pub mod hlc_model;

/// Transaction atomicity model for bounded model-checking (INV-FERR-020).
pub mod transaction_atomicity_model;

/// Backpressure safety model for bounded model-checking (INV-FERR-021).
pub mod backpressure_model;
