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
//! # LIVE-First File Format (INV-FERR-075)
//!
//! ```text
//! +------------------+
//! | Magic    (4B)    | 0x43484B33 ("CHK3") — same as standard V3
//! +------------------+
//! | Version  (2B)    | 0x0103 (little-endian, LIVE-first V3)
//! +------------------+
//! | Epoch    (8B)    | u64 little-endian
//! +------------------+
//! | Genesis  (16B)   | AgentId bytes
//! +------------------+
//! | Payload  (N)     | bincode: V3LiveFirstPayloadWrite/Read
//! |  schema_pairs    |   sorted (String, AttributeDef) pairs
//! |  live_datoms     |   EAVT-sorted LIVE datoms (loaded at cold start)
//! |  hist_datoms     |   EAVT-sorted historical datoms (loaded on demand)
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

/// V3 standard format version.
const V3_VERSION: u16 = 3;

/// V3 LIVE-first format version (INV-FERR-075).
///
/// Same magic (`CHK3`) as standard V3, distinguished by version field.
/// Stores LIVE datoms first, historical datoms second, as separate payload
/// fields. Enables partial cold start: load LIVE-only store, defer history.
pub(crate) const V3_LIVE_FIRST_VERSION: u16 = 0x0103;

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

/// Parse V3 header allowing either standard or LIVE-first version.
///
/// Returns `(epoch, genesis_agent, version)`. Does NOT validate the version
/// field — the caller is responsible for version dispatch.
fn parse_v3_header_versioned(content: &[u8]) -> Result<(u64, AgentId, u16), FerraError> {
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
    let epoch = u64::from_le_bytes(
        content[6..14]
            .try_into()
            .map_err(|_| corrupted("8-byte epoch", "truncated"))?,
    );
    let genesis_bytes: [u8; 16] = content[14..30]
        .try_into()
        .map_err(|_| corrupted("16-byte genesis agent", "truncated"))?;
    Ok((epoch, AgentId::from_bytes(genesis_bytes), version))
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

    Store::from_checkpoint_v3(
        epoch,
        genesis_agent,
        wire_payload.schema_pairs,
        datoms,
        wire_payload.live_bits,
    )
}

// ---------------------------------------------------------------------------
// LIVE-first V3 variant (INV-FERR-075)
// ---------------------------------------------------------------------------

/// Serialization payload for LIVE-first V3 checkpoint (INV-FERR-075).
///
/// LIVE datoms are a separate field from historical datoms. Both partitions
/// preserve EAVT sort order (subsequences of the canonical array).
/// ADR-FERR-010: uses core `Datom` (`Serialize` only).
#[derive(Serialize)]
struct V3LiveFirstPayloadWrite {
    /// Schema attributes sorted by name for deterministic output.
    schema_pairs: Vec<(String, AttributeDef)>,
    /// LIVE datoms in canonical EAVT order (INV-FERR-029).
    live_datoms: Vec<Datom>,
    /// Historical (non-LIVE) datoms in canonical EAVT order (INV-FERR-075).
    hist_datoms: Vec<Datom>,
}

/// Deserialization payload for LIVE-first V3 checkpoint (INV-FERR-075).
///
/// ADR-FERR-010: uses `WireDatom` for trust boundary enforcement.
#[derive(Deserialize)]
struct V3LiveFirstPayloadRead {
    /// Schema attributes (INV-FERR-075).
    schema_pairs: Vec<(String, AttributeDef)>,
    /// LIVE datoms in wire format (INV-FERR-075, INV-FERR-029).
    live_datoms: Vec<ferratom::wire::WireDatom>,
    /// Historical datoms in wire format (INV-FERR-075).
    hist_datoms: Vec<ferratom::wire::WireDatom>,
}

/// Serialize a store to LIVE-first V3 checkpoint bytes (INV-FERR-075).
///
/// INV-FERR-075: LIVE datoms are the first field in the payload. Both
/// partitions preserve EAVT sort order. The version field (0x0103)
/// distinguishes from standard V3 (0x0003).
///
/// # Errors
///
/// Returns `FerraError::CheckpointWrite` if serialization fails.
pub(crate) fn serialize_v3_live_first(store: &Store) -> Result<Vec<u8>, FerraError> {
    let epoch = store.epoch();
    let genesis_agent = store.genesis_agent();

    // Build PositionalStore to get canonical order + live_bits.
    let datoms: Vec<Datom> = store.datoms().cloned().collect();
    let live_bits = match &store.repr {
        StoreRepr::Positional(ps) => ps.live_bits_clone(),
        StoreRepr::OrdMap { .. } => build_live_bitvector_pub(&datoms),
    };

    // Partition datoms by LIVE status. Both partitions are EAVT-sorted
    // (subsequences of the canonical order).
    //
    // Refinement note: the spec (INV-FERR-075 Level 0) defines LIVE_datoms(S)
    // as ALL datoms whose (e,a,v) triple is in the LIVE set. This implementation
    // uses the narrower witness-only partition: only the single latest-Assert
    // datom per (e,a,v) group is placed in live_datoms. This is correct because
    // the LIVE functional property eval(Q, S) = eval(Q, LIVE_datoms(S)) holds
    // for the witness-only subset — the witness alone determines the LIVE view.
    let mut live_datoms = Vec::new();
    let mut hist_datoms = Vec::new();
    for (i, datom) in datoms.into_iter().enumerate() {
        if live_bits.get(i).as_deref() == Some(&true) {
            live_datoms.push(datom);
        } else {
            hist_datoms.push(datom);
        }
    }

    // Sort schema pairs by attribute name for deterministic output.
    let schema_pairs: Vec<(String, AttributeDef)> = {
        let mut sorted: BTreeMap<String, AttributeDef> = BTreeMap::new();
        for (attr, def) in store.schema().iter() {
            sorted.insert(attr.as_str().to_owned(), def.clone());
        }
        sorted.into_iter().collect()
    };

    let payload = V3LiveFirstPayloadWrite {
        schema_pairs,
        live_datoms,
        hist_datoms,
    };

    let payload_bytes =
        bincode::serialize(&payload).map_err(|e| FerraError::CheckpointWrite(e.to_string()))?;

    let total_size = V3_HEADER_SIZE + payload_bytes.len() + HASH_SIZE;
    let mut buf = Vec::with_capacity(total_size);

    // Header: same structure as standard V3, different version.
    buf.extend_from_slice(&V3_MAGIC);
    buf.extend_from_slice(&V3_LIVE_FIRST_VERSION.to_le_bytes());
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(genesis_agent.as_bytes());

    buf.extend_from_slice(&payload_bytes);

    let hash = blake3::hash(&buf);
    buf.extend_from_slice(hash.as_bytes());

    Ok(buf)
}

/// LIVE-only store with retained historical datoms for lazy merge (INV-FERR-075).
///
/// Created by `deserialize_v3_live_first_partial()`. The `store` field
/// contains only LIVE datoms. Call `load_historical()` to merge with
/// retained historical datoms and produce the full store.
pub struct PartialStore {
    /// Store built from LIVE datoms only (INV-FERR-029).
    store: Store,
    /// Historical datoms (already trusted — BLAKE3 verified at load).
    hist_datoms: Vec<Datom>,
}

impl PartialStore {
    /// Access the LIVE-only store for current-state queries (INV-FERR-075, INV-FERR-029).
    ///
    /// The returned store contains only LIVE datoms — the latest Assert for each
    /// `(entity, attribute, value)` group. Sufficient for applications that need
    /// only the current state. Call `load_historical()` to merge retained
    /// historical datoms when temporal queries are needed.
    #[must_use]
    pub fn live_store(&self) -> &Store {
        &self.store
    }

    /// Merge LIVE + HISTORICAL datoms into complete Store (INV-FERR-075).
    ///
    /// Five O(n) passes: merge-sort, positional construction, LIVE bitvector,
    /// `live_causal` rebuild, `live_set` derivation. Uses `from_checkpoint_v3`
    /// to avoid redundant O(n log n) re-sort on already-sorted merge output.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the merged datoms violate
    /// INV-FERR-076 preconditions (should not happen with valid checkpoint data).
    pub fn load_historical(self) -> Result<Store, FerraError> {
        let live_datoms: Vec<Datom> = self.store.datoms().cloned().collect();
        let merged = crate::positional::merge_sort_dedup(&live_datoms, &self.hist_datoms);
        let live_bits = build_live_bitvector_pub(&merged);
        let schema_pairs: Vec<(String, AttributeDef)> = self
            .store
            .schema()
            .iter()
            .map(|(a, d)| (a.as_str().to_owned(), d.clone()))
            .collect();
        Store::from_checkpoint_v3(
            self.store.epoch(),
            self.store.genesis_agent(),
            schema_pairs,
            merged,
            live_bits,
        )
    }
}

/// Deserialize a LIVE-first V3 checkpoint into a `PartialStore` (INV-FERR-075).
///
/// Loads LIVE datoms and builds a LIVE-only Store. Historical datoms are
/// retained for lazy `load_historical()`. BLAKE3 verified before deserialization.
///
/// # Errors
///
/// Returns `FerraError::CheckpointCorrupted` on checksum mismatch, version
/// mismatch, or deserialization failure.
pub(crate) fn deserialize_v3_live_first_partial(data: &[u8]) -> Result<PartialStore, FerraError> {
    let content = verify_v3_checksum(data)?;
    let (epoch, genesis_agent, version) = parse_v3_header_versioned(content)?;
    if version != V3_LIVE_FIRST_VERSION {
        return Err(corrupted(
            &format!("version {V3_LIVE_FIRST_VERSION:#06x} (LIVE-first)"),
            &format!("version {version:#06x}"),
        ));
    }

    let wire_payload: V3LiveFirstPayloadRead = bincode::deserialize(&content[V3_HEADER_SIZE..])
        .map_err(|e| corrupted("valid V3 LIVE-first bincode payload", &e.to_string()))?;

    // Trust boundary: WireDatom → Datom (BLAKE3 verified above).
    let live_datoms: Vec<Datom> = wire_payload
        .live_datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();
    let hist_datoms: Vec<Datom> = wire_payload
        .hist_datoms
        .into_iter()
        .map(ferratom::wire::WireDatom::into_trusted)
        .collect();

    // Sort-order integrity: both live_datoms and hist_datoms are subsequences of
    // the serialized canonical EAVT order. BLAKE3 verification (above) guarantees
    // payload integrity with 2^-128 collision probability (ADR-FERR-010).
    // merge_sort_dedup's debug_assert validates sort order in test/debug builds.
    // An O(n) production sort-order check is omitted: BLAKE3 is the sole defense,
    // consistent with the standard V3 deserialization path.

    // Build LIVE-only Store via zero-construction path. LIVE datoms are already
    // EAVT-sorted (subsequence of canonical). All bits are true (every datom in
    // the LIVE partition is live by definition).
    let live_bits = bitvec::prelude::BitVec::repeat(true, live_datoms.len());
    let store = Store::from_checkpoint_v3(
        epoch,
        genesis_agent,
        wire_payload.schema_pairs,
        live_datoms,
        live_bits,
    )?;

    Ok(PartialStore { store, hist_datoms })
}

/// Deserialize a LIVE-first V3 checkpoint into a full Store (INV-FERR-075).
///
/// Convenience wrapper: loads partial, then immediately merges historical
/// datoms. Use `deserialize_v3_live_first_partial` if you want LIVE-only access.
pub(crate) fn deserialize_v3_live_first_full(data: &[u8]) -> Result<Store, FerraError> {
    let partial = deserialize_v3_live_first_partial(data)?;
    partial.load_historical()
}
