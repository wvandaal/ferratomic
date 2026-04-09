//! Transaction signing newtypes for Ferratomic federation.
//!
//! INV-FERR-051: Every signed transaction binds user datoms, `tx_id`,
//! predecessors, store fingerprint, and signer public key into a single
//! cryptographic attestation.
//!
//! ADR-FERR-021: Signatures and signer keys are stored as datoms
//! (`:tx/signature` and `:tx/signer`) using `Value::Bytes`.
//!
//! These are **pure newtypes** — no cryptographic operations here.
//! Actual signing and verification live in `ferratomic-core` (which
//! depends on `ed25519-dalek`). This crate provides only the
//! Curry-Howard type witnesses: possessing a `TxSignature` proves
//! that 64 bytes of signature data exist; possessing a `TxSigner`
//! proves that 32 bytes of public key data exist.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{FerraError, Value};

// ---------------------------------------------------------------------------
// TxSignature
// ---------------------------------------------------------------------------

/// Ed25519 transaction signature (64 bytes).
///
/// INV-FERR-051: Every signed transaction produces a `TxSignature` that
/// binds the user datoms, `tx_id`, predecessors, store fingerprint, and
/// signer public key into a single cryptographic attestation.
///
/// ADR-FERR-021: Stored as a `:tx/signature` datom (`Value::Bytes`).
///
/// The inner field is private to enforce construction only through
/// `from_bytes`, matching the `EntityId` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TxSignature([u8; 64]);

/// Serialize as raw bytes (serde doesn't support `[u8; 64]` by default).
impl Serialize for TxSignature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

/// Deserialize from raw bytes with 64-byte length validation.
impl<'de> Deserialize<'de> for TxSignature {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let arr: [u8; 64] = bytes.try_into().map_err(|v: Vec<u8>| {
            serde::de::Error::custom(format!(
                "INV-FERR-051: TxSignature requires 64 bytes, got {}",
                v.len()
            ))
        })?;
        Ok(Self(arr))
    }
}

impl TxSignature {
    /// Create a `TxSignature` from a raw 64-byte array.
    ///
    /// INV-FERR-051: The caller is responsible for ensuring the bytes
    /// represent a valid Ed25519 signature. Cryptographic verification
    /// is performed in `ferratomic-core`, not here.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying 64-byte signature array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

/// ADR-FERR-021: Convert a `TxSignature` into `Value::Bytes` for
/// storage as a `:tx/signature` datom.
impl From<TxSignature> for Value {
    fn from(sig: TxSignature) -> Self {
        Value::Bytes(Arc::from(sig.0.as_slice()))
    }
}

/// ADR-FERR-021: Reconstruct a `TxSignature` from a `Value::Bytes`
/// reference. Fails if the value is not `Bytes` or has wrong length.
impl TryFrom<&Value> for TxSignature {
    type Error = FerraError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Bytes(b) if b.len() == 64 => {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(b);
                Ok(Self(arr))
            }
            _ => Err(FerraError::InvariantViolation {
                invariant: "INV-FERR-051".to_string(),
                details: format!(
                    "TxSignature requires Value::Bytes with exactly 64 bytes, got {value:?}"
                ),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// TxSigner
// ---------------------------------------------------------------------------

/// Ed25519 verifying key (32 bytes).
///
/// INV-FERR-051: The public half of the signing keypair. Used to verify
/// `TxSignature`s and identify the transaction's author.
///
/// ADR-FERR-021: Stored as a `:tx/signer` datom (`Value::Bytes`).
///
/// The inner field is private to enforce construction only through
/// `from_bytes`, matching the `EntityId` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TxSigner([u8; 32]);

impl TxSigner {
    /// Create a `TxSigner` from a raw 32-byte public key array.
    ///
    /// INV-FERR-051: The caller is responsible for ensuring the bytes
    /// represent a valid Ed25519 verifying key. Key validation is
    /// performed in `ferratomic-core`, not here.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying 32-byte public key array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// ADR-FERR-021: Convert a `TxSigner` into `Value::Bytes` for
/// storage as a `:tx/signer` datom.
impl From<TxSigner> for Value {
    fn from(signer: TxSigner) -> Self {
        Value::Bytes(Arc::from(signer.0.as_slice()))
    }
}

/// ADR-FERR-021: Reconstruct a `TxSigner` from a `Value::Bytes`
/// reference. Fails if the value is not `Bytes` or has wrong length.
impl TryFrom<&Value> for TxSigner {
    type Error = FerraError;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Bytes(b) if b.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(b);
                Ok(Self(arr))
            }
            _ => Err(FerraError::InvariantViolation {
                invariant: "INV-FERR-051".to_string(),
                details: format!(
                    "TxSigner requires Value::Bytes with exactly 32 bytes, got {value:?}"
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_signature_round_trip() {
        let bytes = [0xABu8; 64];
        let sig = TxSignature::from_bytes(bytes);
        assert_eq!(
            *sig.as_bytes(),
            bytes,
            "INV-FERR-051: TxSignature round-trip must preserve bytes"
        );
    }

    #[test]
    fn test_tx_signer_round_trip() {
        let bytes = [0xCDu8; 32];
        let signer = TxSigner::from_bytes(bytes);
        assert_eq!(
            *signer.as_bytes(),
            bytes,
            "INV-FERR-051: TxSigner round-trip must preserve bytes"
        );
    }

    #[test]
    fn test_tx_signature_to_value() {
        let bytes = [0x42u8; 64];
        let sig = TxSignature::from_bytes(bytes);
        let val: Value = sig.into();
        match &val {
            Value::Bytes(b) => assert_eq!(
                &**b,
                &bytes[..],
                "INV-FERR-051: TxSignature→Value must produce Bytes with same content"
            ),
            other => panic!("INV-FERR-051: expected Value::Bytes, got {other:?}"),
        }
    }

    #[test]
    fn test_tx_signer_to_value() {
        let bytes = [0x13u8; 32];
        let signer = TxSigner::from_bytes(bytes);
        let val: Value = signer.into();
        match &val {
            Value::Bytes(b) => assert_eq!(
                &**b,
                &bytes[..],
                "INV-FERR-051: TxSigner→Value must produce Bytes with same content"
            ),
            other => panic!("INV-FERR-051: expected Value::Bytes, got {other:?}"),
        }
    }

    #[test]
    fn test_tx_signature_try_from_value() {
        let bytes = [0x7Fu8; 64];
        let sig = TxSignature::from_bytes(bytes);
        let val: Value = sig.into();
        let recovered = TxSignature::try_from(&val);
        assert_eq!(
            recovered.ok(),
            Some(sig),
            "INV-FERR-051: TxSignature TryFrom round-trip must recover original"
        );
    }

    #[test]
    fn test_tx_signer_try_from_value() {
        let bytes = [0x3Eu8; 32];
        let signer = TxSigner::from_bytes(bytes);
        let val: Value = signer.into();
        let recovered = TxSigner::try_from(&val);
        assert_eq!(
            recovered.ok(),
            Some(signer),
            "INV-FERR-051: TxSigner TryFrom round-trip must recover original"
        );
    }

    #[test]
    fn test_tx_signature_try_from_wrong_length() {
        // 32 bytes instead of 64.
        let val = Value::Bytes(Arc::from([0u8; 32].as_slice()));
        let result = TxSignature::try_from(&val);
        assert!(
            result.is_err(),
            "INV-FERR-051: TxSignature TryFrom must reject wrong-length Bytes"
        );
    }

    #[test]
    fn test_tx_signer_try_from_wrong_length() {
        // 64 bytes instead of 32.
        let val = Value::Bytes(Arc::from([0u8; 64].as_slice()));
        let result = TxSigner::try_from(&val);
        assert!(
            result.is_err(),
            "INV-FERR-051: TxSigner TryFrom must reject wrong-length Bytes"
        );
    }

    #[test]
    fn test_tx_signature_ord_is_lexicographic() {
        let mut a_bytes = [0u8; 64];
        let mut b_bytes = [0u8; 64];
        a_bytes[0] = 0x01;
        b_bytes[0] = 0x02;
        let a = TxSignature::from_bytes(a_bytes);
        let b = TxSignature::from_bytes(b_bytes);
        assert!(
            a < b,
            "INV-FERR-051: TxSignature ordering must be lexicographic (first byte 0x01 < 0x02)"
        );

        // Reverse: higher first byte is greater.
        assert!(
            b > a,
            "INV-FERR-051: TxSignature ordering must be lexicographic (reverse)"
        );

        // Equal first bytes, differ on second.
        let mut c_bytes = [0u8; 64];
        let mut d_bytes = [0u8; 64];
        c_bytes[0] = 0x01;
        c_bytes[1] = 0x0A;
        d_bytes[0] = 0x01;
        d_bytes[1] = 0x0B;
        let c = TxSignature::from_bytes(c_bytes);
        let d = TxSignature::from_bytes(d_bytes);
        assert!(
            c < d,
            "INV-FERR-051: TxSignature ordering must compare second byte when first bytes equal"
        );
    }
}
