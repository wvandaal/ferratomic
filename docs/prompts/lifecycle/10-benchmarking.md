# 10 Performance Benchmarking & Optimization

> **Purpose**: Measure, analyze, optimize. In that order. Never reverse.
> **DoF**: High for analysis, Low for measurement methodology.
> **Cognitive mode**: Empirical science (hypothesis -> measurement -> conclusion).

---

## Phase 0: Load Context

```bash
ms load spec-first-design -m --full   # Performance targets trace to INV-FERR invariants
```

---

## The One Rule

**Profile BEFORE you optimize. Optimize the measured bottleneck, not the assumed one.
Measure AFTER. Remove instrumentation.**

Every performance change follows this exact cycle:

```
1. Measure (baseline)
2. Profile (find bottleneck)
3. Hypothesize (why is it slow?)
4. Fix (minimal change to the bottleneck)
5. Measure (verify improvement)
6. Guard (CI regression gate)
```

Skipping step 1 or 5 is a defect. Optimizing without profiling (skipping step 2)
produces code that is harder to read and no faster.

---

## Tooling Setup

### Criterion.rs Benchmarks

Location: `ferratomic-core/benches/` (one file per subsystem).

```rust
// benches/store_bench.rs
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_apply_datoms(c: &mut Criterion) {
    let mut group = c.benchmark_group("store/apply_datoms");
    for size in [100, 1_000, 10_000, 100_000] {
        let datoms = generate_datoms(size);
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &datoms,
            |b, datoms| {
                b.iter(|| {
                    let store = Store::empty();
                    store.apply_datoms(datoms).unwrap()
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_apply_datoms);
criterion_main!(benches);
```

### HdrHistogram for Tail Latency

For operations where p99/p999 matters (WAL append, snapshot creation):

```rust
use hdrhistogram::Histogram;

fn measure_tail_latency(iterations: usize) -> Histogram<u64> {
    let mut hist = Histogram::<u64>::new(3).unwrap();
    for _ in 0..iterations {
        let start = std::time::Instant::now();
        // ... operation ...
        hist.record(start.elapsed().as_nanos() as u64).unwrap();
    }
    hist
}

// Report: p50, p99, p999, max
// println!("p50={}ns p99={}ns p999={}ns max={}ns",
//     hist.value_at_quantile(0.5),
//     hist.value_at_quantile(0.99),
//     hist.value_at_quantile(0.999),
//     hist.max());
```

### CI Regression Gate

Criterion supports `--save-baseline` and comparison. The CI gate:

```bash
# Save baseline (run once on known-good commit)
CARGO_TARGET_DIR=/data/cargo-target cargo bench --workspace -- --save-baseline main

# After changes: compare against baseline
CARGO_TARGET_DIR=/data/cargo-target cargo bench --workspace -- --baseline main

# Fail if any benchmark regresses > 10%
# Criterion outputs warnings for regressions; CI script parses these.
```

---

## Demonstration: Profiling Store::merge

**Scenario**: `Store::merge` is slower than expected at 100K datoms.

```bash
# Step 1: Measure baseline
CARGO_TARGET_DIR=/data/cargo-target cargo bench -p ferratomic-core \
  -- store/merge --save-baseline before

# Output:
#   store/merge/1000     time: [145.2 us 147.8 us 150.5 us]
#   store/merge/10000    time: [2.31 ms 2.38 ms 2.45 ms]
#   store/merge/100000   time: [312.5 ms 318.7 ms 325.1 ms]

# 100K takes 318ms. Target is < 50ms. 6.4x too slow.

# Step 2: Profile with flamegraph
CARGO_TARGET_DIR=/data/cargo-target cargo flamegraph \
  --bench store_bench -- --bench store/merge/100000

# Open flamegraph.svg. Read it.
# Finding: 72% of time is in BTreeMap::insert inside rebuild_aevt_index.
# The merge rebuilds AEVT from scratch instead of merging sorted iterators.

# Step 3: Hypothesize
# Hypothesis: AEVT rebuild is O(n log n) when it should be O(n).
# The two stores have sorted AEVT indexes. Merge-join is O(n), not O(n log n).
# Expected improvement: ~6x for 100K (log(100K) ~ 17, but we eliminate
# the constant factor of BTreeMap rebalancing too).

# Step 4: Fix
# In store.rs merge():
#   - was:   let mut aevt = BTreeMap::new();
#             for datom in merged_datoms { aevt.insert(...); }
#   - fixed: let aevt = merge_sorted_indexes(&self.aevt, &other.aevt);
# merge_sorted_indexes uses im::OrdMap::union_with (O(n) for sorted inputs).

# Step 5: Measure improvement
CARGO_TARGET_DIR=/data/cargo-target cargo bench -p ferratomic-core \
  -- store/merge --baseline before

# Output:
#   store/merge/1000     time: [98.1 us 100.3 us 102.7 us]  (-32%)
#   store/merge/10000    time: [1.05 ms 1.08 ms 1.11 ms]    (-55%)
#   store/merge/100000   time: [42.3 ms 43.7 ms 45.1 ms]    (-86%)

# 43.7ms < 50ms target. Improvement scales super-linearly (as expected
# from eliminating the log factor).

# Step 6: Guard
CARGO_TARGET_DIR=/data/cargo-target cargo bench --workspace -- --save-baseline main
# CI will now catch regressions against this new baseline.

# Clean up: remove any ad-hoc println! or timing instrumentation.
# The benchmark IS the permanent measurement.
```

---

## Performance Targets (from spec)

| Operation | Target | INV-FERR |
|-----------|--------|----------|
| Point read (EAVT lookup) | < 1us p99 | 025 |
| apply_datoms (1K datoms) | < 5ms p99 | 025 |
| merge (two 100K stores) | < 50ms | 025 |
| Snapshot creation | < 10us | 006 |
| WAL append (single frame) | < 100us p99 | 027 |
| Checkpoint (100K store) | < 500ms | 028 |
| Cold start (100K store) | < 2s | 029 |
| Prolly tree diff (100 changes, 100M store) | < 50ms | 047 |
| Chunk transfer (100 changes) | < 200ms LAN | 048 |

---

## Benchmark File Organization

```
ferratomic-core/benches/
  store_bench.rs       # apply_datoms, merge, index lookups
  snapshot_bench.rs    # Snapshot creation, read under contention
  wal_bench.rs         # WAL append, fsync, recovery
  checkpoint_bench.rs  # Full + incremental checkpoint
  prolly_bench.rs      # Build, read, diff, transfer
```

Each benchmark file covers one subsystem. Each benchmark group sweeps across
input sizes (100, 1K, 10K, 100K, 1M where feasible).

---

## Issue Tracking

When profiling reveals a regression or a target miss, file it:

```bash
br create \
  --title "PERF: merge 100K exceeds 50ms target (318ms measured)" \
  --type bug --priority 1 --label "phase-4b" \
  --description "$(cat <<'BODY'
**Observed**: store/merge/100000 = 318ms (target < 50ms per INV-FERR-025).
**Root cause**: AEVT index rebuilt via BTreeMap::insert instead of sorted merge.
**Acceptance**: cargo bench store/merge/100000 < 50ms.
BODY
)"

br update <id> --status in_progress   # Before optimizing
br close <id> --reason "Fixed: 43.7ms via merge_sorted_indexes"
```

After closing, check for related work:

```bash
bv --robot-triage   # May surface downstream perf issues
```

---

## Anti-Patterns

- **Optimizing without a benchmark**: You can't know if you improved anything.
- **Micro-optimizing cold paths**: Profile first. If it's < 1% of total time, skip it.
- **Leaving debug instrumentation in**: `eprintln!`, `dbg!`, ad-hoc timers. Remove them.
  The Criterion benchmark is the permanent measurement instrument.
- **Changing data structures without profiling**: "BTreeMap is slow, let's use HashMap"
  is a guess. Profile. Maybe the bottleneck is serialization, not lookup.
- **Ignoring tail latency**: p50 is marketing. p99 is engineering. p999 is reliability.
  Always report percentiles, not just mean.
