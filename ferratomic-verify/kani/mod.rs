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
//!
//! ## Why every harness has `#[ignore]` (bd-1uhm)
//!
//! All `#[ignore]` attributes on Kani harnesses are **architectural**, not
//! incidental. Kani harnesses compile-check under `cargo test` (verifying
//! API compatibility with the production crates), but they can only
//! **execute** under `cargo kani`, which requires the Kani toolchain.
//! Without `#[ignore]`, `cargo test` would run the stub `kani::any()`
//! and hit `unreachable!()`. The `#[ignore]` ensures CI stays green
//! while the Kani nightly gate runs them for real.

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

/// Concrete proof-surface helpers for Kani harnesses.
///
/// `kani::any::<BTreeSet<Datom>>()` doesn't work because `Datom` contains
/// `Arc<str>` which doesn't implement `kani::Arbitrary`. Instead, harnesses
/// use these helpers to build concrete datoms from symbolic u8 indices.
/// Kani explores different index values, providing bounded model checking
/// over the datom space.
pub(crate) mod helpers {
    use std::{
        collections::BTreeSet,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use ferratom::{Attribute, ClockSource, Datom, EntityId, Op, TxId, Value};

    /// Deterministic clock source for Kani harnesses.
    ///
    /// The harness supplies a bounded sequence of wall-clock readings and the
    /// real `HybridClock` algorithm consumes them through the production
    /// `ClockSource` boundary. This keeps the proof target on HLC semantics
    /// rather than on host syscalls such as `clock_gettime`.
    pub struct KaniClock<const N: usize> {
        samples: [u64; N],
        cursor: AtomicUsize,
    }

    impl<const N: usize> KaniClock<N> {
        /// Create a scripted clock from a finite list of wall-clock readings.
        #[must_use]
        pub fn new(samples: [u64; N]) -> Self {
            Self {
                samples,
                cursor: AtomicUsize::new(0),
            }
        }
    }

    impl<const N: usize> ClockSource for KaniClock<N> {
        fn now(&self) -> u64 {
            let slot = self.cursor.fetch_add(1, Ordering::Relaxed);
            let capped = slot.min(N.saturating_sub(1));
            self.samples.get(capped).copied().unwrap_or_default()
        }
    }

    /// Build a fixture `EntityId` without invoking BLAKE3.
    ///
    /// Kani harnesses that verify store, CRDT, or LIVE semantics do not need
    /// to re-prove INV-FERR-012 on every fixture. They use the test-only raw
    /// constructor so the proof target remains the invariant under test.
    #[must_use]
    pub fn proof_entity_id(id: u8) -> EntityId {
        let mut bytes = [0u8; 32];
        bytes[31] = id;
        EntityId::from_bytes(bytes)
    }

    /// Build a concrete datom from a u8 index.
    ///
    /// Deterministic: same index always produces the same datom.
    /// Different indices produce different datoms (distinct entity ids).
    pub fn concrete_datom(id: u8) -> Datom {
        Datom::new(
            proof_entity_id(id),
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
mod codec_conformance;
mod crdt_laws;
mod durability;
mod error_exhaustiveness;
mod live_reconstruction;
mod live_resolution;
mod schema_identity;
mod sharding;
mod store_views;
mod topology;
