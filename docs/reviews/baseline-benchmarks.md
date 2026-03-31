# Ferratomic Baseline Benchmarks

**Date:** 2026-03-30
**Bead:** bd-9rs0
**Commit:** 188ebdb (fix: Phase 4a hardening)

## Hardware Context

| Property | Value |
|----------|-------|
| CPU | AMD EPYC (IBPB), 16 cores, 1 socket, no SMT |
| Clock | 2794 MHz (BogoMIPS 5589.49) |
| RAM | 62 GiB (43 GiB available at bench time) |
| Kernel | 6.17.0-14-generic (x86_64, KVM) |
| L1d / L1i | 512 KiB / 1 MiB |
| Toolchain | nightly (release + LTO thin + codegen-units=1) |
| Criterion | 0.5.1, plotters backend |

## Build

Compilation: 13 min 44 s (bench profile, optimized + debuginfo, LTO thin).

All 5 benchmark suites compiled and ran to completion with zero failures.

---

## 1. Cold Start Recovery (INV-FERR-028)

Measures checkpoint + WAL replay from disk. 10 samples per size.

| Datom count | Time (low) | Time (median) | Time (high) | Throughput (median) |
|-------------|-----------|---------------|------------|---------------------|
| 1,000 | 10.733 ms | 16.079 ms | 24.785 ms | 62.2 Kelem/s |
| 10,000 | 159.42 ms | 172.22 ms | 185.34 ms | 58.1 Kelem/s |
| 100,000 | 3.482 s | 3.843 s | 4.311 s | 26.0 Kelem/s |

**Observations:**
- Near-linear scaling from 1k to 10k (16x datoms, 10.7x time).
- 100k shows super-linear growth (100x datoms, 239x time), likely due to
  checkpoint deserialization dominating at scale.
- 1 high-severe outlier at 100k (IO variance on KVM).

---

## 2. Merge Throughput (INV-FERR-001)

G-Set CRDT merge of two stores with 50% overlap. 100 samples per size.

| Datom count | Time (low) | Time (median) | Time (high) | Throughput (median) |
|-------------|-----------|---------------|------------|---------------------|
| 1,000 | 15.567 ms | 18.425 ms | 21.648 ms | 81.4 Kelem/s |
| 10,000 | 335.67 ms | 368.42 ms | 405.44 ms | 40.7 Kelem/s |
| 100,000 | 5.312 s | 5.551 s | 5.803 s | 27.0 Kelem/s |

**Observations:**
- Throughput degrades from 81.4 to 27.0 Kelem/s as size grows (expected:
  im::OrdSet union is O(n log n)).
- 100k merge at ~5.5 s with 150k resulting datoms is adequate for batch
  replication but would need sharding for interactive merge of large stores.
- Outlier rate 2-13% across sizes (KVM jitter).

---

## 3. Read Latency -- EAVT Index Lookup (INV-FERR-027)

Single-key `eavt.get()` on pre-built store. 100 samples per size.

| Datom count | Time (low) | Time (median) | Time (high) | Throughput (median) |
|-------------|-----------|---------------|------------|---------------------|
| 1,000 | 195.34 ns | 207.08 ns | 220.64 ns | 4.83 Gelem/s |
| 10,000 | 258.35 ns | 282.40 ns | 313.77 ns | 35.4 Gelem/s |
| 100,000 | 319.36 ns | 336.56 ns | 354.63 ns | 297.1 Gelem/s |

**Observations:**
- Sub-microsecond lookups at all scales. O(log n) scaling confirmed:
  10x more datoms adds ~75 ns (roughly one tree level).
- Throughput numbers are per-element basis normalized by store size;
  the raw per-lookup cost is the time column.
- Excellent cache behavior: 100k datoms still under 355 ns.

---

## 4. Snapshot Creation (INV-FERR-006)

Snapshot acquisition latency, both uncontended and under 4 concurrent readers.
10 samples per size.

| Datom count | Mode | Time (low) | Time (median) | Time (high) |
|-------------|------|-----------|---------------|------------|
| 1,000 | uncontended | 28.990 ns | 31.933 ns | 34.871 ns |
| 1,000 | 4 readers | 211.65 ns | 233.14 ns | 260.42 ns |
| 10,000 | uncontended | 31.013 ns | 34.520 ns | 37.452 ns |
| 10,000 | 4 readers | 206.41 ns | 236.22 ns | 258.01 ns |
| 100,000 | uncontended | 32.185 ns | 37.282 ns | 41.943 ns |
| 100,000 | 4 readers | 178.91 ns | 225.42 ns | 296.21 ns |

**Observations:**
- Uncontended snapshots are O(1) as expected: ~32-37 ns regardless of
  store size (Arc clone).
- Under 4-reader contention, cost rises to ~225-236 ns (atomic contention).
  Still sub-microsecond.
- Store size has zero effect on snapshot cost, confirming structural sharing
  via im::OrdMap.

---

## 5. Write Amplification (INV-FERR-026)

Measures WAL bytes written / logical user bytes. 10 samples per size.

| Tx count | Time (low) | Time (median) | Time (high) | Throughput (median) |
|----------|-----------|---------------|------------|---------------------|
| 1,000 | 302.93 ms | 330.27 ms | 360.70 ms | 301.3 KiB/s |
| 10,000 | 9.492 s | 10.683 s | 12.203 s | 94.1 KiB/s |

**Observations:**
- The INV-FERR-026 soft threshold (WA >= 10x) was NOT triggered at either
  size, indicating acceptable write amplification.
- Throughput drops at 10k transactions (94 KiB/s vs 301 KiB/s) -- the WAL
  fsync overhead amortizes poorly over many small single-datom transactions.
- This benchmark uses per-datom transactions (worst case); batched writes
  would show significantly better amplification ratios.

---

## Summary

| Benchmark | Key metric | Status |
|-----------|-----------|--------|
| Cold start (INV-FERR-028) | 26-62 Kelem/s recovery | PASS |
| Merge throughput (INV-FERR-001) | 27-81 Kelem/s union | PASS |
| Read latency (INV-FERR-027) | 207-337 ns per lookup | PASS |
| Snapshot creation (INV-FERR-006) | 32-37 ns uncontended | PASS |
| Write amplification (INV-FERR-026) | < 10x threshold | PASS |

All 5 benchmark suites compiled and executed successfully. No failures.
No INV-FERR threshold violations detected.

### Notes

- Gnuplot not installed; HTML reports use plotters backend. Criterion HTML
  reports are available at `/data/cargo-target/criterion/`.
- The `common.rs` bench binary correctly runs 0 tests (it is a shared module,
  not a benchmark entry point).
- 63 unit tests from Stateright models were skipped (all `#[ignore]`) -- these
  are model-checker tests, not benchmarks.
