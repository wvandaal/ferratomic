//! Topology and replica filtering (INV-FERR-030).
//!
//! Phase 4a defines trait boundaries; Phase 4c implements
//! real topology management and selective replication.
//!
//! See spec/05-federation.md and `FERRATOMIC_ARCHITECTURE.md`.

use ferratom::Datom;

/// Filter predicate for read replica subset selection (INV-FERR-030).
///
/// Implementations determine which datoms a replica stores.
/// `AcceptAll` is the default (full replica).
pub trait ReplicaFilter: Send + Sync {
    /// Returns true if this replica should store the given datom (INV-FERR-030).
    fn accepts(&self, datom: &Datom) -> bool;
}

/// Accept all datoms: full replica behavior (INV-FERR-030).
///
/// This is the default filter for single-node operation. Every datom
/// is accepted, producing a full replica of the store.
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
