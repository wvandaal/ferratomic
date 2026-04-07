//! Checkpoint V4: entropy-coded columnar format.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` -- round-trip identity.
//!
//! V4 decomposes each datom into per-column arrays (entity, attribute,
//! value, tx, op) and serializes them independently. This layout enables
//! per-column compression in Phase 4b (delta coding for `TxId`, RLE for
//! entity, bitpacking for ops). Phase 4a uses bincode for all columns.
//!
//! # File Format
//!
//! ```text
//! +------------------+
//! | Magic    (4B)    | 0x43484B34 ("CHK4")
//! +------------------+
//! | Version  (2B)    | 0x0004 (little-endian, V4)
//! +------------------+
//! | Epoch    (8B)    | u64 little-endian
//! +------------------+
//! | Genesis  (16B)   | AgentId bytes
//! +------------------+
//! | Payload  (N)     | bincode: V4PayloadWrite/V4PayloadRead
//! |  schema_pairs    |   sorted (String, AttributeDef) pairs
//! |  entities        |   EntityId column (32 bytes x N)
//! |  attributes      |   String column (attribute names)
//! |  values          |   Value column (tagged union)
//! |  tx_ids          |   TxId column
//! |  ops             |   bool column (true=Assert, false=Retract)
//! |  live_bits       |   LIVE bitvector
//! +------------------+
//! | BLAKE3   (32B)   | Hash of all preceding bytes
//! +------------------+
//! ```
//!
//! ADR-FERR-010: Deserialization uses wire types (`WireEntityId`,
//! `WireValue`) for trust boundary enforcement, then converts via
//! `into_trusted()` after BLAKE3 verification.

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{AgentId, Attribute, AttributeDef, Datom, EntityId, FerraError, Op, TxId, Value};
use serde::{Deserialize, Serialize};

/// V4 magic bytes: re-exported from lib.rs for single source of truth.
use super::V4_MAGIC;
use crate::CheckpointData;

/// V4 format version.
const V4_VERSION: u16 = 4;

/// Fixed header size: magic(4) + version(2) + epoch(8) + genesis(16) = 30 bytes.
const V4_HEADER_SIZE: usize = 4 + 2 + 8 + 16;

/// BLAKE3 hash size: 32 bytes.
const HASH_SIZE: usize = crate::mmap::HASH_SIZE;

// ---------------------------------------------------------------------------
// Serialization payload (uses core types with Serialize)
// ---------------------------------------------------------------------------

/// V4 columnar serialization payload.
///
/// Each column is serialized independently, enabling per-column compression
/// in Phase 4b. Phase 4a uses bincode for all columns.
///
/// ADR-FERR-010: Only used for serialization. Deserialization uses
/// `V4PayloadRead` with wire types.
#[derive(Serialize)]
struct V4PayloadWrite<'a> {
    /// Schema attributes sorted by name for deterministic output.
    schema_pairs: Vec<(String, AttributeDef)>,
    /// Entity column (32 bytes per datom, borrowed for zero-clone).
    entities: &'a [EntityId],
    /// Attribute column (string names for federation compatibility).
    attributes: Vec<String>,
    /// Value column (tagged union).
    values: &'a [Value],
    /// `TxId` column.
    tx_ids: &'a [TxId],
    /// Op column (true = Assert, false = Retract).
    ops: Vec<bool>,
    /// LIVE bitvector (INV-FERR-029).
    live_bits: BitVec<u64, Lsb0>,
}

// ---------------------------------------------------------------------------
// Deserialization payload (uses wire types for trust boundary)
// ---------------------------------------------------------------------------

/// V4 columnar deserialization payload.
///
/// ADR-FERR-010: Wire types are the ONLY types that touch untrusted bytes.
/// Conversion to core types happens via `into_trusted()` after BLAKE3
/// verification.
#[derive(Deserialize)]
struct V4PayloadRead {
    /// Schema attributes.
    schema_pairs: Vec<(String, AttributeDef)>,
    /// Entity column (wire format, unverified).
    entities: Vec<ferratom::wire::WireEntityId>,
    /// Attribute column (string names).
    attributes: Vec<String>,
    /// Value column (wire format, may contain unverified `EntityId` via Ref).
    values: Vec<ferratom::wire::WireValue>,
    /// `TxId` column (safe -- just integers + agent bytes).
    tx_ids: Vec<TxId>,
    /// Op column (true = Assert, false = Retract).
    ops: Vec<bool>,
    /// LIVE bitvector.
    live_bits: BitVec<u64, Lsb0>,
}

// ---------------------------------------------------------------------------
// Serialize
// ---------------------------------------------------------------------------

/// Serialize store data to V4 columnar checkpoint bytes (in-memory).
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// genesis agent, schema, all datoms decomposed into columns, LIVE bitvector)
/// in the V4 wire format. A trailing BLAKE3 hash covers all preceding bytes
/// for tamper detection.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub fn serialize_v4_bytes(
    datoms: &[Datom],
    schema_pairs: &[(String, AttributeDef)],
    epoch: u64,
    genesis_agent: AgentId,
    live_bits: &BitVec<u64, Lsb0>,
) -> Result<Vec<u8>, FerraError> {
    // Decompose datoms into per-column arrays.
    let n = datoms.len();
    let mut entities: Vec<EntityId> = Vec::with_capacity(n);
    let mut attributes: Vec<String> = Vec::with_capacity(n);
    let mut values: Vec<Value> = Vec::with_capacity(n);
    let mut tx_ids: Vec<TxId> = Vec::with_capacity(n);
    let mut ops: Vec<bool> = Vec::with_capacity(n);

    for datom in datoms {
        entities.push(datom.entity());
        attributes.push(datom.attribute().as_str().to_owned());
        values.push(datom.value().clone());
        tx_ids.push(datom.tx());
        ops.push(datom.op() == Op::Assert);
    }

    let payload = V4PayloadWrite {
        schema_pairs: schema_pairs.to_vec(),
        entities: &entities,
        attributes,
        values: &values,
        tx_ids: &tx_ids,
        ops,
        live_bits: live_bits.clone(),
    };

    let payload_bytes =
        bincode::serialize(&payload).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    // Build the full buffer: header + payload + BLAKE3.
    let total_size = V4_HEADER_SIZE + payload_bytes.len() + HASH_SIZE;
    let mut buf = Vec::with_capacity(total_size);

    // Header: magic + version + epoch + genesis_agent
    buf.extend_from_slice(&V4_MAGIC);
    buf.extend_from_slice(&V4_VERSION.to_le_bytes());
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(genesis_agent.as_bytes());

    // Payload
    buf.extend_from_slice(&payload_bytes);

    // BLAKE3 hash of [magic..payload]
    let hash = blake3::hash(&buf);
    buf.extend_from_slice(hash.as_bytes());

    Ok(buf)
}

// ---------------------------------------------------------------------------
// Deserialize
// ---------------------------------------------------------------------------

/// Verify BLAKE3 checksum and return the content slice (without hash).
/// Delegates to `mmap::verify_blake3` (shared BLAKE3 verification).
fn verify_v4_checksum(data: &[u8]) -> Result<&[u8], FerraError> {
    crate::mmap::verify_blake3(data, V4_HEADER_SIZE)
}

/// Shorthand for `CheckpointCorrupted` error construction.
fn corrupted(expected: &str, actual: &str) -> FerraError {
    FerraError::CheckpointCorrupted {
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

/// Parse the V4 fixed header: magic, version, epoch, `genesis_agent`.
fn parse_v4_header(content: &[u8]) -> Result<(u64, AgentId), FerraError> {
    let magic: [u8; 4] = content[0..4]
        .try_into()
        .map_err(|_| corrupted("CHK4 magic", "truncated"))?;
    if magic != V4_MAGIC {
        return Err(corrupted("CHK4", &String::from_utf8_lossy(&magic)));
    }
    let version = u16::from_le_bytes(
        content[4..6]
            .try_into()
            .map_err(|_| corrupted("2-byte version", "truncated"))?,
    );
    if version != V4_VERSION {
        return Err(corrupted(
            &format!("version {V4_VERSION} (V4)"),
            &format!("version {version}"),
        ));
    }
    let epoch = u64::from_le_bytes(
        content[6..14]
            .try_into()
            .map_err(|_| corrupted("8-byte epoch", "truncated"))?,
    );
    let genesis_bytes: [u8; 16] = content[14..30]
        .try_into()
        .map_err(|_| corrupted("16-byte genesis agent", "truncated"))?;
    Ok((epoch, AgentId::from_bytes(genesis_bytes)))
}

/// Validate that all V4 columns have consistent length.
///
/// INV-FERR-013: Column count mismatch means data corruption or
/// a serialization bug. All columns must have the same length as
/// `entities`, and `live_bits` must match as well.
fn validate_column_lengths(payload: &V4PayloadRead) -> Result<usize, FerraError> {
    let n = payload.entities.len();

    if payload.attributes.len() != n {
        return Err(corrupted(
            &format!("attributes.len() == {n}"),
            &format!("attributes.len() = {}", payload.attributes.len()),
        ));
    }
    if payload.values.len() != n {
        return Err(corrupted(
            &format!("values.len() == {n}"),
            &format!("values.len() = {}", payload.values.len()),
        ));
    }
    if payload.tx_ids.len() != n {
        return Err(corrupted(
            &format!("tx_ids.len() == {n}"),
            &format!("tx_ids.len() = {}", payload.tx_ids.len()),
        ));
    }
    if payload.ops.len() != n {
        return Err(corrupted(
            &format!("ops.len() == {n}"),
            &format!("ops.len() = {}", payload.ops.len()),
        ));
    }
    if payload.live_bits.len() != n {
        return Err(corrupted(
            &format!("live_bits.len() == {n}"),
            &format!("live_bits.len() = {}", payload.live_bits.len()),
        ));
    }

    Ok(n)
}

/// Deserialize V4 columnar checkpoint bytes into raw checkpoint data.
///
/// INV-FERR-013: Verifies the BLAKE3 checksum, parses header, deserializes
/// columnar payload through the ADR-FERR-010 trust boundary (wire types),
/// then reassembles datoms from columns.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// truncation, column length mismatch, or deserialization failure.
pub fn deserialize_v4_bytes(data: &[u8]) -> Result<CheckpointData, FerraError> {
    let content = verify_v4_checksum(data)?;
    let (epoch, genesis_agent) = parse_v4_header(content)?;

    // Deserialize columnar payload through ADR-FERR-010 trust boundary.
    let wire_payload: V4PayloadRead = bincode::deserialize(&content[V4_HEADER_SIZE..])
        .map_err(|e| corrupted("valid V4 bincode payload", &e.to_string()))?;

    // Validate column lengths before reconstruction.
    let n = validate_column_lengths(&wire_payload)?;

    // Reconstruct datoms from columns via trust boundary.
    // WireEntityId -> EntityId, WireValue -> Value (BLAKE3 verified above).
    let mut datoms: Vec<Datom> = Vec::with_capacity(n);

    // Consume columns by value to avoid unnecessary clones.
    let V4PayloadRead {
        schema_pairs,
        entities,
        attributes,
        values,
        tx_ids,
        ops,
        live_bits,
    } = wire_payload;

    // Zip columns back into datoms. Each column is consumed by IntoIterator.
    for ((((entity, attr), value), tx), op_bool) in entities
        .into_iter()
        .zip(attributes)
        .zip(values)
        .zip(tx_ids)
        .zip(ops)
    {
        let op = if op_bool { Op::Assert } else { Op::Retract };

        datoms.push(Datom::new(
            entity.into_trusted(),
            Attribute::from(attr.as_str()),
            value.into_trusted(),
            tx,
            op,
        ));
    }

    Ok(CheckpointData {
        epoch,
        genesis_agent,
        schema_pairs,
        datoms,
        live_bits: Some(live_bits),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bitvec::prelude::{BitVec, Lsb0};
    use ferratom::{AgentId, Attribute, AttributeDef, Datom, EntityId, Op, TxId, Value};

    use super::*;

    /// Helper: build a small sorted datom set with `live_bits` for tests.
    fn make_test_datoms() -> (Vec<Datom>, BitVec<u64, Lsb0>) {
        let mut datoms: Vec<Datom> = vec![
            Datom::new(
                EntityId::from_content(b"entity-1"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("hello world")),
                TxId::new(0, 1, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"entity-2"),
                Attribute::from("db/doc"),
                Value::String(Arc::from("second")),
                TxId::new(0, 2, 0),
                Op::Assert,
            ),
        ];
        datoms.sort();

        let live_bits = ferratomic_positional::build_live_bitvector_pub(&datoms);
        (datoms, live_bits)
    }

    fn test_schema_pairs() -> Vec<(String, AttributeDef)> {
        vec![(
            "db/doc".to_string(),
            AttributeDef::new(
                ferratom::ValueType::String,
                ferratom::Cardinality::One,
                ferratom::ResolutionMode::Lww,
                None,
            ),
        )]
    }

    /// Helper: build a `CheckpointData` from test data.
    fn make_checkpoint_data() -> CheckpointData {
        let (datoms, live_bits) = make_test_datoms();
        CheckpointData {
            epoch: 42,
            genesis_agent: AgentId::from_bytes([1u8; 16]),
            schema_pairs: test_schema_pairs(),
            datoms,
            live_bits: Some(live_bits),
        }
    }

    // -------------------------------------------------------------------
    // V4 columnar format tests
    // -------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_013_v4_roundtrip() {
        let data = make_checkpoint_data();
        let empty_bv = BitVec::new();
        let lb_ref = data.live_bits.as_ref().unwrap_or(&empty_bv);

        let bytes = serialize_v4_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            lb_ref,
        )
        .map_err(|e| format!("serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let loaded = deserialize_v4_bytes(&bytes)
            .map_err(|e| format!("deserialize: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(
            loaded.epoch, data.epoch,
            "INV-FERR-013: epoch must roundtrip"
        );
        assert_eq!(
            loaded.genesis_agent, data.genesis_agent,
            "INV-FERR-013: genesis_agent must roundtrip"
        );
        assert_eq!(
            loaded.datoms, data.datoms,
            "INV-FERR-013: datom set must roundtrip"
        );
        assert_eq!(
            loaded.schema_pairs, data.schema_pairs,
            "INV-FERR-013: schema must roundtrip"
        );
        assert_eq!(
            loaded.live_bits, data.live_bits,
            "INV-FERR-013: live_bits must roundtrip in V4"
        );
    }

    #[test]
    fn test_inv_ferr_013_v4_empty_roundtrip() {
        let data = CheckpointData {
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
            schema_pairs: Vec::new(),
            datoms: Vec::new(),
            live_bits: Some(BitVec::new()),
        };

        let bytes = serialize_v4_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            data.live_bits.as_ref().map_or(&BitVec::new(), |b| b),
        )
        .map_err(|e| format!("serialize empty: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let loaded = deserialize_v4_bytes(&bytes)
            .map_err(|e| format!("deserialize empty: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(loaded.epoch, 0);
        assert_eq!(loaded.datoms.len(), 0);
        assert_eq!(loaded.schema_pairs.len(), 0);
        assert_eq!(loaded.live_bits.as_ref().map(BitVec::len), Some(0));
    }

    #[test]
    fn test_inv_ferr_013_v4_blake3_checksum() {
        let data = make_checkpoint_data();

        let bytes = serialize_v4_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            data.live_bits.as_ref().map_or(&BitVec::new(), |b| b),
        )
        .map_err(|e| format!("serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        // Verify magic.
        assert_eq!(&bytes[0..4], b"CHK4", "V4 must start with CHK4 magic");

        // Corrupt a payload byte and verify rejection.
        let mut corrupted_bytes = bytes.clone();
        let midpoint = corrupted_bytes.len() / 2;
        corrupted_bytes[midpoint] ^= 0xFF;

        let result = deserialize_v4_bytes(&corrupted_bytes);
        assert!(
            result.is_err(),
            "INV-FERR-013: corrupted V4 checkpoint must be rejected"
        );
    }

    #[test]
    fn test_inv_ferr_013_v4_matches_v3() {
        let data = make_checkpoint_data();
        let empty_bv = BitVec::new();
        let lb_ref = data.live_bits.as_ref().unwrap_or(&empty_bv);

        // Serialize as V4.
        let v4_bytes = serialize_v4_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            lb_ref,
        )
        .map_err(|e| format!("V4 serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let v4_loaded = deserialize_v4_bytes(&v4_bytes)
            .map_err(|e| format!("V4 deserialize: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        // Serialize as V3 for comparison.
        let v3_bytes = crate::v3::serialize_v3_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            lb_ref,
        )
        .map_err(|e| format!("V3 serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let v3_loaded = crate::v3::deserialize_v3_bytes(&v3_bytes)
            .map_err(|e| format!("V3 deserialize: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        // V4 must produce identical CheckpointData as V3.
        assert_eq!(v4_loaded.epoch, v3_loaded.epoch, "V4 epoch must match V3");
        assert_eq!(
            v4_loaded.genesis_agent, v3_loaded.genesis_agent,
            "V4 genesis_agent must match V3"
        );
        assert_eq!(
            v4_loaded.datoms, v3_loaded.datoms,
            "V4 datom set must match V3"
        );
        assert_eq!(
            v4_loaded.schema_pairs, v3_loaded.schema_pairs,
            "V4 schema must match V3"
        );
        assert_eq!(
            v4_loaded.live_bits, v3_loaded.live_bits,
            "V4 live_bits must match V3"
        );
    }

    #[test]
    fn test_inv_ferr_013_v4_with_retraction() {
        // Exercise both Assert and Retract ops through the bool column.
        let mut datoms: Vec<Datom> = vec![
            Datom::new(
                EntityId::from_content(b"entity-ret"),
                Attribute::from("db/name"),
                Value::String(Arc::from("original")),
                TxId::new(0, 1, 0),
                Op::Assert,
            ),
            Datom::new(
                EntityId::from_content(b"entity-ret"),
                Attribute::from("db/name"),
                Value::String(Arc::from("original")),
                TxId::new(0, 2, 0),
                Op::Retract,
            ),
        ];
        datoms.sort();

        let live_bits = ferratomic_positional::build_live_bitvector_pub(&datoms);
        let schema = test_schema_pairs();

        let bytes = serialize_v4_bytes(
            &datoms,
            &schema,
            10,
            AgentId::from_bytes([5u8; 16]),
            &live_bits,
        )
        .map_err(|e| format!("serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let loaded = deserialize_v4_bytes(&bytes)
            .map_err(|e| format!("deserialize: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(loaded.datoms, datoms, "Retraction roundtrip");

        // Verify we have both Assert and Retract.
        let has_assert = loaded.datoms.iter().any(|d| d.op() == Op::Assert);
        let has_retract = loaded.datoms.iter().any(|d| d.op() == Op::Retract);
        assert!(has_assert, "Must have at least one Assert");
        assert!(has_retract, "Must have at least one Retract");
    }

    #[test]
    fn test_inv_ferr_013_v4_dispatch_integration() {
        // V4 bytes should be loadable through the main dispatch.
        let data = make_checkpoint_data();
        let empty_bv = BitVec::new();
        let lb_ref = data.live_bits.as_ref().unwrap_or(&empty_bv);

        let bytes = serialize_v4_bytes(
            &data.datoms,
            &data.schema_pairs,
            data.epoch,
            data.genesis_agent,
            lb_ref,
        )
        .map_err(|e| format!("serialize: {e}"))
        .unwrap_or_else(|e| panic!("{e}"));

        let loaded = crate::deserialize_checkpoint_bytes(&bytes)
            .map_err(|e| format!("dispatch deserialize: {e}"))
            .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(loaded.epoch, data.epoch);
        assert_eq!(loaded.datoms, data.datoms);
        assert_eq!(loaded.schema_pairs, data.schema_pairs);
        assert_eq!(loaded.live_bits, data.live_bits);
    }
}
