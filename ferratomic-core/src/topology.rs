//! Topology and replica filtering (INV-FERR-030).
//!
//! Phase 4a defines trait boundaries; Phase 4c implements
//! real topology management and selective replication.
//!
//! See spec/05-federation.md and `FERRATOMIC_ARCHITECTURE.md`.

use ferratom::{Datom, DatomFilter};

/// Filter predicate that controls which datoms a read replica stores.
///
/// INV-FERR-030: replica filter — `filter(R, d) in {accept, reject}` is
/// deterministic for a given replica `R` and datom `d`. The set of datoms
/// accepted by a replica is always a subset of the source store:
/// `accepted(R) ⊆ source`. A filter must be a pure function of the datom
/// alone — it does not depend on ordering, arrival time, or previously
/// accepted datoms. Although `&self` is available, implementations MUST
/// NOT use internal mutable state to vary the return value for the same
/// datom across calls. This determinism guarantee means that two replicas
/// with the same filter configuration converge to the same subset after
/// anti-entropy exchange (INV-FERR-022).
///
/// The trait requires `Send + Sync` because filters are evaluated during
/// merge and anti-entropy operations that may span thread boundaries.
/// `AcceptAll` is the default (full replica).
///
/// # Contract
///
/// Implementors guarantee:
/// - **Determinism**: `accepts(d)` returns the same value for the same
///   datom across all calls, regardless of ordering or timing.
/// - **Purity**: the result depends only on the datom's content, not
///   on internal mutable state or external conditions.
/// - **Subset**: `accepted(R) ⊆ source` — a filter never invents datoms.
pub trait ReplicaFilter: Send + Sync {
    /// Evaluate whether this replica accepts the given datom.
    ///
    /// INV-FERR-030: returns `true` if the datom belongs to this replica's
    /// accepted subset, `false` otherwise. The result is deterministic:
    /// calling `accepts` on the same datom returns the same value every
    /// time, regardless of call order or concurrent mutations to the store.
    fn accepts(&self, datom: &Datom) -> bool;
}

/// Full-replica filter that accepts every datom unconditionally.
///
/// INV-FERR-030: satisfies the filter contract trivially -- `accepted(R) =
/// source` because `accepts` returns `true` for all datoms. This is the
/// default for single-node operation, where the node holds the complete
/// datom set with no subset projection.
///
/// Phase 4c will replace this with real topology management and
/// selective replication filters.
///
/// # Visibility
///
/// `pub` because verification tests in `ferratomic-verify` exercise
/// replica filter semantics (INV-FERR-030 conformance testing).
/// Phase 4c will add non-trivial filter implementations.
#[derive(Debug, Default, Clone)]
pub struct AcceptAll;

impl ReplicaFilter for AcceptAll {
    fn accepts(&self, _datom: &Datom) -> bool {
        true
    }
}

/// INV-FERR-030: `DatomFilter` implements `ReplicaFilter` by delegating
/// to [`DatomFilter::matches`]. This bridges the `ferratom` type
/// (`DatomFilter`) to the `ferratomic-core` trait (`ReplicaFilter`).
impl ReplicaFilter for DatomFilter {
    fn accepts(&self, datom: &Datom) -> bool {
        self.matches(datom)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, EntityId, Op, TxId, Value};

    use super::*;

    /// Helper: build a datom for testing.
    fn sample_datom(seed: &str) -> Datom {
        Datom::new(
            EntityId::from_content(seed.as_bytes()),
            Attribute::from("test/topology"),
            Value::String(Arc::from(seed)),
            TxId::new(1, 0, 0),
            Op::Assert,
        )
    }

    #[test]
    fn test_accept_all_passes_every_datom() {
        let filter = AcceptAll;
        let d1 = sample_datom("alpha");
        let d2 = sample_datom("beta");
        let d3 = sample_datom("gamma");

        assert!(filter.accepts(&d1), "AcceptAll must accept every datom");
        assert!(filter.accepts(&d2), "AcceptAll must accept every datom");
        assert!(filter.accepts(&d3), "AcceptAll must accept every datom");
    }

    #[test]
    fn test_accept_all_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AcceptAll>();
    }

    // -- INV-FERR-030: DatomFilter as ReplicaFilter -----------------------

    #[test]
    fn test_inv_ferr_030_datomfilter_all_matches_acceptall() {
        let accept_all = AcceptAll;
        let filter_all = DatomFilter::All;

        let d1 = sample_datom("alpha");
        let d2 = sample_datom("beta");

        assert_eq!(
            accept_all.accepts(&d1),
            filter_all.accepts(&d1),
            "INV-FERR-030: DatomFilter::All must behave like AcceptAll"
        );
        assert_eq!(
            accept_all.accepts(&d2),
            filter_all.accepts(&d2),
            "INV-FERR-030: DatomFilter::All must behave like AcceptAll"
        );
    }

    #[test]
    fn test_inv_ferr_030_datomfilter_namespace_as_replica_filter() {
        let filter = DatomFilter::AttributeNamespace(vec!["test/".to_string()]);
        let matching = sample_datom("x"); // attribute is "test/topology"
        let non_matching = Datom::new(
            EntityId::from_content(b"y"),
            Attribute::from("other/attr"),
            Value::Long(1),
            TxId::new(1, 0, 0),
            Op::Assert,
        );

        assert!(
            filter.accepts(&matching),
            "INV-FERR-030: DatomFilter namespace must accept matching datoms via ReplicaFilter"
        );
        assert!(
            !filter.accepts(&non_matching),
            "INV-FERR-030: DatomFilter namespace must reject non-matching datoms"
        );
    }

    #[test]
    fn test_inv_ferr_030_datomfilter_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DatomFilter>();
    }
}
