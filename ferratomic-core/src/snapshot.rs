//! `snapshot` module — Phase 4b (planned).
//!
//! Phase 4a (current) implements snapshots directly via `ArcSwap<Store>` in `db.rs`
//! and `Store::snapshot()` in `store.rs`. This module is reserved for Phase 4b,
//! which will add dedicated snapshot types with prolly-tree-backed storage and
//! lazy index materialization.
//!
//! See `FERRATOMIC_ARCHITECTURE.md` and spec/02-concurrency.md (INV-FERR-006).
