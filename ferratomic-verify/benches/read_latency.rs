use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ferratomic_db::indexes::IndexBackend;
use ferratomic_verify::bench_helpers::{
    build_shifted_store, doc_entity, lookup_key, SCALE_INPUT_SIZES,
};

// ---------------------------------------------------------------------------
// Read-latency-specific store builder: promotes to OrdMap for index access
// ---------------------------------------------------------------------------

fn build_store_promoted(count: usize) -> ferratomic_db::store::Store {
    let mut store = build_shifted_store(0, count);
    // bd-h2fz: promote to OrdMap so indexes() returns Some.
    store.promote();
    store
}

fn bench_read_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_027_read_latency_eavt");

    for datom_count in SCALE_INPUT_SIZES {
        let store = build_store_promoted(datom_count);
        let key = lookup_key(datom_count / 2);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("eavt_get", datom_count),
            &datom_count,
            |b, &_datom_count| {
                b.iter(|| {
                    let datom = store.indexes().unwrap().eavt().backend_get(black_box(&key));
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

/// INV-FERR-027: benchmark EAVT full scan with entity filter over a 10K-datom store.
///
/// Iterates ALL datoms in EAVT order via `eavt_datoms()` and filters those
/// whose entity ID falls within a target range. This measures O(n) full-scan
/// throughput with a filter predicate, NOT the O(log n + k) seek-based range
/// query that the spec targets. The current API does not expose a seek-based
/// range scan on `SortedVecIndexes`; that is a Phase 4b capability.
///
/// bd-aagl: Renamed from `bench_eavt_range_scan_10k` to accurately reflect
/// that this is a full scan with a filter, not a true range scan.
fn bench_eavt_full_scan_filter_10k(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_027_eavt_full_scan_filter");

    let datom_count: usize = 10_000;
    let store = build_store_promoted(datom_count);

    // Pre-compute the entity IDs that define the scan range boundaries.
    // We select entities from index 2,500..7,500 (5,000 entities out of 10,000).
    let range_start_entity = doc_entity(datom_count / 4);
    let range_end_entity = doc_entity(3 * datom_count / 4);

    // Determine canonical boundary ordering. EntityId is content-addressed
    // (BLAKE3 hash), so numeric index order does not imply EntityId order.
    // We sort the two boundary entities to get a valid [lo, hi] range.
    let (lo, hi) = if range_start_entity <= range_end_entity {
        (range_start_entity, range_end_entity)
    } else {
        (range_end_entity, range_start_entity)
    };

    // No Throughput::Elements here — the actual match count depends on
    // BLAKE3 hash distribution, not the declared index range. Criterion
    // reports wall-clock time per iteration, which is the meaningful metric.
    group.bench_function(
        BenchmarkId::new("eavt_full_scan_filter_10k", datom_count),
        |b| {
            b.iter(|| {
                let indexes = store
                    .indexes()
                    .expect("INV-FERR-027: promoted store must have indexes");
                let scanned: usize = indexes
                    .eavt_datoms()
                    .filter(|d| {
                        let eid = d.entity();
                        eid >= lo && eid <= hi
                    })
                    .count();
                assert!(
                    scanned > 0,
                    "INV-FERR-027: EAVT full scan filter must find at least one datom in range",
                );
                black_box(scanned);
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_read_latency, bench_eavt_full_scan_filter_10k);
criterion_main!(benches);
