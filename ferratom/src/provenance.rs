//! Epistemic confidence lattice for transaction provenance.
//!
//! INV-FERR-063: Total order `Hypothesized < Inferred < Derived < Observed`.
//! ADR-FERR-028: [`ProvenanceType`] enriches conflict resolution beyond pure
//! timestamp ordering.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// Epistemic confidence lattice for transaction provenance.
///
/// INV-FERR-063: Total order: `Hypothesized` < `Inferred` < `Derived` < `Observed`.
/// Weights: 0.2, 0.5, 0.8, 1.0. Composes with LWW resolution:
/// `resolve(assertions) = max_by(|a| (a.provenance_weight, a.tx_id))`.
///
/// ADR-FERR-028: [`ProvenanceType`] enriches conflict resolution beyond pure
/// timestamp ordering. An observation outranks a hypothesis regardless of
/// temporal ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProvenanceType {
    /// Evidence: none. Confidence: 0.2.
    Hypothesized,
    /// Evidence: indirect reasoning. Confidence: 0.5.
    Inferred,
    /// Evidence: computed from other facts. Confidence: 0.8.
    Derived,
    /// Evidence: direct observation. Confidence: 1.0.
    Observed,
}

impl ProvenanceType {
    /// Integer rank for total ordering.
    ///
    /// INV-FERR-063: Hypothesized(0) < Inferred(1) < Derived(2) < Observed(3).
    /// Used by `Ord` impl — no floating-point comparison needed.
    #[must_use]
    pub fn rank(&self) -> u8 {
        match self {
            Self::Hypothesized => 0,
            Self::Inferred => 1,
            Self::Derived => 2,
            Self::Observed => 3,
        }
    }

    /// Epistemic confidence weight.
    ///
    /// INV-FERR-063: Hardcoded non-NaN literals.
    /// Hypothesized → 0.2, Inferred → 0.5, Derived → 0.8, Observed → 1.0.
    #[must_use]
    pub fn confidence(&self) -> f64 {
        match self {
            Self::Hypothesized => 0.2,
            Self::Inferred => 0.5,
            Self::Derived => 0.8,
            Self::Observed => 1.0,
        }
    }

    /// Namespaced keyword representation.
    ///
    /// ADR-FERR-028: Used in wire format and human-readable serialization.
    #[must_use]
    pub fn as_keyword(&self) -> &'static str {
        match self {
            Self::Hypothesized => "provenance/hypothesized",
            Self::Inferred => "provenance/inferred",
            Self::Derived => "provenance/derived",
            Self::Observed => "provenance/observed",
        }
    }

    /// Parse a namespaced keyword back into a `ProvenanceType`.
    ///
    /// Returns `None` for unrecognized keywords.
    /// Round-trips with [`as_keyword`](Self::as_keyword).
    #[must_use]
    pub fn from_keyword(kw: &str) -> Option<Self> {
        match kw {
            "provenance/hypothesized" => Some(Self::Hypothesized),
            "provenance/inferred" => Some(Self::Inferred),
            "provenance/derived" => Some(Self::Derived),
            "provenance/observed" => Some(Self::Observed),
            _ => None,
        }
    }
}

/// INV-FERR-063: Total order via integer rank. NEG-FERR-001 compliant
/// (no `expect`, no `unwrap`, no floating-point comparison).
impl PartialOrd for ProvenanceType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// INV-FERR-063: Total order via integer rank.
impl Ord for ProvenanceType {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// Default provenance is `Observed` — the highest confidence level.
///
/// Rationale: local transactions originate from direct observation.
/// Federation and inference engines explicitly set lower levels.
impl Default for ProvenanceType {
    fn default() -> Self {
        Self::Observed
    }
}

impl std::fmt::Display for ProvenanceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_keyword())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// INV-FERR-063: All six pairwise comparisons verify total order.
    #[test]
    fn test_provenance_total_order() {
        let h = ProvenanceType::Hypothesized;
        let i = ProvenanceType::Inferred;
        let d = ProvenanceType::Derived;
        let o = ProvenanceType::Observed;

        // Strict ordering: H < I < D < O
        assert!(h < i, "Hypothesized < Inferred");
        assert!(h < d, "Hypothesized < Derived");
        assert!(h < o, "Hypothesized < Observed");
        assert!(i < d, "Inferred < Derived");
        assert!(i < o, "Inferred < Observed");
        assert!(d < o, "Derived < Observed");

        // Equality
        assert_eq!(h, h);
        assert_eq!(i, i);
        assert_eq!(d, d);
        assert_eq!(o, o);
    }

    /// INV-FERR-063: Exact confidence weights.
    #[test]
    fn test_provenance_confidence_weights() {
        assert!((ProvenanceType::Hypothesized.confidence() - 0.2).abs() < f64::EPSILON);
        assert!((ProvenanceType::Inferred.confidence() - 0.5).abs() < f64::EPSILON);
        assert!((ProvenanceType::Derived.confidence() - 0.8).abs() < f64::EPSILON);
        assert!((ProvenanceType::Observed.confidence() - 1.0).abs() < f64::EPSILON);
    }

    /// ADR-FERR-028: Keyword round-trip for all four variants.
    #[test]
    fn test_provenance_keyword_round_trip() {
        let variants = [
            ProvenanceType::Hypothesized,
            ProvenanceType::Inferred,
            ProvenanceType::Derived,
            ProvenanceType::Observed,
        ];
        for v in &variants {
            let kw = v.as_keyword();
            let parsed = ProvenanceType::from_keyword(kw);
            assert_eq!(
                parsed,
                Some(*v),
                "from_keyword(as_keyword({v:?})) must round-trip"
            );
        }
    }

    /// Unrecognized keywords return `None`.
    #[test]
    fn test_provenance_from_keyword_unknown_returns_none() {
        assert_eq!(ProvenanceType::from_keyword("provenance/unknown"), None);
        assert_eq!(ProvenanceType::from_keyword(""), None);
        assert_eq!(ProvenanceType::from_keyword("observed"), None);
        assert_eq!(ProvenanceType::from_keyword("hypothesized"), None);
    }

    /// INV-FERR-063: Rank is monotonically increasing with confidence.
    #[test]
    fn test_provenance_rank_monotone_with_confidence() {
        let variants = [
            ProvenanceType::Hypothesized,
            ProvenanceType::Inferred,
            ProvenanceType::Derived,
            ProvenanceType::Observed,
        ];
        for w in variants.windows(2) {
            assert!(
                w[0].rank() < w[1].rank(),
                "rank({:?}) < rank({:?})",
                w[0],
                w[1]
            );
            assert!(
                w[0].confidence() < w[1].confidence(),
                "confidence({:?}) < confidence({:?})",
                w[0],
                w[1]
            );
        }
    }

    /// Default provenance is `Observed`.
    #[test]
    fn test_provenance_default_is_observed() {
        assert_eq!(ProvenanceType::default(), ProvenanceType::Observed);
    }
}
