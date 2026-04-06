//! Wire-format types for the deserialization trust boundary.
//!
//! ADR-FERR-010: Two-tier type system. Types containing `EntityId`
//! (directly or transitively) do NOT derive `Deserialize` — these are
//! `EntityId`, `Value` (via `Ref(EntityId)`), and `Datom`. Wire variants
//! (`WireEntityId`, `WireValue`, `WireDatom`) carry `Deserialize` and
//! cross the trust boundary via `into_trusted()` (local integrity-verified
//! storage) or `into_verified()` (Phase 4c: cryptographic proof).
//!
//! Provenance-independent types (`Op`, `Attribute`, `TxId`, `AgentId`,
//! schema types) derive `Deserialize` directly because they contain no
//! content-addressed identity and cannot smuggle unverified `EntityId`s.
//!
//! INV-FERR-012: Every `EntityId` has known provenance:
//! - `from_content()` — computed BLAKE3 hash
//! - `into_trusted()` — integrity-verified local storage (CRC/BLAKE3)
//! - `into_verified()` — cryptographic proof (Phase 4c, Ed25519/Merkle)

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
    clock::TxId,
    datom::{Attribute, EntityId, NonNanFloat, Op, Value},
    schema::AttributeDef,
    AgentId, Datom,
};

// ---------------------------------------------------------------------------
// Wire EntityId
// ---------------------------------------------------------------------------

/// Wire-format `EntityId`. NOT verified as BLAKE3 hash.
///
/// Must be converted to `EntityId` through a trust boundary before
/// entering the Store. ADR-FERR-010.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WireEntityId(pub [u8; 32]);

impl WireEntityId {
    /// Convert to `EntityId` for data from integrity-verified local storage.
    ///
    /// CRC (WAL) or BLAKE3 (checkpoint) verification MUST have been
    /// performed on the source bytes before this call. INV-FERR-012.
    #[must_use]
    pub fn into_trusted(self) -> EntityId {
        EntityId::from_trusted_bytes(self.0)
    }
}

// ---------------------------------------------------------------------------
// Wire Value
// ---------------------------------------------------------------------------

/// Wire-format `Value`. May contain `WireEntityId` via `Ref` variant.
///
/// ADR-FERR-010: All deserialization produces wire types first.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WireValue {
    /// Namespaced keyword.
    Keyword(Arc<str>),
    /// UTF-8 string.
    String(Arc<str>),
    /// 64-bit signed integer.
    Long(i64),
    /// Non-NaN 64-bit float (custom `Deserialize` rejects NaN per CR-003).
    Double(NonNanFloat),
    /// Boolean.
    Bool(bool),
    /// Milliseconds since Unix epoch.
    Instant(i64),
    /// 128-bit UUID.
    Uuid([u8; 16]),
    /// Opaque binary blob.
    Bytes(Arc<[u8]>),
    /// Reference to another entity (wire format — unverified).
    Ref(WireEntityId),
    /// Arbitrary-precision integer (i128).
    BigInt(i128),
    /// Arbitrary-precision decimal (i128).
    BigDec(i128),
}

impl WireValue {
    /// Convert to core `Value` after trust boundary verification.
    ///
    /// ADR-FERR-010: Caller MUST have verified source integrity.
    #[must_use]
    pub fn into_trusted(self) -> Value {
        match self {
            Self::Keyword(s) => Value::Keyword(s),
            Self::String(s) => Value::String(s),
            Self::Long(n) => Value::Long(n),
            Self::Double(f) => Value::Double(f),
            Self::Bool(b) => Value::Bool(b),
            Self::Instant(ms) => Value::Instant(ms),
            Self::Uuid(bytes) => Value::Uuid(bytes),
            Self::Bytes(blob) => Value::Bytes(blob),
            Self::Ref(wire_id) => Value::Ref(wire_id.into_trusted()),
            Self::BigInt(n) => Value::BigInt(n),
            Self::BigDec(n) => Value::BigDec(n),
        }
    }
}

// ---------------------------------------------------------------------------
// Wire Datom
// ---------------------------------------------------------------------------

/// Wire-format `Datom`. All fields use wire types for `EntityId`.
///
/// ADR-FERR-010: Deserialization produces `WireDatom`, then `into_trusted()`
/// converts to core `Datom` after integrity verification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireDatom {
    /// Entity (unverified wire format).
    entity: WireEntityId,
    /// Attribute (safe — just `Arc<str>`).
    attribute: Attribute,
    /// Value (may contain unverified `WireEntityId` via `Ref`).
    value: WireValue,
    /// Transaction ID (safe — just integers + agent bytes).
    tx: TxId,
    /// Assert or Retract (safe — enum).
    op: Op,
}

impl WireDatom {
    /// Construct an opaque wire datom.
    ///
    /// ADR-FERR-010: callers may build a `WireDatom`, but they cannot mutate
    /// its contents after construction. Deserialization is the other entry path.
    #[must_use]
    pub fn new(
        entity: WireEntityId,
        attribute: Attribute,
        value: WireValue,
        tx: TxId,
        op: Op,
    ) -> Self {
        Self {
            entity,
            attribute,
            value,
            tx,
            op,
        }
    }

    /// Convert to core `Datom` after trust boundary verification.
    ///
    /// ADR-FERR-010: Caller MUST have verified source integrity
    /// (CRC for WAL, BLAKE3 for checkpoint). For network-received
    /// data (Phase 4c), use `into_verified()` instead.
    #[must_use]
    pub fn into_trusted(self) -> Datom {
        Datom::new(
            self.entity.into_trusted(),
            self.attribute,
            self.value.into_trusted(),
            self.tx,
            self.op,
        )
    }
}

// ---------------------------------------------------------------------------
// Wire Checkpoint Payload
// ---------------------------------------------------------------------------

/// Wire-format checkpoint payload for deserialization.
///
/// ADR-FERR-010: Checkpoint deserialization produces this type, then
/// each `WireDatom` is converted via `into_trusted()` after BLAKE3
/// verification of the checkpoint file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireCheckpointPayload {
    /// Schema attributes (sorted by name for determinism).
    schema: Vec<(String, AttributeDef)>,
    /// Genesis agent identity.
    genesis_agent: AgentId,
    /// All datoms in wire format.
    datoms: Vec<WireDatom>,
}

impl WireCheckpointPayload {
    /// Construct a new wire checkpoint payload.
    #[must_use]
    pub fn new(
        schema: Vec<(String, AttributeDef)>,
        genesis_agent: AgentId,
        datoms: Vec<WireDatom>,
    ) -> Self {
        Self {
            schema,
            genesis_agent,
            datoms,
        }
    }

    /// Schema attributes (sorted by name for determinism).
    #[must_use]
    pub fn schema(&self) -> &[(String, AttributeDef)] {
        &self.schema
    }

    /// Genesis agent identity.
    #[must_use]
    pub fn genesis_agent(&self) -> AgentId {
        self.genesis_agent
    }

    /// All datoms in wire format.
    #[must_use]
    pub fn datoms(&self) -> &[WireDatom] {
        &self.datoms
    }

    /// Consume and return owned components for checkpoint reconstruction.
    ///
    /// ADR-FERR-010: Used by checkpoint deserialization to take ownership
    /// of the schema, genesis agent, and datoms without cloning.
    #[must_use]
    pub fn into_parts(self) -> (Vec<(String, AttributeDef)>, AgentId, Vec<WireDatom>) {
        (self.schema, self.genesis_agent, self.datoms)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_entity_id_roundtrip() {
        let original = EntityId::from_content(b"test content");
        // Serialize as WireEntityId (same bytes)
        let wire = WireEntityId(*original.as_bytes());
        let recovered = wire.into_trusted();
        assert_eq!(
            original, recovered,
            "ADR-FERR-010: wire roundtrip must preserve EntityId"
        );
    }

    #[test]
    fn test_wire_value_ref_roundtrip() {
        let eid = EntityId::from_content(b"target");
        let original = Value::Ref(eid);
        let wire = WireValue::Ref(WireEntityId(*eid.as_bytes()));
        let recovered = wire.into_trusted();
        assert_eq!(
            original, recovered,
            "ADR-FERR-010: wire Ref roundtrip must preserve Value::Ref"
        );
    }

    #[test]
    fn test_wire_datom_roundtrip() {
        let entity = EntityId::from_content(b"entity");
        let attr = Attribute::from("db/doc");
        let value = Value::String(Arc::from("hello"));
        let tx = TxId::new(1, 0, 0);
        let original = Datom::new(entity, attr.clone(), value.clone(), tx, Op::Assert);

        let wire = WireDatom::new(
            WireEntityId(*entity.as_bytes()),
            attr,
            WireValue::String(Arc::from("hello")),
            tx,
            Op::Assert,
        );
        let recovered = wire.into_trusted();
        assert_eq!(
            original, recovered,
            "ADR-FERR-010: wire datom roundtrip must preserve Datom"
        );
    }

    #[test]
    fn test_wire_datom_bincode_roundtrip() {
        let entity = EntityId::from_content(b"entity");
        let attr = Attribute::from("db/doc");
        let tx = TxId::new(42, 0, 0);
        let original = Datom::new(entity, attr, Value::Long(123), tx, Op::Assert);

        // Serialize core Datom (Datom keeps Serialize)
        let bytes = bincode::serialize(&original).expect("serialize Datom");

        // Deserialize as WireDatom (wire types have Deserialize)
        let wire: WireDatom = bincode::deserialize(&bytes).expect("deserialize as WireDatom");

        // Convert through trust boundary
        let recovered = wire.into_trusted();
        assert_eq!(
            original, recovered,
            "ADR-FERR-010: serialize(Datom) -> deserialize(WireDatom) -> into_trusted() = identity"
        );
    }

    #[test]
    fn test_wire_nan_rejected() {
        // NonNanFloat's custom Deserialize (CR-003) rejects NaN even
        // when going through the wire type path.
        let nan_bytes =
            bincode::serialize(&ordered_float::OrderedFloat(f64::NAN)).expect("serialize NaN");
        let result: Result<NonNanFloat, _> = bincode::deserialize(&nan_bytes);
        assert!(
            result.is_err(),
            "CR-003: NaN must be rejected during deserialization"
        );
    }
}
