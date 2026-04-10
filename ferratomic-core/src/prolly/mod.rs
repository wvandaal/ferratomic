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

pub mod boundary;
pub mod build;
pub mod chunk;
