//! Schema: attribute definitions as data in the store.
//!
//! INV-FERR-009: Schema validation on transact. A datom's attribute must
//! exist in the schema and its value must match the declared ValueType.
//!
//! INV-FERR-031: Genesis determinism. The genesis transaction installs
//! 19 axiomatic meta-schema attributes. All genesis() calls produce
//! identical schemas.

// TODO(Phase 3): Implement Schema, AttributeDef, ValueType, Cardinality
// See spec/23-ferratomic.md §23.3 INV-FERR-031 for genesis specification.
