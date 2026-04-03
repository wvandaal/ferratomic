//! Hybrid Logical Clock (HLC) for causal ordering — re-exported from `ferratom-clock`.
//!
//! ADR-FERR-015: Clock types extracted to `ferratom-clock` crate.
//! This module re-exports all public types so downstream code continues
//! to use `ferratom::clock::*` and `ferratom::{TxId, AgentId, ...}` unchanged.

pub use ferratom_clock::{AgentId, ClockSource, Frontier, HybridClock, SystemClock, TxId};

// Tests live in ferratom-clock. This shim has no logic to test.
