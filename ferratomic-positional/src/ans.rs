//! ANS (Asymmetric Numeral Systems) byte compression (bd-qk1mu).
//!
//! Wraps the `ans` crate (pure Rust, MIT) to provide byte-level entropy
//! coding for leaf chunk payloads. Achieves the empirical entropy bound
//! H(X) + ε bits/symbol — near-optimal compression for federation
//! transfer bandwidth reduction.
//!
//! Format: `[freq_counts: 256×u32-le (1024 bytes)][symbol_count: u32-le][compressed_payload]`
//!
//! Reference: Duda 2009, "Asymmetric Numeral Systems".

use ferratom::FerraError;

/// Precision bits for the ANS frequency table. Higher = better compression
/// ratio but larger table. 12 bits (4096 states) is the sweet spot.
const PRECISION_BITS: u32 = 12;

/// Compress a byte slice using rANS.
///
/// Returns the compressed data including the frequency table header.
/// For skewed distributions (e.g., EAVT-sorted entity columns with
/// σ ≈ 3-5), achieves ~2-3x compression.
///
/// # Errors
///
/// Returns `FerraError::InvariantViolation` if input is empty.
pub fn ans_compress(data: &[u8]) -> Result<Vec<u8>, FerraError> {
    if data.is_empty() {
        return Err(FerraError::InvariantViolation {
            invariant: "ANS".into(),
            details: "cannot compress empty input".into(),
        });
    }

    // Count byte frequencies
    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }

    // Build frequency table
    let table = ans::FrequencyTable::from_counts(&counts, PRECISION_BITS).map_err(|e| {
        FerraError::InvariantViolation {
            invariant: "ANS".into(),
            details: format!("frequency table build failed: {e}"),
        }
    })?;

    // Encode: convert bytes to u32 symbols for the ans crate
    let symbols: Vec<u32> = data.iter().map(|&b| u32::from(b)).collect();
    let compressed = ans::encode(&symbols, &table).map_err(|e| FerraError::InvariantViolation {
        invariant: "ANS".into(),
        details: format!("encode failed: {e}"),
    })?;

    // Pack: [counts: 256×u32-le (1024 bytes)][len: u32-le][compressed...]
    let sym_count = u32::try_from(data.len()).map_err(|_| FerraError::InvariantViolation {
        invariant: "ANS".into(),
        details: "input exceeds u32 length".into(),
    })?;

    let mut out = Vec::with_capacity(1024 + 4 + compressed.len());
    for &c in &counts {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out.extend_from_slice(&sym_count.to_le_bytes());
    out.extend_from_slice(&compressed);

    Ok(out)
}

/// Decompress ANS-compressed data.
///
/// # Errors
///
/// Returns `FerraError::TruncatedChunk` if input is too short,
/// `FerraError::InvariantViolation` on decode errors.
pub fn ans_decompress(compressed: &[u8]) -> Result<Vec<u8>, FerraError> {
    let header_size = 1024 + 4; // counts + symbol_count
    if compressed.len() < header_size {
        return Err(FerraError::TruncatedChunk);
    }

    // Read frequency counts
    let mut counts = [0u32; 256];
    for (i, c) in counts.iter_mut().enumerate() {
        let off = i * 4;
        *c = u32::from_le_bytes([
            compressed[off],
            compressed[off + 1],
            compressed[off + 2],
            compressed[off + 3],
        ]);
    }

    // Read symbol count
    let sym_count = u32::from_le_bytes([
        compressed[1024],
        compressed[1025],
        compressed[1026],
        compressed[1027],
    ]) as usize;

    // Rebuild frequency table
    let table = ans::FrequencyTable::from_counts(&counts, PRECISION_BITS).map_err(|e| {
        FerraError::InvariantViolation {
            invariant: "ANS".into(),
            details: format!("frequency table rebuild failed: {e}"),
        }
    })?;

    // Decode
    let payload = &compressed[header_size..];
    let symbols =
        ans::decode(payload, &table, sym_count).map_err(|e| FerraError::InvariantViolation {
            invariant: "ANS".into(),
            details: format!("decode failed: {e}"),
        })?;

    // Convert u32 symbols back to bytes
    let bytes: Vec<u8> = symbols
        .into_iter()
        .map(|s| u8::try_from(s).unwrap_or(0))
        .collect();

    Ok(bytes)
}

/// Compression ratio estimate for a given byte frequency distribution.
///
/// Returns `(entropy_bits_per_symbol, raw_bits_per_symbol)` where raw is 8.0.
/// The ratio `8.0 / entropy` is the theoretical compression factor.
#[must_use]
pub fn estimate_ratio(data: &[u8]) -> (f64, f64) {
    if data.is_empty() {
        return (8.0, 8.0);
    }
    let mut counts = [0u64; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let n = f64::from(u32::try_from(data.len()).unwrap_or(u32::MAX));
    let mut entropy = 0.0f64;
    for &c in &counts {
        if c > 0 {
            let p = f64::from(u32::try_from(c).unwrap_or(u32::MAX)) / n;
            entropy -= p * p.log2();
        }
    }
    (entropy, 8.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_single_symbol() {
        let data = vec![42u8; 100];
        let c = ans_compress(&data).expect("compress");
        let d = ans_decompress(&c).expect("decompress");
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_two_symbols() {
        let mut data = vec![0u8; 500];
        data.extend(vec![1u8; 500]);
        let c = ans_compress(&data).expect("compress");
        let d = ans_decompress(&c).expect("decompress");
        assert_eq!(d, data);
    }

    #[test]
    fn test_roundtrip_hello() {
        let data = b"hello world hello world hello";
        let c = ans_compress(data).expect("compress");
        let d = ans_decompress(&c).expect("decompress");
        assert_eq!(d, &data[..]);
    }

    #[test]
    fn test_roundtrip_all_bytes() {
        let data: Vec<u8> = (0..=255).collect();
        let c = ans_compress(&data).expect("compress");
        let d = ans_decompress(&c).expect("decompress");
        assert_eq!(d, data);
    }

    #[test]
    fn test_compression_large_skewed() {
        // 10K symbols — header overhead amortized, compression visible
        let mut data = vec![1u8; 5000];
        data.extend(vec![2u8; 3000]);
        data.extend(vec![3u8; 2000]);
        let c = ans_compress(&data).expect("compress");
        let d = ans_decompress(&c).expect("decompress");
        assert_eq!(d, data, "roundtrip must be lossless");
        assert!(
            c.len() < data.len(),
            "10K skewed must compress: {} -> {}",
            data.len(),
            c.len(),
        );
    }

    #[test]
    fn test_estimate_ratio() {
        let data = vec![0u8; 1000];
        let (h, _) = estimate_ratio(&data);
        assert!(h < 0.01, "single symbol entropy should be ~0, got {h}");
    }

    #[test]
    fn test_empty_error() {
        assert!(ans_compress(&[]).is_err());
    }

    #[test]
    fn test_truncated_error() {
        assert!(ans_decompress(&[0u8; 10]).is_err());
    }
}
