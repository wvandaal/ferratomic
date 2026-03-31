//! Stateright protocol models for ferratomic verification.
//!
//! Imported by `ferratomic-verify/src/lib.rs` as `stateright_models`.
//! Contains bounded model-checking for CRDT merge convergence (INV-FERR-010),
//! crash-recovery correctness (INV-FERR-014), write linearizability (INV-FERR-007),
//! snapshot isolation (INV-FERR-006), transaction atomicity (INV-FERR-020),
//! and backpressure safety (INV-FERR-021).

/// CRDT merge protocol model for bounded model-checking (INV-FERR-010).
pub mod crdt_model;

/// Crash-recovery state machine model for bounded model-checking (INV-FERR-014).
pub mod crash_recovery_model;

/// Write linearizability model for bounded model-checking (INV-FERR-007).
pub mod write_linearizability_model;

/// Snapshot isolation model for bounded model-checking (INV-FERR-006).
pub mod snapshot_isolation_model;

/// Transaction atomicity model for bounded model-checking (INV-FERR-020).
pub mod transaction_atomicity_model;

/// Backpressure safety model for bounded model-checking (INV-FERR-021).
pub mod backpressure_model;
