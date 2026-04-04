# Ferratomic — Goals & Alignment

> **Status**: Canonical. Changes at most once per phase transition.
> **Consumed by**: All agents, all lifecycle prompts, all alignment evaluations.
> **Scope**: WHY this project exists, WHAT it is, and HOW to resolve value conflicts.
> For operational methodology, see AGENTS.md. For formal specification, see spec/.

---

## 1. Purpose

Every agentic system decomposes into three components: an append-only event log,
an opaque runtime, and a stateless policy function. This decomposition is not
incidental — it is algebraically necessary. The event log mediates between the
policy (which needs epistemic state) and the runtime (which provides persistence).
It is simultaneously a formal prerequisite for agency, a concrete artifact requiring
storage, and the interface binding agent to environment.

Current systems treat this log as a flat, unstructured buffer. Context windows are
lossy projections of a linear history. The bottleneck in agentic systems is not
intelligence — it is memory architecture. Expert performance arises not from
superior reasoning but from superior associative retrieval over a structured fact
store.

**Ferratomic is that fact store.**

It reifies the algebraic store `(P(D), ∪)` — a grow-only set of datoms, merged
by set union — as a production-grade embedded database. Append-only.
Content-addressed. CRDT-mergeable without coordination. Temporally queryable.
Horizontally scalable. It is the persistence substrate that makes durable knowledge
accumulation, multi-agent federation, and self-evolving knowledge graphs possible.

Ferratomic is to agentic systems what the filesystem is to operating systems — the
substrate that makes everything else possible. Without it, every capability
(retrieval, coordination, verification, knowledge transfer) must be reinvented
per-application, on ad-hoc substrates that cannot merge, cannot trace provenance,
and cannot scale.

---

## 2. Identity

### What Ferratomic IS

- A **general-purpose embedded datom database engine**. It stores
  `[entity, attribute, value, transaction, operation]` tuples and enforces
  schema constraints. Everything domain-specific enters through the schema, not
  the engine.

- The **persistence layer for the universal agent decomposition** `(E, R, A)`.
  It provides the structured event log that lifts a stateless language model into
  a stateful, durable, collaborative agent.

- **Substrate-independent**. Runs on a cloud server, a personal laptop, or an
  embedded device. No dependency on any specific runtime, cloud provider,
  operating system, or async framework.

- **Federation-native**. Designed from day one for agents spanning heterogeneous
  compute environments. Independent stores merge by set union — the mathematical
  operation, not a heuristic. No coordination protocol required for writes.

### What Ferratomic is NOT

These anti-goals prevent scope creep. Each rules out a direction that agents are
tempted toward but that would compromise the project's purpose.

- **Not an application framework.** Ferratomic has zero knowledge of
  application-layer concerns — no domain entities, no workflow logic, no UI.
  Applications build ON Ferratomic, not IN it.

- **Not tied to any runtime or substrate.** No hard dependency on tokio, Linux
  syscalls, AWS APIs, or any specific compute platform. The core engine must be
  portable across any environment where Rust compiles.

- **Not a component of any specific application.** Ferratomic is an independent
  project. Other systems (including those that motivated its design) are consumers,
  not owners. No consumer-specific primitives enter the engine.

- **Not a consensus system.** CRDT merge (`Store = (P(D), ∪)`) means the data
  structure IS the consistency mechanism. Writes are commutative, associative, and
  idempotent by construction. Adding Raft, Paxos, or any coordination protocol
  for writes would contradict the algebraic foundation.

- **Not a retrieval heuristic.** Vector similarity finds "related" content.
  Ferratomic provides a verification substrate — structured, queryable, with
  provenance, temporal completeness, and formal consistency guarantees. Semantic
  search may be built on top; it is not a substitute for the substrate.

---

## 3. Value Hierarchy

When two good things conflict, this hierarchy resolves the conflict. Higher tiers
win unconditionally. Within a tier, use judgment and be transparent about the
tradeoff.

### Tier 1 — Non-Negotiable

No tradeoff permitted. Violating these means the project has failed its purpose.

- **Algebraic correctness.** The CRDT laws hold under all conditions —
  commutativity, associativity, idempotency, monotonic growth. A system that
  loses, duplicates, or silently reorders datoms has failed, regardless of how
  fast or feature-rich it is.

- **Append-only durability.** Committed data survives any software crash. The
  WAL-before-snapshot discipline is the load-bearing guarantee. Data loss is not
  a bug — it is a fundamental breach.

- **Safety.** No panics in production. The type system and the borrow checker are
  verification instruments. The application's entire callable surface area must be
  safe — `unsafe` is a containment problem, not a blanket prohibition. Unsafe code
  is permitted (in dependencies or in our own crates) if and only if: (1) it is
  firewalled behind a safe public API so callers cannot trigger UB, (2) it is the
  only possible way to achieve a performance or scaling objective that is
  mission-critical to top-line goals, and (3) the proof obligation is bounded,
  documented (via ADR), and auditable. Unsafe that leaks into the callable
  surface, or that exists for convenience rather than necessity, shifts proof
  obligations from the compiler to hope — a Tier 1 violation.

### Tier 2 — Foundation Priorities

Tradeoff only against Tier 1, with explicit evidence that Tier 1 is at stake.

- **Verification depth.** Prove 30 invariants across all 6 verification layers
  rather than implement 55 with partial coverage. In agentic development, agents
  mimic whatever patterns exist in the codebase. Poorly-verified code creates
  mediocrity basins that are extraordinarily difficult to escape. Provably correct
  foundations prevent this.

- **Architectural clarity.** Clean separation of concerns. Single responsibility.
  Minimal coupling. Acyclic dependency graphs. This project exists because poor
  architecture in a predecessor system made performance problems undetectable and
  unfixable until the system had to be rebuilt from scratch. Architecture is not
  aesthetics — it is the early warning system for every other quality dimension.

- **Spec-implementation alignment.** Code without spec grounding is code that
  cannot be verified. Every module traces to a named invariant. Every invariant
  traces to an algebraic law. Zero drift tolerance.

### Tier 3 — Production Priorities

Tradeoff against Tier 2 when justified with measured evidence, never against Tier 1.

- **Performance at scale.** The agentic future requires sustained throughput
  across heterogeneous compute meshes. The predecessor system became unusable at
  200K datoms. Ferratomic must handle 100M+ with sub-10ms point reads and
  efficient merge. Performance is not optional for production — it is the
  difference between a proof of concept and infrastructure.

- **Completeness.** All phases implemented. All invariants fully specified across
  all verification layers. Completeness is the long-run goal, but depth-first
  beats breadth-first: a narrow, deep, provably correct system is more valuable
  than a broad, shallow, possibly-correct one.

- **Federation.** Agents spanning different machines, networks, and organizations
  merge seamlessly, query across stores, and collaborate without central
  coordination. This is the ultimate scaling mechanism.

### Tier 4 — Desirable

Yield to higher tiers without resistance.

- **API ergonomics.** Simple, intuitive interfaces. But implementation complexity
  is acceptable — even necessary — when it serves Tier 1-3 goals. A complex
  implementation behind a simple API is the target.

- **Feature breadth.** Only features with spec grounding. Speculative additions
  without invariant backing are alignment violations regardless of how useful
  they seem.

### Resolving a Tradeoff (Worked Example)

> An agent is implementing prolly tree chunk boundaries. The simple approach
> uses O(n) scanning. A more complex approach achieves O(d log n) diff per
> INV-FERR-047 but requires 200 more lines of code and a subtle correctness
> argument involving rolling hash determinism.
>
> **Aligned**: Implement O(d log n). Tier 1 (correctness) is preserved because
> the approach has a formal proof. Tier 2 (verification) is served because the
> proof can be checked. Tier 3 (performance) is served because O(d log n) is
> essential at 100M datoms. Tier 4 (simplicity) yields — 200 more LOC is an
> acceptable cost.
>
> **Misaligned**: Implement O(n) because it's simpler. This is Tier 4 winning
> over Tier 3 — a value inversion.

---

## 4. Success Criteria

Three levels, each with testable predicates. Each level subsumes the previous.

### Level 1 — Foundation Complete

- [ ] All development phases (0 through 4d) implemented
- [ ] All Stage 0 invariants: 6 verification layers passing
      (Lean proof, proptest, Kani harness, Stateright model, integration test,
      type-level enforcement)
- [ ] Zero `sorry` in Lean proofs for Stage 0 invariants
- [ ] Performance targets met: <10ms p99 point read at 100M datoms (INV-FERR-027),
      <5s cold start at 100M datoms (INV-FERR-028)
- [ ] Crate dependency DAG acyclic, LOC budgets respected

### Level 2 — Production Ready

- [ ] All Stage 1 invariants: fully implemented and verified
- [ ] Multi-node federation operational (INV-FERR-037 through INV-FERR-044)
- [ ] Prolly tree block store with O(d log n) diff (INV-FERR-047)
- [ ] Datalog query engine with CALM-compliant fan-out (INV-FERR-037)
- [ ] Sustained performance at scale: 100M+ datoms, multi-store merge,
      heterogeneous network topologies
- [ ] The bootstrap test: Ferratomic's own specification stored as datoms
      within itself

### Level 3 — Mission Accomplished

- [ ] Adopted as persistence substrate for real agentic systems
- [ ] The harvest/seed lifecycle operational on Ferratomic
      (knowledge survives conversation boundaries via the datom store)
- [ ] Multi-agent federation across heterogeneous compute environments
      (the virtualized runtime vision)
- [ ] Self-authored knowledge graphs: agents write associations into the
      datom store, retrieval improves with use, expertise accumulates
      in the data rather than the model

Level 1 is fully within this project's control. Level 2 depends on integration
with consuming systems. Level 3 depends on external adoption. Optimize for
Level 1 first — without a correct, performant foundation, Levels 2 and 3
are unreachable.

---

## 5. Methodology Commitment

These are axioms, not suggestions. They are non-negotiable because they are the
mechanisms by which the Tier 1 values are enforced.

**Spec-first, always.** The refinement tower — Goals → Specification → Lean Model
→ Rust Types → Rust Code — is not a process preference. It is a correctness
guarantee. Each level refines the one above it. Skipping a level breaks the
verification chain. Implementation without spec grounding produces code that
cannot be formally verified, only tested — and testing provides confidence,
not proof.

**Zero-defect cleanroom engineering.** In agentic development, the codebase IS
the training signal for every agent that touches it. Toxic patterns (unwrap,
unsafe, unverified invariants, dead code, suppressed warnings) propagate through
agent behavior. Clean patterns propagate too. The quality of the codebase
determines the quality of all future work on it. Zero-defect is not a
productivity target — it is a compound interest argument.

**Formal verification is not optional.** Lean proofs, property-based testing,
bounded model checking, and protocol model checking are the mechanisms by which
algebraic correctness (Tier 1) is enforced. Without them, "correct" is an
opinion. With them, "correct" is a theorem.

**Types are propositions.** The Curry-Howard correspondence means compilation IS
verification. Every type encodes an invariant. Every function signature is a
contract. Invalid states must be unrepresentable. The compiler is the first and
most reliable verifier — engage it fully.

---

## 6. Defensive Engineering Standards

These standards define the quality floor. They are non-negotiable for the same
reason Tier 1 values are non-negotiable: without them, claims of correctness are
assertions, not evidence. The project targets the intersection of NASA/JPL flight
software discipline, ISO/IEC 25010 Product Quality, and Cleanroom Software
Engineering — adapted for a Rust embedded database.

### 6.1 Verification Layers (all required for Stage 0 invariants)

Every Stage 0 invariant must be verified across all six layers before its phase
gate can close. Missing a layer is a gap, not a deferral.

| Layer | Tool | What It Proves | Enforcement |
|-------|------|---------------|-------------|
| **Algebraic proof** | Lean 4 + mathlib | Mathematical law holds | 0 `sorry` required |
| **Bounded model checking** | Kani | Property holds for all inputs within bound | CI gate (nightly) |
| **Protocol model checking** | Stateright | Correct under all message orderings | CI gate |
| **Property-based testing** | proptest (10K cases) | Statistical confidence >99.97% | CI gate, Bayesian confidence (ADR-FERR-012) |
| **Fault injection** | FaultInjectingBackend | Survives adversarial storage faults | CI gate |
| **Type-level enforcement** | Rust type system | Invalid states unrepresentable | Compilation IS the proof |

### 6.2 Unsafe Code Containment

Unsafe is a containment problem, not a blanket prohibition. The application's
entire callable surface area must be safe.

Unsafe code is permitted (in dependencies or in our own crates) if and only if:

1. **Firewalled behind a safe public API** — callers cannot trigger undefined
   behavior through the interface under any input.
2. **Mission-critical necessity** — it is the only possible way to achieve a
   performance or scaling objective critical to top-line goals.
3. **ADR-documented** — the proof obligation is bounded, the unsafe sites are
   enumerated, the containment argument is auditable.

Unsafe that leaks into the callable surface, or that exists for convenience
rather than necessity, is a Tier 1 violation. Dependencies with internal unsafe
behind safe APIs (e.g., `im::OrdMap`, `blake3`, `memmap2`) are acceptable —
the abstraction boundary IS the safety guarantee.

### 6.3 Static Analysis

| Tool | What It Catches | Enforcement |
|------|----------------|-------------|
| `cargo clippy` (permissive) | All standard lints, all targets | CI gate, every commit |
| `cargo clippy` (strict, `--lib` only) | `unwrap_used`, `expect_used`, `panic` in production code | CI gate, every commit |
| `clippy.toml` limits | Cognitive complexity >10, functions >50 LOC, >5 args | CI gate |
| `cargo fmt` | Formatting drift | CI gate |
| `cargo deny` | Vulnerable deps, license violations, banned crates | CI gate |
| `cargo doc` with `-D warnings` | Documentation gaps on public items | CI gate |
| Zero lint suppressions | No `#[allow(...)]` anywhere, including tests | CI gate + pre-commit hook |

### 6.4 Dynamic Analysis

| Tool | What It Catches | Enforcement |
|------|----------------|-------------|
| **MIRI** | Undefined behavior across unsafe boundaries: uninitialized reads, dangling pointers, data races | CI gate (nightly). All pure-logic tests must pass under MIRI. I/O-bound tests may be excluded. |
| **AddressSanitizer** | Out-of-bounds access, use-after-free, double-free, memory leaks in C/FFI dependencies | Scheduled CI (nightly or pre-tag). `RUSTFLAGS="-Zsanitizer=address"`. |
| **Fuzz testing** | Edge cases in deserialization, WAL parsing, checkpoint loading, wire type decoding | CI smoke runs (60s per target). Extended runs pre-tag. 5 targets with seed corpus. |
| **Mutation testing** | Weak assertions — tests that pass but don't verify behavior. Measures test STRENGTH, not coverage. | Periodic (weekly or pre-tag). `cargo-mutants`. Target: >80% killed mutants. |

### 6.5 Coverage and Confidence

| Metric | Minimum Threshold | Tool | Rationale |
|--------|------------------|------|-----------|
| **Line coverage** | 90% per crate (ferratom, ferratomic-core) | `cargo-llvm-cov` | You cannot claim zero-defect without knowing the denominator. |
| **Branch coverage** | 80% per crate | `cargo-llvm-cov` | Untested branches are unverified code paths. |
| **Proptest confidence** | Beta(n+1,1) lower bound >= 0.9997 at 10K cases | ADR-FERR-012 | Bayesian quantification of statistical confidence. |
| **Mutation kill rate** | >80% of injected mutants killed | `cargo-mutants` | Verifies test assertions are strong enough to catch defects. |
| **Coverage direction** | Must not decrease between commits | CI gate | Ratchet: coverage only goes up. |

### 6.6 Supply Chain Security

| Practice | Tool | What It Prevents |
|----------|------|-----------------|
| **Dependency advisory check** | `cargo-deny` (advisories) | Known CVEs in transitive deps |
| **License audit** | `cargo-deny` (licenses) | Copyleft/unlicensed contamination |
| **Crate ban list** | `cargo-deny` (bans) | Explicitly banned crates (openssl, etc.) |
| **Source restriction** | `cargo-deny` (sources) | No unknown registries or git sources |
| **Transitive unsafe audit** | `cargo-geiger` | Visibility into dependency unsafe surface |
| **SBOM generation** | CycloneDX or SPDX | Machine-readable bill of materials (pre-release) |

### 6.7 Threat Modeling

Before implementing any phase that introduces adversarial trust boundaries
(Phase 4c federation, Phase 4c signing, transport), a STRIDE-based threat model
must be authored as `docs/design/THREAT_MODEL.md`. The threat model must cover:

- Trust boundaries (local vs. peer vs. untrusted)
- Attack surfaces (deserialization, transport, signing, merge)
- Mitigations for each identified threat
- Residual risk and acceptance rationale

The wire/core trust boundary (ADR-FERR-010) is the Phase 4a threat model.
Federation requires its own analysis.

### 6.8 Process Gates

Every commit to main must pass ALL of the following. No exceptions. No
`--no-verify`. Failures are defects, not inconveniences.

```
Gate 1:  cargo fmt --all -- --check
Gate 2:  cargo clippy --workspace --all-targets -- -D warnings
Gate 3:  cargo clippy --workspace --lib -- -D warnings \
           -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
Gate 4:  cargo test --workspace
Gate 5:  cargo deny check
Gate 6:  #![forbid(unsafe_code)] verified in all crate roots
Gate 7:  cargo doc --workspace --no-deps -- -D warnings
Gate 8:  File complexity limits (500 LOC, clippy.toml thresholds)
Gate 9:  lake build (Lean proofs, 0 sorry) — unconditional
Gate 10: cargo +nightly miri test (pure-logic subset)
Gate 11: Coverage >= thresholds (no regression)
```

### 6.9 Regression Discipline

- **Every bug gets a regression test.** The test must fail before the fix and
  pass after. No exceptions.
- **Every fuzz crash gets a seed corpus entry.** The crashing input is preserved
  in `fuzz/corpus/` so it is re-tested on every subsequent run.
- **Coverage ratchet.** Coverage thresholds only increase. A PR that drops
  coverage below the threshold is rejected.
- **Lean proofs are unconditional in CI.** Not gated on commit message keywords.
  A code change that breaks a Lean proof fails CI regardless of the commit message.
