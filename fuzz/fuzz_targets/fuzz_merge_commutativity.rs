//! Fuzz target: CRDT merge commutativity (differential oracle).
//!
//! INV-FERR-001: `merge(A, B) == merge(B, A)` for all stores A, B.
//! This harness uses structure-aware fuzzing to generate two arbitrary
//! stores and verifies the commutative property holds.

#![no_main]
use libfuzzer_sys::fuzz_target;

use arbitrary::Arbitrary;
use std::collections::BTreeSet;

use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::{merge::merge, store::Store};

/// A small datom for structure-aware fuzzing.
/// Generates valid datoms with constrained field space
/// to maximize coverage of merge logic.
#[derive(Debug, Arbitrary)]
struct FuzzDatom {
    entity_seed: [u8; 4],
    attr_idx: u8,
    value_long: i64,
    tx_physical: u16,
    is_retract: bool,
}

impl FuzzDatom {
    fn to_datom(&self) -> Datom {
        let entity = EntityId::from_content(&self.entity_seed);
        // 4 attributes to keep the space small but non-trivial
        let attr = match self.attr_idx % 4 {
            0 => Attribute::from("db/doc"),
            1 => Attribute::from("db/ident"),
            2 => Attribute::from("tx/provenance"),
            _ => Attribute::from("tx/rationale"),
        };
        let value = Value::Long(self.value_long);
        let tx = TxId::new(u64::from(self.tx_physical), 0, 0);
        let op = if self.is_retract {
            Op::Retract
        } else {
            Op::Assert
        };
        Datom::new(entity, attr, value, tx, op)
    }
}

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    datoms_a: Vec<FuzzDatom>,
    datoms_b: Vec<FuzzDatom>,
}

fuzz_target!(|input: FuzzInput| {
    // Guard: keep stores small for speed.
    if input.datoms_a.len() > 50 || input.datoms_b.len() > 50 {
        return;
    }

    let set_a: BTreeSet<Datom> = input.datoms_a.iter().map(FuzzDatom::to_datom).collect();
    let set_b: BTreeSet<Datom> = input.datoms_b.iter().map(FuzzDatom::to_datom).collect();

    let store_a = Store::from_datoms(set_a);
    let store_b = Store::from_datoms(set_b);

    // INV-FERR-001: commutativity
    let ab = merge(&store_a, &store_b).expect("merge(A,B) must succeed");
    let ba = merge(&store_b, &store_a).expect("merge(B,A) must succeed");

    assert_eq!(
        ab.datom_set(),
        ba.datom_set(),
        "INV-FERR-001 VIOLATION: merge(A,B).datoms != merge(B,A).datoms"
    );
    assert_eq!(
        ab.epoch(),
        ba.epoch(),
        "INV-FERR-001 VIOLATION: merge(A,B).epoch != merge(B,A).epoch"
    );

    // INV-FERR-003: idempotency
    let aa = merge(&store_a, &store_a).expect("merge(A,A) must succeed");
    assert_eq!(
        aa.datom_set(),
        store_a.datom_set(),
        "INV-FERR-003 VIOLATION: merge(A,A).datoms != A.datoms"
    );
});
