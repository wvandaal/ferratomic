//! Index key types, `IndexBackend` trait, and `GenericIndexes` struct.
//!
//! INV-FERR-005: four secondary indexes maintained in bijection with
//! the primary datom set. Each uses a distinct key type whose `Ord`
//! implementation arranges datom fields in index-specific order.
//!
//! INV-FERR-025: the index backend is interchangeable via the
//! [`IndexBackend`] trait. All backends produce identical query results.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![warn(clippy::pedantic)]

mod backend;
mod indexes;
mod keys;

pub use backend::{IndexBackend, SortedVecBackend};
pub use indexes::{GenericIndexes, Indexes, SortedVecIndexes};
pub use keys::{AevtKey, AvetKey, EavtKey, VaetKey};
