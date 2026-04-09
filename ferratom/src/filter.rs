//! Positive-only datom filter for selective merge and namespace isolation.
//!
//! INV-FERR-039: `DatomFilter` determines which datoms to accept during
//! selective merge. All 6 variants are monotone functions — adding datoms
//! to the store can only ADD matches, never REMOVE them. By the CALM
//! theorem, monotone filters are exactly the class safe for
//! coordination-free federation.
//!
//! ADR-FERR-022: Phase 4a.5 scope is positive-only. `Not`, `Custom`,
//! `AfterEpoch` are deferred to Phase 4c.

use serde::Serialize;

use crate::{Datom, EntityId, NodeId};

/// Format a byte slice as a lowercase hex string without external dependencies.
fn bytes_to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // write! on String is infallible — the Result is always Ok.
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Positive-only datom filter for selective merge and namespace isolation.
///
/// INV-FERR-039: All 6 variants are monotone functions — adding datoms
/// to the store can only ADD matches, never REMOVE them. By the CALM
/// theorem, monotone filters are exactly the class safe for
/// coordination-free federation.
///
/// ADR-FERR-022: Phase 4a.5 scope is positive-only. `Not`, `Custom`,
/// `AfterEpoch` are deferred to Phase 4c.
///
/// C8 (Substrate Independence): `FromNodes` uses `NodeId` (not `AgentId`).
#[derive(Debug, Clone, Serialize)]
pub enum DatomFilter {
    /// Match all datoms (identity filter).
    All,

    /// Match datoms whose attribute starts with any of the given prefixes.
    ///
    /// INV-FERR-044: namespace isolation. A datom matches if its attribute
    /// name starts with at least one of the supplied prefixes.
    AttributeNamespace(Vec<String>),

    /// Match datoms originating from any of the given nodes.
    ///
    /// C8 (Substrate Independence): uses `NodeId`, not `AgentId`.
    FromNodes(Vec<NodeId>),

    /// Match datoms with any of the given entity IDs.
    Entities(Vec<EntityId>),

    /// Match datoms that satisfy ALL sub-filters (intersection).
    ///
    /// Empty `And` matches everything (vacuous truth).
    And(Vec<DatomFilter>),

    /// Match datoms that satisfy ANY sub-filter (union).
    ///
    /// Empty `Or` matches nothing (vacuous disjunction).
    Or(Vec<DatomFilter>),
}

impl DatomFilter {
    /// Evaluate whether a datom passes this filter.
    ///
    /// INV-FERR-039: Each variant is a monotone function over the datom set.
    /// Adding datoms to the store can only ADD matches, never REMOVE them.
    #[must_use]
    pub fn matches(&self, datom: &Datom) -> bool {
        match self {
            Self::All => true,

            Self::AttributeNamespace(prefixes) => {
                let attr = datom.attribute().as_str();
                prefixes
                    .iter()
                    .any(|prefix| attr.starts_with(prefix.as_str()))
            }

            Self::FromNodes(nodes) => nodes.contains(&datom.tx().node()),

            Self::Entities(entities) => entities.contains(&datom.entity()),

            Self::And(filters) => filters.iter().all(|f| f.matches(datom)),

            Self::Or(filters) => filters.iter().any(|f| f.matches(datom)),
        }
    }

    /// Serialize this filter to a human-readable string for merge receipts.
    ///
    /// INV-FERR-062: Merge receipts include the filter description so that
    /// the provenance of each selective merge is auditable.
    #[must_use]
    pub fn serialize(&self) -> String {
        match self {
            Self::All => "All".to_string(),

            Self::AttributeNamespace(prefixes) => {
                let joined: Vec<&str> = prefixes.iter().map(String::as_str).collect();
                format!("AttributeNamespace([{}])", joined.join(", "))
            }

            Self::FromNodes(nodes) => {
                let hex_ids: Vec<String> =
                    nodes.iter().map(|n| bytes_to_hex(n.as_bytes())).collect();
                format!("FromNodes([{}])", hex_ids.join(", "))
            }

            Self::Entities(entities) => {
                let hex_ids: Vec<String> = entities
                    .iter()
                    .map(|e| bytes_to_hex(e.as_bytes()))
                    .collect();
                format!("Entities([{}])", hex_ids.join(", "))
            }

            Self::And(filters) => {
                let subs: Vec<String> = filters.iter().map(Self::serialize).collect();
                format!("And([{}])", subs.join(", "))
            }

            Self::Or(filters) => {
                let subs: Vec<String> = filters.iter().map(Self::serialize).collect();
                format!("Or([{}])", subs.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Attribute, Op, TxId, Value};

    /// Helper: build a datom with configurable attribute and node seed.
    fn make_datom(attr_name: &str, node_seed: u16) -> Datom {
        let entity = EntityId::from_content(b"test-entity");
        let attribute = Attribute::from(attr_name);
        let value = Value::Long(42);
        let tx = TxId::new(1_000_000, 0, node_seed);
        Datom::new(entity, attribute, value, tx, Op::Assert)
    }

    /// Helper: build a datom with a specific entity content key.
    fn make_datom_with_entity(entity_content: &[u8], attr_name: &str, node_seed: u16) -> Datom {
        let entity = EntityId::from_content(entity_content);
        let attribute = Attribute::from(attr_name);
        let value = Value::Long(1);
        let tx = TxId::new(1_000_000, 0, node_seed);
        Datom::new(entity, attribute, value, tx, Op::Assert)
    }

    #[test]
    fn test_inv_ferr_039_filter_all_matches_everything() {
        let filter = DatomFilter::All;
        let d1 = make_datom("user/name", 1);
        let d2 = make_datom("system/config", 2);
        assert!(
            filter.matches(&d1),
            "INV-FERR-039: All filter must match every datom"
        );
        assert!(
            filter.matches(&d2),
            "INV-FERR-039: All filter must match every datom"
        );
    }

    #[test]
    fn test_inv_ferr_044_filter_namespace_prefix() {
        let filter = DatomFilter::AttributeNamespace(vec!["user/".to_string(), "db/".to_string()]);
        let matching = make_datom("user/name", 1);
        let matching_db = make_datom("db/ident", 1);
        let non_matching = make_datom("system/config", 1);

        assert!(
            filter.matches(&matching),
            "INV-FERR-044: datom with user/ prefix must match"
        );
        assert!(
            filter.matches(&matching_db),
            "INV-FERR-044: datom with db/ prefix must match"
        );
        assert!(
            !filter.matches(&non_matching),
            "INV-FERR-044: datom with system/ prefix must not match user/ or db/"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_from_nodes() {
        let node1 = NodeId::from_seed(1);
        let node2 = NodeId::from_seed(2);
        let filter = DatomFilter::FromNodes(vec![node1]);

        let from_node1 = make_datom("user/name", 1);
        let from_node2 = make_datom("user/name", 2);

        assert!(
            filter.matches(&from_node1),
            "INV-FERR-039: datom from allowed node must match"
        );
        assert!(
            !filter.matches(&from_node2),
            "INV-FERR-039: datom from disallowed node must not match"
        );

        // Verify node2 matches when included.
        let filter_both = DatomFilter::FromNodes(vec![node1, node2]);
        assert!(
            filter_both.matches(&from_node2),
            "INV-FERR-039: datom from node in expanded set must match"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_entities() {
        let e_alpha = EntityId::from_content(b"alpha");
        let e_bravo = EntityId::from_content(b"bravo");
        let filter = DatomFilter::Entities(vec![e_alpha]);

        let d_alpha = make_datom_with_entity(b"alpha", "x/y", 1);
        let d_bravo = make_datom_with_entity(b"bravo", "x/y", 1);

        assert!(
            filter.matches(&d_alpha),
            "INV-FERR-039: datom with listed entity must match"
        );
        assert!(
            !filter.matches(&d_bravo),
            "INV-FERR-039: datom with unlisted entity must not match"
        );

        let filter_both = DatomFilter::Entities(vec![e_alpha, e_bravo]);
        assert!(
            filter_both.matches(&d_bravo),
            "INV-FERR-039: datom with entity in expanded set must match"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_and_composition() {
        // Require both: attribute in user/ namespace AND from node 1.
        let node1 = NodeId::from_seed(1);
        let filter = DatomFilter::And(vec![
            DatomFilter::AttributeNamespace(vec!["user/".to_string()]),
            DatomFilter::FromNodes(vec![node1]),
        ]);

        let both_match = make_datom("user/name", 1);
        let wrong_ns = make_datom("system/config", 1);
        let wrong_node = make_datom("user/name", 2);

        assert!(
            filter.matches(&both_match),
            "INV-FERR-039: And requires all sub-filters to match"
        );
        assert!(
            !filter.matches(&wrong_ns),
            "INV-FERR-039: And fails when namespace sub-filter fails"
        );
        assert!(
            !filter.matches(&wrong_node),
            "INV-FERR-039: And fails when node sub-filter fails"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_or_composition() {
        // Accept datoms in user/ namespace OR from node 2.
        let node2 = NodeId::from_seed(2);
        let filter = DatomFilter::Or(vec![
            DatomFilter::AttributeNamespace(vec!["user/".to_string()]),
            DatomFilter::FromNodes(vec![node2]),
        ]);

        let ns_match = make_datom("user/name", 1);
        let node_match = make_datom("system/config", 2);
        let neither = make_datom("system/config", 1);

        assert!(
            filter.matches(&ns_match),
            "INV-FERR-039: Or succeeds when namespace sub-filter matches"
        );
        assert!(
            filter.matches(&node_match),
            "INV-FERR-039: Or succeeds when node sub-filter matches"
        );
        assert!(
            !filter.matches(&neither),
            "INV-FERR-039: Or fails when no sub-filter matches"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_empty_and_is_true() {
        let filter = DatomFilter::And(vec![]);
        let d = make_datom("any/thing", 1);
        assert!(
            filter.matches(&d),
            "INV-FERR-039: empty And is vacuous truth (matches all)"
        );
    }

    #[test]
    fn test_inv_ferr_039_filter_empty_or_is_false() {
        let filter = DatomFilter::Or(vec![]);
        let d = make_datom("any/thing", 1);
        assert!(
            !filter.matches(&d),
            "INV-FERR-039: empty Or is vacuous disjunction (matches none)"
        );
    }

    #[test]
    fn test_inv_ferr_062_filter_serialize_round_trip() {
        let filters = vec![
            DatomFilter::All,
            DatomFilter::AttributeNamespace(vec!["foo/".to_string(), "bar/".to_string()]),
            DatomFilter::FromNodes(vec![NodeId::from_seed(1)]),
            DatomFilter::Entities(vec![EntityId::from_content(b"test")]),
            DatomFilter::And(vec![DatomFilter::All]),
            DatomFilter::Or(vec![DatomFilter::All, DatomFilter::All]),
        ];

        for filter in &filters {
            let serialized = filter.serialize();
            assert!(
                !serialized.is_empty(),
                "INV-FERR-062: serialize must produce non-empty string for {filter:?}"
            );
        }

        // Verify specific formats.
        assert_eq!(
            DatomFilter::All.serialize(),
            "All",
            "INV-FERR-062: All serializes to \"All\""
        );

        let ns_filter =
            DatomFilter::AttributeNamespace(vec!["foo/".to_string(), "bar/".to_string()]);
        assert_eq!(
            ns_filter.serialize(),
            "AttributeNamespace([foo/, bar/])",
            "INV-FERR-062: AttributeNamespace serializes with comma-separated prefixes"
        );

        let nested = DatomFilter::And(vec![DatomFilter::All, DatomFilter::Or(vec![])]);
        assert_eq!(
            nested.serialize(),
            "And([All, Or([])])",
            "INV-FERR-062: nested filters serialize recursively"
        );
    }
}
