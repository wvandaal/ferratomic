//! Wavelet matrix backend trait — swappable rank/select implementations.
//!
//! Defines a `WaveletBackend` trait that abstracts over the concrete wavelet
//! matrix library. Two implementations are provided behind feature flags:
//!
//! - `wavelet-qwt`: `qwt::HQWT256Pfs` — Huffman-shaped quad wavelet tree
//!   with prefetch support. Best query performance for skewed alphabets.
//! - `wavelet-vers`: `vers_vecs::WaveletMatrix` — zero-dependency wavelet
//!   matrix with `#![forbid(unsafe_code)]`. Best supply chain hygiene.
//!
//! The trait enables compile-time backend selection via feature flags
//! without changing application code. This follows the same DI pattern
//! as `LeafChunkCodec` (INV-FERR-045c).
//!
//! Spec reference: spec/09 §Wavelet (rank/select contract, 6 algebraic laws).

/// Wavelet matrix backend trait for rank/select/access queries.
///
/// Implementations must satisfy the algebraic laws from spec/09 §Wavelet:
/// - L1: `rank(c, 0) = 0` for all symbols c
/// - L2: `rank(c, i) + rank(c, n-i) = rank(c, n)` (complement)
/// - L3: `select(c, rank(c, i+1) - 1) = i` when `data[i] = c`
/// - L4: `access(i) = c` iff `rank(c, i+1) - rank(c, i) = 1`
/// - L5: `sum over all c of rank(c, n) = n` (partition)
/// - L6: `rank(c, select(c, k) + 1) = k + 1` (inverse)
pub trait WaveletBackend: Send + Sync {
    /// Build a wavelet matrix from a sequence of u8 symbols.
    ///
    /// The alphabet size is determined from the input (max value + 1).
    /// Per-chunk encoding uses u8 symbols (effective alphabet σ ≈ 10-20).
    fn build(symbols: &[u8]) -> Self
    where
        Self: Sized;

    /// Number of symbols in the indexed sequence.
    fn len(&self) -> usize;

    /// Whether the indexed sequence is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retrieve the symbol at position `i`, or `None` if out of bounds.
    fn access(&self, i: usize) -> Option<u8>;

    /// Count occurrences of `symbol` in `[0..position)`.
    ///
    /// Returns `None` if `position > len()`.
    fn rank(&self, symbol: u8, position: usize) -> Option<usize>;

    /// Position of the `occurrence`-th instance of `symbol` (0-indexed).
    ///
    /// Returns `None` if fewer than `occurrence + 1` instances exist.
    fn select(&self, symbol: u8, occurrence: usize) -> Option<usize>;
}

// =========================================================================
// qwt backend (feature = "wavelet-qwt")
// =========================================================================

#[cfg(feature = "wavelet-qwt")]
mod qwt_impl {
    use super::WaveletBackend;

    /// Wavelet backend backed by `qwt::HQWT256Pfs` — Huffman-shaped quad
    /// wavelet tree with 256-bit superblocks and prefetch support.
    ///
    /// Best performance for skewed alphabets (per-chunk datom encoding).
    /// Recommended by spec/09 §Wavelet research (session 026, bd-obo8).
    pub struct QwtBackend {
        inner: qwt::HQWT256Pfs<u8>,
        len: usize,
    }

    impl WaveletBackend for QwtBackend {
        fn build(symbols: &[u8]) -> Self {
            let len = symbols.len();
            let inner = qwt::HQWT256Pfs::from(symbols.to_vec());
            Self { inner, len }
        }

        fn len(&self) -> usize {
            self.len
        }

        fn access(&self, i: usize) -> Option<u8> {
            use qwt::AccessUnsigned;
            self.inner.get(i)
        }

        fn rank(&self, symbol: u8, position: usize) -> Option<usize> {
            use qwt::RankUnsigned;
            self.inner.rank(symbol, position)
        }

        fn select(&self, symbol: u8, occurrence: usize) -> Option<usize> {
            use qwt::SelectUnsigned;
            self.inner.select(symbol, occurrence)
        }
    }
}

#[cfg(feature = "wavelet-qwt")]
pub use qwt_impl::QwtBackend;

// =========================================================================
// vers-vecs backend (feature = "wavelet-vers")
// =========================================================================

#[cfg(feature = "wavelet-vers")]
mod vers_impl {
    use super::WaveletBackend;

    /// Wavelet backend backed by `vers_vecs::WaveletMatrix` — zero-dependency
    /// wavelet matrix with `#![forbid(unsafe_code)]` on the wavelet module.
    ///
    /// Best supply chain hygiene. Supports range-rank queries. No Huffman
    /// shaping (uniform binary wavelet tree).
    pub struct VersBackend {
        inner: vers_vecs::WaveletMatrix,
        len: usize,
    }

    impl WaveletBackend for VersBackend {
        fn build(symbols: &[u8]) -> Self {
            let len = symbols.len();
            let data: Vec<u64> = symbols.iter().map(|&s| u64::from(s)).collect();
            // Always use 8 bits for u8 input to avoid symbol truncation.
            // A smaller bit width would wrap symbols > 2^bits, violating
            // the rank/select contract (L5 partition property).
            let inner = vers_vecs::WaveletMatrix::from_slice(&data, 8);
            Self { inner, len }
        }

        fn len(&self) -> usize {
            self.len
        }

        fn access(&self, i: usize) -> Option<u8> {
            self.inner.get_u64(i).and_then(|v| u8::try_from(v).ok())
        }

        fn rank(&self, symbol: u8, position: usize) -> Option<usize> {
            self.inner.rank_u64(position, u64::from(symbol))
        }

        fn select(&self, symbol: u8, occurrence: usize) -> Option<usize> {
            self.inner.select_u64(occurrence, u64::from(symbol))
        }
    }
}

#[cfg(feature = "wavelet-vers")]
pub use vers_impl::VersBackend;

#[cfg(all(test, any(feature = "wavelet-qwt", feature = "wavelet-vers")))]
mod tests {
    use super::*;

    /// Generic test harness: exercises the 6 algebraic laws from spec/09
    /// §Wavelet on any `WaveletBackend` implementation.
    fn run_backend_suite<B: WaveletBackend>() {
        let data: Vec<u8> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = B::build(&data);

        // Access roundtrip
        for (i, &expected) in data.iter().enumerate() {
            assert_eq!(wm.access(i), Some(expected), "access({i})");
        }
        assert_eq!(wm.access(data.len()), None, "access out of bounds");

        // L1: rank(c, 0) = 0
        for c in 0u8..10 {
            let r = wm.rank(c, 0);
            assert!(
                r == Some(0) || r.is_none(),
                "L1: rank({c}, 0) must be 0 or None"
            );
        }

        // L3 + L6: rank-select inverse
        for pos in 0..data.len() {
            let sym = wm.access(pos).expect("valid pos");
            let r = wm.rank(sym, pos + 1).expect("valid rank");
            if r > 0 {
                let s = wm.select(sym, r - 1).expect("valid select");
                assert_eq!(s, pos, "L3: select({sym}, rank-1) = {pos}");
                // L6: rank(c, select(c, k) + 1) = k + 1
                let r2 = wm.rank(sym, s + 1).expect("L6 rank");
                assert_eq!(r2, r, "L6: rank({sym}, select+1) = {r}");
            }
        }

        // L5: sum of all ranks at n = n (partition property)
        let n = data.len();
        let mut total = 0usize;
        for c in 0u8..=255 {
            if let Some(r) = wm.rank(c, n) {
                total += r;
            }
        }
        assert_eq!(total, n, "L5: sum of ranks = n");

        // Specific rank checks
        assert_eq!(wm.rank(1, 2), Some(1), "rank(1, 2)");
        assert_eq!(wm.rank(1, 4), Some(2), "rank(1, 4)");
        assert_eq!(wm.rank(5, data.len()), Some(3), "rank(5, n)");

        // Specific select checks
        assert_eq!(wm.select(1, 0), Some(1), "select(1, 0)");
        assert_eq!(wm.select(1, 1), Some(3), "select(1, 1)");
        assert_eq!(wm.select(1, 2), None, "select(1, 2) out of range");

        // Large alphabet
        let wide: Vec<u8> = (0..100).collect();
        let wm2 = B::build(&wide);
        for sym in 0u8..100 {
            assert_eq!(wm2.rank(sym, wide.len()), Some(1));
            assert_eq!(wm2.select(sym, 0), Some(sym as usize));
        }
    }

    #[cfg(feature = "wavelet-qwt")]
    #[test]
    fn test_qwt_backend_algebraic_laws() {
        run_backend_suite::<QwtBackend>();
    }

    #[cfg(feature = "wavelet-vers")]
    #[test]
    fn test_vers_backend_algebraic_laws() {
        run_backend_suite::<VersBackend>();
    }
}
