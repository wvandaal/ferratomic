//! Positional content addressing (INV-FERR-076).
//!
//! Every datom in the store has a canonical position `p : u32` in the
//! sorted canonical array. Positions serve as internal addresses for
//! index permutations, LIVE bitvector, and merge bookkeeping.
//!
//! This is a faithful functor from the datom semilattice to the natural
//! number ordering: same datom set -> same sort -> same positions.
//!
//! INV-FERR-076: positional determinism, stability under append,
//! LIVE as bitvector, merge as merge-sort.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod bloom;
pub mod chunk_fingerprints;
mod fingerprint;
pub mod live;
pub mod merge;
pub(crate) mod mph;
pub(crate) mod perm;
mod search;
pub mod store;
pub mod succinct;

#[cfg(test)]
mod tests;

// Public re-exports for downstream crates.
pub use chunk_fingerprints::{ChunkFingerprints, DEFAULT_CHUNK_SIZE};
pub use live::build_live_bitvector_pub;
#[cfg(any(test, feature = "test-utils"))]
pub use live::{live_positions_for_test, live_positions_from_sorted_run_keys_for_test};
pub use merge::{merge_positional, merge_sort_dedup};
pub use store::PositionalStore;
pub use succinct::SuccinctBitVec;
