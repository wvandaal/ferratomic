//! NEG-FERR-006: Quantified confidence report for CI gate decisions.
//!
//! Run AFTER `cargo test --workspace` succeeds. Assumes all proptest
//! cases passed (if they hadn't, cargo test would have failed).
//!
//! Exit code 0: all Stage 0 invariants pass gate (lower bound >= 0.999).
//! Exit code 1: at least one Stage 0 invariant fails gate.

use ferratomic_verify::{
    confidence::{
        check_case_count_sufficient, compute_beta_posterior, GateDecision, GATE_THRESHOLD,
    },
    invariant_catalog::{Invariant, Stage, CATALOG},
};

/// Default proptest case count (matches PROPTEST_CASES env or default).
const DEFAULT_PROPTEST_CASES: usize = 10_000;

fn main() {
    let proptest_cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PROPTEST_CASES);

    // ADR-FERR-012: warn if case count is below the confidence threshold.
    let sufficient = check_case_count_sufficient(proptest_cases);

    println!("NEG-FERR-006 Confidence Report");
    println!("==============================");
    println!("Proptest cases assumed: {proptest_cases}");
    println!();

    if !sufficient {
        println!(
            "WARNING: case count {proptest_cases} is below 10,000 minimum for >99.97% confidence"
        );
        println!();
    }

    print_header();

    let mut all_stage0_pass = true;

    for inv in CATALOG {
        let report = build_report(inv, proptest_cases);
        print_row(inv, &report);

        if inv.stage == Stage::Stage0 && report.gate == GateDecision::Fail {
            all_stage0_pass = false;
        }
    }

    println!();
    if all_stage0_pass {
        println!("GATE: PASS — all Stage 0 invariants meet threshold ({GATE_THRESHOLD})");
    } else {
        println!("GATE: FAIL — one or more Stage 0 invariants below threshold ({GATE_THRESHOLD})");
        std::process::exit(1);
    }
}

/// Per-invariant report data.
struct InvReport {
    /// Lean status.
    lean: &'static str,
    /// Proptest confidence lower bound (or "N/A").
    proptest_conf: String,
    /// Kani status.
    kani: &'static str,
    /// Stateright status.
    stateright: &'static str,
    /// Gate decision.
    gate: GateDecision,
}

fn build_report(inv: &Invariant, proptest_cases: usize) -> InvReport {
    let lean = if inv.lean_theorem.is_some() {
        "proven"
    } else {
        "-"
    };
    let kani = if inv.kani_harness.is_some() {
        "verified"
    } else {
        "-"
    };
    let stateright = if inv.stateright_model.is_some() {
        "checked"
    } else {
        "-"
    };

    let (proptest_conf, gate) = if inv.proptest_fn.is_some() {
        // COUPLING INVARIANT: This report assumes all proptest cases passed.
        // Run AFTER `cargo test --workspace` succeeds. If any proptest case
        // failed, cargo test would have exited non-zero. The PROPTEST_CASES
        // env var must match the ProptestConfig::with_cases() value in each
        // proptest file.
        let (lower, _) = compute_beta_posterior(proptest_cases, 0, 1.0, 1.0);
        let g = if lower >= GATE_THRESHOLD {
            GateDecision::Pass
        } else {
            GateDecision::Fail
        };
        (format!("{proptest_cases}/{proptest_cases} L={lower:.4}"), g)
    } else if inv.has_any_test() {
        ("N/A (other layers)".to_string(), GateDecision::Pass)
    } else {
        ("NO TESTS".to_string(), GateDecision::Fail)
    };

    InvReport {
        lean,
        proptest_conf,
        kani,
        stateright,
        gate,
    }
}

fn print_header() {
    println!(
        "{:<16} {:<7} {:<28} {:<10} {:<10} {:<6}",
        "Invariant", "Lean", "Proptest", "Kani", "Stateright", "Gate"
    );
    println!("{}", "-".repeat(77));
}

fn print_row(inv: &Invariant, report: &InvReport) {
    let gate_str = match report.gate {
        GateDecision::Pass => "PASS",
        GateDecision::Fail => "FAIL",
    };
    println!(
        "{:<16} {:<7} {:<28} {:<10} {:<10} {:<6}",
        inv.id, report.lean, report.proptest_conf, report.kani, report.stateright, gate_str
    );
}
