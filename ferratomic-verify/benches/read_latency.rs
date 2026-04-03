use std::{collections::BTreeSet, sync::Arc};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::{indexes::EavtKey, store::Store};

const SCALE_INPUT_SIZES: [usize; 3] = [1_000, 10_000, 100_000];

const DOC_ATTRIBUTE: &str = "db/doc";

fn doc_entity(index: usize) -> EntityId {
    EntityId::from_content(format!("entity-{index}").as_bytes())
}

fn doc_value(index: usize) -> Value {
    Value::String(Arc::from(format!("document-{index}").as_str()))
}

fn doc_datom(index: usize) -> Datom {
    Datom::new(
        doc_entity(index),
        Attribute::from(DOC_ATTRIBUTE),
        doc_value(index),
        TxId::new(index as u64 + 1, 0, 1),
        Op::Assert,
    )
}

fn build_shifted_store(start: usize, count: usize) -> Store {
    let datoms = (start..start + count)
        .map(doc_datom)
        .collect::<BTreeSet<_>>();
    let mut store = Store::from_datoms(datoms);
    // bd-h2fz: promote to OrdMap so indexes() returns Some.
    store.promote();
    store
}

fn build_store(count: usize) -> Store {
    build_shifted_store(0, count)
}

fn lookup_key(index: usize) -> EavtKey {
    EavtKey::from_datom(&doc_datom(index))
}

fn bench_read_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_027_read_latency_eavt");

    for datom_count in SCALE_INPUT_SIZES {
        let store = build_store(datom_count);
        let key = lookup_key(datom_count / 2);

        group.throughput(Throughput::Elements(datom_count as u64));
        group.bench_with_input(
            BenchmarkId::new("eavt_get", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let datom = store.indexes().unwrap().eavt().get(black_box(&key));
                    assert!(
                        datom.is_some(),
                        "INV-FERR-027: benchmark lookup key must exist"
                    );
                    black_box(datom);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_read_latency);
criterion_main!(benches);
