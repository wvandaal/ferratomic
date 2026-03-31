//! `transport` module — Phase 4c (planned).
//!
//! Federation transport layer for chunk-level sync between peers.
//! Depends on prolly tree block store (Phase 4b) for content-addressed
//! chunk identification and O(|delta|) transfer.
//!
//! See spec/05-federation.md (INV-FERR-037..055) and `FERRATOMIC_ARCHITECTURE.md`.
