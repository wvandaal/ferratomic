//! INV-FERR-025: Index backend interchangeability benchmark.
//!
//! Compares trait-dispatched OrdMap operations vs direct OrdMap to verify
//! the IndexBackend abstraction adds no measurable overhead.
//!
//! Two benchmarks per input size:
//! - `trait_dispatched`: calls `IndexBackend::backend_get` on the EAVT
//!   index obtained from `store.indexes().unwrap().eavt()`.
//! - `direct_ordmap`: calls `OrdMap::get` on an identically-populated
//!   bare `im::OrdMap<EavtKey, Datom>`.
//!
//! If the trait abstraction is zero-cost (as expected for monomorphized
//! generics), both benchmarks should produce statistically identical
//! results at each size.

use std::{collections::BTreeSet, sync::Arc};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_db::{
    indexes::{EavtKey, IndexBackend},
    store::Store,
};
use im::OrdMap;

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

/// Build a bare `OrdMap<EavtKey, Datom>` with the same data the store would
/// hold, bypassing the `IndexBackend` trait entirely.
fn build_direct_ordmap(count: usize) -> OrdMap<EavtKey, Datom> {
    let mut map = OrdMap::new();
    for i in 0..count {
        let datom = doc_datom(i);
        let key = EavtKey::from_datom(&datom);
        map.insert(key, datom);
    }
    map
}

fn bench_index_backend(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_025_index_backend");

    for &size in &[1_000usize, 10_000] {
        let store = build_store(size);
        let direct = build_direct_ordmap(size);
        let key = lookup_key(size / 2);

        group.throughput(Throughput::Elements(1));

        // Trait-dispatched lookup via IndexBackend::backend_get
        group.bench_with_input(
            BenchmarkId::new("trait_dispatched", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    let datom = store.indexes().unwrap().eavt().backend_get(black_box(&key));
                    assert!(
                        datom.is_some(),
                        "INV-FERR-025: trait-dispatched lookup must find key"
                    );
                    black_box(datom);
                });
            },
        );

        // Direct OrdMap::get — bypasses the trait entirely
        group.bench_with_input(
            BenchmarkId::new("direct_ordmap", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    let datom = direct.get(black_box(&key));
                    assert!(
                        datom.is_some(),
                        "INV-FERR-025: direct OrdMap lookup must find key"
                    );
                    black_box(datom);
                });
            },
        );
    }

    group.finish();
}

/// PERF-3: Hard assertion that trait-dispatched lookup overhead < 50%
/// compared to direct `OrdMap::get`.
///
/// Unlike the Criterion benchmarks above (which measure and report but do
/// not fail), this function performs wall-clock timing with enough
/// iterations to be stable and panics if the trait path is slower than
/// 1.5x the direct path. This provides a CI-enforceable regression gate.
///
/// Registered as a Criterion benchmark so it runs during `cargo bench`.
/// The assertion fires inside the benchmark body on the first iteration.
fn bench_perf3_overhead_assertion(c: &mut Criterion) {
    use std::time::Instant;

    let mut group = c.benchmark_group("perf3_overhead_assertion");

    const DATOM_COUNT: usize = 1_000;
    const LOOKUP_ITERATIONS: usize = 10_000;
    // 1.5x generous bound to avoid flaky CI on loaded machines
    const MAX_OVERHEAD_RATIO: f64 = 1.5;

    let store = build_store(DATOM_COUNT);
    let direct = build_direct_ordmap(DATOM_COUNT);
    let key = lookup_key(DATOM_COUNT / 2);

    // --- Wall-clock comparison with hard assertion ---
    // Warm up both paths first to avoid cold-cache bias.
    for _ in 0..1_000 {
        black_box(store.indexes().unwrap().eavt().backend_get(black_box(&key)));
        black_box(direct.get(black_box(&key)));
    }

    let trait_start = Instant::now();
    for _ in 0..LOOKUP_ITERATIONS {
        black_box(store.indexes().unwrap().eavt().backend_get(black_box(&key)));
    }
    let trait_elapsed = trait_start.elapsed();

    let direct_start = Instant::now();
    for _ in 0..LOOKUP_ITERATIONS {
        black_box(direct.get(black_box(&key)));
    }
    let direct_elapsed = direct_start.elapsed();

    let ratio = trait_elapsed.as_nanos() as f64 / direct_elapsed.as_nanos().max(1) as f64;

    assert!(
        ratio < MAX_OVERHEAD_RATIO,
        "PERF-3 / INV-FERR-025: trait-dispatched lookup is {ratio:.2}x \
         slower than direct OrdMap::get (limit: {MAX_OVERHEAD_RATIO}x). \
         trait={trait_elapsed:?}, direct={direct_elapsed:?}, \
         iterations={LOOKUP_ITERATIONS}"
    );

    // Also register a Criterion benchmark so the comparison appears in
    // the HTML report alongside the existing groups.
    group.throughput(Throughput::Elements(1));
    group.bench_function("trait_vs_direct_ratio", |b| {
        b.iter(|| {
            black_box(store.indexes().unwrap().eavt().backend_get(black_box(&key)));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_index_backend, bench_perf3_overhead_assertion);
criterion_main!(benches);
