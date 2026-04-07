//! Bayesian confidence quantification for verification results (ADR-FERR-012).
//!
//! NEG-FERR-006: No phase gate closure without per-invariant confidence bounds.
//!
//! For `n` passes and `k` failures with a uniform Beta(1,1) prior, the posterior
//! is Beta(1 + n - k, 1 + k). The 95% credible interval lower bound quantifies
//! minimum confidence. Gate threshold: lower bound >= 0.999.

/// Gate decision for a single invariant's confidence bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    /// Lower bound meets or exceeds the threshold (0.999).
    Pass,
    /// Lower bound is below the threshold.
    Fail,
}

/// Per-invariant confidence report (ADR-FERR-012, NEG-FERR-006).
#[derive(Debug, Clone)]
pub struct ConfidenceReport {
    /// Invariant identifier (e.g. "INV-FERR-001").
    pub invariant_id: String,
    /// Number of passing test cases.
    pub n_pass: usize,
    /// Number of failing test cases.
    pub n_fail: usize,
    /// Beta distribution alpha parameter (prior + successes).
    pub alpha: f64,
    /// Beta distribution beta parameter (prior + failures).
    pub beta: f64,
    /// 95% credible interval lower bound.
    pub lower_bound_95: f64,
    /// Gate decision based on threshold.
    pub gate_decision: GateDecision,
}

/// Gate threshold: minimum lower bound for a passing gate decision.
pub const GATE_THRESHOLD: f64 = 0.999;

/// Minimum proptest cases required for >99.97% Bayesian confidence (ADR-FERR-012).
pub const MIN_CASES_FOR_CONFIDENCE: usize = 10_000;

/// ADR-FERR-012: Emit a warning to stderr if `cases` is below the 10,000
/// minimum required for >99.97% Bayesian confidence.
///
/// Call this at the start of any confidence report or gate calculation to
/// alert operators that the result is statistically insufficient.
/// Returns `true` if the case count is sufficient, `false` if a warning was emitted.
#[must_use]
pub fn check_case_count_sufficient(cases: usize) -> bool {
    if cases < MIN_CASES_FOR_CONFIDENCE {
        eprintln!(
            "WARNING [ADR-FERR-012]: {cases} proptest cases is below the \
             {MIN_CASES_FOR_CONFIDENCE} minimum for >99.97% Bayesian confidence"
        );
        false
    } else {
        true
    }
}

/// Compute the 95% credible interval lower bound for Beta(alpha, beta).
///
/// For k=0 failures, uses the closed-form: `1 - significance^(1/n)`
/// where significance = 0.05 for a 95% interval.
///
/// For k>0 failures, uses a Wald-type normal approximation to the Beta
/// posterior: `p_hat - z * sqrt(p_hat * (1 - p_hat) / n)` where z = 1.96
/// and `p_hat = alpha/(alpha+beta)` is the posterior mean (not the classical
/// sample proportion).
///
/// ADR-FERR-012: returns (lower_bound, upper_bound).
#[must_use]
pub fn compute_beta_posterior(
    n_pass: usize,
    n_fail: usize,
    prior_alpha: f64,
    prior_beta: f64,
) -> (f64, f64) {
    let alpha = prior_alpha + n_pass as f64;
    let beta = prior_beta + n_fail as f64;
    let n = alpha + beta - 2.0; // total observations

    if n <= 0.0 {
        return (0.0, 1.0);
    }

    if n_fail == 0 {
        // Closed-form for k=0: Beta(n+1, 1) has quantile Q(p) = p^(1/alpha).
        // 95% lower bound = 0.05^(1/(n+1)).
        let lower = 0.05_f64.powf(1.0 / (n + 1.0));
        // Upper bound is always 1.0 when k=0 (no failures observed)
        (lower, 1.0)
    } else {
        // Wald-type normal approximation to the Beta posterior.
        // Uses posterior mean p_hat = alpha/(alpha+beta) rather than
        // the classical sample proportion.
        let p_hat = alpha / (alpha + beta);
        let std_err = (p_hat * (1.0 - p_hat) / (alpha + beta)).sqrt();
        let z = 1.96; // 95% CI
        let lower = (p_hat - z * std_err).max(0.0);
        let upper = (p_hat + z * std_err).min(1.0);
        (lower, upper)
    }
}

/// Generate confidence reports for a set of invariant results.
///
/// Each entry is `(invariant_id, n_pass, n_fail)`. Uses a uniform
/// Beta(1,1) prior per ADR-FERR-012.
///
/// ADR-FERR-012: Emits a WARNING to stderr if the total case count for any
/// invariant is below [`MIN_CASES_FOR_CONFIDENCE`] (10,000).
#[must_use]
pub fn generate_confidence_report(results: &[(String, usize, usize)]) -> Vec<ConfidenceReport> {
    // ADR-FERR-012: warn once per report if any invariant has insufficient cases.
    for (id, n_pass, n_fail) in results {
        let total = n_pass + n_fail;
        if total < MIN_CASES_FOR_CONFIDENCE {
            eprintln!(
                "WARNING [ADR-FERR-012]: invariant {id} has {total} cases, \
                 below the {MIN_CASES_FOR_CONFIDENCE} minimum for >99.97% Bayesian confidence"
            );
        }
    }

    results
        .iter()
        .map(|(id, n_pass, n_fail)| {
            let (lower, _upper) = compute_beta_posterior(*n_pass, *n_fail, 1.0, 1.0);
            let alpha = 1.0 + *n_pass as f64;
            let beta = 1.0 + *n_fail as f64;
            let gate_decision = if lower >= GATE_THRESHOLD {
                GateDecision::Pass
            } else {
                GateDecision::Fail
            };
            ConfidenceReport {
                invariant_id: id.clone(),
                n_pass: *n_pass,
                n_fail: *n_fail,
                alpha,
                beta,
                lower_bound_95: lower,
                gate_decision,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_beta_posterior_known_values() {
        // n=10000, k=0: L = 1 - 0.05^(1/10001) ≈ 0.99970
        let (lower, _upper) = compute_beta_posterior(10_000, 0, 1.0, 1.0);
        assert!(
            (lower - 0.999_700).abs() < 0.001,
            "ADR-FERR-012: n=10000, k=0 → L={lower:.6}, expected ~0.999700"
        );
    }

    #[test]
    fn test_gate_threshold_pass() {
        // n=10000 k=0 should pass gate (L ≈ 0.9997 > 0.999)
        let (lower, _) = compute_beta_posterior(10_000, 0, 1.0, 1.0);
        assert!(
            lower >= GATE_THRESHOLD,
            "ADR-FERR-012: n=10000 k=0 should pass gate, L={lower}"
        );
    }

    #[test]
    fn test_gate_threshold_fail() {
        // n=100 k=0 should fail gate (L ≈ 0.9707 < 0.999)
        let (lower, _) = compute_beta_posterior(100, 0, 1.0, 1.0);
        assert!(
            lower < GATE_THRESHOLD,
            "ADR-FERR-012: n=100 k=0 should fail gate, L={lower}"
        );
    }

    #[test]
    fn test_report_generation() {
        let results = vec![
            ("INV-FERR-001".to_string(), 10_000, 0),
            ("INV-FERR-002".to_string(), 10_000, 0),
            ("INV-FERR-003".to_string(), 100, 0),
        ];
        let reports = generate_confidence_report(&results);
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].gate_decision, GateDecision::Pass);
        assert_eq!(reports[2].gate_decision, GateDecision::Fail);
    }

    #[test]
    fn test_report_fields_populated() {
        let results = vec![("INV-FERR-001".to_string(), 10_000, 0)];
        let reports = generate_confidence_report(&results);
        let r = &reports[0];
        assert_eq!(r.invariant_id, "INV-FERR-001");
        assert_eq!(r.n_pass, 10_000);
        assert_eq!(r.n_fail, 0);
        assert!((r.alpha - 10_001.0).abs() < f64::EPSILON);
        assert!((r.beta - 1.0).abs() < f64::EPSILON);
        assert!(r.lower_bound_95 > 0.999);
        assert_eq!(r.gate_decision, GateDecision::Pass);
    }

    #[test]
    fn test_adr_ferr_012_case_count_sufficient() {
        assert!(
            check_case_count_sufficient(10_000),
            "ADR-FERR-012: 10,000 cases must be sufficient"
        );
        assert!(
            check_case_count_sufficient(100_000),
            "ADR-FERR-012: 100,000 cases must be sufficient"
        );
        assert!(
            !check_case_count_sufficient(9_999),
            "ADR-FERR-012: 9,999 cases must be insufficient"
        );
        assert!(
            !check_case_count_sufficient(1_000),
            "ADR-FERR-012: 1,000 cases must be insufficient"
        );
        assert!(
            !check_case_count_sufficient(0),
            "ADR-FERR-012: 0 cases must be insufficient"
        );
    }

    /// ADR-FERR-012: boundary test for the PROPTEST_CASES env var threshold.
    /// Verifies the exact boundary at MIN_CASES_FOR_CONFIDENCE (10,000).
    #[test]
    fn test_adr_ferr_012_check_case_count_boundary() {
        assert!(
            check_case_count_sufficient(10_000),
            "ADR-FERR-012: exactly 10,000 cases must be sufficient"
        );
        assert!(
            !check_case_count_sufficient(9_999),
            "ADR-FERR-012: 9,999 cases must be insufficient"
        );
        assert!(
            !check_case_count_sufficient(0),
            "ADR-FERR-012: 0 cases must be insufficient"
        );
    }

    #[test]
    fn test_failures_reduce_confidence() {
        let (lower_clean, _) = compute_beta_posterior(1000, 0, 1.0, 1.0);
        let (lower_fail, _) = compute_beta_posterior(999, 1, 1.0, 1.0);
        assert!(
            lower_clean > lower_fail,
            "ADR-FERR-012: failures must reduce confidence: \
             clean={lower_clean} vs fail={lower_fail}"
        );
    }
}
