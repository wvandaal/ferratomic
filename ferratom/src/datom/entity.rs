//! Content-addressed entity identifier.
//!
//! INV-FERR-012: `EntityId = BLAKE3(content)`. Two entities with identical
//! content produce identical identifiers.

use serde::Serialize;

/// Content-addressed entity identifier: BLAKE3 hash of content bytes.
///
/// INV-FERR-012: `EntityId = BLAKE3(content)`. Two entities with identical
/// content produce identical identifiers. The inner field is private to
/// enforce construction only through `from_content` (production),
/// `from_trusted_bytes` (integrity-verified storage), or `from_bytes` (testing).
///
/// ADR-FERR-010: `Deserialize` is intentionally NOT derived. All
/// deserialization goes through `WireEntityId` in the `wire` module.
/// This prevents unverified bytes from entering the store as `EntityId`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct EntityId([u8; 32]);

impl EntityId {
    /// Create an `EntityId` by BLAKE3-hashing arbitrary content bytes.
    ///
    /// This is the ONLY production constructor. Every entity in the store
    /// derives its identity from content, making the store a content-addressed
    /// structure (INV-FERR-012).
    #[must_use]
    pub fn from_content(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }

    /// Reconstruct an `EntityId` from integrity-verified storage bytes.
    ///
    /// ADR-FERR-010: Caller MUST have verified source integrity (CRC for
    /// WAL, BLAKE3 for checkpoint) before calling this. For network-received
    /// data (Phase 4c), use `WireEntityId::into_verified()` instead.
    ///
    /// This is `pub(crate)` — only the `wire` module can call it.
    /// External crates (including the future federation crate) cannot
    /// bypass the trust boundary.
    #[must_use]
    pub(crate) fn from_trusted_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create an `EntityId` from raw bytes. **Testing only.**
    ///
    /// Bypasses the BLAKE3 derivation (INV-FERR-012). Used by proptest
    /// generators to cover the full 256-bit ID space without manufacturing
    /// content for each case.
    ///
    /// Gated behind `test-utils` feature or `#[cfg(test)]` to prevent
    /// accidental use in production code.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the underlying 32-byte array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_ferr_012_entity_id_content_addressed() {
        // Same content produces same EntityId.
        let a = EntityId::from_content(b"same content");
        let b = EntityId::from_content(b"same content");
        assert_eq!(
            a, b,
            "INV-FERR-012: identical content must yield identical EntityId"
        );
    }

    #[test]
    fn test_inv_ferr_012_entity_id_different_content() {
        let a = EntityId::from_content(b"alpha");
        let b = EntityId::from_content(b"bravo");
        assert_ne!(
            a, b,
            "INV-FERR-012: distinct content must yield distinct EntityId"
        );
    }

    #[test]
    fn test_entity_id_from_bytes_roundtrip() {
        let bytes = [42u8; 32];
        let eid = EntityId::from_bytes(bytes);
        assert_eq!(*eid.as_bytes(), bytes);
    }
}
