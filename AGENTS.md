# Ferratomic — Agent Guidelines

Formally verified, distributed embedded datom database engine.
**Store = (P(D), U)** — G-Set CRDT semilattice. No conflicts. No consensus.

---

## Build (CRITICAL — read first)

```bash
export CARGO_TARGET_DIR=/data/cargo-target  # MUST set. Default uses /tmp (RAM-backed, fills up)
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
PROPTEST_CASES=1000 cargo test --workspace  # Fast: 1K cases. Full: omit env var (10K, use --release)
```

Strict gate (production code — `--lib` only, test code exempt):
```bash
cargo clippy --workspace --lib -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
```

Full verification (pre-tag): `cargo test --workspace --release && cd ferratomic-verify/lean && lake build`

---

## Hard Constraints

**C1** Append-only. Never delete or mutate datoms. Retractions are new datoms.
**C2** Content-addressed identity. `EntityId = BLAKE3(content)`.
**C4** CRDT merge = set union. Commutative, associative, idempotent.
**NEG-FERR-001** No panics. No `unwrap()`, no `expect()` in production code. `Result<T, FerraError>` everywhere.
**INV-FERR-023** Safe callable surface. `#![forbid(unsafe_code)]` by default. Internal `unsafe` permitted only when: (1) firewalled behind safe API — callers cannot trigger UB, (2) mission-critical for performance/scaling, (3) ADR-documented. See GOALS.md §6.2.
**Zero lint suppressions.** No `#[allow(...)]` anywhere — not in tests, not in verification, not "temporarily." No `#[cfg(...)]` hiding code from the type checker. The Kani incident proved this: `cfg(kani)` hid 7 API drift bugs. Fix root causes, not symptoms.
**Forbidden crates in core:** `tokio`, `hyper`, `reqwest`, `axum`, `async-std`, `smol`. Core depends on `asupersync` only (ADR-FERR-002).

---

## Quality Standard: GOALS.md §6

Read **GOALS.md §6** (Defensive Engineering Standards) — it is the canonical quality reference. Summary:

### CI Gates (all must pass every commit, no `--no-verify`)

| Gate | Command | What It Catches |
|------|---------|----------------|
| 1 | `cargo fmt --all -- --check` | Formatting drift |
| 2 | `cargo clippy --workspace --all-targets -- -D warnings` | All lints |
| 3 | `cargo clippy --workspace --lib -- -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic` | Panics in production |
| 4 | `cargo test --workspace` | Correctness |
| 5 | `cargo deny check` | CVEs, license violations, banned crates |
| 6 | `#![forbid(unsafe_code)]` in all 5 crate roots | Unsafe containment |
| 7 | `cargo doc --workspace --no-deps -- -D warnings` | Doc gaps |
| 8 | File complexity (500 LOC, clippy.toml) | Complexity creep |
| 9 | `lake build` (unconditional) | Lean proof regressions (0 sorry) |
| 10 | `cargo +nightly miri test` | Undefined behavior |
| 11 | Coverage >= thresholds (no regression) | Untested code |

### Dynamic Analysis + Coverage (GOALS.md §6.4-6.5)

| Tool | Command | Frequency | Threshold |
|------|---------|-----------|-----------|
| MIRI | `cargo +nightly miri test` | CI nightly | All pure-logic tests pass |
| ASan | `RUSTFLAGS="-Zsanitizer=address" cargo test` | Pre-tag | Clean |
| Fuzz | `cargo fuzz run <target> -- -max_total_time=60` | CI smoke | Crashes → seed corpus |
| Mutation | `cargo-mutants` | Weekly/pre-tag | >80% kill rate |
| Coverage | `cargo llvm-cov` | CI gate | Line >=90%, branch >=80%, ratchet up only |

**Verification layers** (Stage 0 invariants require ALL six): Lean 4 (0 sorry) + Kani + Stateright + proptest (10K, >99.97% Bayesian) + FaultInjectingBackend + type-level.

---

## Phase Ordering (non-negotiable)

| Phase | DoF | Status | Activation Question |
|-------|-----|--------|-------------------|
| 0: Specification | Very high | DONE | "What algebraic structure governs this domain?" |
| 1: Lean proofs | High | DONE | "What must always be true? Prove it." |
| 2: Tests (red) | Low | DONE | "What input would falsify this invariant?" |
| 3: Types | Low | DONE | "What states must be unrepresentable?" |
| 4: Implementation | Very low | IN PROGRESS | "Implement RULE-X with exact types from spec." |
| 5: Integration | Low | — | — |

Phase N+1 CANNOT begin until Phase N passes isomorphism check. Gaps between spec, algebra, and tests are DEFECTS. Spec Level 2 uses `BTreeSet` conceptually; implementation uses `im::OrdSet`/`im::OrdMap` (ADR-FERR-001).

---

## Specification

- **Spec**: `spec/` (canonical) — see `spec/README.md` for module index
- **Architecture**: `docs/design/` (MIGRATION.md, ARCHITECTURAL_INFLUENCES.md, REFINEMENT_CHAINS.md)
- **Goals & values**: `GOALS.md` (value hierarchy, success criteria, defensive standards)
- **Lifecycle prompts**: `docs/prompts/lifecycle/` (one prompt per cognitive phase)

### Crate Map

```
ferratom-clock/     Leaf: HLC, TxId, AgentId, Frontier (ZERO project deps)
ferratom/           Leaf: Datom, EntityId, Value, Schema, Wire types
ferratomic-tx/      Leaf: Transaction typestate builder (depends on ferratom)
ferratomic-storage/ Leaf: StorageBackend trait, FsBackend, InMemoryBackend (depends on ferratom)
ferratomic-wal/     Leaf: WAL frames, CRC32, fsync, recovery (depends on ferratom)
ferratomic-index/   Leaf: Index key types, IndexBackend trait, SortedVecBackend (depends on ferratom + im)
ferratomic-positional/ PositionalStore, Bloom, CHD, Eytzinger, LIVE bitvector (depends on ferratom + index)
ferratomic-core/    Core: Store, Database, checkpoint, merge
ferratomic-datalog/ Facade: Datalog parser, planner, evaluator (stubs — Phase 4d)
ferratomic-verify/  Proofs: Lean 4, Stateright, Kani, proptest, fault injection
```

Dependency: clock -> ferratom -> {tx, storage, wal} -> index -> positional -> core -> datalog. Acyclic.

---

## Agentic Development Rules

**Worktrees FORBIDDEN.** `isolation: "worktree"` corrupts .beads/ and .cass/. Always use default (non-worktree) agents.
**Agents don't run cargo.** Orchestrator compiles once after all agents complete. Prevents lock contention.
**Disjoint file sets.** Two agents NEVER edit the same file. Coordinate via beads + dependency edges.
**Session prompts define scope.** `docs/prompts/lifecycle/` — one prompt per phase. The prompt IS the task spec.
**Beads for task tracking.** `br ready` (actionable), `bv --robot-next` (top pick), `br update <id> --status in_progress` (claim).
**Skill loading.** One skill per cognitive phase. Discovery: `ms load spec-first-design -m --full`. Implementation: `ms load rust-formal-engineering -m --full`. Never stack.

---

## Code Discipline (by demonstration)

This example encodes the expected quality. Match this standard:

```rust
/// INV-FERR-006: Snapshot isolation — returns a consistent point-in-time view.
///
/// The returned snapshot is immutable under future writes (INV-FERR-011).
/// Callers see datoms committed at or before `self.epoch`, never partial
/// transactions (INV-FERR-020).
pub fn snapshot(&self) -> Result<Snapshot, FerraError> {
    let store = self.current.load();          // ArcSwap: ~1ns, lock-free
    Ok(Snapshot::new(Arc::clone(&store)))
}

#[test]
fn test_inv_ferr_006_snapshot_stable_under_write() {
    let db = Database::genesis();
    let snap = db.snapshot().expect("genesis snapshot must succeed");
    // ... transact new datoms ...
    assert_eq!(snap.datoms().count(), 0,
        "INV-FERR-006: snapshot must not see post-snapshot writes");
}
```

**What this encodes**: INV-FERR citations in doc comments. `Result<T, FerraError>` everywhere (no unwrap). Newtypes (`Snapshot`, not raw data). Test names cite invariants. Assertions explain the invariant they verify. One concept per function.

**Additional non-discoverable rules:**
- No `#[ignore]` without a tracking bead
- Conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `perf:`. Atomic — one change per commit.
- Every bug gets a regression test. Every fuzz crash gets a seed corpus entry. Coverage ratchet: only goes up.

---

## Complexity Limits (enforced by clippy.toml + CI Gate 8)

| Scope | Limit |
|-------|-------|
| Function | 50 LOC, cyclomatic complexity 10, 5 parameters |
| File | 500 LOC (excl. tests), 1500 LOC (incl. tests) |
| Module | One concept. `store.rs` must not contain WAL logic. |
| ferratom-clock | < 1,000 LOC |
| ferratom | < 2,000 LOC |
| ferratomic-core | < 10,000 LOC |
| ferratomic-datalog | < 5,000 LOC |
| ferratomic-verify | No limit |
