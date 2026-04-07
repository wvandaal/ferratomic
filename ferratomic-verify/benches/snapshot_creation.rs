use std::{
    collections::BTreeSet,
    hint::black_box as hint_black_box,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};
use ferratomic_db::{db::Database, store::Store};

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

fn build_database(count: usize) -> Database {
    Database::from_store(build_store(count))
}

fn bench_snapshot_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("inv_ferr_006_snapshot_creation");
    group.sample_size(10);

    for datom_count in SCALE_INPUT_SIZES {
        let db = Arc::new(build_database(datom_count));
        bench_uncontended(&mut group, Arc::clone(&db), datom_count);
        bench_under_read_load(&mut group, db, datom_count, 4);
    }

    group.finish();
}

fn bench_uncontended(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    db: Arc<Database>,
    datom_count: usize,
) {
    group.bench_with_input(
        BenchmarkId::new("uncontended", datom_count),
        &datom_count,
        |b, &_datom_count| {
            b.iter(|| {
                let snapshot = db.snapshot();
                black_box(snapshot.epoch());
            });
        },
    );
}

fn bench_under_read_load(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    db: Arc<Database>,
    datom_count: usize,
    readers: usize,
) {
    group.bench_with_input(
        BenchmarkId::new(format!("{readers}_readers"), datom_count),
        &datom_count,
        |b, &_datom_count| {
            let stop = Arc::new(AtomicBool::new(false));
            let handles = spawn_snapshot_readers(Arc::clone(&db), Arc::clone(&stop), readers);

            b.iter(|| {
                let snapshot = db.snapshot();
                black_box(snapshot.epoch());
            });

            stop.store(true, Ordering::Relaxed);
            for handle in handles {
                handle.join().expect("snapshot reader thread");
            }
        },
    );
}

fn spawn_snapshot_readers(
    db: Arc<Database>,
    stop: Arc<AtomicBool>,
    readers: usize,
) -> Vec<thread::JoinHandle<()>> {
    (0..readers)
        .map(|_| {
            let db = Arc::clone(&db);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let snapshot = db.snapshot();
                    hint_black_box(snapshot.epoch());
                }
            })
        })
        .collect()
}

criterion_group!(benches, bench_snapshot_creation);
criterion_main!(benches);
