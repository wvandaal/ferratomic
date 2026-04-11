//! Wavelet matrix library validation (bd-jolx).
//!
//! Smoke tests for `qwt` (primary candidate) and `sucds` (alternative)
//! to validate rank/select correctness, API ergonomics, and basic
//! performance characteristics before committing to a library choice.
//!
//! These tests exercise the exact operations needed by the Ferratomic
//! wavelet matrix backend (spec/09 §Wavelet):
//! - Construction from a symbol sequence
//! - `rank(symbol, position)`: count occurrences of symbol in `[0..position)`
//! - `select(symbol, occurrence)`: position of the k-th occurrence of symbol
//! - `get(position)`: retrieve the symbol at a given position

// =========================================================================
// qwt — Quad Wavelet Tree (primary candidate, per session 026 / bd-obo8)
// =========================================================================

mod qwt_tests {
    use qwt::{AccessUnsigned, HQWT256Pfs, RankUnsigned, SelectUnsigned};

    /// Build using the recommended `HQWT256Pfs` type alias (Huffman-shaped
    /// quad wavelet tree with 256-bit superblocks and prefetch support).
    /// This is the type recommended by session 026 / bd-obo8.
    fn build_qwt(data: &[u8]) -> HQWT256Pfs<u8> {
        HQWT256Pfs::from(data.to_vec())
    }

    #[test]
    fn test_qwt_construction_and_access() {
        let data: Vec<u8> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let qwt = build_qwt(&data);

        for (i, &expected) in data.iter().enumerate() {
            let actual = qwt.get(i);
            assert_eq!(actual, Some(expected), "qwt get({i}) should be {expected}");
        }
    }

    #[test]
    fn test_qwt_rank() {
        let data: Vec<u8> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let qwt = build_qwt(&data);

        // rank(symbol, position) = count of symbol in [0..position)
        // Symbol 1 appears at positions 1, 3
        assert_eq!(qwt.rank(1, 0), Some(0), "rank(1, 0): no 1s before pos 0");
        assert_eq!(qwt.rank(1, 2), Some(1), "rank(1, 2): one 1 in [0..2)");
        assert_eq!(qwt.rank(1, 4), Some(2), "rank(1, 4): two 1s in [0..4)");
        assert_eq!(
            qwt.rank(1, data.len()),
            Some(2),
            "rank(1, len): two 1s total"
        );

        // Symbol 5 appears at positions 4, 8, 10
        assert_eq!(qwt.rank(5, 5), Some(1), "rank(5, 5): one 5 in [0..5)");
        assert_eq!(
            qwt.rank(5, data.len()),
            Some(3),
            "rank(5, len): three 5s total"
        );

        // Symbol 7 never appears — qwt returns None for out-of-alphabet symbols
        assert_eq!(
            qwt.rank(7, data.len()),
            None,
            "rank(7, len): not in alphabet"
        );
    }

    #[test]
    fn test_qwt_select() {
        let data: Vec<u8> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let qwt = build_qwt(&data);

        // select(symbol, k) = position of the (k+1)-th occurrence
        // Symbol 1 at positions 1, 3
        assert_eq!(qwt.select(1, 0), Some(1), "select(1, 0): first 1 at pos 1");
        assert_eq!(qwt.select(1, 1), Some(3), "select(1, 1): second 1 at pos 3");
        assert_eq!(qwt.select(1, 2), None, "select(1, 2): no third 1");

        // Symbol 5 at positions 4, 8, 10
        assert_eq!(qwt.select(5, 0), Some(4), "select(5, 0): first 5 at pos 4");
        assert_eq!(
            qwt.select(5, 2),
            Some(10),
            "select(5, 2): third 5 at pos 10"
        );
    }

    #[test]
    fn test_qwt_empty() {
        let qwt = build_qwt(&[]);
        // qwt returns None for empty sequences (no alphabet defined)
        assert_eq!(qwt.rank(0, 0), None);
        assert_eq!(qwt.select(0, 0), None);
        assert_eq!(qwt.get(0), None);
    }

    #[test]
    fn test_qwt_single_element() {
        let qwt = build_qwt(&[42]);
        assert_eq!(qwt.get(0), Some(42));
        assert_eq!(qwt.rank(42, 1), Some(1));
        assert_eq!(qwt.select(42, 0), Some(0));
    }

    #[test]
    fn test_qwt_large_alphabet() {
        // Simulate per-chunk entity encoding with alphabet size ~100
        let data: Vec<u8> = (0..100).collect();
        let qwt = build_qwt(&data);

        for sym in 0u8..100 {
            assert_eq!(
                qwt.rank(sym, data.len()),
                Some(1),
                "rank({sym}, len) should be 1"
            );
            assert_eq!(
                qwt.select(sym, 0),
                Some(sym as usize),
                "select({sym}, 0) should be {sym}"
            );
        }
    }

    #[test]
    fn test_qwt_rank_select_inverse() {
        // Spec/09 §Wavelet rank/select law L3: select(c, rank(c, i)) <= i
        let data: Vec<u8> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let qwt = build_qwt(&data);

        for pos in 0..data.len() {
            let sym = qwt.get(pos).expect("valid position");
            let r = qwt.rank(sym, pos + 1).expect("valid rank");
            if r > 0 {
                let s = qwt.select(sym, r - 1).expect("valid select");
                assert_eq!(
                    s, pos,
                    "L3: select({sym}, rank({sym}, {pos}+1)-1) should equal {pos}"
                );
            }
        }
    }

    #[test]
    fn test_qwt_1k_scale() {
        // 1K entries with skewed distribution (realistic datom encoding)
        let data: Vec<u8> = (0u32..1000).map(|i| (i % 50) as u8).collect();
        let qwt = build_qwt(&data);

        // Each of 50 symbols appears 20 times
        for sym in 0u8..50 {
            assert_eq!(
                qwt.rank(sym, data.len()),
                Some(20),
                "1K: rank({sym}, len) should be 20"
            );
        }
    }
}

// =========================================================================
// sucds — Succinct Data Structures (alternative candidate)
// =========================================================================

mod sucds_tests {
    use sucds::{
        bit_vectors::BitVector, char_sequences::WaveletMatrix, int_vectors::CompactVector,
    };

    /// Build a `WaveletMatrix` from a slice of u64 values.
    fn build_wm(data: &[usize]) -> WaveletMatrix<BitVector> {
        let max_val = data.iter().copied().max().unwrap_or(0);
        let width = sucds::utils::needed_bits(max_val + 1);
        let mut cv = CompactVector::new(width).expect("compact vector");
        for &v in data {
            cv.push_int(v).expect("push");
        }
        WaveletMatrix::new(cv).expect("build wavelet matrix")
    }

    #[test]
    fn test_sucds_construction_and_access() {
        let data: Vec<usize> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_wm(&data);

        for (i, &expected) in data.iter().enumerate() {
            let actual = wm.access(i).expect("valid position");
            assert_eq!(actual, expected, "sucds access({i}) should be {expected}");
        }
    }

    #[test]
    fn test_sucds_rank() {
        let data: Vec<usize> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_wm(&data);

        // sucds rank: rank(position, symbol)
        assert_eq!(wm.rank(2, 1), Some(1), "rank(2, 1): one 1 in [0..2)");
        assert_eq!(wm.rank(4, 1), Some(2), "rank(4, 1): two 1s in [0..4)");
        assert_eq!(
            wm.rank(data.len(), 5),
            Some(3),
            "rank(len, 5): three 5s total"
        );
    }

    #[test]
    fn test_sucds_select() {
        let data: Vec<usize> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_wm(&data);

        // sucds select: select(occurrence, symbol)
        assert_eq!(wm.select(0, 1), Some(1), "select(0, 1): first 1 at pos 1");
        assert_eq!(wm.select(1, 1), Some(3), "select(1, 1): second 1 at pos 3");
    }

    #[test]
    fn test_sucds_rank_select_inverse() {
        let data: Vec<usize> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_wm(&data);

        for pos in 0..data.len() {
            let sym = wm.access(pos).expect("valid position");
            let r = wm.rank(pos + 1, sym).expect("valid rank");
            if r > 0 {
                let s = wm.select(r - 1, sym).expect("valid select");
                assert_eq!(
                    s, pos,
                    "L3: select(rank({sym}, {pos}+1)-1, {sym}) should equal {pos}"
                );
            }
        }
    }
}

// =========================================================================
// vers-vecs — Zero-Dep Succinct Data Structures (bd-xck9t evaluation)
// =========================================================================

mod vers_tests {
    use vers_vecs::WaveletMatrix;

    fn build_vers(data: &[u64], bits: u16) -> WaveletMatrix {
        WaveletMatrix::from_slice(data, bits)
    }

    #[test]
    fn test_vers_construction_and_access() {
        let data: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_vers(&data, 4); // 4 bits covers max value 9

        for (i, &expected) in data.iter().enumerate() {
            let actual = wm.get_u64(i);
            assert_eq!(
                actual,
                Some(expected),
                "vers get_u64({i}) should be {expected}"
            );
        }
    }

    #[test]
    fn test_vers_rank() {
        let data: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_vers(&data, 4);

        // rank_u64(position, symbol) = count of symbol in [0..position)
        assert_eq!(
            wm.rank_u64(0, 1),
            Some(0),
            "rank_u64(0, 1): no 1s before pos 0"
        );
        assert_eq!(
            wm.rank_u64(2, 1),
            Some(1),
            "rank_u64(2, 1): one 1 in [0..2)"
        );
        assert_eq!(
            wm.rank_u64(4, 1),
            Some(2),
            "rank_u64(4, 1): two 1s in [0..4)"
        );
        assert_eq!(
            wm.rank_u64(data.len(), 1),
            Some(2),
            "rank_u64(len, 1): two 1s total"
        );
        assert_eq!(
            wm.rank_u64(data.len(), 5),
            Some(3),
            "rank_u64(len, 5): three 5s total"
        );
        // Symbol 7 never appears
        assert_eq!(
            wm.rank_u64(data.len(), 7),
            Some(0),
            "rank_u64(len, 7): no 7s"
        );
    }

    #[test]
    fn test_vers_select() {
        let data: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_vers(&data, 4);

        // select_u64(occurrence, symbol)
        assert_eq!(
            wm.select_u64(0, 1),
            Some(1),
            "select_u64(0, 1): first 1 at pos 1"
        );
        assert_eq!(
            wm.select_u64(1, 1),
            Some(3),
            "select_u64(1, 1): second 1 at pos 3"
        );
        assert_eq!(wm.select_u64(2, 1), None, "select_u64(2, 1): no third 1");
        assert_eq!(
            wm.select_u64(0, 5),
            Some(4),
            "select_u64(0, 5): first 5 at pos 4"
        );
        assert_eq!(
            wm.select_u64(2, 5),
            Some(10),
            "select_u64(2, 5): third 5 at pos 10"
        );
    }

    #[test]
    fn test_vers_rank_select_inverse() {
        // Spec/09 §Wavelet law L3: select(c, rank(c, i+1)-1) == i
        let data: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_vers(&data, 4);

        for pos in 0..data.len() {
            let sym = wm.get_u64(pos).expect("valid position");
            let r = wm.rank_u64(pos + 1, sym).expect("valid rank");
            if r > 0 {
                let s = wm.select_u64(r - 1, sym).expect("valid select");
                assert_eq!(
                    s, pos,
                    "L3: select_u64(rank_u64({sym}, {pos}+1)-1, {sym}) should equal {pos}"
                );
            }
        }
    }

    #[test]
    fn test_vers_large_alphabet() {
        let data: Vec<u64> = (0..100).collect();
        let wm = build_vers(&data, 7); // 7 bits covers 0..127

        for sym in 0u64..100 {
            assert_eq!(
                wm.rank_u64(data.len(), sym),
                Some(1),
                "rank_u64({sym}, len) should be 1"
            );
            assert_eq!(
                wm.select_u64(0, sym),
                Some(sym as usize),
                "select_u64(0, {sym}) should be {sym}"
            );
        }
    }

    #[test]
    fn test_vers_1k_scale() {
        let data: Vec<u64> = (0u64..1000).map(|i| i % 50).collect();
        let wm = build_vers(&data, 6); // 6 bits covers 0..63

        for sym in 0u64..50 {
            assert_eq!(
                wm.rank_u64(data.len(), sym),
                Some(20),
                "1K: rank_u64({sym}, len) should be 20"
            );
        }
    }

    #[test]
    fn test_vers_out_of_alphabet_returns_zero() {
        // Unlike qwt which returns None for out-of-alphabet symbols,
        // vers-vecs returns Some(0) — treating them as valid but absent.
        let data: Vec<u64> = vec![1, 2, 3];
        let wm = build_vers(&data, 4); // 4 bits, alphabet up to 15

        assert_eq!(
            wm.rank_u64(data.len(), 7),
            Some(0),
            "vers: out-of-alphabet symbol returns Some(0)"
        );
    }

    #[test]
    fn test_vers_range_rank() {
        // vers-vecs unique feature: rank over arbitrary ranges
        let data: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let wm = build_vers(&data, 4);

        // rank_range_u64(2..8, 1) = count of 1s in positions [2..8)
        // Positions 2..8: [4, 1, 5, 9, 2, 6] — one 1 at position 3
        assert_eq!(
            wm.rank_range_u64(2..8, 1),
            Some(1),
            "rank_range_u64(2..8, 1): one 1 in range"
        );
    }
}
