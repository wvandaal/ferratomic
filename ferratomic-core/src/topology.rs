//! Topology and replica filtering (INV-FERR-030).
//!
//! Phase 4a defines trait boundaries; Phase 4c implements
//! real topology management and selective replication.
//!
//! See spec/05-federation.md and `FERRATOMIC_ARCHITECTURE.md`.

use ferratom::Datom;

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
}
