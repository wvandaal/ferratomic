//! Unsafe boundary for memory-mapped zero-copy access (ADR-FERR-020).
//!
//! This module is the ONLY location in ferratomic-core where unsafe code
//! is permitted. It provides [`validate_and_cast`] — a BLAKE3-guarded
//! pointer cast from raw bytes to a typed reference.
//!
//! ## Unsafe Budget
//!
//! - bd-erfj: 1 unsafe block in `validate_and_cast`
//! - bd-ta8c: adds `MappedStore::archived()` + `memmap2::Mmap::map`
//!
//! Total: 3 unsafe sites, all pointer casts or OS mmap with BLAKE3 guard.
#![allow(unsafe_code)]

use ferratom::FerraError;

/// BLAKE3 hash size in bytes. Used by `validate_and_cast` and checkpoint V3.
pub(crate) const HASH_SIZE: usize = 32;

/// Verify BLAKE3 checksum on `[content | hash(32)]` layout.
///
/// Returns the content slice (without trailing hash) on success.
/// Used by checkpoint V3 and mmap cold start.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on size or checksum failure.
pub(crate) fn verify_blake3(bytes: &[u8], min_content: usize) -> Result<&[u8], FerraError> {
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

#[cfg(test)]
mod tests {
    use ferratom::FerraError;

    use super::*;

    /// Marker trait for test: implementor is repr(C), all bit patterns valid.
    ///
    /// # Safety
    ///
    /// Only repr(C) test structs with no padding should implement this.
    /// Production version will be promoted from test to pub(crate) in bd-ta8c.
    unsafe trait ValidMmapTarget: Sized {}

    /// BLAKE3-verified cast from raw bytes to typed reference (ADR-FERR-020).
    /// Currently test-only; bd-ta8c promotes to pub(crate).
    fn validate_and_cast<T: ValidMmapTarget>(bytes: &[u8]) -> Result<&T, FerraError> {
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

    #[repr(C)]
    #[derive(Debug, PartialEq)]
    struct TestPayload {
        a: u64,
        b: u32,
    }

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
}
