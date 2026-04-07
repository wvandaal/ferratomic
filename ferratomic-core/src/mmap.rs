//! Unsafe boundary for memory-mapped zero-copy access (ADR-FERR-020).
//!
//! Production mmap types are re-exported from `ferratomic_checkpoint::mmap`.
//! A local `validate_and_cast` is retained for test builds because the
//! checkpoint crate's copy is cfg-gated behind `feature = "mmap"` which
//! is not active in default test builds.
//!
//! ## Unsafe Budget
//!
//! - Production: 3 unsafe sites in `ferratomic-checkpoint::mmap` (ADR-FERR-020)
//! - Test-only: 1 unsafe block in local `make_test_data` + 1 unsafe trait impl
#![allow(unsafe_code)]

// Feature-gated mmap types (production)
#[cfg(feature = "mmap")]
pub(crate) use ferratomic_checkpoint::mmap::{
    mmap_cold_start, serialize_mmap_checkpoint, MappedStore, MmapPayload,
};
// For mmap feature builds, re-export from ferratomic-checkpoint
#[cfg(feature = "mmap")]
pub(crate) use ferratomic_checkpoint::mmap::{validate_and_cast, ValidMmapTarget};

// For test-only builds (no mmap feature), provide local definitions
// that delegate to ferratomic_checkpoint::mmap::verify_blake3.
#[cfg(all(test, not(feature = "mmap")))]
mod test_support {
    use ferratom::FerraError;

    /// Marker trait: implementor is repr(C), no padding, all bit patterns valid.
    ///
    /// # Safety
    ///
    /// Only `#[repr(C)]` types with no interior references, pointers, or
    /// interior mutability should implement this.
    pub(crate) unsafe trait ValidMmapTarget: Sized {}

    /// BLAKE3-verified cast from raw bytes to typed reference (ADR-FERR-020).
    pub(crate) fn validate_and_cast<T: ValidMmapTarget>(bytes: &[u8]) -> Result<&T, FerraError> {
        ferratomic_checkpoint::mmap::verify_blake3(bytes, std::mem::size_of::<T>()).and_then(
            |content| {
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
            },
        )
    }
}

#[cfg(all(test, not(feature = "mmap")))]
pub(crate) use test_support::{validate_and_cast, ValidMmapTarget};

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    #[derive(Debug, PartialEq)]
    struct TestPayload {
        a: u64,
        b: u64, // u64 instead of u32 to eliminate struct padding
    }

    // SAFETY: TestPayload is repr(C), all fields u64, no padding, all bit patterns valid.
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
        let hash_size = ferratomic_checkpoint::mmap::HASH_SIZE;
        let data = vec![0u8; hash_size];
        assert!(validate_and_cast::<TestPayload>(&data).is_err());
    }

    /// INV-FERR-070: mmap round-trip test.
    #[cfg(feature = "mmap")]
    #[test]
    fn test_inv_ferr_070_mmap_roundtrip() {
        use std::collections::BTreeSet;

        use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

        use crate::positional::PositionalStore;

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

        let mmap_bytes = serialize_mmap_checkpoint(
            positional.datoms(),
            &live_bits,
            42,
            [7u8; 16],
            &schema_bytes,
        )
        .expect("INV-FERR-070: mmap serialize must succeed");

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.mmap");
        std::fs::write(&path, &mmap_bytes).unwrap();

        let mapped = mmap_cold_start(&path).expect("INV-FERR-070: mmap cold start must succeed");
        assert_eq!(mapped.epoch(), 42);
        assert_eq!(mapped.datom_count(), 10);

        let recovered = mapped
            .promote_to_positional()
            .expect("INV-FERR-070: promote must succeed");
        let original_set: BTreeSet<&Datom> = positional.datoms().iter().collect();
        let recovered_set: BTreeSet<&Datom> = recovered.datoms().iter().collect();
        assert_eq!(original_set, recovered_set);
    }
}
