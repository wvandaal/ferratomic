use std::{collections::BTreeSet, sync::Arc};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_core::{merge::merge, store::Store};

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
    Store::from_datoms(datoms)
}

fn build_store(count: usize) -> Store {
    build_shifted_store(0, count)
}

fn bench_merge_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_001_merge_throughput");

    for datom_count in SCALE_INPUT_SIZES {
        let left = build_store(datom_count);
        let right = build_shifted_store(datom_count / 2, datom_count);
        let expected_len = datom_count + datom_count / 2;

        group.throughput(Throughput::Elements(expected_len as u64));
        group.bench_with_input(
            BenchmarkId::new("overlap_50pct", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let merged = merge(black_box(&left), black_box(&right))
                        .expect("schemas compatible in benchmark");
                    assert_eq!(
                        merged.len(),
                        expected_len,
                        "INV-FERR-001: merge throughput fixture must preserve set union cardinality",
                    );
                    black_box(merged);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_merge_throughput);
criterion_main!(benches);
