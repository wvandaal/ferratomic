//! Snapshot manifest: `RootSet` serialization and manifest CAS.
//!
//! `INV-FERR-049`: A snapshot is identified by a single manifest hash
//! that resolves to a `RootSet` of five tree roots. The manifest hash
//! is `BLAKE3(RootSet::canonical_bytes())` per S23.9.0.6.
//!
//! `INV-FERR-050b`: Manifest storage uses content-addressed chunks via
//! the `ChunkStore` trait. Physical CAS (write-temp→fsync→rename→fsync-dir)
//! is the responsibility of the `ChunkStore` implementation, not this module.
//!
//! ## Layout (S23.9.0.5)
//!
//! The manifest is a fixed 160-byte chunk:
//! ```text
//! [0..32)    primary tree root
//! [32..64)   eavt index root
//! [64..96)   aevt index root
//! [96..128)  vaet index root
//! [128..160) avet index root
//! ```

use ferratom::error::FerraError;

use crate::prolly::chunk::{Chunk, ChunkStore, Hash};

/// The five prolly tree roots that compose a snapshot.
///
/// `INV-FERR-049` (S23.9.0.4): A `RootSet` captures the complete state
/// of all five index trees at a point in time. Its canonical 160-byte
/// serialization is stored as a content-addressed chunk; the chunk's
/// BLAKE3 address is the manifest hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSet {
    /// Primary datom tree root.
    pub primary: Hash,
    /// Entity-Attribute-Value-Time index root.
    pub eavt: Hash,
    /// Attribute-Entity-Value-Time index root.
    pub aevt: Hash,
    /// Value-Attribute-Entity-Time index root.
    pub vaet: Hash,
    /// Attribute-Value-Entity-Time index root.
    pub avet: Hash,
}

/// Manifest chunk size: 5 tree roots × 32 bytes each.
const MANIFEST_SIZE: usize = 5 * 32;

impl RootSet {
    /// Serialize to the canonical 160-byte fixed layout (S23.9.0.5).
    ///
    /// `INV-FERR-049`: Deterministic — same `RootSet` always produces
    /// the same bytes. No padding, no variable-length fields.
    #[must_use]
    pub fn canonical_bytes(&self) -> [u8; MANIFEST_SIZE] {
        let mut buf = [0u8; MANIFEST_SIZE];
        buf[0..32].copy_from_slice(&self.primary);
        buf[32..64].copy_from_slice(&self.eavt);
        buf[64..96].copy_from_slice(&self.aevt);
        buf[96..128].copy_from_slice(&self.vaet);
        buf[128..160].copy_from_slice(&self.avet);
        buf
    }

    /// Deserialize from the canonical 160-byte layout.
    ///
    /// `INV-FERR-049`: `from_canonical_bytes(canonical_bytes(rs)) = rs`.
    #[must_use]
    pub fn from_canonical_bytes(buf: &[u8; MANIFEST_SIZE]) -> Self {
        let mut primary = [0u8; 32];
        let mut eavt = [0u8; 32];
        let mut aevt = [0u8; 32];
        let mut vaet = [0u8; 32];
        let mut avet = [0u8; 32];
        primary.copy_from_slice(&buf[0..32]);
        eavt.copy_from_slice(&buf[32..64]);
        aevt.copy_from_slice(&buf[64..96]);
        vaet.copy_from_slice(&buf[96..128]);
        avet.copy_from_slice(&buf[128..160]);
        RootSet {
            primary,
            eavt,
            aevt,
            vaet,
            avet,
        }
    }
}

/// Create a manifest chunk from a `RootSet` and store it.
///
/// Returns the manifest hash (BLAKE3 of the 160-byte canonical bytes).
/// This hash is the externally-visible snapshot identifier per INV-FERR-049.
///
/// # Errors
///
/// Returns `FerraError` if the chunk store write fails.
pub fn create_manifest(
    root_set: &RootSet,
    chunk_store: &dyn ChunkStore,
) -> Result<Hash, FerraError> {
    let manifest_bytes = root_set.canonical_bytes();
    let chunk = Chunk::from_bytes(&manifest_bytes);
    let addr = *chunk.addr();
    chunk_store.put_chunk(&chunk)?;
    Ok(addr)
}

/// Resolve a manifest hash to a `RootSet`.
///
/// `INV-FERR-049`: Loads the 160-byte manifest chunk and deserializes
/// the five tree roots.
///
/// # Errors
///
/// Returns `FerraError::InvariantViolation` if the manifest chunk is
/// missing or is not exactly 160 bytes.
pub fn resolve_manifest(
    manifest: &Hash,
    chunk_store: &dyn ChunkStore,
) -> Result<RootSet, FerraError> {
    let chunk = chunk_store
        .get_chunk(manifest)?
        .ok_or_else(|| FerraError::InvariantViolation {
            invariant: "INV-FERR-049".into(),
            details: format!(
                "manifest chunk {:02x}{:02x}{:02x}{:02x}... not found",
                manifest[0], manifest[1], manifest[2], manifest[3],
            ),
        })?;

    let buf: &[u8; MANIFEST_SIZE] =
        chunk
            .data()
            .try_into()
            .map_err(|_| FerraError::InvariantViolation {
                invariant: "INV-FERR-049".into(),
                details: format!(
                    "manifest chunk must be {MANIFEST_SIZE} bytes, got {}",
                    chunk.len(),
                ),
            })?;

    Ok(RootSet::from_canonical_bytes(buf))
}

/// Transfer a complete snapshot (manifest + all five trees) from `src` to `dst`.
///
/// Two-phase protocol (S23.9.0.6):
/// 1. Copy the 160-byte manifest chunk (idempotent).
/// 2. Transfer each of the five tree roots via `ChunkTransfer`.
///
/// After completion, `resolve_manifest(manifest, dst)` succeeds and
/// all five trees are navigable from `dst`.
///
/// # Errors
///
/// Returns `FerraError` if any chunk store operation or tree transfer fails.
pub fn transfer_snapshot(
    manifest: &Hash,
    src: &dyn ChunkStore,
    dst: &dyn ChunkStore,
    transfer: &dyn super::transfer::ChunkTransfer,
) -> Result<(), FerraError> {
    // Phase 1: copy manifest chunk
    if !dst.has_chunk(manifest)? {
        let chunk = src
            .get_chunk(manifest)?
            .ok_or_else(|| FerraError::InvariantViolation {
                invariant: "INV-FERR-049".into(),
                details: "manifest chunk not found in source".into(),
            })?;
        dst.put_chunk(&chunk)?;
    }

    // Phase 2: resolve and transfer each tree
    let rs = resolve_manifest(manifest, src)?;
    transfer.transfer(src, dst, &rs.primary)?;
    transfer.transfer(src, dst, &rs.eavt)?;
    transfer.transfer(src, dst, &rs.aevt)?;
    transfer.transfer(src, dst, &rs.vaet)?;
    transfer.transfer(src, dst, &rs.avet)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::prolly::{
        boundary::DEFAULT_PATTERN_WIDTH, build::build_prolly_tree, chunk::MemoryChunkStore,
        read::read_prolly_tree, transfer::RecursiveTransfer,
    };

    fn empty_hash() -> Hash {
        [0u8; 32]
    }

    fn build(kvs: &BTreeMap<Vec<u8>, Vec<u8>>, store: &MemoryChunkStore) -> Hash {
        build_prolly_tree(kvs, store, DEFAULT_PATTERN_WIDTH).expect("build")
    }

    #[test]
    fn test_inv_ferr_049_rootset_roundtrip() {
        let rs = RootSet {
            primary: [1u8; 32],
            eavt: [2u8; 32],
            aevt: [3u8; 32],
            vaet: [4u8; 32],
            avet: [5u8; 32],
        };
        let bytes = rs.canonical_bytes();
        let recovered = RootSet::from_canonical_bytes(&bytes);
        assert_eq!(rs, recovered, "INV-FERR-049: RootSet canonical roundtrip");
    }

    #[test]
    fn test_inv_ferr_049_rootset_deterministic() {
        let rs = RootSet {
            primary: [0xAA; 32],
            eavt: [0xBB; 32],
            aevt: [0xCC; 32],
            vaet: [0xDD; 32],
            avet: [0xEE; 32],
        };
        let b1 = rs.canonical_bytes();
        let b2 = rs.canonical_bytes();
        assert_eq!(
            b1, b2,
            "INV-FERR-049: canonical_bytes must be deterministic"
        );
    }

    #[test]
    fn test_inv_ferr_049_rootset_distinct() {
        let rs1 = RootSet {
            primary: [1u8; 32],
            eavt: empty_hash(),
            aevt: empty_hash(),
            vaet: empty_hash(),
            avet: empty_hash(),
        };
        let rs2 = RootSet {
            primary: [2u8; 32],
            eavt: empty_hash(),
            aevt: empty_hash(),
            vaet: empty_hash(),
            avet: empty_hash(),
        };
        assert_ne!(
            rs1.canonical_bytes(),
            rs2.canonical_bytes(),
            "different RootSets must produce different bytes"
        );
    }

    #[test]
    fn test_inv_ferr_049_manifest_size() {
        let store = MemoryChunkStore::new();
        let rs = RootSet {
            primary: [1u8; 32],
            eavt: [2u8; 32],
            aevt: [3u8; 32],
            vaet: [4u8; 32],
            avet: [5u8; 32],
        };
        let manifest = create_manifest(&rs, &store).expect("create");
        let chunk = store
            .get_chunk(&manifest)
            .expect("get")
            .expect("manifest must exist");
        assert_eq!(
            chunk.len(),
            MANIFEST_SIZE,
            "S23.9.0.5: manifest must be exactly 160 bytes"
        );
    }

    #[test]
    fn test_inv_ferr_049_manifest_roundtrip() {
        let store = MemoryChunkStore::new();
        let rs = RootSet {
            primary: [0x11; 32],
            eavt: [0x22; 32],
            aevt: [0x33; 32],
            vaet: [0x44; 32],
            avet: [0x55; 32],
        };
        let manifest = create_manifest(&rs, &store).expect("create");
        let recovered = resolve_manifest(&manifest, &store).expect("resolve");
        assert_eq!(
            rs, recovered,
            "INV-FERR-049: manifest create→resolve roundtrip"
        );
    }

    #[test]
    fn test_inv_ferr_049_manifest_deterministic() {
        let store = MemoryChunkStore::new();
        let rs = RootSet {
            primary: [0xFF; 32],
            eavt: empty_hash(),
            aevt: empty_hash(),
            vaet: empty_hash(),
            avet: empty_hash(),
        };
        let m1 = create_manifest(&rs, &store).expect("create1");
        let m2 = create_manifest(&rs, &store).expect("create2");
        assert_eq!(
            m1, m2,
            "INV-FERR-049: same RootSet must produce same manifest hash"
        );
    }

    #[test]
    fn test_inv_ferr_049_primary_tree_roundtrip() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..100 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![(i & 0xFF) as u8; 4]);
        }
        let primary_root = build(&kvs, &store);

        let rs = RootSet {
            primary: primary_root,
            eavt: empty_hash(),
            aevt: empty_hash(),
            vaet: empty_hash(),
            avet: empty_hash(),
        };
        let manifest = create_manifest(&rs, &store).expect("create");
        let resolved_rs = resolve_manifest(&manifest, &store).expect("resolve");
        let recovered = read_prolly_tree(&resolved_rs.primary, &store).expect("read");

        assert_eq!(
            recovered, kvs,
            "INV-FERR-049: manifest → RootSet → primary tree roundtrip"
        );
    }

    #[test]
    fn test_inv_ferr_049_resolve_missing_manifest() {
        let store = MemoryChunkStore::new();
        let bogus = [0xDEu8; 32];
        let err = resolve_manifest(&bogus, &store).unwrap_err();
        assert!(
            matches!(err, FerraError::InvariantViolation { .. }),
            "missing manifest must return InvariantViolation"
        );
    }

    #[test]
    fn test_inv_ferr_049_resolve_wrong_size() {
        let store = MemoryChunkStore::new();
        // Store a chunk that's not 160 bytes
        let bad_data = vec![0u8; 100];
        let chunk = Chunk::from_bytes(&bad_data);
        let addr = *chunk.addr();
        store.put_chunk(&chunk).expect("put");

        let err = resolve_manifest(&addr, &store).unwrap_err();
        assert!(
            matches!(err, FerraError::InvariantViolation { .. }),
            "wrong-size manifest must return InvariantViolation"
        );
    }

    #[test]
    fn test_inv_ferr_049_transfer_snapshot() {
        let src = MemoryChunkStore::new();
        let dst = MemoryChunkStore::new();

        let mut kvs = BTreeMap::new();
        for i in 0u32..50 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }
        let primary_root = build(&kvs, &src);

        // Build empty trees for the other 4 indexes
        let empty_kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let empty_root = build(&empty_kvs, &src);

        let rs = RootSet {
            primary: primary_root,
            eavt: empty_root,
            aevt: empty_root,
            vaet: empty_root,
            avet: empty_root,
        };
        let manifest = create_manifest(&rs, &src).expect("create");

        let xfer = RecursiveTransfer;
        transfer_snapshot(&manifest, &src, &dst, &xfer).expect("transfer");

        // Verify manifest resolvable from dst
        let recovered_rs = resolve_manifest(&manifest, &dst).expect("resolve from dst");
        assert_eq!(recovered_rs, rs);

        // Verify primary tree accessible from dst
        let recovered = read_prolly_tree(&recovered_rs.primary, &dst).expect("read");
        assert_eq!(recovered, kvs);
    }
}
