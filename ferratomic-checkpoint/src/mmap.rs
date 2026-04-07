//! Unsafe boundary for memory-mapped zero-copy access (ADR-FERR-020).
//!
//! This module is the ONLY location in ferratomic-checkpoint where unsafe code
//! is permitted. It provides [`validate_and_cast`] — a BLAKE3-guarded
//! pointer cast from raw bytes to a typed reference.
//!
//! ## Unsafe Budget
//!
//! - bd-erfj: 1 unsafe block in `validate_and_cast`
//! - bd-ta8c: adds `MappedStore::archived()` + `memmap2::Mmap::map`
//!
//! Total: 3 unsafe sites, all pointer casts or OS mmap with BLAKE3 guard.

use ferratom::FerraError;

/// BLAKE3 hash size in bytes. Used by `validate_and_cast` and checkpoint V3.
pub const HASH_SIZE: usize = 32;

/// Verify BLAKE3 checksum on `[content | hash(32)]` layout.
///
/// Returns the content slice (without trailing hash) on success.
/// Used by checkpoint V3 and mmap cold start.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on size or checksum failure.
pub fn verify_blake3(bytes: &[u8], min_content: usize) -> Result<&[u8], FerraError> {
    let min_size = HASH_SIZE + min_content;
    if bytes.len() < min_size {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("at least {min_size} bytes"),
            actual: format!("{} bytes", bytes.len()),
        });
    }

    let (content, stored_hash) = bytes.split_at(bytes.len() - HASH_SIZE);

    let computed = blake3::hash(content);
    if computed.as_bytes() != stored_hash {
        return Err(FerraError::CheckpointCorrupted {
            expected: "valid BLAKE3 checksum".to_string(),
            actual: "checksum mismatch".to_string(),
        });
    }

    Ok(content)
}

// ---------------------------------------------------------------------------
// ValidMmapTarget + validate_and_cast (ADR-FERR-020)
// Feature-gated: only compiled when mmap feature is enabled.
// Also available in tests for ADR-FERR-020 verification.
// ---------------------------------------------------------------------------

/// Marker trait: implementor is repr(C), no padding, all bit patterns valid.
///
/// # Safety
///
/// Only `#[repr(C)]` types with no interior references, pointers, or
/// interior mutability should implement this.
#[cfg(any(feature = "mmap", test))]
pub unsafe trait ValidMmapTarget: Sized {}

/// BLAKE3-verified cast from raw bytes to typed reference (ADR-FERR-020).
///
/// Checks: size, BLAKE3 integrity, alignment. Single unsafe ptr cast.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on any check failure.
#[cfg(any(feature = "mmap", test))]
pub fn validate_and_cast<T: ValidMmapTarget>(bytes: &[u8]) -> Result<&T, FerraError> {
    let content = verify_blake3(bytes, std::mem::size_of::<T>())?;

    let ptr = content.as_ptr();
    let align = std::mem::align_of::<T>();
    if (ptr as usize) % align != 0 {
        return Err(FerraError::CheckpointCorrupted {
            expected: format!("{align}-byte alignment"),
            actual: format!("alignment offset {}", (ptr as usize) % align),
        });
    }

    // SAFETY: BLAKE3 verified, size checked, alignment checked, T: ValidMmapTarget.
    Ok(unsafe { &*ptr.cast::<T>() })
}

// ---------------------------------------------------------------------------
// Feature-gated mmap types (INV-FERR-070, bd-ta8c)
// ---------------------------------------------------------------------------

#[cfg(feature = "mmap")]
mod mmap_impl {
    use bitvec::prelude::{BitVec, Lsb0};
    use ferratom::{wire::WireDatom, Datom};
    use ferratomic_positional::PositionalStore;
    use rkyv::Archive;

    use super::*;

    // -----------------------------------------------------------------------
    // MmapPayload — rkyv-serializable checkpoint format
    // -----------------------------------------------------------------------

    /// Rkyv-serializable mmap checkpoint payload (INV-FERR-070).
    ///
    /// Stores datoms as bincode-serialized `Vec<WireDatom>` bytes (Phase 4a).
    /// Phase 4b replaces this with fixed-size integer tuples for true
    /// per-datom zero-copy. The mmap infrastructure is permanent; only the
    /// datom representation evolves.
    #[derive(Archive, rkyv::Serialize, rkyv::Deserialize)]
    pub struct MmapPayload {
        /// Bincode-serialized `Vec<WireDatom>` (ADR-FERR-010 trust boundary).
        pub datoms_bytes: Vec<u8>,
        /// Number of datoms in `datoms_bytes`.
        pub datom_count: u64,
        /// LIVE bitvector backing words (BitVec<u64, Lsb0> internals).
        pub live_bits_words: Vec<u64>,
        /// Logical length of the LIVE bitvector in bits.
        pub live_bits_len: u64,
        /// Store epoch at checkpoint time.
        pub epoch: u64,
        /// Genesis agent identity bytes.
        pub genesis_agent: [u8; 16],
        /// Bincode-serialized `Vec<(String, AttributeDef)>` schema.
        pub schema_bytes: Vec<u8>,
    }

    // -----------------------------------------------------------------------
    // MappedStore — wraps Mmap + cached metadata (NO transmute)
    // -----------------------------------------------------------------------

    /// Memory-mapped store for O(1) cold start (INV-FERR-070).
    ///
    /// Wraps `memmap2::Mmap` with cached epoch and datom count. The mmap
    /// is BLAKE3-verified at construction. `archived()` re-derives the
    /// typed reference from the validated bytes using rkyv access.
    ///
    /// `promote_to_positional()` performs the O(n) deserialization from
    /// rkyv-archived bytes to owned `PositionalStore`.
    pub struct MappedStore {
        /// The memory-mapped file.
        mmap: memmap2::Mmap,
        /// End of content region (before BLAKE3 hash).
        payload_end: usize,
        /// Cached epoch from archived payload (avoids re-deriving).
        epoch: u64,
        /// Cached datom count from archived payload.
        datom_count: u64,
    }

    impl MappedStore {
        /// Epoch at checkpoint time.
        pub fn epoch(&self) -> u64 {
            self.epoch
        }

        /// Number of datoms in the mapped payload.
        pub fn datom_count(&self) -> u64 {
            self.datom_count
        }

        /// Re-derive archived reference from mmap bytes. O(1), no BLAKE3 re-check.
        ///
        /// Uses rkyv's `access_unchecked` since the bytes were validated at
        /// construction time (BLAKE3 + rkyv validation in `mmap_cold_start`).
        fn archived(&self) -> &ArchivedMmapPayload {
            let content = &self.mmap[..self.payload_end];
            // SAFETY: BLAKE3-verified at construction. rkyv layout validated
            // at construction via rkyv::access. Mmap alive (self holds it).
            // Page-aligned (mmap guarantee). Content is immutable.
            unsafe { rkyv::access_unchecked::<ArchivedMmapPayload>(content) }
        }

        /// Promote to owned `PositionalStore`. O(n) — copies mapped data to heap.
        ///
        /// Phase 4a: bincode deserialize `WireDatom` -> `into_trusted()` -> `Datom`.
        /// Phase 4b: value pool makes this a thin ID->value mapping instead.
        ///
        /// # Errors
        ///
        /// Returns `FerraError::CheckpointCorrupted` if bincode deserialization
        /// of the datom bytes fails.
        pub fn promote_to_positional(&self) -> Result<PositionalStore, FerraError> {
            let archived = self.archived();

            // ArchivedVec<u8> -> &[u8] via Deref
            let datoms_bytes: &[u8] = archived.datoms_bytes.as_slice();

            // ADR-FERR-010: WireDatom trust boundary
            let wire_datoms: Vec<WireDatom> = bincode::deserialize(datoms_bytes).map_err(|e| {
                FerraError::CheckpointCorrupted {
                    expected: "valid bincode WireDatom in mmap payload".into(),
                    actual: e.to_string(),
                }
            })?;

            let datoms: Vec<Datom> = wire_datoms
                .into_iter()
                .map(WireDatom::into_trusted)
                .collect();

            // Reconstruct BitVec from archived words
            let live_bits_len: usize = archived.live_bits_len.to_native() as usize;
            let words: Vec<u64> = archived
                .live_bits_words
                .iter()
                .map(|w| w.to_native())
                .collect();
            let mut live_bits = BitVec::<u64, Lsb0>::from_vec(words);
            live_bits.truncate(live_bits_len);

            PositionalStore::from_sorted_with_live(datoms, live_bits)
        }
    }

    /// Access the archived payload from BLAKE3-verified content bytes.
    ///
    /// Uses rkyv's validated access to ensure the archived layout is sound.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::CheckpointCorrupted` if rkyv validation fails.
    fn access_archived(content: &[u8]) -> Result<&ArchivedMmapPayload, FerraError> {
        rkyv::access::<ArchivedMmapPayload, rkyv::rancor::Error>(content).map_err(|e| {
            FerraError::CheckpointCorrupted {
                expected: "valid rkyv ArchivedMmapPayload".into(),
                actual: format!("{e}"),
            }
        })
    }

    /// Map a file and validate as mmap checkpoint (INV-FERR-070).
    ///
    /// Performs O(1) mmap syscall + O(n) BLAKE3 verification + rkyv validation.
    /// The returned `MappedStore` caches epoch and datom count;
    /// `promote_to_positional()` performs the O(n) deserialization.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::Io` if the file cannot be opened or mapped.
    /// Returns `FerraError::CheckpointCorrupted` if BLAKE3 or rkyv validation fails.
    pub fn mmap_cold_start(path: &std::path::Path) -> Result<MappedStore, FerraError> {
        let file = std::fs::File::open(path).map_err(|e| FerraError::Io {
            kind: std::io::ErrorKind::NotFound.to_string(),
            message: format!("mmap_cold_start: {}: {e}", path.display()),
        })?;

        // SAFETY: File opened read-only. Mmap is immutable. OS guarantees
        // page-aligned mapping. BLAKE3 + rkyv verification follows immediately.
        let mmap =
            unsafe { memmap2::MmapOptions::new().map(&file) }.map_err(|e| FerraError::Io {
                kind: std::io::ErrorKind::Other.to_string(),
                message: format!("mmap failed: {e}"),
            })?;

        // Step 1: BLAKE3 verify — content integrity
        let content = verify_blake3(&mmap, 0)?;

        // Step 2: rkyv validate — layout integrity
        let archived = access_archived(content)?;

        let epoch = archived.epoch.to_native();
        let datom_count = archived.datom_count.to_native();
        let payload_end = mmap.len() - HASH_SIZE;

        Ok(MappedStore {
            mmap,
            payload_end,
            epoch,
            datom_count,
        })
    }

    /// Serialize a store snapshot to rkyv mmap checkpoint format.
    ///
    /// Layout: `[rkyv(MmapPayload) | BLAKE3(content)]`.
    /// The caller writes the returned bytes to a file for `mmap_cold_start`.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::CheckpointCorrupted` if bincode or rkyv serialization fails.
    pub fn serialize_mmap_checkpoint(
        datoms: &[Datom],
        live_bits: &BitVec<u64, Lsb0>,
        epoch: u64,
        genesis_agent: [u8; 16],
        schema_bytes: &[u8],
    ) -> Result<Vec<u8>, FerraError> {
        // Serialize datoms as bincode Vec<WireDatom> — ADR-FERR-010 boundary
        let datoms_bytes =
            bincode::serialize(datoms).map_err(|e| FerraError::CheckpointCorrupted {
                expected: "bincode-serializable datoms".into(),
                actual: e.to_string(),
            })?;

        let payload = MmapPayload {
            datoms_bytes,
            datom_count: datoms.len() as u64,
            live_bits_words: live_bits.as_raw_slice().to_vec(),
            live_bits_len: live_bits.len() as u64,
            epoch,
            genesis_agent,
            schema_bytes: schema_bytes.to_vec(),
        };

        let rkyv_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&payload).map_err(|e| {
            FerraError::CheckpointCorrupted {
                expected: "rkyv-serializable MmapPayload".into(),
                actual: format!("{e}"),
            }
        })?;

        let mut result = rkyv_bytes.into_vec();
        let hash = blake3::hash(&result);
        result.extend_from_slice(hash.as_bytes());

        Ok(result)
    }
}

#[cfg(feature = "mmap")]
pub use mmap_impl::{mmap_cold_start, serialize_mmap_checkpoint, MappedStore, MmapPayload};

#[cfg(test)]
mod tests {

    use super::*;

    #[repr(C)]
    #[derive(Debug, PartialEq)]
    struct TestPayload {
        a: u64,
        b: u32,
    }

    // SAFETY: TestPayload is repr(C). All fields are integer types.
    // All bit patterns of u64 and u32 are valid.
    unsafe impl ValidMmapTarget for TestPayload {}

    fn make_test_data(payload: &TestPayload) -> Vec<u8> {
        let payload_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                std::ptr::from_ref(payload).cast::<u8>(),
                std::mem::size_of::<TestPayload>(),
            )
        };
        let mut data = payload_bytes.to_vec();
        let hash = blake3::hash(&data);
        data.extend_from_slice(hash.as_bytes());
        data
    }

    #[test]
    fn test_adr_ferr_020_valid_cast() {
        let payload = TestPayload { a: 42, b: 7 };
        let data = make_test_data(&payload);
        let result = validate_and_cast::<TestPayload>(&data);
        assert!(result.is_ok());
        let reference = result.unwrap();
        assert_eq!(reference.a, 42);
        assert_eq!(reference.b, 7);
    }

    #[test]
    fn test_adr_ferr_020_corrupted_hash() {
        let payload = TestPayload { a: 1, b: 2 };
        let mut data = make_test_data(&payload);
        data[0] ^= 0xFF;
        assert!(validate_and_cast::<TestPayload>(&data).is_err());
    }

    #[test]
    fn test_adr_ferr_020_too_short() {
        let data = vec![0u8; 10];
        assert!(validate_and_cast::<TestPayload>(&data).is_err());
    }

    #[test]
    fn test_adr_ferr_020_empty_input() {
        assert!(validate_and_cast::<TestPayload>(&[]).is_err());
    }

    #[test]
    fn test_adr_ferr_020_hash_only_no_content() {
        let data = vec![0u8; HASH_SIZE];
        assert!(validate_and_cast::<TestPayload>(&data).is_err());
    }

    /// INV-FERR-070: mmap round-trip test.
    ///
    /// Serializes a store to rkyv mmap format, writes to tempfile,
    /// mmaps it back, promotes to PositionalStore, verifies datom identity.
    #[cfg(feature = "mmap")]
    #[test]
    fn test_inv_ferr_070_mmap_roundtrip() {
        use std::collections::BTreeSet;

        use bitvec::prelude::{BitVec, Lsb0};
        use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
        use ferratomic_positional::PositionalStore;

        // Build test datoms.
        let datoms: Vec<Datom> = (0..10)
            .map(|i| {
                Datom::new(
                    EntityId::from_content(format!("mmap-entity-{i}").as_bytes()),
                    Attribute::from("db/doc"),
                    Value::String(format!("mmap-value-{i}").into()),
                    TxId::new(0, i, 0),
                    Op::Assert,
                )
            })
            .collect();

        let positional = PositionalStore::from_datoms(datoms.iter().cloned());
        let live_bits = positional.live_bits_clone();
        let schema_bytes = bincode::serialize(&Vec::<(String, ferratom::AttributeDef)>::new())
            .expect("empty schema serializable");

        // Serialize to mmap format.
        let mmap_bytes = serialize_mmap_checkpoint(
            positional.datoms(),
            &live_bits,
            42,        // epoch
            [7u8; 16], // genesis_agent
            &schema_bytes,
        )
        .expect("INV-FERR-070: mmap serialize must succeed");

        // Write to tempfile and mmap back.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.mmap");
        std::fs::write(&path, &mmap_bytes).unwrap();

        let mapped = mmap_cold_start(&path).expect("INV-FERR-070: mmap cold start must succeed");
        assert_eq!(
            mapped.epoch(),
            42,
            "INV-FERR-070: epoch must survive round-trip"
        );
        assert_eq!(
            mapped.datom_count(),
            10,
            "INV-FERR-070: datom count must survive round-trip"
        );

        // Promote to PositionalStore and compare datom sets.
        let recovered = mapped
            .promote_to_positional()
            .expect("INV-FERR-070: promote must succeed");
        let original_set: BTreeSet<&Datom> = positional.datoms().iter().collect();
        let recovered_set: BTreeSet<&Datom> = recovered.datoms().iter().collect();
        assert_eq!(
            original_set, recovered_set,
            "INV-FERR-070: datom sets must be identical after mmap round-trip"
        );
    }
}
