use std::{
    hint::black_box as hint_black_box,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ferratomic_db::db::Database;
use ferratomic_verify::bench_helpers::{build_database, SCALE_INPUT_SIZES};

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
