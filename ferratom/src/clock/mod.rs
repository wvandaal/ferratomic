//! Hybrid Logical Clock (HLC) for causal ordering — re-exported from `ferratom-clock`.
//!
//! ADR-FERR-015: Clock types extracted to `ferratom-clock` crate.
//! This module re-exports all public types so downstream code continues
//! to use `ferratom::clock::*` and `ferratom::{TxId, NodeId, ...}` unchanged.
//!
//! C8 (Substrate Independence): The engine-level writer identifier is
//! `NodeId`, not `AgentId`. Application-layer code may still use
//! `:agent/*` namespace conventions on top of this primitive.

pub use ferratom_clock::{ClockSource, Frontier, HybridClock, NodeId, SystemClock, TxId};

// Tests live in ferratom-clock. This shim has no logic to test.
