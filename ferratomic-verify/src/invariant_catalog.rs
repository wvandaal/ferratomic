//! Machine-readable invariant catalog (ADR-FERR-013).
//!
//! Enumerates every INV-FERR and CI-FERR invariant from the canonical spec
//! (`spec/`) with traceability links to each verification layer: Lean 4
//! theorems, proptest properties, Kani harnesses, Stateright models, and
//! integration tests.
//!
//! The catalog is a compile-time constant array so that coverage queries
//! are zero-cost and cannot drift from the source of truth.

/// Verification stage for phased gate closure.
///
/// Stage boundaries align with the phase gate beads:
/// - Stage 0: core + concurrency + performance (INV-FERR-001..032)
/// - Stage 1: decisions + federation + prolly tree (INV-FERR-033..050)
/// - Stage 2: VKN + verification infrastructure (INV-FERR-051..059)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Core, concurrency, and performance invariants.
    Stage0,
    /// Decision, federation, and prolly tree invariants.
    Stage1,
    /// VKN and verification infrastructure invariants.
    Stage2,
}

/// A single invariant entry with links to each verification layer.
#[derive(Debug, Clone, Copy)]
pub struct Invariant {
    /// Canonical identifier (e.g. "INV-FERR-001" or "CI-FERR-001").
    pub id: &'static str,
    /// Short human-readable name from the spec header.
    pub name: &'static str,
    /// Phase gate stage this invariant belongs to.
    pub stage: Stage,
    /// Lean 4 theorem name in `ferratomic-verify/lean/Ferratomic/`, if proven.
    pub lean_theorem: Option<&'static str>,
    /// Proptest function name in `ferratomic-verify/proptest/`, if exists.
    pub proptest_fn: Option<&'static str>,
    /// Kani proof harness name in `ferratomic-verify/kani/`, if exists.
    pub kani_harness: Option<&'static str>,
    /// Stateright model-checked test name in `ferratomic-verify/stateright/`, if exists.
    pub stateright_model: Option<&'static str>,
    /// Integration test name in `ferratomic-verify/integration/`, if exists.
    pub integration_test: Option<&'static str>,
}

impl Invariant {
    /// Returns `true` if at least one verification layer covers this invariant.
    #[must_use]
    pub const fn has_any_test(&self) -> bool {
        self.lean_theorem.is_some()
            || self.proptest_fn.is_some()
            || self.kani_harness.is_some()
            || self.stateright_model.is_some()
            || self.integration_test.is_some()
    }
}

/// Complete catalog of all 61 invariants (59 INV-FERR + 2 CI-FERR).
///
/// Order matches spec module order: 01-core, 02-concurrency, 03-performance,
/// 04-decisions, 05-federation, 06-prolly, 07-refinement, 08-verification.
pub const CATALOG: &[Invariant] = &[
    // -----------------------------------------------------------------------
    // 01-core-invariants.md: INV-FERR-001..012 (Stage 0)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-001",
        name: "Merge Commutativity",
        stage: Stage::Stage0,
        lean_theorem: Some("merge_comm"),
        proptest_fn: Some("inv_ferr_001_merge_commutativity"),
        kani_harness: Some("merge_commutativity"),
        stateright_model: Some("inv_ferr_001_merge_commutativity_model"),
        integration_test: Some("inv_ferr_001_merge_commutes_concrete"),
    },
    Invariant {
        id: "INV-FERR-002",
        name: "Merge Associativity",
        stage: Stage::Stage0,
        lean_theorem: Some("merge_assoc"),
        proptest_fn: Some("inv_ferr_002_merge_associativity"),
        kani_harness: Some("merge_associativity"),
        stateright_model: Some("inv_ferr_002_merge_associativity_model"),
        integration_test: Some("inv_ferr_002_merge_associates_concrete"),
    },
    Invariant {
        id: "INV-FERR-003",
        name: "Merge Idempotency",
        stage: Stage::Stage0,
        lean_theorem: Some("merge_idemp"),
        proptest_fn: Some("inv_ferr_003_merge_idempotency"),
        kani_harness: Some("merge_idempotency"),
        stateright_model: Some("inv_ferr_003_merge_idempotency_model"),
        integration_test: Some("inv_ferr_003_merge_idempotent_concrete"),
    },
    Invariant {
        id: "INV-FERR-004",
        name: "Monotonic Growth",
        stage: Stage::Stage0,
        lean_theorem: Some("apply_superset"),
        proptest_fn: Some("inv_ferr_004_monotonic_growth_transact"),
        kani_harness: Some("monotonic_growth"),
        stateright_model: None,
        integration_test: Some("inv_ferr_004_transact_grows_store"),
    },
    Invariant {
        id: "INV-FERR-005",
        name: "Index Bijection",
        stage: Stage::Stage0,
        lean_theorem: Some("index_bijection_card"),
        proptest_fn: Some("inv_ferr_005_index_bijection_after_transactions"),
        kani_harness: Some("index_bijection"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_005_bijection_after_transact"),
    },
    Invariant {
        id: "INV-FERR-006",
        name: "Snapshot Isolation",
        stage: Stage::Stage0,
        lean_theorem: Some("snapshot_stable_under_future_write"),
        proptest_fn: Some("inv_ferr_006_snapshot_sees_no_future_txns"),
        kani_harness: Some("snapshot_isolation"),
        stateright_model: Some("test_inv_ferr_006_visible_datoms_at_epoch_single_commit"),
        integration_test: Some("inv_ferr_006_snapshot_stability"),
    },
    Invariant {
        id: "INV-FERR-007",
        name: "Write Linearizability",
        stage: Stage::Stage0,
        lean_theorem: Some("next_epoch_strict"),
        proptest_fn: Some("inv_ferr_007_epochs_strictly_increase"),
        kani_harness: Some("write_linearizability"),
        stateright_model: Some("test_inv_ferr_007_single_write_commits"),
        integration_test: Some("inv_ferr_007_epoch_ordering"),
    },
    Invariant {
        id: "INV-FERR-008",
        name: "WAL Fsync Ordering",
        stage: Stage::Stage0,
        lean_theorem: Some("wal_fsync_before_publish"),
        proptest_fn: Some("inv_ferr_008_wal_roundtrip"),
        kani_harness: Some("kani_inv_ferr_008_wal_fsync_ordering"),
        stateright_model: None,
        integration_test: Some("inv_ferr_008_wal_write_and_recover"),
    },
    Invariant {
        id: "INV-FERR-009",
        name: "Schema Validation",
        stage: Stage::Stage0,
        lean_theorem: Some("schema_valid_implies_success"),
        proptest_fn: Some("inv_ferr_009_valid_datoms_accepted"),
        kani_harness: Some("schema_rejects_unknown_attr"),
        stateright_model: None,
        integration_test: Some("inv_ferr_009_genesis_schema"),
    },
    Invariant {
        id: "INV-FERR-010",
        name: "Merge Convergence",
        stage: Stage::Stage0,
        lean_theorem: Some("merge_convergence"),
        proptest_fn: Some("inv_ferr_010_convergence"),
        kani_harness: Some("convergence_two_replicas"),
        stateright_model: Some("inv_ferr_010_model_checker_finds_a_converged_state"),
        integration_test: Some("inv_ferr_010_convergence_three_replicas"),
    },
    Invariant {
        id: "INV-FERR-011",
        name: "Observer Monotonicity",
        stage: Stage::Stage0,
        lean_theorem: Some("observer_monotone"),
        proptest_fn: Some("inv_ferr_011_observer_never_regresses"),
        kani_harness: Some("observer_monotonicity"),
        stateright_model: None,
        integration_test: Some("inv_ferr_011_observer_epoch_monotonic"),
    },
    Invariant {
        id: "INV-FERR-012",
        name: "Content-Addressed Identity",
        stage: Stage::Stage0,
        lean_theorem: Some("content_identity"),
        proptest_fn: Some("inv_ferr_012_content_addressed_identity"),
        kani_harness: Some("content_identity"),
        stateright_model: None,
        integration_test: Some("inv_ferr_012_same_content_same_id"),
    },
    // -----------------------------------------------------------------------
    // 02-concurrency.md: INV-FERR-013..024 (Stage 0)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-013",
        name: "Checkpoint Equivalence",
        stage: Stage::Stage0,
        lean_theorem: Some("checkpoint_roundtrip"),
        proptest_fn: Some("inv_ferr_013_checkpoint_roundtrip"),
        kani_harness: Some("checkpoint_roundtrip"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_013_checkpoint_corruption"),
    },
    Invariant {
        id: "INV-FERR-014",
        name: "Recovery Correctness",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_014_wal_recovery_correctness"),
        kani_harness: Some("recovery_superset"),
        stateright_model: Some("inv_ferr_014_committed_data_survives_crash"),
        integration_test: Some("test_inv_ferr_014_crash_then_transact"),
    },
    Invariant {
        id: "INV-FERR-015",
        name: "HLC Monotonicity",
        stage: Stage::Stage0,
        lean_theorem: Some("hlc_tick_monotone"),
        proptest_fn: Some("inv_ferr_015_hlc_monotonicity"),
        kani_harness: Some("hlc_monotonicity"),
        stateright_model: None,
        integration_test: Some("inv_ferr_015_hlc_tick_monotonic"),
    },
    Invariant {
        id: "INV-FERR-016",
        name: "HLC Causality",
        stage: Stage::Stage0,
        lean_theorem: Some("hlc_receive_gt_remote"),
        proptest_fn: Some("inv_ferr_016_hlc_causality"),
        kani_harness: Some("hlc_causality"),
        stateright_model: None,
        integration_test: Some("inv_ferr_016_hlc_causality_two_agents"),
    },
    Invariant {
        id: "INV-FERR-017",
        name: "Shard Equivalence",
        stage: Stage::Stage0,
        lean_theorem: Some("shard_union"),
        proptest_fn: Some("inv_ferr_017_shard_equivalence"),
        kani_harness: Some("shard_equivalence"),
        stateright_model: None,
        integration_test: Some("inv_ferr_017_shard_equivalence_concrete"),
    },
    Invariant {
        id: "INV-FERR-018",
        name: "Append-Only",
        stage: Stage::Stage0,
        lean_theorem: Some("append_only_merge_left"),
        proptest_fn: Some("inv_ferr_018_transact_monotonic_growth"),
        kani_harness: Some("append_only"),
        stateright_model: None,
        integration_test: Some("inv_ferr_018_retract_adds_datom"),
    },
    Invariant {
        id: "INV-FERR-019",
        name: "Error Exhaustiveness",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_019_typed_errors"),
        kani_harness: Some("error_display_non_empty"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_019_error_exhaustiveness"),
    },
    Invariant {
        id: "INV-FERR-020",
        name: "Transaction Atomicity",
        stage: Stage::Stage0,
        lean_theorem: Some("transaction_epoch_uniform"),
        proptest_fn: Some("inv_ferr_020_transaction_single_epoch"),
        kani_harness: Some("transaction_atomicity"),
        stateright_model: Some("inv_ferr_020_commit_assigns_single_epoch"),
        integration_test: Some("inv_ferr_020_transaction_epoch_atomicity"),
    },
    Invariant {
        id: "INV-FERR-021",
        name: "Backpressure Safety",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("test_inv_ferr_021_backpressure_safety"),
        kani_harness: Some("write_limiter_capacity_enforcement"),
        stateright_model: Some("inv_ferr_021_submit_to_empty_queue_accepted"),
        integration_test: Some("inv_ferr_021_backpressure_integration"),
    },
    Invariant {
        id: "INV-FERR-022",
        name: "Anti-Entropy Convergence",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_022_null_anti_entropy"),
        kani_harness: None,
        stateright_model: None,
        integration_test: Some("test_inv_ferr_022_anti_entropy_trait"),
    },
    Invariant {
        id: "INV-FERR-023",
        name: "No Unsafe Code",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_023_no_unsafe_code"),
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-024",
        name: "Substrate Agnosticism",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_024_in_memory_cold_start_roundtrip"),
        kani_harness: None,
        stateright_model: None,
        integration_test: Some("test_inv_ferr_024_in_memory_backend"),
    },
    // -----------------------------------------------------------------------
    // 03-performance.md: INV-FERR-025..032 (Stage 0)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-025",
        name: "Index Backend Interchangeability",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_025_index_backend_roundtrip"),
        kani_harness: None,
        stateright_model: None,
        integration_test: Some("test_inv_ferr_025_index_backend_trait"),
    },
    Invariant {
        id: "INV-FERR-026",
        name: "Write Amplification Bound",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_026_write_amplification"),
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-027",
        name: "Read P99.99 Latency",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_027_read_latency_lookup"),
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-028",
        name: "Cold Start Latency",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_028_cold_start_checkpoint_correctness"),
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-029",
        name: "LIVE View Resolution",
        stage: Stage::Stage0,
        lean_theorem: Some("retraction_removes"),
        proptest_fn: Some("test_inv_ferr_029_live_resolution"),
        kani_harness: Some("retraction_removes_from_live_view"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_029_live_resolution"),
    },
    Invariant {
        id: "INV-FERR-030",
        name: "Read Replica Subset",
        stage: Stage::Stage0,
        lean_theorem: None,
        proptest_fn: Some("inv_ferr_030_accept_all_filter"),
        kani_harness: None,
        stateright_model: None,
        integration_test: Some("test_inv_ferr_030_replica_filter"),
    },
    Invariant {
        id: "INV-FERR-031",
        name: "Genesis Determinism",
        stage: Stage::Stage0,
        lean_theorem: Some("genesis_bottom"),
        proptest_fn: Some("inv_ferr_031_genesis_determinism"),
        kani_harness: Some("genesis_determinism"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_031_genesis_determinism"),
    },
    Invariant {
        id: "INV-FERR-032",
        name: "LIVE Resolution Correctness",
        stage: Stage::Stage0,
        lean_theorem: Some("live_asserted_not_retracted"),
        proptest_fn: Some("test_inv_ferr_032_live_semantics"),
        kani_harness: Some("live_view_contains_only_asserted"),
        stateright_model: None,
        integration_test: Some("test_inv_ferr_032_live_correctness"),
    },
    // -----------------------------------------------------------------------
    // 04-decisions-and-constraints.md: INV-FERR-033..036 (Stage 1)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-033",
        name: "Cross-Shard Query Correctness",
        stage: Stage::Stage1,
        lean_theorem: Some("cross_shard_query"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-034",
        name: "Partition Detection",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-035",
        name: "Partition-Safe Operation",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-036",
        name: "Partition Recovery",
        stage: Stage::Stage1,
        lean_theorem: Some("partition_recovery"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    // -----------------------------------------------------------------------
    // 05-federation.md: INV-FERR-037..044 (Stage 1)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-037",
        name: "Federated Query Correctness",
        stage: Stage::Stage1,
        lean_theorem: Some("federated_query_two"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-038",
        name: "Federation Substrate Transparency",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-039",
        name: "Selective Merge (Knowledge Transfer)",
        stage: Stage::Stage1,
        lean_theorem: Some("selective_merge_mono"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-040",
        name: "Merge Provenance Preservation",
        stage: Stage::Stage1,
        lean_theorem: Some("merge_provenance"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-041",
        name: "Transport Latency Tolerance",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-042",
        name: "Live Migration (Substrate Transition)",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-043",
        name: "Schema Compatibility Check",
        stage: Stage::Stage1,
        lean_theorem: Some("schema_compat_symmetric"),
        proptest_fn: Some("inv_ferr_043_schema_conflict_merge_commutativity"),
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-044",
        name: "Namespace Isolation",
        stage: Stage::Stage1,
        lean_theorem: Some("ns_filter_sound"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    // -----------------------------------------------------------------------
    // 06-prolly-tree.md: INV-FERR-045..050 (Stage 1)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-045",
        name: "Chunk Content Addressing",
        stage: Stage::Stage1,
        lean_theorem: Some("chunk_content_identity"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-046",
        name: "Prolly Tree History Independence",
        stage: Stage::Stage1,
        lean_theorem: Some("history_independence"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-047",
        name: "O(d) Diff Complexity",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-048",
        name: "Chunk-Based Federation Transfer",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-049",
        name: "Snapshot = Root Hash",
        stage: Stage::Stage1,
        lean_theorem: Some("snapshot_deterministic"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-050",
        name: "Block Store Substrate Independence",
        stage: Stage::Stage1,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    // -----------------------------------------------------------------------
    // 05-federation.md (VKN section): INV-FERR-051..055 (Stage 2)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-051",
        name: "Signed Transactions",
        stage: Stage::Stage2,
        lean_theorem: Some("signed_verify_roundtrip"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-052",
        name: "Merkle Proof of Inclusion",
        stage: Stage::Stage2,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-053",
        name: "Light Client Protocol",
        stage: Stage::Stage2,
        lean_theorem: Some("light_client_completeness"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-054",
        name: "Trust Gradient Query",
        stage: Stage::Stage2,
        lean_theorem: Some("trust_all_identity"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-055",
        name: "Verifiable Knowledge Commitment (VKC)",
        stage: Stage::Stage2,
        lean_theorem: Some("vkc_authentic"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    // -----------------------------------------------------------------------
    // 08-verification-infrastructure.md: INV-FERR-056..059 (Stage 2)
    // -----------------------------------------------------------------------
    Invariant {
        id: "INV-FERR-056",
        name: "Crash Recovery Under Adversarial Fault Model",
        stage: Stage::Stage2,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-057",
        name: "Sustained Load Invariant Preservation",
        stage: Stage::Stage2,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-058",
        name: "Query Metamorphic Equivalence",
        stage: Stage::Stage2,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "INV-FERR-059",
        name: "Optimization Behavioral Preservation",
        stage: Stage::Stage2,
        lean_theorem: None,
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    // -----------------------------------------------------------------------
    // 07-refinement.md: CI-FERR-001..002 (Stage 0 -- foundational coupling)
    // -----------------------------------------------------------------------
    Invariant {
        id: "CI-FERR-001",
        name: "Lean-Rust Coupling Invariant",
        stage: Stage::Stage0,
        lean_theorem: Some("ci_genesis"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
    Invariant {
        id: "CI-FERR-002",
        name: "Type-Level Refinement (Curry-Howard Encoding)",
        stage: Stage::Stage0,
        lean_theorem: Some("cm_replay_epoch"),
        proptest_fn: None,
        kani_harness: None,
        stateright_model: None,
        integration_test: None,
    },
];

/// Coverage counts per stage: `(stage, covered_count, total_count)`.
///
/// An invariant is "covered" if it has at least one verification layer.
#[must_use]
pub fn coverage_by_stage() -> [(Stage, usize, usize); 3] {
    let mut counts = [
        (Stage::Stage0, 0_usize, 0_usize),
        (Stage::Stage1, 0, 0),
        (Stage::Stage2, 0, 0),
    ];
    for inv in CATALOG {
        let idx = match inv.stage {
            Stage::Stage0 => 0,
            Stage::Stage1 => 1,
            Stage::Stage2 => 2,
        };
        counts[idx].2 += 1;
        if inv.has_any_test() {
            counts[idx].1 += 1;
        }
    }
    counts
}

/// Count of invariants with each verification layer type.
///
/// Returns `[("lean", n), ("proptest", n), ("kani", n), ("stateright", n), ("integration", n)]`.
#[must_use]
pub fn coverage_by_layer() -> [(&'static str, usize); 5] {
    let (mut lean, mut prop, mut kani, mut sr, mut integ) = (0, 0, 0, 0, 0);
    for inv in CATALOG {
        if inv.lean_theorem.is_some() {
            lean += 1;
        }
        if inv.proptest_fn.is_some() {
            prop += 1;
        }
        if inv.kani_harness.is_some() {
            kani += 1;
        }
        if inv.stateright_model.is_some() {
            sr += 1;
        }
        if inv.integration_test.is_some() {
            integ += 1;
        }
    }
    [
        ("lean", lean),
        ("proptest", prop),
        ("kani", kani),
        ("stateright", sr),
        ("integration", integ),
    ]
}

/// Returns IDs of invariants that have no proptest, Kani, or integration test.
///
/// Lean-only or Stateright-only invariants are included since those layers
/// do not exercise the Rust implementation directly.
#[must_use]
pub fn invariants_without_test() -> Vec<&'static str> {
    CATALOG
        .iter()
        .filter(|inv| {
            inv.proptest_fn.is_none()
                && inv.kani_harness.is_none()
                && inv.integration_test.is_none()
        })
        .map(|inv| inv.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-FERR-013: catalog must contain all 61 invariants (59 INV + 2 CI).
    #[test]
    fn test_catalog_count() {
        assert_eq!(
            CATALOG.len(),
            61,
            "ADR-FERR-013: expected 61 invariants (59 INV-FERR + 2 CI-FERR), got {}",
            CATALOG.len()
        );
    }

    /// ADR-FERR-013: every Stage 0 invariant must have at least one test layer.
    #[test]
    fn test_no_untested_stage0() {
        let untested: Vec<&str> = CATALOG
            .iter()
            .filter(|inv| inv.stage == Stage::Stage0 && !inv.has_any_test())
            .map(|inv| inv.id)
            .collect();
        assert!(
            untested.is_empty(),
            "ADR-FERR-013: Stage 0 invariants without any test: {untested:?}"
        );
    }

    /// ADR-FERR-013: stage counts must partition correctly.
    #[test]
    fn test_coverage_by_stage() {
        let counts = coverage_by_stage();

        // Stage 0: INV-FERR-001..032 (32) + CI-FERR-001..002 (2) = 34
        assert_eq!(
            counts[0].2, 34,
            "Stage 0 total: expected 34, got {}",
            counts[0].2
        );
        // Stage 1: INV-FERR-033..050 = 18
        assert_eq!(
            counts[1].2, 18,
            "Stage 1 total: expected 18, got {}",
            counts[1].2
        );
        // Stage 2: INV-FERR-051..059 = 9
        assert_eq!(
            counts[2].2, 9,
            "Stage 2 total: expected 9, got {}",
            counts[2].2
        );

        // All covered counts must be <= total
        for (stage, covered, total) in &counts {
            assert!(
                covered <= total,
                "ADR-FERR-013: {stage:?} covered ({covered}) > total ({total})"
            );
        }
    }

    /// Verify no duplicate IDs in the catalog.
    #[test]
    fn test_no_duplicate_ids() {
        let mut seen = std::collections::HashSet::new();
        for inv in CATALOG {
            assert!(
                seen.insert(inv.id),
                "ADR-FERR-013: duplicate invariant ID: {}",
                inv.id
            );
        }
    }

    /// Verify layer counts are plausible.
    #[test]
    fn test_coverage_by_layer() {
        let layers = coverage_by_layer();
        // Every layer should have at least some coverage
        for (name, count) in &layers {
            // Stateright has fewer models, but at least some
            if *name == "stateright" {
                assert!(
                    *count >= 5,
                    "ADR-FERR-013: {name} layer has only {count} invariants covered"
                );
            }
        }
        // Lean and proptest should cover most Stage 0
        let lean_count = layers[0].1;
        let proptest_count = layers[1].1;
        assert!(
            lean_count >= 20,
            "ADR-FERR-013: Lean covers only {lean_count} invariants"
        );
        assert!(
            proptest_count >= 25,
            "ADR-FERR-013: proptest covers only {proptest_count} invariants"
        );
    }
}
