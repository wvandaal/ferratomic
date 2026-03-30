//! Error types for Ferratomic.
//! INV-FERR-019: Every API function returns typed Result. No panics.
//! NEG-FERR-001: No `unwrap()`, no `expect()` in production code.

use std::fmt;

/// Exhaustive error type for all Ferratomic operations.
/// Callers pattern-match on the variant category, not message strings.
#[derive(Debug, Clone)]
pub enum FerraError {
    // Storage errors (retryable)
    /// WAL write failed. INV-FERR-008.
    WalWrite(String),
    /// WAL read failed during recovery.
    WalRead(String),
    /// Checkpoint corrupted (checksum mismatch). INV-FERR-013.
    CheckpointCorrupted {
        /// Expected checksum.
        expected: String,
        /// Actual checksum found.
        actual: String,
    },
    /// Checkpoint write failed.
    CheckpointWrite(String),
    /// I/O error.
    Io(String),

    // Validation errors (caller bug, not retryable)
    /// Unknown attribute in transaction. INV-FERR-009.
    UnknownAttribute {
        /// The attribute name that was not found in the schema.
        attribute: String,
    },
    /// Value type doesn't match schema. INV-FERR-009.
    SchemaViolation {
        /// The attribute where the violation occurred.
        attribute: String,
        /// The expected value type per schema.
        expected: String,
        /// The actual value type that was supplied.
        got: String,
    },
    /// Empty transaction submitted.
    EmptyTransaction,

    // Concurrency errors (transient, retryable)
    /// Write lock contention. INV-FERR-021.
    Backpressure,

    // Federation errors
    /// Remote store unreachable.
    PeerUnreachable {
        /// The network address of the unreachable peer.
        addr: String,
        /// Why the peer could not be reached.
        reason: String,
    },

    // Invariant violations (OUR bug — should never happen)
    /// An internal invariant was violated. File a bug.
    InvariantViolation {
        /// Which invariant was violated (e.g. "INV-FERR-005").
        invariant: String,
        /// Human-readable description of what went wrong.
        details: String,
    },
}

impl fmt::Display for FerraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WalWrite(msg) => write!(f, "WAL write failed: {msg}"),
            Self::WalRead(msg) => write!(f, "WAL read failed: {msg}"),
            Self::CheckpointCorrupted { expected, actual } => {
                write!(f, "Checkpoint corrupted: expected {expected}, got {actual}")
            }
            Self::CheckpointWrite(msg) => write!(f, "Checkpoint write failed: {msg}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::UnknownAttribute { attribute } => {
                write!(f, "Unknown attribute: {attribute}")
            }
            Self::SchemaViolation {
                attribute,
                expected,
                got,
            } => {
                write!(
                    f,
                    "Schema violation on {attribute}: expected {expected}, got {got}"
                )
            }
            Self::EmptyTransaction => write!(f, "Empty transaction"),
            Self::Backpressure => write!(f, "Write queue full (backpressure)"),
            Self::PeerUnreachable { addr, reason } => {
                write!(f, "Peer unreachable at {addr}: {reason}")
            }
            Self::InvariantViolation { invariant, details } => {
                write!(f, "INVARIANT VIOLATION {invariant}: {details}")
            }
        }
    }
}

impl std::error::Error for FerraError {}

impl From<std::io::Error> for FerraError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
