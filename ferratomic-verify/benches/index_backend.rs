//! INV-FERR-025: Index backend interchangeability benchmark.
//!
//! Compares trait-dispatched OrdMap operations vs direct OrdMap to verify
//! the IndexBackend abstraction adds no measurable overhead.
//!
//! Two benchmarks per input size:
//! - `trait_dispatched`: calls `IndexBackend::backend_get` on the EAVT
//!   index obtained from `store.indexes().eavt()`.
//! - `direct_ordmap`: calls `OrdMap::get` on an identically-populated
//!   bare `im::OrdMap<EavtKey, Datom>`.
//!
//! If the trait abstraction is zero-cost (as expected for monomorphized
//! generics), both benchmarks should produce statistically identical
//! results at each size.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratomic_core::indexes::{EavtKey, IndexBackend};
use im::OrdMap;

mod common;

/// Build a bare `OrdMap<EavtKey, Datom>` with the same data the store would
/// hold, bypassing the `IndexBackend` trait entirely.
fn build_direct_ordmap(count: usize) -> OrdMap<EavtKey, ferratom::Datom> {
    let mut map = OrdMap::new();
    for i in 0..count {
        let datom = common::doc_datom(i);
        let key = EavtKey::from_datom(&datom);
        map.insert(key, datom);
    }
    map
}

fn bench_index_backend(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_025_index_backend");

    for &size in &[1_000usize, 10_000] {
        let store = common::build_store(size);
        let direct = build_direct_ordmap(size);
        let key = common::lookup_key(size / 2);

        group.throughput(Throughput::Elements(size as u64));

        // Trait-dispatched lookup via IndexBackend::backend_get
        group.bench_with_input(
            BenchmarkId::new("trait_dispatched", size),
            &size,
            |b, &_size| {
                b.iter(|| {
                    let datom = store.indexes().eavt().backend_get(black_box(&key));
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

criterion_group!(benches, bench_index_backend);
criterion_main!(benches);
