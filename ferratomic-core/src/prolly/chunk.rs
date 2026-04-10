//! Chunk type and `ChunkStore` trait.
//!
//! `INV-FERR-045`: Every chunk is stored under `addr = BLAKE3(data)`.
//! `INV-FERR-050`: `ChunkStore` abstracts physical storage — operations are
//! observationally equivalent across all implementations.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::error::FerraError;

/// A content-addressed hash (32 bytes, BLAKE3).
pub type Hash = [u8; 32];

/// A content-addressed chunk of bytes.
///
/// `INV-FERR-045`: `addr = BLAKE3(data)`. Two chunks with identical data
/// produce identical addresses. Chunks are immutable after creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    addr: Hash,
    data: Arc<[u8]>,
}

impl Chunk {
    /// Create a chunk from raw bytes. Address is computed deterministically.
    ///
    /// `INV-FERR-045`: `addr = BLAKE3(data)`. Pure function.
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Chunk {
            addr: *hash.as_bytes(),
            data: Arc::from(data),
        }
    }

    /// The content-addressed hash of this chunk.
    #[must_use]
    pub fn addr(&self) -> &Hash {
        &self.addr
    }

    /// The raw bytes.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The length of the raw bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the chunk is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// The chunk store trait. Abstracts physical storage.
///
/// `INV-FERR-050`: All implementations produce observationally equivalent
/// results for the same sequence of operations.
///
/// `INV-FERR-050c` precondition: `put_chunk` is idempotent.
pub trait ChunkStore: Send + Sync {
    /// Store a chunk. Returns the content-addressed hash.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the underlying storage fails.
    fn put_chunk(&self, chunk: &Chunk) -> Result<Hash, FerraError>;

    /// Retrieve a chunk by its content-addressed hash.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the underlying storage fails.
    fn get_chunk(&self, addr: &Hash) -> Result<Option<Chunk>, FerraError>;

    /// Check whether a chunk exists without loading it.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the underlying storage fails.
    fn has_chunk(&self, addr: &Hash) -> Result<bool, FerraError>;

    /// Return the set of all chunk addresses.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the underlying storage fails.
    fn all_addrs(&self) -> Result<BTreeSet<Hash>, FerraError>;

    /// Delete a chunk by address (GC only, `INV-FERR-050d`).
    ///
    /// # Errors
    ///
    /// Returns `FerraError` if the underlying storage fails.
    fn delete_chunk(&self, addr: &Hash) -> Result<(), FerraError>;
}

/// In-memory chunk store for testing.
///
/// `INV-FERR-050`: observationally equivalent to file-backed stores.
#[derive(Debug, Default)]
pub struct MemoryChunkStore {
    chunks: std::sync::RwLock<std::collections::BTreeMap<Hash, Chunk>>,
}

impl MemoryChunkStore {
    /// Create an empty in-memory chunk store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

fn lock_err() -> FerraError {
    FerraError::InvariantViolation {
        invariant: "INV-FERR-050".into(),
        details: "MemoryChunkStore lock poisoned".into(),
    }
}

impl ChunkStore for MemoryChunkStore {
    fn put_chunk(&self, chunk: &Chunk) -> Result<Hash, FerraError> {
        let mut store = self.chunks.write().map_err(|_| lock_err())?;
        let addr = *chunk.addr();
        store.entry(addr).or_insert_with(|| chunk.clone());
        Ok(addr)
    }

    fn get_chunk(&self, addr: &Hash) -> Result<Option<Chunk>, FerraError> {
        let store = self.chunks.read().map_err(|_| lock_err())?;
        Ok(store.get(addr).cloned())
    }

    fn has_chunk(&self, addr: &Hash) -> Result<bool, FerraError> {
        let store = self.chunks.read().map_err(|_| lock_err())?;
        Ok(store.contains_key(addr))
    }

    fn all_addrs(&self) -> Result<BTreeSet<Hash>, FerraError> {
        let store = self.chunks.read().map_err(|_| lock_err())?;
        Ok(store.keys().copied().collect())
    }

    fn delete_chunk(&self, addr: &Hash) -> Result<(), FerraError> {
        let mut store = self.chunks.write().map_err(|_| lock_err())?;
        store.remove(addr);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_ferr_045_chunk_content_addressing() {
        let data = b"hello, prolly tree";
        let c1 = Chunk::from_bytes(data);
        let c2 = Chunk::from_bytes(data);
        assert_eq!(
            c1.addr(),
            c2.addr(),
            "INV-FERR-045: identical data must produce identical addresses"
        );

        let c3 = Chunk::from_bytes(b"different data");
        assert_ne!(c1.addr(), c3.addr());
    }

    #[test]
    fn test_inv_ferr_050_chunk_store_roundtrip() {
        let store = MemoryChunkStore::new();
        let chunk = Chunk::from_bytes(b"test chunk data");
        let addr = *chunk.addr();

        let stored_addr = store.put_chunk(&chunk).expect("put");
        assert_eq!(stored_addr, addr);

        let retrieved = store.get_chunk(&addr).expect("get");
        assert_eq!(retrieved, Some(chunk));
        assert!(store.has_chunk(&addr).expect("has"));
        assert!(store.all_addrs().expect("addrs").contains(&addr));
    }

    #[test]
    fn test_inv_ferr_050_put_idempotent() {
        let store = MemoryChunkStore::new();
        let chunk = Chunk::from_bytes(b"idempotent");

        store.put_chunk(&chunk).expect("put1");
        store.put_chunk(&chunk).expect("put2");
        assert_eq!(store.all_addrs().expect("addrs").len(), 1);
    }

    #[test]
    fn test_inv_ferr_050d_delete_chunk() {
        let store = MemoryChunkStore::new();
        let chunk = Chunk::from_bytes(b"to delete");
        let addr = *chunk.addr();

        store.put_chunk(&chunk).expect("put");
        store.delete_chunk(&addr).expect("delete");
        assert!(!store.has_chunk(&addr).expect("has"));
        store.delete_chunk(&addr).expect("delete non-existent");
    }

    #[test]
    fn test_inv_ferr_050_get_nonexistent() {
        let store = MemoryChunkStore::new();
        assert_eq!(store.get_chunk(&[0u8; 32]).expect("get"), None);
    }
}
