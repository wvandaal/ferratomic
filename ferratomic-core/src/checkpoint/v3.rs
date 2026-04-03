//! Checkpoint V3: pre-sorted index arrays with zero-construction cold start.
//!
//! INV-FERR-013: `load(checkpoint(S)) = S` — round-trip identity.
//!
//! V3 persists the LIVE bitvector alongside the datom array, so cold-start
//! deserialization can build a `PositionalStore` directly without recomputing
//! liveness. Permutation arrays (`perm_aevt/vaet/avet`) remain lazy
//! (`OnceLock::new()`) — they are rebuilt on first query access.
//!
//! # File Format
//!
//! ```text
//! +------------------+
//! | Magic    (4B)    | 0x43484B33 ("CHK3")
//! +------------------+
//! | Version  (2B)    | 0x0003 (little-endian, V3)
//! +------------------+
//! | Epoch    (8B)    | u64 little-endian
//! +------------------+
//! | Genesis  (16B)   | AgentId bytes
//! +------------------+
//! | Payload  (N)     | bincode: V3PayloadWrite/V3PayloadRead
//! +------------------+
//! | BLAKE3   (32B)   | Hash of all preceding bytes
//! +------------------+
//! ```
//!
//! ADR-FERR-010: Deserialization uses `WireDatom` for trust boundary
//! enforcement, then converts via `into_trusted()` after BLAKE3 verification.

use std::collections::BTreeMap;

use bitvec::prelude::{BitVec, Lsb0};
use ferratom::{AgentId, AttributeDef, Datom, FerraError};
use serde::{Deserialize, Serialize};

use crate::{
    positional::build_live_bitvector_pub,
    store::{Store, StoreRepr},
};

/// V3 magic bytes: ASCII "CHK3".
const V3_MAGIC: [u8; 4] = *b"CHK3";

/// V3 format version.
const V3_VERSION: u16 = 3;

/// Fixed header size: magic(4) + version(2) + epoch(8) + genesis(16) = 30 bytes.
const V3_HEADER_SIZE: usize = 4 + 2 + 8 + 16;

/// BLAKE3 hash size: 32 bytes.
use crate::mmap::HASH_SIZE;

/// Serialization payload (uses core `Datom` which has `Serialize`).
///
/// ADR-FERR-010: Only used for serialization. Deserialization uses
/// `V3PayloadRead` with `WireDatom`.
#[derive(Serialize)]
struct V3PayloadWrite {
    /// Schema attributes sorted by name for deterministic output.
    schema_pairs: Vec<(String, AttributeDef)>,
    /// All datoms in canonical EAVT order.
    datoms: Vec<Datom>,
    /// LIVE bitvector (INV-FERR-029): `live_bits[p] = true` iff datom p is live.
    live_bits: BitVec<u64, Lsb0>,
}

/// Deserialization payload (uses `WireDatom` for trust boundary).
///
/// ADR-FERR-010: Wire types are the ONLY types that touch untrusted bytes.
/// Conversion to core types happens via `into_trusted()` after BLAKE3
/// verification.
#[derive(Deserialize)]
struct V3PayloadRead {
    /// Schema attributes.
    schema_pairs: Vec<(String, AttributeDef)>,
    /// Datoms in wire format (unverified `EntityId`).
    datoms: Vec<ferratom::wire::WireDatom>,
    /// LIVE bitvector.
    live_bits: BitVec<u64, Lsb0>,
}

/// Serialize a store to V3 checkpoint bytes (in-memory).
///
/// INV-FERR-013: The returned bytes contain the full store state (epoch,
/// genesis agent, schema, all datoms, LIVE bitvector) in the V3 wire format.
/// A trailing BLAKE3 hash covers all preceding bytes for tamper detection.
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub(crate) fn serialize_v3_bytes(store: &Store) -> Result<Vec<u8>, FerraError> {
    let epoch = store.epoch();
    let genesis_agent = store.genesis_agent();

    // Collect datoms in canonical EAVT order.
    let datoms: Vec<Datom> = store.datoms().cloned().collect();

    // Extract live_bits from PositionalStore if available, else rebuild.
    let live_bits = match &store.repr {
        StoreRepr::Positional(ps) => ps.live_bits_clone(),
        StoreRepr::OrdMap { .. } => build_live_bitvector_pub(&datoms),
    };

    // Sort schema pairs by attribute name for deterministic output.
    let schema_pairs: Vec<(String, AttributeDef)> = {
        let mut sorted: BTreeMap<String, AttributeDef> = BTreeMap::new();
        for (attr, def) in store.schema().iter() {
            sorted.insert(attr.as_str().to_owned(), def.clone());
        }
        sorted.into_iter().collect()
    };

    let payload = V3PayloadWrite {
        schema_pairs,
        datoms,
        live_bits,
    };

    let payload_bytes =
        bincode::serialize(&payload).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    // Build the full buffer: header + payload + BLAKE3.
    let total_size = V3_HEADER_SIZE + payload_bytes.len() + HASH_SIZE;
    let mut buf = Vec::with_capacity(total_size);

    // Header: magic + version + epoch + genesis_agent
    buf.extend_from_slice(&V3_MAGIC);
    buf.extend_from_slice(&V3_VERSION.to_le_bytes());
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(genesis_agent.as_bytes());

    // Payload
    buf.extend_from_slice(&payload_bytes);

    // BLAKE3 hash of [magic..payload]
    let hash = blake3::hash(&buf);
    buf.extend_from_slice(hash.as_bytes());

    Ok(buf)
}

/// Deserialize a store from V3 checkpoint bytes (in-memory).
///
/// INV-FERR-013: Verifies the BLAKE3 checksum, parses header, deserializes
/// payload through the ADR-FERR-010 trust boundary (`WireDatom`), and
/// constructs a `PositionalStore` directly from the pre-sorted datoms and
/// persisted LIVE bitvector. Permutation arrays are deferred (`OnceLock`).
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch,
/// truncation, or deserialization failure.
/// Verify BLAKE3 checksum and return the content slice (without hash).
/// Delegates to `mmap::verify_blake3` (shared BLAKE3 verification).
fn verify_v3_checksum(data: &[u8]) -> Result<&[u8], FerraError> {
    crate::mmap::verify_blake3(data, V3_HEADER_SIZE)
}

/// Parse the V3 fixed header: magic, version, epoch, `genesis_agent`.
fn parse_v3_header(content: &[u8]) -> Result<(u64, AgentId), FerraError> {
    let magic: [u8; 4] = content[0..4]
        .try_into()
        .map_err(|_| corrupted("CHK3 magic", "truncated"))?;
    if magic != V3_MAGIC {
        return Err(corrupted("CHK3", &String::from_utf8_lossy(&magic)));
    }
    let version = u16::from_le_bytes(
        content[4..6]
            .try_into()
            .map_err(|_| corrupted("2-byte version", "truncated"))?,
    );
    if version != V3_VERSION {
        return Err(corrupted(
            &format!("version {V3_VERSION} (V3)"),
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

/// Shorthand for `CheckpointCorrupted` error construction.
fn corrupted(expected: &str, actual: &str) -> FerraError {
    FerraError::CheckpointCorrupted {
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

pub(crate) fn deserialize_v3_bytes(data: &[u8]) -> Result<Store, FerraError> {
    let content = verify_v3_checksum(data)?;
    let (epoch, genesis_agent) = parse_v3_header(content)?;

    // Deserialize payload through ADR-FERR-010 trust boundary.
    let wire_payload: V3PayloadRead = bincode::deserialize(&content[V3_HEADER_SIZE..])
        .map_err(|e| corrupted("valid V3 bincode payload", &e.to_string()))?;

    // Validate live_bits length matches datom count.
    if wire_payload.live_bits.len() != wire_payload.datoms.len() {
        return Err(corrupted(
            &format!(
                "live_bits.len() == datoms.len() ({})",
                wire_payload.datoms.len()
            ),
            &format!("live_bits.len() = {}", wire_payload.live_bits.len()),
        ));
    }

    // Convert WireDatom → Datom via trust boundary (BLAKE3 verified above).
    let datoms: Vec<Datom> = wire_payload
        .datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();

    Ok(Store::from_checkpoint_v3(
        epoch,
        genesis_agent,
        wire_payload.schema_pairs,
        datoms,
        wire_payload.live_bits,
    ))
}
