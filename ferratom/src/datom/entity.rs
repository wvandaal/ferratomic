//! Content-addressed entity identifier.
//!
//! INV-FERR-012: `EntityId = BLAKE3(content)`. Two entities with identical
//! content produce identical identifiers.

use serde::{Deserialize, Serialize};

/// Content-addressed entity identifier: BLAKE3 hash of content bytes.
///
/// INV-FERR-012: `EntityId = BLAKE3(content)`. Two entities with identical
/// content produce identical identifiers. The inner field is private to
/// enforce construction only through `from_content` (production) or
/// `from_bytes` (testing).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
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
