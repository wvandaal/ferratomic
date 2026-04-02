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

mod backpressure_bounds;
mod clock;
mod crdt_laws;
mod durability;
mod error_exhaustiveness;
mod live_resolution;
mod schema_identity;
mod sharding;
mod store_views;
