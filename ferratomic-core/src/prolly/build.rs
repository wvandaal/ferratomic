//! Prolly tree construction from sorted key-value pairs.
//!
//! `INV-FERR-046`: History independence — two prolly trees built from the
//! same key-value set produce identical root hashes regardless of
//! insertion order.
//!
//! `INV-FERR-045`: Chunk content addressing — every chunk is stored under
//! `addr = BLAKE3(serialized_bytes)`.

use std::collections::BTreeMap;

use ferratom::error::FerraError;

use crate::prolly::{
    boundary::is_boundary,
    chunk::{Chunk, ChunkStore, Hash},
};

/// A key-value entry: (`key_bytes`, `value_bytes`).
type KvEntry = (Vec<u8>, Vec<u8>);

/// Build a prolly tree from sorted key-value pairs.
///
/// `INV-FERR-046`: The result is independent of insertion order. `BTreeMap`
/// iteration is sorted by key, ensuring deterministic ordering.
///
/// # Errors
///
/// Returns `FerraError` if chunk storage fails.
pub fn build_prolly_tree(
    kvs: &BTreeMap<Vec<u8>, Vec<u8>>,
    chunk_store: &dyn ChunkStore,
    pattern_width: u32,
) -> Result<Hash, FerraError> {
    if kvs.is_empty() {
        let empty_leaf = serialize_leaf_chunk(&[])?;
        let chunk = Chunk::from_bytes(&empty_leaf);
        let addr = *chunk.addr();
        chunk_store.put_chunk(&chunk)?;
        return Ok(addr);
    }

    let sorted_kvs: Vec<(&Vec<u8>, &Vec<u8>)> = kvs.iter().collect();
    let leaf_groups = split_at_boundaries(&sorted_kvs, pattern_width);

    let mut leaf_addrs: Vec<(Vec<u8>, Hash)> = Vec::with_capacity(leaf_groups.len());
    for group in &leaf_groups {
        let serialized = serialize_leaf_chunk(group)?;
        let chunk = Chunk::from_bytes(&serialized);
        let addr = *chunk.addr();
        chunk_store.put_chunk(&chunk)?;

        // group is guaranteed non-empty by split_at_boundaries
        let separator = (*group[0].0).clone();
        leaf_addrs.push((separator, addr));
    }

    build_internal_nodes(&leaf_addrs, chunk_store, pattern_width, 1)
}

/// Split sorted key-value pairs into groups at boundary keys.
fn split_at_boundaries<'a>(
    sorted_kvs: &[(&'a Vec<u8>, &'a Vec<u8>)],
    pattern_width: u32,
) -> Vec<Vec<(&'a Vec<u8>, &'a Vec<u8>)>> {
    let mut groups: Vec<Vec<(&Vec<u8>, &Vec<u8>)>> = Vec::new();
    let mut current_group: Vec<(&Vec<u8>, &Vec<u8>)> = Vec::new();

    for (i, &(key, value)) in sorted_kvs.iter().enumerate() {
        current_group.push((key, value));
        let entries_since = current_group.len();

        if i < sorted_kvs.len() - 1 && is_boundary(key, pattern_width, entries_since) {
            groups.push(std::mem::take(&mut current_group));
        }
    }

    if !current_group.is_empty() {
        groups.push(current_group);
    }

    groups
}

/// Serialize a leaf chunk to bytes.
///
/// Format: `[0x01][u32 LE count][entries...]` where each entry is
/// `[u32 LE key_len][key bytes][u32 LE value_len][value bytes]`.
///
/// `INV-FERR-045a`: Deterministic — same entries always produce same bytes.
///
/// Returns `FerraError::Validation` if any length exceeds `u32::MAX`.
/// This makes overflow a type-checked error path per GOALS.md §5
/// (invalid states must be unrepresentable).
fn serialize_leaf_chunk(entries: &[(&Vec<u8>, &Vec<u8>)]) -> Result<Vec<u8>, FerraError> {
    let mut buf = Vec::new();
    buf.push(0x01);

    let count = len_u32(entries.len())?;
    buf.extend_from_slice(&count.to_le_bytes());

    for (key, value) in entries {
        let key_len = len_u32(key.len())?;
        buf.extend_from_slice(&key_len.to_le_bytes());
        buf.extend_from_slice(key);

        let value_len = len_u32(value.len())?;
        buf.extend_from_slice(&value_len.to_le_bytes());
        buf.extend_from_slice(value);
    }

    Ok(buf)
}

/// Serialize an internal chunk to bytes.
///
/// Format: `[0x02][u8 level][u32 LE count][children...]` where each child
/// is `[u32 LE sep_len][sep bytes][32 bytes hash]`.
fn serialize_internal_chunk(
    level: u8,
    children: &[(Vec<u8>, Hash)],
) -> Result<Vec<u8>, FerraError> {
    let mut buf = Vec::new();
    buf.push(0x02);
    buf.push(level);

    let count = len_u32(children.len())?;
    buf.extend_from_slice(&count.to_le_bytes());

    for (separator, hash) in children {
        let sep_len = len_u32(separator.len())?;
        buf.extend_from_slice(&sep_len.to_le_bytes());
        buf.extend_from_slice(separator);
        buf.extend_from_slice(hash);
    }

    Ok(buf)
}

/// Build internal nodes recursively until a single root remains.
///
/// `level` tracks the tree depth: 1 = one above leaves, 2 = two above, etc.
fn build_internal_nodes(
    child_addrs: &[(Vec<u8>, Hash)],
    chunk_store: &dyn ChunkStore,
    pattern_width: u32,
    level: u8,
) -> Result<Hash, FerraError> {
    if child_addrs.len() == 1 {
        return Ok(child_addrs[0].1);
    }

    let mut groups: Vec<Vec<(Vec<u8>, Hash)>> = Vec::new();
    let mut current: Vec<(Vec<u8>, Hash)> = Vec::new();

    for (i, (sep, hash)) in child_addrs.iter().enumerate() {
        current.push((sep.clone(), *hash));
        let entries_since = current.len();

        if i < child_addrs.len() - 1 && is_boundary(sep, pattern_width, entries_since) {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let mut next_level_addrs: Vec<(Vec<u8>, Hash)> = Vec::with_capacity(groups.len());
    for group in &groups {
        let serialized = serialize_internal_chunk(level, group)?;
        let chunk = Chunk::from_bytes(&serialized);
        let addr = *chunk.addr();
        chunk_store.put_chunk(&chunk)?;

        // group is guaranteed non-empty by the split loop above
        let separator = group[0].0.clone();
        next_level_addrs.push((separator, addr));
    }

    build_internal_nodes(
        &next_level_addrs,
        chunk_store,
        pattern_width,
        level.saturating_add(1),
    )
}

/// Deserialize a leaf chunk back to key-value pairs.
///
/// `INV-FERR-045a` round-trip: `deserialize(serialize(entries)) = entries`.
///
/// # Errors
///
/// Returns `FerraError::TruncatedChunk` if the data is too short,
/// `FerraError::UnknownCodecTag` if the tag byte is not `0x01`,
/// `FerraError::TrailingBytes` if there are leftover bytes.
pub fn deserialize_leaf_chunk(data: &[u8]) -> Result<Vec<KvEntry>, FerraError> {
    if data.is_empty() {
        return Err(FerraError::EmptyChunk);
    }
    if data[0] != 0x01 {
        return Err(FerraError::UnknownCodecTag(data[0]));
    }
    if data.len() < 5 {
        return Err(FerraError::TruncatedChunk);
    }

    let count = u32::from_le_bytes(read4(data, 1)?) as usize;
    let mut entries = Vec::with_capacity(count);
    let mut pos = 5;

    for _ in 0..count {
        let key_len = u32::from_le_bytes(read4(data, pos)?) as usize;
        pos += 4;
        check_len(data, pos, key_len)?;
        let key = data[pos..pos + key_len].to_vec();
        pos += key_len;

        let value_len = u32::from_le_bytes(read4(data, pos)?) as usize;
        pos += 4;
        check_len(data, pos, value_len)?;
        let value = data[pos..pos + value_len].to_vec();
        pos += value_len;

        entries.push((key, value));
    }

    if pos != data.len() {
        return Err(FerraError::TrailingBytes);
    }

    Ok(entries)
}

/// Decode child addresses from an internal chunk.
///
/// `INV-FERR-048`: Used by `RecursiveTransfer` to walk the tree.
/// Returns empty vec for leaf chunks (leaves have no children).
///
/// # Errors
///
/// Returns `FerraError::TruncatedChunk` if the chunk data is truncated,
/// `FerraError::UnknownCodecTag` if the tag is unrecognized.
pub fn decode_child_addrs(chunk: &Chunk) -> Result<Vec<Hash>, FerraError> {
    let data = chunk.data();
    if data.is_empty() {
        return Err(FerraError::EmptyChunk);
    }

    match data[0] {
        0x01 => Ok(Vec::new()),
        0x02 => {
            if data.len() < 6 {
                return Err(FerraError::TruncatedChunk);
            }
            // data[1] is the level byte — not needed for address decoding
            let count = u32::from_le_bytes(read4(data, 2)?) as usize;

            let mut addrs = Vec::with_capacity(count);
            let mut pos = 6;

            for _ in 0..count {
                let sep_len = u32::from_le_bytes(read4(data, pos)?) as usize;
                pos += 4 + sep_len;
                check_len(data, pos, 32)?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&data[pos..pos + 32]);
                addrs.push(hash);
                pos += 32;
            }

            Ok(addrs)
        }
        tag => Err(FerraError::UnknownCodecTag(tag)),
    }
}

/// Convert a `usize` length to `u32`, returning `FerraError::Validation` on overflow.
///
/// GOALS.md §5 (Curry-Howard-Lambek): invalid states must be unrepresentable.
/// The wire format uses u32 for lengths — this function makes overflow a
/// checked error path rather than a silent truncation.
fn len_u32(len: usize) -> Result<u32, FerraError> {
    u32::try_from(len).map_err(|_| FerraError::InvariantViolation {
        invariant: "INV-FERR-045".to_string(),
        details: format!("length {len} exceeds u32::MAX for wire format"),
    })
}

/// Read 4 bytes from `data` at `offset`, returning `TruncatedChunk` on failure.
fn read4(data: &[u8], offset: usize) -> Result<[u8; 4], FerraError> {
    if offset + 4 > data.len() {
        return Err(FerraError::TruncatedChunk);
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&data[offset..offset + 4]);
    Ok(buf)
}

/// Check that `data` has at least `need` bytes from `offset`.
fn check_len(data: &[u8], offset: usize, need: usize) -> Result<(), FerraError> {
    if offset + need > data.len() {
        return Err(FerraError::TruncatedChunk);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prolly::{boundary::DEFAULT_PATTERN_WIDTH, chunk::MemoryChunkStore};

    #[test]
    fn test_inv_ferr_045a_leaf_roundtrip() {
        let k1 = vec![1u8, 2, 3];
        let v1 = vec![10u8, 20];
        let k2 = vec![4u8, 5, 6];
        let v2 = vec![30u8, 40, 50];
        let entries: Vec<(&Vec<u8>, &Vec<u8>)> = vec![(&k1, &v1), (&k2, &v2)];
        let serialized = serialize_leaf_chunk(&entries).expect("serialize");
        let deserialized = deserialize_leaf_chunk(&serialized).expect("round-trip must succeed");

        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized[0].0, vec![1, 2, 3]);
        assert_eq!(deserialized[0].1, vec![10, 20]);
        assert_eq!(deserialized[1].0, vec![4, 5, 6]);
        assert_eq!(deserialized[1].1, vec![30, 40, 50]);
    }

    #[test]
    fn test_inv_ferr_045a_leaf_deterministic() {
        let k = vec![1u8, 2];
        let v = vec![3u8, 4];
        let entries: Vec<(&Vec<u8>, &Vec<u8>)> = vec![(&k, &v)];
        let s1 = serialize_leaf_chunk(&entries).expect("s1");
        let s2 = serialize_leaf_chunk(&entries).expect("s2");
        assert_eq!(s1, s2, "INV-FERR-045a: serialization must be deterministic");
    }

    #[test]
    fn test_inv_ferr_046_history_independence() {
        let store1 = MemoryChunkStore::new();
        let store2 = MemoryChunkStore::new();

        let mut kvs = BTreeMap::new();
        for i in 0u32..50 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8; 4]);
        }

        let root1 =
            build_prolly_tree(&kvs, &store1, DEFAULT_PATTERN_WIDTH).expect("build must succeed");
        let root2 =
            build_prolly_tree(&kvs, &store2, DEFAULT_PATTERN_WIDTH).expect("build must succeed");

        assert_eq!(
            root1, root2,
            "INV-FERR-046: same key-value set must produce same root hash"
        );
    }

    #[test]
    fn test_inv_ferr_046_empty_tree() {
        let store = MemoryChunkStore::new();
        let kvs: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH)
            .expect("empty tree build must succeed");
        let root2 = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH)
            .expect("empty tree build must succeed");
        assert_eq!(root, root2, "empty tree must have deterministic root");
    }

    #[test]
    fn test_inv_ferr_045_content_addressing_in_tree() {
        let store = MemoryChunkStore::new();
        let mut kvs = BTreeMap::new();
        for i in 0u32..100 {
            kvs.insert(i.to_be_bytes().to_vec(), vec![0u8]);
        }

        let root = build_prolly_tree(&kvs, &store, DEFAULT_PATTERN_WIDTH).expect("build");
        let root_chunk = store
            .get_chunk(&root)
            .expect("get")
            .expect("root must exist");
        assert_eq!(
            root_chunk.addr(),
            &root,
            "INV-FERR-045: stored chunk address must equal computed address"
        );
    }

    #[test]
    fn test_decode_child_addrs_leaf() {
        let k = vec![1u8];
        let v = vec![2u8];
        let leaf = serialize_leaf_chunk(&[(&k, &v)]).expect("serialize");
        let chunk = Chunk::from_bytes(&leaf);
        let addrs = decode_child_addrs(&chunk).expect("decode leaf");
        assert!(addrs.is_empty(), "leaf chunks have no children");
    }

    #[test]
    fn test_decode_child_addrs_internal() {
        let children = vec![
            (vec![1u8, 2, 3], [0xAAu8; 32]),
            (vec![4u8, 5, 6], [0xBBu8; 32]),
        ];
        let serialized = serialize_internal_chunk(1, &children).expect("serialize");
        let chunk = Chunk::from_bytes(&serialized);
        let addrs = decode_child_addrs(&chunk).expect("decode internal");

        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0], [0xAA; 32]);
        assert_eq!(addrs[1], [0xBB; 32]);
    }

    // DEFECT-003 regression tests: deserialization error paths
    #[test]
    fn test_deserialize_empty_input() {
        assert!(matches!(
            deserialize_leaf_chunk(&[]),
            Err(FerraError::EmptyChunk)
        ));
    }

    #[test]
    fn test_deserialize_wrong_tag() {
        assert!(matches!(
            deserialize_leaf_chunk(&[0xFF, 0, 0, 0, 0]),
            Err(FerraError::UnknownCodecTag(0xFF))
        ));
    }

    #[test]
    fn test_deserialize_truncated() {
        // Tag byte only — missing count
        assert!(matches!(
            deserialize_leaf_chunk(&[0x01]),
            Err(FerraError::TruncatedChunk)
        ));
    }

    #[test]
    fn test_deserialize_trailing_bytes() {
        // Valid empty chunk (tag=0x01, count=0) + extra byte
        assert!(matches!(
            deserialize_leaf_chunk(&[0x01, 0, 0, 0, 0, 0xFF]),
            Err(FerraError::TrailingBytes)
        ));
    }
}
