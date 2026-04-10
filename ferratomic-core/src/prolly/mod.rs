//! # Prolly Tree — Content-Addressed Merkle B-Tree
//!
//! Content-addressed, history-independent sorted key-value store using
//! rolling-hash-determined chunk boundaries. Implements INV-FERR-045
//! through INV-FERR-050e.
//!
//! ## Module structure
//!
//! - [`chunk`]: `Chunk` type and `ChunkStore` trait (INV-FERR-045, 050)
//! - [`boundary`]: Gear hash and chunk boundary detection (INV-FERR-046a)
//! - [`build`]: Prolly tree construction from sorted key-value pairs (INV-FERR-046)
//! - [`read`]: Prolly tree traversal from root hash to key-value pairs (INV-FERR-049)
//! - [`diff`]: O(d) diff between two prolly trees (INV-FERR-047)
//! - [`transfer`]: Chunk-based federation transfer (INV-FERR-048)
//! - [`snapshot`]: Snapshot manifest — `RootSet` serialization and CAS (INV-FERR-049, 050b)

pub mod boundary;
pub mod build;
pub mod chunk;
pub mod diff;
pub mod read;
pub mod snapshot;
pub mod transfer;
