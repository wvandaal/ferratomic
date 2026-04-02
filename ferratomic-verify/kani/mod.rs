//! Kani bounded model checking harnesses for Ferratomic.
//!
//! These modules mirror the Level 2 Rust contracts in
//! `spec/01-core-invariants.md` and `spec/02-concurrency.md`.
//!
//! **Dual-mode compilation**: Under the Kani verifier (`cargo kani`),
//! harnesses run as bounded proofs with symbolic execution. Under normal
//! `cargo check --all-targets` / `cargo test`, they compile as
//! `#[test] #[ignore]` functions — ensuring type-level API compatibility
//! is continuously checked without requiring the Kani toolchain.

/// Stub implementations of Kani primitives for non-Kani compilation.
///
/// Under Kani, `kani::any()` and `kani::assume()` are provided by the
/// verifier. Under normal cargo, these stubs satisfy the type checker.
/// Harnesses using them are `#[ignore]`d, so the stubs never execute.
#[cfg(not(kani))]
mod kani {
    /// Symbolic value stub — panics if called outside Kani.
    pub fn any<T>() -> T {
        unreachable!("kani::any() requires the Kani verifier — run with `cargo kani`")
    }

    /// Precondition stub — no-op outside Kani.
    pub fn assume(_condition: bool) {}
}

/// Concrete datom construction helpers for Kani harnesses.
///
/// `kani::any::<BTreeSet<Datom>>()` doesn't work because `Datom` contains
/// `Arc<str>` which doesn't implement `kani::Arbitrary`. Instead, harnesses
/// use these helpers to build concrete datoms from symbolic u8 indices.
/// Kani explores different index values, providing bounded model checking
/// over the datom space.
pub(crate) mod helpers {
    use std::collections::BTreeSet;

    use ferratom::{Attribute, Datom, EntityId, Op, TxId, Value};

    /// Build a concrete datom from a u8 index.
    ///
    /// Deterministic: same index always produces the same datom.
    /// Different indices produce different datoms (distinct entity hashes).
    pub fn concrete_datom(id: u8) -> Datom {
        Datom::new(
            EntityId::from_content(&[id]),
            Attribute::from("test/val"),
            Value::Long(i64::from(id)),
            TxId::new(u64::from(id) + 1, 0, 0),
            Op::Assert,
        )
    }

    /// Build a concrete BTreeSet of datoms from indices 0..count.
    pub fn concrete_datom_set(count: u8) -> BTreeSet<Datom> {
        (0..count).map(concrete_datom).collect()
    }
}

mod anti_entropy;
mod backpressure_bounds;
mod clock;
mod crdt_laws;
mod durability;
mod error_exhaustiveness;
mod live_resolution;
mod schema_identity;
mod sharding;
mod store_views;
mod topology;
