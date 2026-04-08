//! V4 checkpoint deserialization (read path).
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` -- round-trip identity.
//! ADR-FERR-010: Wire types are the ONLY types that touch untrusted bytes.

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{Attribute, AttributeDef, Datom, FerraError, Op, TxId};
use serde::Deserialize;

use crate::{
    v4::{V4_HEADER_SIZE, V4_VERSION},
    CheckpointData, V4_MAGIC,
};

// ---------------------------------------------------------------------------
// Deserialization payload (uses wire types for trust boundary)
// ---------------------------------------------------------------------------

/// V4 columnar deserialization payload.
///
/// ADR-FERR-010: Wire types are the ONLY types that touch untrusted bytes.
/// Conversion to core types happens via `into_trusted()` after BLAKE3
/// verification.
#[derive(Deserialize)]
pub(crate) struct V4PayloadRead {
    /// Schema attributes.
    pub(crate) schema_pairs: Vec<(String, AttributeDef)>,
    /// Entity column (wire format, unverified).
    pub(crate) entities: Vec<ferratom::wire::WireEntityId>,
    /// Attribute column (string names).
    pub(crate) attributes: Vec<String>,
    /// Value column (wire format, may contain unverified `EntityId` via Ref).
    pub(crate) values: Vec<ferratom::wire::WireValue>,
    /// `TxId` column (safe -- just integers + agent bytes).
    pub(crate) tx_ids: Vec<TxId>,
    /// Op column (true = Assert, false = Retract).
    pub(crate) ops: Vec<bool>,
    /// LIVE bitvector.
    pub(crate) live_bits: BitVec<u64, Lsb0>,
}

// ---------------------------------------------------------------------------
// Deserialize helpers
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
fn parse_v4_header(content: &[u8]) -> Result<(u64, ferratom::AgentId), FerraError> {
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
    Ok((epoch, ferratom::AgentId::from_bytes(genesis_bytes)))
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
