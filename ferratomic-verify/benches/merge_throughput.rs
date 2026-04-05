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

/// Build a `Store` in `Positional` representation (cold-start path).
fn build_shifted_store(start: usize, count: usize) -> Store {
    let datoms = (start..start + count)
        .map(doc_datom)
        .collect::<BTreeSet<_>>();
    Store::from_datoms(datoms)
}

fn build_store(count: usize) -> Store {
    build_shifted_store(0, count)
}

/// Build a `Store` in `OrdMap` representation (write-active path).
///
/// Starts from genesis and transacts datoms individually, then promotes
/// to the `OrdMap` variant with `SortedVecIndexes`. The explicit
/// `promote()` call is needed because `transact_test` demotes back to
/// `Positional` after each write (INV-FERR-072).
fn build_shifted_store_ordmap(start: usize, count: usize) -> Store {
    let mut store = Store::genesis();
    for index in start..start + count {
        let tx =
            ferratomic_core::writer::Transaction::new(ferratom::AgentId::from_bytes([0xBBu8; 16]))
                .assert_datom(
                    doc_entity(index),
                    Attribute::from(DOC_ATTRIBUTE),
                    doc_value(index),
                )
                .commit_unchecked();
        store
            .transact_test(tx)
            .expect("INV-FERR-001: merge benchmark setup must succeed");
    }
    // Keep in OrdMap repr so merge exercises the mixed/OrdMap code paths.
    store.promote();
    store
}

// bd-kbck: `build_store` / `build_shifted_store` produce `Positional` repr.
// This benchmark exercises only the Positional-Positional merge path
// (`merge_positional`). The mixed-repr benchmarks below exercise
// Positional-OrdMap and OrdMap-OrdMap paths via `merge_repr`.
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

/// INV-FERR-001: benchmark merging two disjoint 10K-datom stores.
///
/// Both stores contain 10,000 datoms with no overlap (disjoint entity
/// ranges). The merged result must contain exactly 20,000 datoms,
/// confirming set union cardinality on non-overlapping inputs.
fn bench_merge_10k_x_10k(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_001_merge_10k_x_10k");

    let datom_count: usize = 10_000;
    let left = build_store(datom_count);
    let right = build_shifted_store(datom_count, datom_count);
    let expected_len = datom_count * 2;

    group.throughput(Throughput::Elements(expected_len as u64));
    group.bench_function("merge_10k_x_10k", |b| {
        b.iter(|| {
            let merged = merge(black_box(&left), black_box(&right))
                .expect("schemas compatible in benchmark");
            assert_eq!(
                merged.len(),
                expected_len,
                "INV-FERR-001: disjoint 10K merge must yield 20K datoms",
            );
            black_box(merged);
        });
    });

    group.finish();
}

/// bd-kbck: INV-FERR-001: benchmark merging Positional + OrdMap representations.
///
/// Exercises the mixed-repr merge path where one store is `Positional`
/// (cold-start) and the other is `OrdMap` (write-active). Uses 1K datoms
/// to keep CI fast while covering the code path.
///
/// Note: `build_shifted_store_ordmap` produces stores with metadata datoms
/// (`:tx/time`, `:tx/agent` per transaction), so datom counts are larger
/// than the user-datom count. Assertions verify set-union monotonicity
/// rather than exact counts.
fn bench_merge_mixed_repr(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_001_merge_mixed_repr");
    let datom_count: usize = 1_000;

    // Positional + OrdMap (mixed-repr path).
    let left_pos = build_store(datom_count);
    let right_ord = build_shifted_store_ordmap(datom_count, datom_count);
    let min_expected = left_pos.len().max(right_ord.len());

    // Throughput denominator is the sum of input sizes. The actual merged
    // output may differ due to metadata datoms, but this is a stable proxy
    // for comparing merge implementations at the same scale.
    group.throughput(Throughput::Elements(
        (left_pos.len() + right_ord.len()) as u64,
    ));
    group.bench_function("positional_ordmap_1k", |b| {
        b.iter(|| {
            let merged = merge(black_box(&left_pos), black_box(&right_ord))
                .expect("schemas compatible in benchmark");
            assert!(
                merged.len() >= min_expected,
                "INV-FERR-001: mixed-repr merge must be >= max(|A|, |B|)",
            );
            black_box(merged);
        });
    });

    // OrdMap + OrdMap path.
    let left_ord = build_shifted_store_ordmap(0, datom_count);
    let right_ord2 = build_shifted_store_ordmap(datom_count, datom_count);
    let min_expected_ord = left_ord.len().max(right_ord2.len());

    group.bench_function("ordmap_ordmap_1k", |b| {
        b.iter(|| {
            let merged = merge(black_box(&left_ord), black_box(&right_ord2))
                .expect("schemas compatible in benchmark");
            assert!(
                merged.len() >= min_expected_ord,
                "INV-FERR-001: OrdMap-OrdMap merge must be >= max(|A|, |B|)",
            );
            black_box(merged);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_merge_throughput,
    bench_merge_10k_x_10k,
    bench_merge_mixed_repr,
);
criterion_main!(benches);
