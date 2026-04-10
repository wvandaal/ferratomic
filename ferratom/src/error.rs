//! Error types for Ferratomic.
//! INV-FERR-019: Every API function returns typed Result. No panics.
//! NEG-FERR-001: No `unwrap()`, no `expect()` in production code.

use std::fmt;

/// Exhaustive error type for all Ferratomic operations.
/// Callers pattern-match on the variant category, not message strings.
///
/// # Error categories
///
/// | Category | Fault | Retryable | Examples |
/// |----------|-------|-----------|----------|
/// | Storage | Infrastructure | Yes | `WalWrite`, `WalRead`, `CheckpointWrite`, `Io` |
/// | Corruption | Infrastructure | No (recover from checkpoint) | `CheckpointCorrupted` |
/// | Validation | Caller | No (fix input) | `UnknownAttribute`, `SchemaViolation`, `EmptyTransaction` |
/// | Merge | Caller | No (reconcile schemas) | `SchemaIncompatible` |
/// | Concurrency | Transient | Yes (backoff) | `Backpressure` |
/// | Federation | Infrastructure | Yes (retry/reconnect) | `PeerUnreachable`, `SignatureInvalid`, `TransportError` |
/// | Internal | Our bug | No (file a bug) | `InvariantViolation` |
///
/// `PartialEq` compares full variant structure including string fields.
/// For variant-only matching in tests, prefer `matches!(err, FerraError::WalWrite(_))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FerraError {
    // ── Storage errors (retryable) ──────────────────────────────────
    /// WAL write failed.
    ///
    /// **Cause**: Disk I/O error during WAL append (full disk, permission
    /// denied, hardware fault).
    /// **Fault**: Infrastructure.
    /// **Recovery**: Retry with backoff. If persistent, check disk health
    /// and free space. The transaction was NOT committed.
    /// INV-FERR-008: WAL fsync ordering — a failed WAL write means the
    /// transaction never became durable.
    WalWrite(String),

    /// WAL read failed during recovery.
    ///
    /// **Cause**: WAL file is missing, truncated, or unreadable during
    /// crash-recovery replay.
    /// **Fault**: Infrastructure (disk corruption, incomplete prior write).
    /// **Recovery**: Fall back to the latest valid checkpoint. If the WAL
    /// file is irrecoverably damaged, committed transactions after the
    /// last checkpoint may be lost.
    WalRead(String),

    /// Checkpoint corrupted (checksum mismatch).
    ///
    /// **Cause**: Stored checkpoint data does not match its content hash.
    /// Disk bit-rot, incomplete write, or storage-layer corruption.
    /// **Fault**: Infrastructure.
    /// **Recovery**: Discard the corrupted checkpoint and rebuild from the
    /// previous valid checkpoint plus WAL replay. If no valid checkpoint
    /// exists, the store must be rebuilt from peers or backup.
    /// INV-FERR-013: Checkpoint equivalence — `deserialize(serialize(S)) = S`.
    /// A checksum mismatch proves this round-trip property was violated.
    CheckpointCorrupted {
        /// Expected checksum.
        expected: String,
        /// Actual checksum found.
        actual: String,
    },

    /// Checkpoint write failed.
    ///
    /// **Cause**: Disk I/O error while writing a checkpoint file (full disk,
    /// permission denied, hardware fault).
    /// **Fault**: Infrastructure.
    /// **Recovery**: Retry with backoff. The store remains consistent via
    /// the WAL — checkpoint failure delays optimization but does not lose
    /// data.
    CheckpointWrite(String),

    /// I/O error.
    ///
    /// **Cause**: Generic filesystem or device I/O failure not specific to
    /// WAL or checkpoint operations.
    /// **Fault**: Infrastructure.
    /// **Recovery**: Retry with backoff. Match on `kind` to distinguish
    /// `"NotFound"` from `"PermissionDenied"` from `"Other"` without
    /// parsing the message string.
    ///
    /// HI-017: Preserves `io::ErrorKind` as its `Debug` string so callers
    /// can pattern-match on error category programmatically. The `message`
    /// field holds the display message for diagnostics.
    Io {
        /// The `io::ErrorKind` debug string (e.g. `"NotFound"`,
        /// `"PermissionDenied"`, `"Other"`).
        kind: String,
        /// Human-readable error message for diagnostics.
        message: String,
    },

    // ── Validation errors (caller bug, not retryable) ───────────────
    /// Unknown attribute in transaction.
    ///
    /// **Cause**: A datom in the transaction references an attribute name
    /// that does not exist in the store's schema.
    /// **Fault**: Caller bug. The caller constructed a transaction with an
    /// unregistered attribute.
    /// **Recovery**: Fix the transaction to use only attributes defined in
    /// the schema, or register the new attribute first.
    /// INV-FERR-009: Schema validation — every datom must reference a
    /// schema-defined attribute with the correct value type.
    UnknownAttribute {
        /// The attribute name that was not found in the schema.
        attribute: String,
    },

    /// Value type does not match schema.
    ///
    /// **Cause**: A datom supplies a value whose type differs from the
    /// attribute's declared type in the schema.
    /// **Fault**: Caller bug. The caller passed a value of the wrong type.
    /// **Recovery**: Fix the transaction so the value matches the expected
    /// type for the given attribute.
    /// INV-FERR-009: Schema validation — value types are checked at the
    /// transact boundary, before any datoms are applied.
    SchemaViolation {
        /// The attribute where the violation occurred.
        attribute: String,
        /// The expected value type per schema.
        expected: String,
        /// The actual value type that was supplied.
        got: String,
    },

    /// Attribute name exceeds the canonical format length limit.
    ///
    /// **Cause**: An attribute name was provided with more than 65,535 bytes
    /// (`u16::MAX`). The INV-FERR-086 canonical byte format uses u16-le for
    /// attribute length and cannot represent longer names.
    /// **Fault**: Caller bug. Attribute names are short namespace/name patterns
    /// (e.g., "db/ident", "provenance/observed").
    /// **Recovery**: Use shorter attribute names. Production attributes are
    /// typically 10-40 bytes.
    /// INV-FERR-086: Canonical Datom Format Determinism — the u16 length
    /// prefix is part of the frozen canonical format.
    AttributeTooLong {
        /// The byte length of the oversized attribute name.
        len: usize,
    },

    /// Empty transaction submitted.
    ///
    /// **Cause**: The caller submitted a transaction containing zero datoms.
    /// **Fault**: Caller bug. Transactions must contain at least one datom.
    /// **Recovery**: Do not submit empty transactions. Check transaction
    /// construction logic.
    EmptyTransaction,

    // ── Merge errors (caller must reconcile schemas before merging) ──
    /// Schemas are incompatible — merge is undefined.
    ///
    /// **Cause**: Two stores define the same attribute name with different
    /// types or cardinalities, making set-union merge semantically invalid.
    /// **Fault**: Caller. The caller must reconcile schemas before merging.
    /// **Recovery**: Evolve one or both store schemas to be compatible
    /// before retrying the merge. See the schema evolution protocol.
    /// INV-FERR-043: Schema compatibility check — merge requires that
    /// shared attribute names have identical definitions.
    SchemaIncompatible {
        /// The attribute with conflicting definitions.
        attribute: String,
        /// Definition from store A.
        left: String,
        /// Definition from store B.
        right: String,
    },

    // ── Concurrency errors (transient, retryable) ───────────────────
    /// Write queue full (backpressure).
    ///
    /// **Cause**: The bounded write queue has reached capacity. Too many
    /// concurrent writers or the single writer thread is slow.
    /// **Fault**: Transient. Normal under burst load.
    /// **Recovery**: Retry with exponential backoff. If persistent, the
    /// caller may need to throttle write rate or increase queue capacity.
    /// INV-FERR-021: Backpressure safety — the write queue depth is
    /// bounded to prevent unbounded memory growth.
    Backpressure,

    // ── Federation errors ───────────────────────────────────────────
    /// Remote store unreachable.
    ///
    /// **Cause**: Network connection to a peer store failed (DNS resolution,
    /// TCP timeout, TLS handshake failure, peer process down).
    /// **Fault**: Infrastructure (network or remote host).
    /// **Recovery**: Retry with exponential backoff. If persistent, verify
    /// the peer address, check network connectivity, and confirm the peer
    /// process is running.
    PeerUnreachable {
        /// The network address of the unreachable peer.
        addr: String,
        /// Why the peer could not be reached.
        reason: String,
    },

    /// Ed25519 signature verification failed.
    ///
    /// **Cause**: The signature does not match the signing message computed
    /// from (`user_datoms`, `tx_id`, `predecessors`, `store_fingerprint`, `signer_pk`).
    /// **Fault**: Either the transaction was tampered with, the signer key
    /// is wrong, or the signing message was computed incorrectly.
    /// **Recovery**: Reject the transaction. Do not apply it to the store.
    /// INV-FERR-051: Signed transaction verification.
    SignatureInvalid {
        /// Which transaction failed verification.
        tx_description: String,
    },

    /// Federation transport operation failed.
    ///
    /// **Cause**: The Transport trait implementation encountered an error
    /// during `fetch_datoms` or `fetch_signed_transactions`.
    /// **Fault**: Infrastructure (network, I/O, serialization).
    /// **Recovery**: Retry with backoff. Check transport configuration.
    /// INV-FERR-038: Federation substrate transparency.
    TransportError(String),

    // ── Codec / chunk errors (INV-FERR-045a, INV-FERR-045c) ─────────
    /// Leaf chunk payload is truncated (fewer bytes than required).
    ///
    /// **Cause**: On-disk or wire bytes are shorter than the format requires.
    /// **Fault**: Corruption or truncated transfer.
    /// **Recovery**: Discard the chunk and re-fetch from a peer.
    TruncatedChunk,

    /// Leaf chunk payload has trailing bytes after the last entry.
    ///
    /// **Cause**: Extra bytes appended after the encoded entries.
    /// **Fault**: Corruption, buggy encoder, or format version mismatch.
    /// **Recovery**: Reject the chunk; do not process partial data.
    TrailingBytes,

    /// Chunk entries are not in canonical (strict ascending key) order.
    ///
    /// **Cause**: Entries in the chunk violate the sorted-deduplicated predicate.
    /// **Fault**: Buggy encoder or adversarial peer.
    /// **Recovery**: Reject the chunk. Defense in depth per INV-FERR-045a.
    NonCanonicalChunk,

    /// Chunk is empty (zero entries). `boundary_key` cannot produce a result.
    ///
    /// **Cause**: Empty chunks are syntactically valid but never appear in
    /// well-formed prolly trees.
    /// **Fault**: Structural error in tree construction.
    EmptyChunk,

    /// Unknown codec discriminator tag in a leaf chunk.
    ///
    /// **Cause**: The leading byte of the leaf chunk payload does not match
    /// any registered codec in §23.9.8.
    /// **Fault**: Corruption, future format, or experimental codec.
    UnknownCodecTag(u8),

    /// Feature not yet implemented.
    ///
    /// **Cause**: A code path that is spec-defined but not yet implemented.
    NotImplemented(&'static str),

    // ── Invariant violations (OUR bug — should never happen) ────────
    /// An internal invariant was violated. This is a bug in Ferratomic.
    ///
    /// **Cause**: A condition that should be structurally impossible was
    /// detected at runtime. The named invariant was violated.
    /// **Fault**: Internal bug. This should never happen in correct code.
    /// **Recovery**: File a bug report including the invariant ID and
    /// details string. Do not retry — the store may be in an inconsistent
    /// state. Restart the process and recover from checkpoint + WAL.
    InvariantViolation {
        /// Which invariant was violated (e.g. "INV-FERR-005").
        invariant: String,
        /// Human-readable description of what went wrong.
        details: String,
    },
}

/// INV-FERR-019: Human-readable error messages for every variant.
impl fmt::Display for FerraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WalWrite(msg) => write!(f, "WAL write failed: {msg}"),
            Self::WalRead(msg) => write!(f, "WAL read failed: {msg}"),
            Self::CheckpointCorrupted { expected, actual } => {
                write!(f, "Checkpoint corrupted: expected {expected}, got {actual}")
            }
            Self::CheckpointWrite(msg) => write!(f, "Checkpoint write failed: {msg}"),
            Self::Io { kind, message } => write!(f, "I/O error ({kind}): {message}"),
            Self::UnknownAttribute { attribute } => {
                write!(f, "Unknown attribute: {attribute}")
            }
            Self::SchemaViolation {
                attribute,
                expected,
                got,
            } => write!(f, "Schema violation: {attribute}: {expected} vs {got}"),
            Self::SchemaIncompatible {
                attribute,
                left,
                right,
            } => write!(f, "Schema incompatible on {attribute}: {left} vs {right}"),
            Self::AttributeTooLong { len } => {
                write!(f, "INV-FERR-086: attribute exceeds u16 max: {len} bytes")
            }
            Self::EmptyTransaction => write!(f, "Empty transaction"),
            Self::Backpressure => write!(f, "Write queue full (backpressure)"),
            Self::PeerUnreachable { addr, reason } => {
                write!(f, "Peer unreachable at {addr}: {reason}")
            }
            Self::SignatureInvalid { tx_description } => {
                write!(f, "Signature invalid: {tx_description}")
            }
            Self::TransportError(msg) => write!(f, "Transport error: {msg}"),
            Self::TruncatedChunk => write!(f, "Chunk payload truncated"),
            Self::TrailingBytes => write!(f, "Chunk payload has trailing bytes"),
            Self::NonCanonicalChunk => write!(f, "Chunk entries not in canonical order"),
            Self::EmptyChunk => write!(f, "Empty chunk (no entries)"),
            Self::UnknownCodecTag(tag) => write!(f, "Unknown codec tag: 0x{tag:02x}"),
            Self::NotImplemented(msg) => write!(f, "Not implemented: {msg}"),
            Self::InvariantViolation { invariant, details } => {
                write!(f, "INVARIANT VIOLATION {invariant}: {details}")
            }
        }
    }
}

/// INV-FERR-019: `FerraError` implements `std::error::Error` for
/// interoperability with the standard error handling ecosystem.
impl std::error::Error for FerraError {}

/// Convert `ClockError` into `FerraError::InvariantViolation` for `?` propagation.
///
/// INV-FERR-021: `InvariantViolation` is the correct category because the
/// bounded retry loop in `HybridClock::tick()` already waited ~1-10ms
/// (`MAX_BUSY_WAIT_RETRIES` yield iterations). If the wall clock still
/// has not advanced after that delay, the clock source is fundamentally
/// broken (frozen mock, stuck hardware, sandboxed monotonic clock), not
/// transiently unavailable. Retrying will not help -- the caller must
/// treat this as an unrecoverable internal fault.
impl From<ferratom_clock::ClockError> for FerraError {
    fn from(e: ferratom_clock::ClockError) -> Self {
        Self::InvariantViolation {
            invariant: "INV-FERR-021".to_string(),
            details: e.to_string(),
        }
    }
}

/// Convert `std::io::Error` into `FerraError::Io` for `?` propagation.
///
/// HI-017: Preserves `io::ErrorKind` as its `Debug` string so callers
/// can match on error category without parsing the message.
impl From<std::io::Error> for FerraError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            kind: format!("{:?}", e.kind()),
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert that a single `FerraError` variant's `Display` output is
    /// non-empty and contains the expected keyword.
    fn assert_display_contains(error: &FerraError, keyword: &str) {
        let output = format!("{error}");
        assert!(
            !output.is_empty(),
            "Display output for {error:?} must not be empty",
        );
        assert!(
            output.contains(keyword),
            "Display output for {error:?} must contain \"{keyword}\", got: \"{output}\"",
        );
    }

    /// Simple error variants (single-field or unit) for Display testing.
    fn simple_display_cases() -> Vec<(FerraError, &'static str)> {
        vec![
            (FerraError::WalWrite("disk full".into()), "WAL"),
            (FerraError::WalRead("truncated entry".into()), "WAL"),
            (FerraError::CheckpointWrite("denied".into()), "Checkpoint"),
            (
                FerraError::Io {
                    kind: "Other".into(),
                    message: "broken pipe".into(),
                },
                "I/O",
            ),
            (FerraError::AttributeTooLong { len: 70000 }, "INV-FERR-086"),
            (FerraError::EmptyTransaction, "Empty transaction"),
            (FerraError::Backpressure, "backpressure"),
            (
                FerraError::SignatureInvalid {
                    tx_description: "tx-42".into(),
                },
                "Signature invalid",
            ),
            (
                FerraError::TransportError("timeout".into()),
                "Transport error",
            ),
            (FerraError::TruncatedChunk, "truncated"),
            (FerraError::TrailingBytes, "trailing"),
            (FerraError::NonCanonicalChunk, "canonical"),
            (FerraError::EmptyChunk, "Empty chunk"),
            (FerraError::UnknownCodecTag(0x42), "0x42"),
            (FerraError::NotImplemented("test"), "Not implemented"),
        ]
    }

    /// Struct-variant error cases for Display testing.
    fn struct_display_cases() -> Vec<(FerraError, &'static str)> {
        vec![
            (
                FerraError::CheckpointCorrupted {
                    expected: "a".into(),
                    actual: "b".into(),
                },
                "Checkpoint",
            ),
            (
                FerraError::UnknownAttribute {
                    attribute: "x".into(),
                },
                "Unknown attribute",
            ),
            (
                FerraError::SchemaViolation {
                    attribute: "n".into(),
                    expected: "S".into(),
                    got: "I".into(),
                },
                "Schema violation",
            ),
            (
                FerraError::SchemaIncompatible {
                    attribute: "e".into(),
                    left: "S".into(),
                    right: "R".into(),
                },
                "Schema incompatible",
            ),
            (
                FerraError::PeerUnreachable {
                    addr: "1.2.3.4:80".into(),
                    reason: "refused".into(),
                },
                "Peer unreachable",
            ),
            (
                FerraError::InvariantViolation {
                    invariant: "INV-005".into(),
                    details: "desync".into(),
                },
                "INVARIANT VIOLATION",
            ),
        ]
    }

    /// Construct every `FerraError` variant, format it with `Display`, and
    /// verify that the output is non-empty and contains a keyword that
    /// identifies the error category. This catches regressions where a new
    /// variant is added but its `Display` arm is missing or empty.
    #[test]
    fn display_output_is_nonempty_and_contains_keyword() {
        for (error, keyword) in &simple_display_cases() {
            assert_display_contains(error, keyword);
        }
        for (error, keyword) in &struct_display_cases() {
            assert_display_contains(error, keyword);
        }
    }

    /// Build one instance of every `FerraError` variant for exhaustive testing.
    fn all_error_variants() -> Vec<FerraError> {
        vec![
            FerraError::WalWrite("test".into()),
            FerraError::WalRead("test".into()),
            FerraError::CheckpointCorrupted {
                expected: "a".into(),
                actual: "b".into(),
            },
            FerraError::CheckpointWrite("test".into()),
            FerraError::Io {
                kind: "Other".into(),
                message: "test".into(),
            },
            FerraError::UnknownAttribute {
                attribute: "x".into(),
            },
            FerraError::SchemaViolation {
                attribute: "x".into(),
                expected: "A".into(),
                got: "B".into(),
            },
            FerraError::AttributeTooLong { len: 70000 },
            FerraError::EmptyTransaction,
            FerraError::SchemaIncompatible {
                attribute: "x".into(),
                left: "A".into(),
                right: "B".into(),
            },
            FerraError::Backpressure,
            FerraError::PeerUnreachable {
                addr: "addr".into(),
                reason: "r".into(),
            },
            FerraError::SignatureInvalid {
                tx_description: "tx-1".into(),
            },
            FerraError::TransportError("timeout".into()),
            FerraError::InvariantViolation {
                invariant: "INV".into(),
                details: "d".into(),
            },
            FerraError::TruncatedChunk,
            FerraError::TrailingBytes,
            FerraError::NonCanonicalChunk,
            FerraError::EmptyChunk,
            FerraError::UnknownCodecTag(0x42),
            FerraError::NotImplemented("test"),
        ]
    }

    /// Every variant must implement `Debug` without panicking.
    #[test]
    fn debug_output_is_nonempty() {
        for v in &all_error_variants() {
            let dbg = format!("{v:?}");
            assert!(!dbg.is_empty(), "Debug output must not be empty");
        }
    }

    /// `FerraError` implements `std::error::Error`.
    #[test]
    fn implements_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(FerraError::Io {
            kind: "Other".into(),
            message: "test".into(),
        });
        // Display through the trait object — proves the impl is wired up.
        let msg = format!("{err}");
        assert!(msg.contains("I/O"), "std::error::Error Display should work");
    }

    /// `From<ClockError>` maps to `InvariantViolation` with INV-FERR-021.
    #[test]
    fn test_from_clock_error_maps_to_invariant_violation() {
        let err = FerraError::from(ferratom_clock::ClockError::LogicalOverflow);
        match err {
            FerraError::InvariantViolation { invariant, .. } => {
                assert_eq!(invariant, "INV-FERR-021");
            }
            other => panic!("Expected InvariantViolation, got {other:?}"),
        }
    }

    /// `From<std::io::Error>` produces `FerraError::Io` with kind preserved.
    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let ferra_err = FerraError::from(io_err);
        let msg = format!("{ferra_err}");
        assert!(msg.contains("I/O"), "converted error should be Io variant");
        assert!(
            msg.contains("file missing"),
            "inner message should be preserved"
        );
        // HI-017: ErrorKind is preserved as a struct field.
        if let FerraError::Io { kind, message } = &ferra_err {
            assert_eq!(kind, "NotFound", "ErrorKind must be preserved");
            assert!(
                message.contains("file missing"),
                "message must contain original text"
            );
        } else {
            panic!("expected FerraError::Io variant");
        }
    }
}
