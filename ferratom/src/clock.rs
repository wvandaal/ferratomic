//! Hybrid Logical Clock (HLC) for causal ordering.
//!
//! INV-FERR-015: HLC monotonicity — tick() always produces a TxId
//! strictly greater than the previous one, even under NTP clock regression.
//!
//! INV-FERR-016: HLC causality — if e₁ happens-before e₂, then
//! hlc(e₁) < hlc(e₂). Causality is defined by the predecessor graph,
//! NOT by HLC comparison (see SEED.md §4, INV-STORE-010).

// TODO(Phase 3): Implement TxId, AgentId, HybridClock, Frontier
// See spec/23-ferratomic.md §23.2 INV-FERR-015/016 for the Rust contract.
