//! Error exhaustiveness Kani harnesses.
//!
//! Covers INV-FERR-019: every FerraError variant produces a non-empty
//! Display string. No error variant is forgotten in the Display impl.

use ferratom::FerraError;

/// INV-FERR-019: every FerraError variant produces a non-empty Display string.
///
/// Constructs all 11 FerraError variants with minimal content and verifies
/// that `fmt::Display` produces a non-empty string for each. This ensures
/// no variant is accidentally forgotten in the `Display` implementation.
#[kani::proof]
#[kani::unwind(4)]
fn error_display_non_empty() {
    let variants: [FerraError; 11] = [
        FerraError::WalWrite("w".to_string()),
        FerraError::WalRead("r".to_string()),
        FerraError::CheckpointCorrupted {
            expected: "a".to_string(),
            actual: "b".to_string(),
        },
        FerraError::CheckpointWrite("c".to_string()),
        FerraError::Io("i".to_string()),
        FerraError::UnknownAttribute {
            attribute: "x".to_string(),
        },
        FerraError::SchemaViolation {
            attribute: "a".to_string(),
            expected: "e".to_string(),
            got: "g".to_string(),
        },
        FerraError::EmptyTransaction,
        FerraError::SchemaIncompatible {
            attribute: "a".to_string(),
            left: "l".to_string(),
            right: "r".to_string(),
        },
        FerraError::Backpressure,
        FerraError::PeerUnreachable {
            addr: "h".to_string(),
            reason: "r".to_string(),
        },
    ];

    for err in &variants {
        let msg = format!("{err}");
        assert!(
            !msg.is_empty(),
            "INV-FERR-019: FerraError::Display must produce non-empty string"
        );
    }

    // Also verify InvariantViolation (the 12th variant, separated for clarity)
    let inv_err = FerraError::InvariantViolation {
        invariant: "INV-FERR-999".to_string(),
        details: "test".to_string(),
    };
    let inv_msg = format!("{inv_err}");
    assert!(
        !inv_msg.is_empty(),
        "INV-FERR-019: InvariantViolation Display must produce non-empty string"
    );
}

/// INV-FERR-019: FerraError implements std::error::Error.
///
/// Verifies that the Error trait is satisfied for a representative variant,
/// which guarantees it is implemented for the enum (since it is not
/// per-variant).
#[kani::proof]
#[kani::unwind(4)]
fn error_trait_implemented() {
    let err: FerraError = FerraError::Backpressure;
    // std::error::Error requires Display + Debug. If this compiles,
    // the trait bound is satisfied. We call to_string() to exercise Display.
    let display = err.to_string();
    assert!(!display.is_empty());

    // Debug is also required by std::error::Error.
    let debug = format!("{err:?}");
    assert!(!debug.is_empty());
}
