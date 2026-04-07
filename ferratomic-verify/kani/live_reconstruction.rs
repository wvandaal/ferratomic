//! Positional LIVE reconstruction Kani harnesses.
//!
//! These proofs isolate the semantic kernel of LIVE reconstruction from the
//! concrete `BitVec` representation in `PositionalStore`. The runtime still
//! uses the positional bitvector; Kani targets the same run-grouping algorithm
//! over proof-friendly keys rather than over full datom field machinery.

use ferratom::Op;
use ferratomic_db::positional::live_positions_from_sorted_run_keys_for_test;

#[cfg(not(kani))]
use super::kani;

/// INV-FERR-029: the canonical LIVE kernel matches a latest-event model.
///
/// Two value groups under one `(entity, attribute)` pair vary independently.
/// For each group, the highest-`TxId` operation decides whether the group's
/// final canonical position is LIVE.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn positional_live_kernel_matches_latest_event_model() {
    let op11 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op12 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op21 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op22 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };

    let entries = [
        ((0x41_u8, 0_u8, 1_i64), op11),
        ((0x41_u8, 0_u8, 1_i64), op12),
        ((0x41_u8, 0_u8, 2_i64), op21),
        ((0x41_u8, 0_u8, 2_i64), op22),
    ];
    let actual = live_positions_from_sorted_run_keys_for_test(&entries);
    let expects_first_group = op12 == Op::Assert;
    let expects_second_group = op22 == Op::Assert;
    let expected_len = usize::from(expects_first_group) + usize::from(expects_second_group);

    assert_eq!(
        actual.len(),
        expected_len,
        "INV-FERR-029: positional LIVE kernel must emit exactly the live group tails"
    );
    if expects_first_group {
        assert_eq!(
            actual[0], 1,
            "INV-FERR-029: first value group's live tail must be canonical position 1"
        );
    }
    if expects_second_group {
        let index = usize::from(expects_first_group);
        assert_eq!(
            actual[index], 3,
            "INV-FERR-029: second value group's live tail must be canonical position 3"
        );
    }
}

/// INV-FERR-029: the canonical LIVE kernel preserves entity boundaries.
///
/// Two entities share the same attribute and value; the kernel must decide
/// liveness independently for each entity's canonical group.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn positional_live_kernel_respects_entity_boundaries() {
    let op11 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op12 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op21 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };
    let op22 = if kani::any::<bool>() {
        Op::Assert
    } else {
        Op::Retract
    };

    let entries = [
        ((0x51_u8, 0_u8, 7_i64), op11),
        ((0x51_u8, 0_u8, 7_i64), op12),
        ((0x52_u8, 0_u8, 7_i64), op21),
        ((0x52_u8, 0_u8, 7_i64), op22),
    ];
    let actual = live_positions_from_sorted_run_keys_for_test(&entries);
    let expects_first_entity = op12 == Op::Assert;
    let expects_second_entity = op22 == Op::Assert;
    let expected_len = usize::from(expects_first_entity) + usize::from(expects_second_entity);

    assert_eq!(
        actual.len(),
        expected_len,
        "INV-FERR-029: positional LIVE kernel must keep entity runs independent"
    );
    if expects_first_entity {
        assert_eq!(
            actual[0], 1,
            "INV-FERR-029: first entity's live tail must be canonical position 1"
        );
    }
    if expects_second_entity {
        let index = usize::from(expects_first_entity);
        assert_eq!(
            actual[index], 3,
            "INV-FERR-029: second entity's live tail must be canonical position 3"
        );
    }
}
