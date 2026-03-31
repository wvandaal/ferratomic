# Ferratomic — Agent Guidelines

> Ferratomic is a formally verified, distributed embedded datom database engine.
> A general-purpose storage foundation for any system built on the datom model.

---

## True North

Ferratomic provides the universal substrate: an append-only datom store with
content-addressed identity, CRDT merge, indexed random access, and cloud-scale
distribution. It is to applications what PostgreSQL is to a web app — the
foundational infrastructure that everything else builds on.

**Store = (P(D), ∪)** — a G-Set CRDT semilattice. Writes are commutative,
associative, and idempotent by construction. No conflicts. No consensus protocol.
The data structure IS the consistency mechanism.

---

## Development Methodology: Spec-First TDD (Curry-Howard-Lambek)

**Non-negotiable phase ordering:**
```
Phase 0: Formal specification (spec/ — canonical, modular)     ← DONE
Phase 1: Lean 4 theorem statements + proofs
Phase 2: Test suite (Stateright, Kani, proptest) — ALL FAIL (red phase)
Phase 3: Type definitions (ferratom crate — types ARE propositions)
Phase 4: Implementation (ferratomic-core — programs ARE proofs)
Phase 5: Integration (application migration)
```

**Phase gate**: Phase N+1 CANNOT begin until Phase N passes isomorphism check.
A gap between spec, algebra, and tests is a DEFECT, not technical debt.

**Compilation expectation**: The workspace may not compile during Phases 1-2.
This is expected. Phase 3 creates type stubs that make Phase 2 tests compilable.

**Spec Level 2 contracts**: Level 2 Rust contracts are conceptual -- they illustrate
algebraic properties using `BTreeSet`. Implementation uses `im::OrdSet`/`im::OrdMap`
(ADR-FERR-001).

---

## Specification

The canonical specification lives in `spec/` in THIS repository.

- **Formal spec**: `spec/` (55 INV, 10 ADR, 5 NEG) — modular files, see `spec/README.md`
- **Architecture**: `docs/design/FERRATOMIC_ARCHITECTURE.md`
- **Design decisions**: `docs/design/`

---

## Crate Architecture

```
ferratomic/
├── ferratom/           # Leaf: core types (ZERO project deps)
├── ferratomic-core/    # Core: storage + concurrency engine
├── ferratomic-datalog/ # Facade: query engine
└── ferratomic-verify/  # Verification: Lean 4 + Stateright + Kani + proptest
```

Dependency direction: leaf → core → facade. No cycles.

---

## Complexity Standards (Hard Limits, Not Guidelines)

### File-level
- **Max 500 LOC per file** (excluding tests). If approaching, split by responsibility.
- **Max 1,500 LOC per file** including inline tests. Extract tests to `tests/` if over.
- **One concept per module.** `store.rs` must not contain WAL logic. `db.rs` must not contain checkpoint logic.

### Function-level
- **Max 50 LOC per function.** Decompose if longer.
- **Max cyclomatic complexity 10.** Enforced via `clippy::cognitive_complexity`.
- **Max 5 parameters.** More → introduce a config/params struct.

### Crate-level
- **ferratom**: < 2,000 LOC (pure types, should be small)
- **ferratomic-core**: < 10,000 LOC. If approaching, split into sub-crates.
- **ferratomic-datalog**: < 5,000 LOC.
- **ferratomic-verify**: No limit (tests and proofs can be verbose).

### Splitting strategy
- When a module grows > 500 LOC, split by responsibility into submodules:
  `store/indexes.rs`, `store/merge.rs`, `store/apply.rs`.
- Public API surface per crate: minimal. Re-export through `lib.rs`, keep internals private.
- Never put two unrelated concepts in one file for convenience.

### Enforced via CI
```toml
# clippy.toml
cognitive-complexity-threshold = 10
too-many-arguments-threshold = 5
too-many-lines-threshold = 50
```

---

## Code Quality Standards

### Type Discipline (Curry-Howard — types ARE propositions)

- **Minimal cardinality types.** Every type admits exactly the valid states.
  Invalid states are unrepresentable. `Port(u16)` not `u16`. `EntityId([u8; 32])`
  not `Vec<u8>`. Every invalid state your type CAN represent is a proof obligation
  shifted from compiler to runtime.
- **Newtype wrappers for all domain concepts.** No raw primitives in APIs.
  `EntityId`, not `[u8; 32]`. `Attribute`, not `String`. `Epoch`, not `u64`.
- **Typestate for lifecycles.** `Transaction<Building>` → `Transaction<Committed>`.
  `Database<Opening>` → `Database<Ready>`. Invalid state transitions are compile errors.
- **Exhaustive pattern matching.** No `_ =>` wildcards on enums that may grow.
  Every match arm names the variant. Adding a variant produces compile errors
  at every match site — which is the point.
- **Parse, don't validate.** Accept raw input at system boundaries, produce typed
  values. Internal code never re-validates — the type IS the proof.

### Error Discipline

- **`Result<T, FerraError>` everywhere.** No panics, no `unwrap()`, no `expect()`
  in production code. Test code may use `unwrap()` with descriptive messages.
- **Error categories matter.** `FerraError::Io` is retryable. `FerraError::SchemaViolation`
  is a caller bug. `FerraError::InvariantViolation` is OUR bug. Callers pattern-match
  on category, not message strings.
- **`?` propagation, not `.unwrap()`.** The only acceptable `unwrap()` in production
  is on infallible operations (e.g., `regex::Regex::new` with a compile-time-known pattern).
  Even then, prefer `const` initialization.

### Documentation Standards

- **Every public item has a doc comment.** Enforced by `#![deny(missing_docs)]`.
- **Doc comments state the invariant, not the implementation.** "Returns the datom's
  entity, which is a BLAKE3 hash of the content (INV-FERR-012)" — not "returns the
  first field of the tuple."
- **INV-FERR references in doc comments.** Every function that upholds or relies on
  an invariant cites it: `/// INV-FERR-006: snapshot isolation guarantees this returns
  /// a consistent view.`
- **No aspirational docs.** Don't document what the function WILL do. Document what
  it DOES. If it's not implemented, the doc says `TODO(Phase N)`.

### Naming Conventions

- **Types**: `PascalCase`. Names encode semantics: `DatomStore`, not `Store`. `ChunkAddress`, not `Hash`.
- **Functions**: `snake_case`. Verb-first: `apply_datoms`, `merge_stores`, `load_checkpoint`.
- **Constants**: `SCREAMING_SNAKE`. `GENESIS_HASH`, `MAX_CHUNK_SIZE`.
- **Modules**: `snake_case`. One concept per module. Name = concept: `wal`, `checkpoint`, `snapshot`.
- **No abbreviations** except universally understood ones (WAL, HLC, CRDT, IO).
  `transaction`, not `txn`. `attribute`, not `attr`. Exception: local variables
  in tight scopes where the full name adds noise.

### Testing Standards

- **Every public function has at least one test.** No exceptions.
- **Property-based tests for algebraic laws.** proptest with 10,000+ cases for any
  function involving CRDT operations, ordering, or identity.
- **Named invariants in test names.** `test_inv_ferr_001_merge_commutativity`,
  not `test_merge_works`.
- **Test failure messages document expected behavior.** `assert_eq!(result, expected,
  "INV-FERR-005: datom in primary must also be in entity index")`.
- **No `#[ignore]` without a tracking issue.** Ignored tests are hidden failures.

### Dependency Discipline

- **Minimal dependencies.** Every dependency is a liability. Justify each one.
- **ferratom has ZERO project-internal dependencies.** It depends only on blake3,
  ordered-float, serde. Adding a dependency to ferratom requires an ADR.
- **No transitive dependency on tokio from ferratom.** The leaf crate must be
  runtime-agnostic. Only ferratomic-core may depend on async runtime.
- **Pin major versions.** `im = "15"` not `im = "*"`. Reproducible builds.
- **Audit new dependencies.** Check for `unsafe`, check maintenance status,
  check license compatibility.

### Git Standards

- **Main branch only.** No long-lived feature branches. Short-lived branches
  for PRs, merged within 1-2 days.
- **Conventional commits.** `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `perf:`.
- **Every commit compiles and passes tests.** No "WIP" commits on main.
- **Atomic commits.** One logical change per commit. Don't mix refactoring with features.

### Agentic Development Optimization

- **AGENTS.md is the agent's onboarding document.** An agent should be productive
  within 5 minutes of reading it. Keep it current.
- **Session prompts (`docs/prompts/`) define execution scope.** One prompt per
  major work phase. The prompt IS the task specification.
- **Beads for task tracking.** Use `br ready`, `br create`, `br close` to manage
  project tasks. Beads IS the source of truth for issue state.
- **Skill loading protocol.** Load ONE methodology skill per cognitive phase:
  - Discovery: `ms load spec-first-design -m --full`
  - Implementation: `ms load rust-formal-engineering -m --full`
  - Optimization: `ms load prompt-optimization -m --pack 2000`
  - Never stack multiple full skills simultaneously (k* budget).
- **Disjoint file sets for parallel agents.** Two agents NEVER edit the same file.
  Agent coordination via beads tasks + dependency edges.
- **NEVER use worktrees.** `isolation: "worktree"` is FORBIDDEN for all subagents.
  Worktrees corrupt shared state (.beads/, .cass/) and create unmergeable branches.
  Always use default (non-worktree) agents with disjoint file sets.
- **Agents don't run cargo.** The orchestrator (human or primary agent) runs build/test
  ONCE after all agents complete. Prevents build lock contention and disk exhaustion.

---

## Hard Constraints

**C1: Append-only store.** Never delete or mutate datoms. Retractions are new datoms.
**C2: Content-addressed identity.** EntityId = BLAKE3(content).
**C4: CRDT merge = set union.** Commutative, associative, idempotent.
**INV-FERR-023: `#![forbid(unsafe_code)]`** in ALL crates. No exceptions.
**NEG-FERR-001: No panics.** No `unwrap()`, no `expect()` in production code.
**Zero clippy suppressions.** No `#[allow(clippy::...)]` or `#[allow(dead_code)]` in
production code. If clippy flags it, fix the root cause. Suppressions defeat the
purpose of static analysis. If the lint is genuinely wrong, restructure the code so
the lint no longer fires — do not silence it.
**Forbidden crates in core:** `tokio`, `hyper`, `reqwest`, `axum`, `async-std`, `smol`.
Tokio-only dependencies must be behind `asupersync-tokio-compat` adapter modules.
Core domain code depends on `asupersync` only (ADR-FERR-002).

---

## Build

**CRITICAL**: Set `export CARGO_TARGET_DIR=/data/cargo-target` at session start.
This is NOT auto-configured. Omitting it uses /tmp (RAM-backed, will fill up).
Every cargo command must use this target dir.

```bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

Lean proofs:
```bash
cd ferratomic-verify/lean && lake build
```

---

## Quality Standard

`ms load rust-formal-engineering -m --full` — the standard methodology.
Every type encodes an invariant. Every function proves a property.
NASA-grade, zero-defect, cleanroom engineering. No shortcuts.
