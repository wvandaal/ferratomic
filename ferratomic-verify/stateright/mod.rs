#![forbid(unsafe_code)]
//! Stateright protocol models for ferratomic verification.
//!
//! These files stay under `ferratomic-verify/stateright/` so the Phase 2 model
//! surface is isolated from the crate wiring work that will import them later.

pub mod crdt_model;
