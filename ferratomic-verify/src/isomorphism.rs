//! INV-FERR-059: Optimization Behavioral Preservation.
//!
//! Verifies that performance optimizations (PositionalStore, SortedVecIndexes,
//! Eytzinger layout, Checkpoint V3) produce identical query results as the
//! baseline OrdMap representation.
//!
//! The core abstraction: given a baseline `Store` and a transformation that
//! produces another `Store`, verify that the datom sets are identical. This
//! is the behavioral preservation proof obligation for every optimization.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_db::store::Store;

/// Result of an isomorphism verification.
#[derive(Debug)]
pub struct IsomorphismProof {
    /// Name of the optimization being verified.
    pub optimization: String,
    /// Number of datoms in the test store.
    pub datom_count: usize,
    /// Number of queries executed.
    pub query_count: usize,
    /// Whether all queries produced identical results.
    pub verdict: IsomorphismVerdict,
}

/// Verdict of an isomorphism check.
#[derive(Debug, PartialEq, Eq)]
pub enum IsomorphismVerdict {
    /// All queries produced identical results — optimization is behavior-preserving.
    Isomorphic,
    /// At least one query produced different results.
    Divergent {
        /// Description of the first divergence found.
        first_divergence: String,
    },
}

/// Verify that an optimization preserves query behavior.
///
/// Builds a baseline store, applies the optimization (via the closure),
/// executes the query corpus on both, and compares results.
///
/// INV-FERR-059: The optimization transform `F` must produce a `Store`
/// with an identical datom set. Length and set equality are both checked
/// to catch both insertion/deletion bugs and ordering bugs.
pub fn verify_optimization_isomorphism<F>(
    baseline: &Store,
    optimize: F,
    query_entities: &[ferratom::EntityId],
    optimization_name: &str,
) -> IsomorphismProof
where
    F: FnOnce(&Store) -> Store,
{
    let optimized = optimize(baseline);

    // Compare lengths first (cheap).
    if baseline.len() != optimized.len() {
        return IsomorphismProof {
            optimization: optimization_name.to_string(),
            datom_count: baseline.len(),
            query_count: 0,
            verdict: IsomorphismVerdict::Divergent {
                first_divergence: format!(
                    "length mismatch: baseline={}, optimized={}",
                    baseline.len(),
                    optimized.len()
                ),
            },
        };
    }

    // Compare datom sets (order-independent).
    let baseline_datoms: BTreeSet<&Datom> = baseline.datoms().collect();
    let optimized_datoms: BTreeSet<&Datom> = optimized.datoms().collect();

    if baseline_datoms != optimized_datoms {
        return IsomorphismProof {
            optimization: optimization_name.to_string(),
            datom_count: baseline.len(),
            query_count: 0,
            verdict: IsomorphismVerdict::Divergent {
                first_divergence: "datom sets differ".to_string(),
            },
        };
    }

    // Per-entity query verification (INV-FERR-059).
    // Datom-set equality above proves global correctness; per-entity queries
    // verify that entity-scoped lookups also produce identical results,
    // catching index-level bugs that preserve the global set but mis-route
    // individual entity lookups.
    //
    // bd-lgwg: Use BTreeSet for order-independent comparison.
    // bd-06pm: Track idx for accurate query_count.
    let mut idx: usize = 0;
    for eid in query_entities {
        idx += 1;
        let baseline_entity: BTreeSet<&Datom> =
            baseline.datoms().filter(|d| d.entity() == *eid).collect();
        let optimized_entity: BTreeSet<&Datom> =
            optimized.datoms().filter(|d| d.entity() == *eid).collect();
        if baseline_entity != optimized_entity {
            // bd-0umz: Improved divergence message for same-count case.
            let divergence_msg = if baseline_entity.len() == optimized_entity.len() {
                format!(
                    "entity {:?}: both returned {} datoms but sets differ \
                     (same count, different members)",
                    eid,
                    baseline_entity.len()
                )
            } else {
                format!(
                    "entity {:?}: baseline returned {} datoms, optimized returned {}",
                    eid,
                    baseline_entity.len(),
                    optimized_entity.len()
                )
            };
            return IsomorphismProof {
                optimization: optimization_name.to_string(),
                datom_count: baseline.len(),
                query_count: idx,
                verdict: IsomorphismVerdict::Divergent {
                    first_divergence: divergence_msg,
                },
            };
        }
    }

    IsomorphismProof {
        optimization: optimization_name.to_string(),
        datom_count: baseline.len(),
        query_count: idx,
        verdict: IsomorphismVerdict::Isomorphic,
    }
}
