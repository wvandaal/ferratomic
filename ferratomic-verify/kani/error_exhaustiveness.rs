//! Error exhaustiveness and safety Kani harnesses.
//!
//! Covers INV-FERR-019 (every FerraError variant produces a non-empty
//! Display string) and INV-FERR-023 (no unsafe code — FerraError is
//! Send + Sync and all variants satisfy std::error::Error).

use ferratom::FerraError;

/// INV-FERR-019: every FerraError variant produces a non-empty Display string.
///
/// Constructs all 11 FerraError variants with minimal content and verifies
/// that `fmt::Display` produces a non-empty string for each. This ensures
/// no variant is accidentally forgotten in the `Display` implementation.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn error_display_non_empty() {
    let variants: [FerraError; 11] = [
        FerraError::WalWrite("w".to_string()),
        FerraError::WalRead("r".to_string()),
        FerraError::CheckpointCorrupted {
            expected: "a".to_string(),
            actual: "b".to_string(),
        },
        FerraError::CheckpointWrite("c".to_string()),
        FerraError::Io {
            kind: "Other".to_string(),
            message: "i".to_string(),
        },
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
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
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

/// INV-FERR-023: FerraError is Send + Sync.
///
/// No unsafe code is permitted (INV-FERR-023). All error variants must
/// be thread-safe so they can cross async boundaries. This is a
/// compile-time property: if this harness compiles, the bounds hold.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn error_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<FerraError>();

    fn assert_error<T: std::error::Error>() {}
    assert_error::<FerraError>();
}

/// INV-FERR-023: FerraError variant count is exhaustive.
///
/// Verifies that all 12 known variants compile and satisfy the
/// std::error::Error bound. If a variant is added but not covered
/// here, the compile-time exhaustiveness of the array literal will
/// need updating (a manual but intentional gate).
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn error_all_variants_are_error() {
    let variants: [FerraError; 12] = [
        FerraError::WalWrite("w".to_string()),
        FerraError::WalRead("r".to_string()),
        FerraError::CheckpointCorrupted {
            expected: "a".to_string(),
            actual: "b".to_string(),
        },
        FerraError::CheckpointWrite("c".to_string()),
        FerraError::Io {
            kind: "Other".to_string(),
            message: "i".to_string(),
        },
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
        FerraError::InvariantViolation {
            invariant: "INV-FERR-023".to_string(),
            details: "test".to_string(),
        },
    ];

    // Every variant must implement std::error::Error (Display + Debug).
    for err in &variants {
        let display = format!("{err}");
        let debug = format!("{err:?}");
        assert!(
            !display.is_empty(),
            "INV-FERR-023: Display must be non-empty"
        );
        assert!(!debug.is_empty(), "INV-FERR-023: Debug must be non-empty");
    }
}
