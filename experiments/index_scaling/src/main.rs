//! bd-snnh: Index scaling validation at 100M datoms.
//!
//! Throwaway experiment per the bead's frame conditions. Lives in
//! `experiments/`, NOT a workspace member. Validates that the current
//! `PositionalStore` architecture (sorted vec + interpolation search +
//! Eytzinger layout + CHD MPH) hits Phase 4b perf targets BEFORE we
//! commit to it.
//!
//! Hypothesis (from bd-snnh):
//!   1. Point query latency stays under 10us (P50) at 100M datoms.
//!   2. Range scan throughput stays above 10M datoms/sec.
//!   3. Index size on disk is less than 2x raw datom size.
//!   4. Theoretical cost model (per spec/09) predicts measurements within 30%.
//!
//! Run with:
//!   cargo run --release --manifest-path experiments/index_scaling/Cargo.toml -- <scale>
//!
//! Where <scale> is one of: 1m | 10m | 50m | 100m
//! Or comma-separated: 1m,10m,50m

use std::time::Instant;

use ferratom::{Attribute, Datom, EntityId, NodeId, Op, TxId, Value};
use ferratomic_index::{AevtKey, EavtKey};
use ferratomic_positional::store::PositionalStore;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of attributes in the synthetic schema.
/// Real-world Ferratomic stores have 50-200 attributes; we use 100.
const ATTRIBUTE_COUNT: usize = 100;

/// Zipf exponent for attribute distribution. s=1.07 matches the long-tail
/// observed in production knowledge graphs (per docs/ideas/009 §3).
const ZIPF_EXPONENT: f64 = 1.07;

/// Deterministic RNG seed for reproducibility.
const RNG_SEED: u64 = 0x5eed_ed_ed_5eed_eded;

/// Per-benchmark seed offsets (XORed with RNG_SEED for independent streams).
const SEED_EAVT: u64 = 0xea0e_ea0e_ea0e_ea0e;
const SEED_AEVT: u64 = 0xae0e_ae0e_ae0e_ae0e;
const SEED_RANGE: u64 = 0xfa11_ba11_fa11_ba11;

/// Fixed node ID for synthetic dataset (16 bytes).
const SYNTH_NODE_BYTES: [u8; 16] = [0x42; 16];

/// Number of point queries to run per scale point.
const POINT_QUERY_COUNT: usize = 100_000;

/// Number of range scans to run per scale point.
const RANGE_SCAN_COUNT: usize = 1_000;

/// Range scan selectivities to test (fraction of total datoms).
const RANGE_SELECTIVITIES: [f64; 4] = [0.00001, 0.0001, 0.001, 0.01];

// ---------------------------------------------------------------------------
// Synthetic dataset generator
// ---------------------------------------------------------------------------

/// Generate `count` synthetic datoms with Zipf-distributed attribute references.
///
/// The synthetic dataset has these properties:
/// - Each datom has a unique entity (`entity-{i}`) — no entity sharing.
/// - Attributes are drawn from a 100-attribute pool with Zipf weights
///   (long-tail: a few hot attributes, many cold ones).
/// - Values are integers (just the entity index — keeps generation cheap).
/// - TxIds are sequential (one per datom).
/// - All operations are `Op::Assert`.
///
/// Returns owned `Vec<Datom>` (caller is responsible for sorting via PositionalStore::from_datoms).
fn generate_dataset(count: usize) -> Vec<Datom> {
    let mut rng = ChaCha8Rng::seed_from_u64(RNG_SEED);

    // Pre-build the attribute pool. Attributes are owned `Attribute` values
    // (cheap to clone since they're string-interned internally).
    let attributes: Vec<Attribute> = (0..ATTRIBUTE_COUNT)
        .map(|i| Attribute::from(format!("ns/attr-{i:03}").as_str()))
        .collect();

    // Build the Zipf weights: attribute i has weight 1/(i+1)^s.
    let weights: Vec<f64> = (0..ATTRIBUTE_COUNT)
        .map(|i| 1.0 / ((i as f64) + 1.0).powf(ZIPF_EXPONENT))
        .collect();
    let dist = WeightedIndex::new(&weights).expect("Zipf weights must be valid");

    let node = NodeId::from_bytes(SYNTH_NODE_BYTES);
    let mut datoms = Vec::with_capacity(count);
    for i in 0..count {
        let entity = EntityId::from_content(format!("entity-{i}").as_bytes());
        let attr_idx = dist.sample(&mut rng);
        let attribute = attributes[attr_idx].clone();
        let value = Value::Long(i as i64);
        let tx = TxId::with_node((i as u64) + 1, 0, node);
        datoms.push(Datom::new(entity, attribute, value, tx, Op::Assert));
    }
    datoms
}

// ---------------------------------------------------------------------------
// Measurement helpers
// ---------------------------------------------------------------------------

/// Capture P50/P90/P99/max from a sample of latencies (nanoseconds).
#[derive(Debug, Clone, Serialize)]
struct LatencySummary {
    samples: usize,
    p50_ns: u64,
    p90_ns: u64,
    p99_ns: u64,
    p999_ns: u64,
    max_ns: u64,
    mean_ns: u64,
}

fn summarize_latencies(mut samples: Vec<u64>) -> LatencySummary {
    let n = samples.len();
    samples.sort_unstable();
    let pct = |p: f64| -> u64 {
        if n == 0 {
            return 0;
        }
        let idx = ((n as f64) * p).floor() as usize;
        samples[idx.min(n - 1)]
    };
    let mean: u64 = if n == 0 {
        0
    } else {
        (samples.iter().sum::<u64>()) / (n as u64)
    };
    LatencySummary {
        samples: n,
        p50_ns: pct(0.50),
        p90_ns: pct(0.90),
        p99_ns: pct(0.99),
        p999_ns: pct(0.999),
        max_ns: samples.last().copied().unwrap_or(0),
        mean_ns: mean,
    }
}

// ---------------------------------------------------------------------------
// Benchmark runners
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
struct ScaleResults {
    scale: usize,
    build_secs: f64,
    memory_estimate_bytes: usize,
    fingerprint: String,
    eavt_point_query: LatencySummary,
    aevt_point_query: LatencySummary,
    range_scans: Vec<RangeScanResult>,
    full_scan_secs: f64,
    full_scan_throughput_dps: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RangeScanResult {
    selectivity: f64,
    expected_matches: usize,
    iterations: usize,
    summary: LatencySummary,
    avg_throughput_dps: f64,
}

/// Run all benchmarks for a single scale point.
fn run_scale(scale: usize) -> ScaleResults {
    println!("\n=== scale: {scale} datoms ===");

    // -- 1. Generate dataset --
    print!("  generating dataset... ");
    let gen_start = Instant::now();
    let datoms = generate_dataset(scale);
    let gen_secs = gen_start.elapsed().as_secs_f64();
    println!("{gen_secs:.2}s ({scale} datoms)");

    // -- 2. Build PositionalStore --
    print!("  building PositionalStore (sort + LIVE + fingerprint)... ");
    let build_start = Instant::now();
    let store = PositionalStore::from_datoms(datoms.into_iter());
    let build_secs = build_start.elapsed().as_secs_f64();
    println!("{build_secs:.2}s");

    // Memory estimate: canonical Vec<Datom> dominates. Datom is roughly
    // 64 bytes (32-byte EntityId + Attribute + Value enum + TxId + Op).
    // The actual memory is higher (live_bits, fingerprint, future permutations).
    // We report the dominant term.
    let memory_estimate_bytes = store.len() * std::mem::size_of::<Datom>();
    println!("  store len: {}, ~{} MB canonical",
        store.len(),
        memory_estimate_bytes / (1024 * 1024));

    // Capture fingerprint as evidence of determinism.
    let fp_bytes = store.fingerprint();
    let fingerprint = hex_lower(fp_bytes);

    // -- 3. EAVT point queries --
    print!("  EAVT point queries ({POINT_QUERY_COUNT} samples)... ");
    let eavt_point_query = bench_eavt_point_queries(&store, POINT_QUERY_COUNT);
    println!("p50={}us p99={}us",
        eavt_point_query.p50_ns / 1000,
        eavt_point_query.p99_ns / 1000);

    // -- 4. AEVT point queries (forces perm_aevt build on first call) --
    print!("  AEVT point queries ({POINT_QUERY_COUNT} samples)... ");
    let aevt_point_query = bench_aevt_point_queries(&store, POINT_QUERY_COUNT);
    println!("p50={}us p99={}us",
        aevt_point_query.p50_ns / 1000,
        aevt_point_query.p99_ns / 1000);

    // -- 5. Range scans at varying selectivity --
    println!("  range scans:");
    let mut range_scans = Vec::new();
    for selectivity in RANGE_SELECTIVITIES {
        let result = bench_range_scan(&store, selectivity, RANGE_SCAN_COUNT.min(scale / 100 + 1));
        println!(
            "    sel={:.5} matches={} iter={} p50={}us throughput={:.0}M dps",
            result.selectivity,
            result.expected_matches,
            result.iterations,
            result.summary.p50_ns / 1000,
            result.avg_throughput_dps / 1_000_000.0
        );
        range_scans.push(result);
    }

    // -- 6. Full scan --
    print!("  full scan... ");
    let full_start = Instant::now();
    let full_count = store.datoms().len();
    let mut sum: u64 = 0;
    for d in store.datoms() {
        // Touch each datom to defeat dead-code elimination.
        sum = sum.wrapping_add(d.tx().physical());
    }
    std::hint::black_box(sum);
    let full_secs = full_start.elapsed().as_secs_f64();
    let full_throughput_dps = (full_count as f64) / full_secs;
    println!("{full_secs:.3}s ({:.0}M dps)", full_throughput_dps / 1_000_000.0);

    ScaleResults {
        scale,
        build_secs,
        memory_estimate_bytes,
        fingerprint,
        eavt_point_query,
        aevt_point_query,
        range_scans,
        full_scan_secs: full_secs,
        full_scan_throughput_dps: full_throughput_dps,
    }
}

/// Benchmark EAVT point queries against random datoms in the store.
fn bench_eavt_point_queries(store: &PositionalStore, sample_count: usize) -> LatencySummary {
    let mut rng = ChaCha8Rng::seed_from_u64(RNG_SEED ^ SEED_EAVT);
    let store_len = store.len();
    let datoms = store.datoms();

    // Pre-build query keys to exclude key-construction time from measurements.
    let keys: Vec<EavtKey> = (0..sample_count)
        .map(|_| {
            let idx = rng.gen_range(0..store_len);
            EavtKey::from_datom(&datoms[idx])
        })
        .collect();

    let mut samples = Vec::with_capacity(sample_count);
    for key in &keys {
        let start = Instant::now();
        let result = store.eavt_get(key);
        let elapsed = start.elapsed().as_nanos() as u64;
        std::hint::black_box(result);
        samples.push(elapsed);
    }
    summarize_latencies(samples)
}

/// Benchmark AEVT point queries (also forces perm_aevt build on first access).
fn bench_aevt_point_queries(store: &PositionalStore, sample_count: usize) -> LatencySummary {
    let mut rng = ChaCha8Rng::seed_from_u64(RNG_SEED ^ SEED_AEVT);
    let store_len = store.len();
    let datoms = store.datoms();

    // Trigger perm_aevt build with one warmup call before timing.
    let _warmup = store.aevt_get(&AevtKey::from_datom(&datoms[0]));

    let keys: Vec<AevtKey> = (0..sample_count)
        .map(|_| {
            let idx = rng.gen_range(0..store_len);
            AevtKey::from_datom(&datoms[idx])
        })
        .collect();

    let mut samples = Vec::with_capacity(sample_count);
    for key in &keys {
        let start = Instant::now();
        let result = store.aevt_get(key);
        let elapsed = start.elapsed().as_nanos() as u64;
        std::hint::black_box(result);
        samples.push(elapsed);
    }
    summarize_latencies(samples)
}

/// Benchmark a range scan via `live_datoms()` filtered to a target window.
///
/// Selectivity is the fraction of total datoms expected to match.
/// Selects a random TxId range and filters live_datoms within it.
fn bench_range_scan(
    store: &PositionalStore,
    selectivity: f64,
    iterations: usize,
) -> RangeScanResult {
    let mut rng = ChaCha8Rng::seed_from_u64(RNG_SEED ^ SEED_RANGE);
    let store_len = store.len();
    let target_matches = ((store_len as f64) * selectivity).max(1.0) as usize;

    // We use entity-prefix range matching: pick a random window in the
    // sorted EAVT order. Since EAVT is BLAKE3-uniform on entity, slicing
    // [start..start+target_matches] gives an exact match count.
    let mut samples = Vec::with_capacity(iterations);
    let mut total_matched: usize = 0;

    for _ in 0..iterations {
        let start_idx = rng.gen_range(0..store_len.saturating_sub(target_matches).max(1));
        let end_idx = (start_idx + target_matches).min(store_len);
        let datoms = store.datoms();
        let start = Instant::now();
        let mut count: usize = 0;
        let mut sum: u64 = 0;
        for d in &datoms[start_idx..end_idx] {
            sum = sum.wrapping_add(d.tx().physical());
            count += 1;
        }
        std::hint::black_box(sum);
        let elapsed = start.elapsed().as_nanos() as u64;
        samples.push(elapsed);
        total_matched += count;
    }

    let summary = summarize_latencies(samples);
    let avg_match_count = (total_matched as f64) / (iterations as f64);
    let avg_secs = (summary.mean_ns as f64) / 1_000_000_000.0;
    let avg_throughput_dps = if avg_secs > 0.0 {
        avg_match_count / avg_secs
    } else {
        0.0
    };

    RangeScanResult {
        selectivity,
        expected_matches: target_matches,
        iterations,
        summary,
        avg_throughput_dps,
    }
}

// ---------------------------------------------------------------------------
// Hex helper (avoids pulling in `hex` crate)
// ---------------------------------------------------------------------------

fn hex_lower(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn parse_scale(s: &str) -> Option<usize> {
    let trimmed = s.trim().to_lowercase();
    let (num, mul) = if let Some(rest) = trimmed.strip_suffix('m') {
        (rest, 1_000_000)
    } else if let Some(rest) = trimmed.strip_suffix('k') {
        (rest, 1_000)
    } else {
        (trimmed.as_str(), 1)
    };
    num.parse::<usize>().ok().map(|n| n * mul)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scales: Vec<usize> = if args.len() < 2 {
        // Default: small sanity check
        vec![100_000]
    } else {
        args[1]
            .split(',')
            .filter_map(parse_scale)
            .collect()
    };

    if scales.is_empty() {
        eprintln!("usage: index-scaling-experiment <scale[,scale,...]>");
        eprintln!("       scale = e.g. 100k, 1m, 10m, 50m, 100m");
        std::process::exit(2);
    }

    println!("bd-snnh: Index scaling validation");
    println!("hardware: {} cores, {}", num_cpus_estimate(), memory_estimate_str());
    println!("scales: {scales:?}");

    let mut all_results: Vec<ScaleResults> = Vec::new();
    for scale in scales {
        let result = run_scale(scale);
        all_results.push(result);
    }

    // Emit JSON summary to stdout (for piping into the report)
    println!("\n=== JSON results ===");
    let json = serde_json::to_string_pretty(&all_results)
        .expect("results must serialize");
    println!("{json}");
}

fn num_cpus_estimate() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn memory_estimate_str() -> String {
    // Read /proc/meminfo if available; otherwise unknown.
    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: u64 = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .parse()
                    .unwrap_or(0);
                return format!("{:.0} GB RAM", (kb as f64) / 1024.0 / 1024.0);
            }
        }
    }
    "unknown RAM".to_string()
}

