# Spec Gap Analysis: Plan vs. Specification Coverage

> **Date**: 2026-03-31
> **Analyst**: Claude Opus 4.6 (8 Opus subagents, /effort max)
> **Methodology**: `docs/prompts/lifecycle/12-deep-analysis.md` — first-principles decomposition
> **Scope**: All decisions from conversation history (13 sessions, ~35MB JSONL) cross-referenced against all 7 spec files (55 INV, 9 ADR, 5 NEG)
> **Grounding**: spec/, docs/design/, docs/reviews/2026-03-31-cleanroom-audit-phase4a.md, session JSONL, .beads/issues.jsonl (298 beads)

---

## Phase 1: Grounding Summary

**Relevant INV-FERR**: 012 (content identity), 015 (HLC monotonicity), 021 (backpressure), 054 (trust gradient)
**Relevant ADR-FERR**: 004 (observer delivery), 005 (clock model), 007 (Lean-Rust bridge)
**Relevant NEG-FERR**: 001 (no panics), 003 (no data loss)
**Session history**: 13 sessions spanning 2026-03-29 to 2026-03-31. Key decision session: 6da9b454 (cleanroom audit + design decisions).
**Beads**: 298 total (113 open, 182 closed). 12 duplicate pairs identified. bd-5p51 (audit epic) blocks 50 downstream.

---

## Phase 2: First-Principles Decomposition

### GAP-1: Architecture C (Two-Tier Wire/Core Types) Has No Spec ADR

#### Step 1: Axioms

- INV-FERR-012: `EntityId = BLAKE3(content)`. Every constructor must be a proof of content-addressing.
- INV-FERR-054: Trust is a continuous gradient computed from verifiable calibration history.
- NEG-FERR-003: No data loss on crash — WAL/checkpoint integrity protects local storage.
- Curry-Howard principle (CI-FERR-002): Types are propositions. An unguarded `Deserialize` on `EntityId` is a `sorry` axiom — it admits arbitrary 32-byte values as "proven" BLAKE3 hashes.

The derived `Deserialize` on `EntityId` violates INV-FERR-012 at the type level. The cleanroom audit identified this as DEFECT-P3-002 (CRITICAL). The user chose Architecture C (two-tier wire/core types) after first-principles analysis of three alternatives.

#### Step 2: Algebraic Structure

Architecture C is a **functor** between categories:
- **Category Wire**: Objects are wire types (`WireDatom`, `WireEntityId`, `WireValue`). Morphisms are serde `Deserialize`.
- **Category Core**: Objects are core types (`Datom`, `EntityId`, `Value`). Morphisms are store operations (`transact`, `merge`).
- **Functor `into_trusted`**: A faithful structure-preserving map from Wire to Core. It is:
  - Identity on the bytes (zero-cost — field-by-field move)
  - A trust boundary: the caller MUST have verified source integrity before calling
  - `pub(crate)` visibility: only `ferratomic-core` can call `into_trusted`

For Phase 4c, `into_verified` is a second functor from Wire to Core that requires cryptographic proof (Ed25519 signature or Merkle inclusion). The category structure ensures that ALL paths from untrusted bytes to trusted types pass through exactly one verification functor.

#### Step 3: Location in Dependency Graph

```
ferratom (leaf)           <-- Wire types + core types live here
  └── ferratomic-core     <-- into_trusted() callsites (WAL, checkpoint, recovery)
       └── ferratomic-datalog  <-- unaffected (no deserialization)
  └── ferratomic-verify   <-- test callsites change from Vec<Datom> to Vec<WireDatom>
```

This is a **leaf-level change** that propagates UP to core (callsite changes). The ferratom public API surface changes: `Datom`, `Value`, `EntityId` lose `Deserialize`. All deserialization goes through `ferratom::wire`.

#### Step 4: Severity x Leverage

- **Severity**: Blocking. Phase 4c (federation) cannot be securely implemented without Architecture C. A Byzantine peer could forge `EntityId` values, poisoning the Merkle tree.
- **Leverage**: High. Architecture C resolves CR-003 (NonNanFloat bypass) and CR-004 (EntityId bypass) simultaneously, and provides the foundation for INV-FERR-051-055 (VKN layer). One design decision closes 2 critical defects and enables an entire future phase.
- **Priority**: Blocking x High = **FIRST**.

#### Step 5: Hidden Coupling

- `Datom` losing `Deserialize` means ANY code that calls `bincode::deserialize::<Vec<Datom>>()` will fail to compile. This is by design — it forces all callsites through the wire type functor.
- Checkpoint serialization (CR-005) depends on Architecture C because the checkpoint payload struct needs `WireCheckpointPayload`.
- The `Serialize` trait is RETAINED on core types — only `Deserialize` is removed. Write paths are unaffected.
- Performance impact: zero. `into_trusted()` is an identity function on the bytes; the optimizer erases it.

**This decision must be an ADR because it:**
1. Has alternatives that were considered and rejected (Architecture A: manual impls; Architecture B: newtype with From)
2. Constrains all future deserialization design
3. Defines a trust boundary model that Phase 4c depends on
4. Changes the public API surface of the leaf crate

**Proposed**: ADR-FERR-010 in `spec/04-decisions-and-constraints.md`.

---

### GAP-2: INV-FERR-015 Specifies `u16` Logical Counter; Code Uses `u32`

#### Step 1: Axioms

- INV-FERR-015: HLC monotonicity. The logical counter disambiguates events within the same physical millisecond.
- INV-FERR-021: Backpressure safety. Write queue depth is bounded.
- ADR-FERR-005: HLC chosen over Lamport and TrueTime.

The cleanroom audit (Decision 1) analyzed the u16 vs u32 tradeoff:
- u16 provides "natural backpressure" at 65K events/ms = 65M events/sec. But this conflates disambiguation with rate limiting.
- u32 provides disambiguation up to ~4.3B events/ms (physically unreachable).
- The WriteLimiter (INV-FERR-021) is the intentional backpressure mechanism at the Database level with configurable policy.
- The u16 overflow backpressure in the spec is an incidental consequence of a narrow counter, not a design requirement.

#### Step 2: Algebraic Structure

The logical counter is an element of a **bounded monotonic counter**. The counter's width determines the maximum rate before backpressure, but this is orthogonal to the HLC's ordering function. The total order `(physical, logical, agent)` works identically for u16 and u32 — only the overflow threshold changes.

#### Step 3: Location

Spec-only change. The code (`ferratom/src/clock/txid.rs:78`) already uses u32. The Lean model (`Concurrency.lean`) uses `Nat` which has no width constraint. Only `spec/02-concurrency.md` needs updating.

#### Step 4: Severity x Leverage

- **Severity**: Degrading. Spec says one thing, code does another. Agents consulting the spec will implement u16.
- **Leverage**: Medium. Fixes spec drift item SD-001 (HIGH from cleanroom audit).
- **Priority**: Degrading x Medium = **SECOND**.

#### Step 5: Hidden Coupling

Updating u16 → u32 in INV-FERR-015 requires also updating:
- The backpressure narrative at `02-concurrency.md:538-541` (remove "65536 events/ms" natural backpressure language, reference WriteLimiter instead)
- The Level 2 code contract at `02-concurrency.md:557` (`logical: u16` → `logical: u32`)
- The tick() implementation at `02-concurrency.md:569` (`u16::MAX` → `u32::MAX`)
- The falsification section at `02-concurrency.md:623` (`u16::MAX` reference)
- The proptest strategy at `02-concurrency.md:656` (`0u16..1000` → `0u32..1000`)

Total: 7 edits in one file, all mechanical.

---

### GAP-3: Backpressure Narrative Attributes Rate Limiting to HLC Overflow

#### Step 1: Axioms

This is the same axiom set as GAP-2, but the concern is different. GAP-2 is about the counter width. GAP-3 is about the backpressure attribution.

The spec says (`02-concurrency.md:538-541`):
> "The logical counter is a `u16` (65536 values). If 65536 events occur within the same physical millisecond on the same agent, the HLC blocks until the physical clock advances. This provides natural backpressure: at 65536 events/ms = 65M events/second per agent, the system self-limits rather than overflowing."

This narrative frames HLC overflow as the primary backpressure mechanism. But:
- INV-FERR-021 defines `WriteLimiter` as the backpressure mechanism (default capacity: 64 concurrent writes)
- The WriteLimiter operates at the Database level with configurable policy
- HLC overflow backpressure at u32::MAX (~4.3B events/ms) is a theoretical safety valve, not the operational mechanism

#### Step 2: Algebraic Structure

The WriteLimiter is a **bounded semaphore**: `try_acquire()` increments an atomic counter, returns a guard if below capacity, rejects otherwise. The HLC busy-wait is a **degenerate case** — a last-resort safety valve that should never fire in practice.

These are two different algebraic objects providing backpressure at different layers:
- WriteLimiter: application-level, configurable, the operational mechanism
- HLC overflow: clock-level, fixed, a safety valve

The spec should describe both, with WriteLimiter as primary and HLC overflow as safety valve.

#### Step 3: Location

Same file as GAP-2 (`spec/02-concurrency.md`), same paragraph. This is a narrative correction that accompanies the u16→u32 edit.

#### Step 4: Severity x Leverage

- **Severity**: Degrading. Misattributes the backpressure source, causing implementing agents to rely on HLC overflow instead of WriteLimiter.
- **Leverage**: High. Correcting this prevents a class of implementation errors where agents omit the WriteLimiter because they believe HLC provides sufficient backpressure.
- **Priority**: Degrading x High = **SECOND** (bundled with GAP-2).

#### Step 5: Hidden Coupling

None beyond GAP-2. The backpressure section at `02-concurrency.md:1728-1897` (INV-FERR-021) already correctly describes the WriteLimiter. The only coupling is the misleading cross-reference in INV-FERR-015.

---

### GAP-4: Checkpoint Serialization Format Not Specified

#### Step 1: Axioms

- INV-FERR-013: `load(checkpoint(S)) = S`. Round-trip identity.
- INV-FERR-028: `cold_start(S) < 5s` at 100M datoms.
- NEG-FERR-003: No data loss on crash.

The cleanroom audit (CR-005) identified that the checkpoint currently uses JSON serialization. At 100M datoms (~500 bytes/datom in JSON), the checkpoint is ~50GB — impossible to parse in 5s (INV-FERR-028 violated). The WAL already uses bincode. The decision to switch checkpoint to bincode was made in the cleanroom audit.

#### Step 2: Algebraic Structure

Serialization format is a **codec**: a pair of functions `(encode: Store → Bytes, decode: Bytes → Store)` such that `decode(encode(S)) = S` (INV-FERR-013). The choice of codec affects only performance, not correctness — as long as the round-trip identity holds.

JSON and bincode are both correct codecs for the checkpoint payload. The difference is:
- JSON: ~500 bytes/datom, human-readable, parse time O(n) with large constant
- Bincode: ~200 bytes/datom, binary, parse time O(n) with small constant

At 100M datoms: JSON ~50GB, bincode ~20GB. The 2.5x size difference plus binary parsing makes bincode approximately 5-10x faster for cold start.

#### Step 3: Location

This is a **spec-level** gap. The checkpoint format is described in `spec/02-concurrency.md` (INV-FERR-013) and the architecture doc, but neither specifies the serialization codec. The implementation detail is in `ferratomic-core/src/checkpoint.rs`.

The spec should state that the checkpoint uses bincode serialization (same as WAL) for INV-FERR-028 compliance. This is not a new ADR — it's a Level 2 implementation contract amendment to INV-FERR-013.

#### Step 4: Severity x Leverage

- **Severity**: Degrading. The current JSON checkpoint violates INV-FERR-028 at scale.
- **Leverage**: Medium. Fixes one critical defect (CR-005) and aligns checkpoint with WAL format.
- **Priority**: Degrading x Medium = **THIRD**.

#### Step 5: Hidden Coupling

- Architecture C (GAP-1) changes the checkpoint deserialization path: `Vec<Datom>` → `Vec<WireDatom>` + `into_trusted()`. The bincode switch (GAP-4) and Architecture C (GAP-1) touch the same code in `checkpoint.rs`.
- Dependency: GAP-1 should be implemented before GAP-4, because GAP-4 changes the serialization while GAP-1 changes the type being serialized. Doing them in the wrong order means double-touching the same callsites.
- This dependency is already captured in beads: bd-734s depends on bd-jh1f.

---

### GAP-5: ADR-FERR-004 Says "At-Least-Once" but Decision Was "Advisory-Only"

#### Step 1: Axioms

- INV-FERR-011: Observer epoch monotonically non-decreasing.
- INV-FERR-008: WAL durable-before-visible.
- ADR-FERR-004: Currently says "at-least-once" delivery with retry.

The cleanroom audit (HI-004 / DEFECT-P2-006) identified that observer notification error propagates as a transact error, even though the transaction IS committed (WAL fsynced, ArcSwap swapped). The caller thinks the transaction failed and may retry, creating duplicate transactions.

The decision was: observer errors should be advisory-only. The transaction is committed regardless of observer delivery success. This changes the semantics from "at-least-once" (with retry on failure) to "best-effort with anti-entropy fallback" — which is closer to Option C in the original ADR.

#### Step 2: Algebraic Structure

The observer delivery model is an **asynchronous channel** from the write path to subscribers. The question is whether failures on this channel propagate back to the write path.

- "At-least-once" = failures retry, errors propagate → write path blocks on observer
- "Advisory-only" = fire-and-forget, errors logged → write path never blocks on observer
- Anti-entropy (INV-FERR-022) is the convergence mechanism regardless of delivery semantics

The CRDT properties (INV-FERR-001-003) make any delivery semantics correct for convergence — the only question is latency to convergence. At-least-once converges faster (immediate retry). Advisory-only converges via anti-entropy (periodic). Both converge.

#### Step 3: Location

`spec/04-decisions-and-constraints.md`, ADR-FERR-004 section (lines 174-213). The ADR selected Option A (at-least-once) and rejected Option C (best-effort). The cleanroom audit effectively chose a hybrid: best-effort delivery (no error propagation to writer) with anti-entropy as convergence guarantee.

#### Step 4: Severity x Leverage

- **Severity**: Degrading. ADR says one thing, implementation decision says another. Agents implementing Phase 4c observers will code retry loops that block the writer.
- **Leverage**: Medium. Affects observer implementations in current and future phases.
- **Priority**: Degrading x Medium = **FOURTH**.

#### Step 5: Hidden Coupling

- The observer catch-up mechanism (`ObserverBroadcast::publish`) already implements best-effort delivery. The ADR language about retry loops was never implemented.
- Changing the ADR does not require code changes — it aligns the spec with the actual decision.
- The anti-entropy fallback (INV-FERR-022) is explicitly stated in the current ADR as a safety net. Promoting it from "fallback" to "convergence mechanism" is a framing change, not a design change.

---

### GAP-6: 12 Duplicate Bead Pairs Inflate Open Count

#### Step 1: Axioms

This is a process issue, not a spec issue. But it affects triage accuracy and agent work selection.

The 12 duplicate pairs were created across two separate bead filing sessions (likely by different agents or in different conversation turns). Each defect (HI-001 through HI-012) was filed twice with different bead IDs.

#### Step 2: Assessment

- **Severity**: Cosmetic. Duplicates don't cause incorrect behavior, but they confuse triage and inflate the open count from 101 to 113.
- **Leverage**: Low. Closing 12 beads is housekeeping.
- **Priority**: Cosmetic x Low = **LAST**.

For each pair, keep the bead that:
1. Has richer description/acceptance criteria
2. Is wired into the dependency graph (depends on bd-5p51)
3. Has earlier creation timestamp

---

## Phase 3: Solution Synthesis

### FINDING-1: Architecture C needs ADR-FERR-010

**Root cause**: The two-tier wire/core type system was decided in the cleanroom audit (session 6da9b454) but was only written to `docs/reviews/`, not to the canonical spec.

**Proposed fix**: Add `ADR-FERR-010: Deserialization Trust Boundary (Two-Tier Type System)` to `spec/04-decisions-and-constraints.md` after ADR-FERR-009. Include:
- Problem statement: derived `Deserialize` on `EntityId` is a `sorry` axiom
- Three alternatives considered (A: manual impls, B: newtype+From, C: two-tier wire/core)
- Decision: Architecture C
- Traces to INV-FERR-012, INV-FERR-054
- Phase 4c forward compatibility (`into_verified`, `into_merkle_verified`)

**Verification**:
- Test: spec review — ADR-FERR-010 present with complete alternatives-considered section
- INV-FERR: 012 (content identity), 054 (trust gradient)

**Risk**: None. This is a spec addition, not a code change.

**Effort**: S — ~60 lines of spec text, adapted from cleanroom audit Section 14.

**Depends on**: Nothing.

---

### FINDING-2: INV-FERR-015 u16 → u32 + backpressure narrative

**Root cause**: Spec was written before the cleanroom audit analyzed the u16 vs u32 tradeoff. The backpressure narrative conflates HLC overflow with the WriteLimiter.

**Proposed fix**: Edit `spec/02-concurrency.md`:
1. INV-FERR-015 Level 0: `logical : u16` → `logical : u32`
2. Level 1 narrative: Remove "65536 values" and "65M events/second" backpressure language. Add: "The logical counter is u32 (~4.3 billion values). Operational backpressure is provided by the WriteLimiter (INV-FERR-021), not by HLC overflow. The u32::MAX busy-wait in `tick()` is a theoretical safety valve."
3. Level 2 contract: `logical: u16` → `logical: u32` in Hlc struct
4. tick() implementation: `u16::MAX` → `u32::MAX`
5. Falsification: `u16::MAX` → `u32::MAX`
6. proptest: `0u16..1000` → `0u32..1000`

**Verification**:
- Test: grep for `u16` in `spec/02-concurrency.md` — should find 0 occurrences in HLC sections
- INV-FERR: 015 (HLC monotonicity), 021 (backpressure)

**Risk**: Lean proofs use `Nat` (unbounded), so no Lean changes needed. Proptest already uses u32. Code already uses u32.

**Effort**: S — 7 mechanical edits in one file.

**Depends on**: Nothing.

---

### FINDING-3: Checkpoint serialization format unspecified

**Root cause**: INV-FERR-013 Level 2 describes the checkpoint structure (magic, version, epoch, payload, BLAKE3 hash) but does not specify the payload serialization codec. The implementation uses JSON; the decision is to use bincode.

**Proposed fix**: Add a paragraph to INV-FERR-013 Level 2 in `spec/02-concurrency.md`:
> "The checkpoint payload is serialized using bincode (the same binary codec as the WAL). JSON serialization was used in the Phase 4a prototype but violates INV-FERR-028 (cold start < 5s) at 100M datoms due to ~2.5x size overhead and text parsing costs."

**Verification**:
- Test: spec review — INV-FERR-013 Level 2 states "bincode"
- INV-FERR: 013 (checkpoint equivalence), 028 (cold start)

**Risk**: None. Spec-only change.

**Effort**: S — 3 lines of spec text.

**Depends on**: Nothing.

---

### FINDING-4: ADR-FERR-004 observer semantics misalignment

**Root cause**: ADR-FERR-004 selected "at-least-once" delivery with retry loops. The cleanroom audit (HI-004) decided that observer errors should not propagate to the transact caller because the transaction is already committed.

**Proposed fix**: Amend ADR-FERR-004 in `spec/04-decisions-and-constraints.md`:
- Change "Option A: At-least-once" to a two-tier model:
  - **Intra-process observers**: Best-effort delivery. Errors logged, not propagated. Anti-entropy (INV-FERR-022) ensures convergence.
  - **Cross-process observers** (Phase 4c): At-least-once via anti-entropy protocol. No synchronous retry loop.
- Add a "Phase 4a Amendment" paragraph (following the pattern of ADR-FERR-003):
  > "Phase 4a Amendment: Observer delivery errors are advisory-only and do not propagate as transact failures. The transaction is committed when WAL fsync completes (INV-FERR-008); observer delivery is a post-commit side effect. If delivery fails, anti-entropy (INV-FERR-022) ensures eventual convergence."

**Verification**:
- Test: spec review — ADR-FERR-004 states "advisory-only" for intra-process
- INV-FERR: 011 (observer monotonicity), 008 (WAL ordering)

**Risk**: Changes the documented delivery guarantee. Observers that relied on "at-least-once" semantics (none exist yet) would need to handle missed events.

**Effort**: S — ~15 lines of spec amendment.

**Depends on**: Nothing.

---

### FINDING-5: Close 12 duplicate bead pairs

**Root cause**: Two separate bead creation passes filed the same HI-001 through HI-012 defects.

**Proposed fix**: For each pair, close the duplicate (the one NOT wired into bd-5p51's dependency chain) with reason "Duplicate of bd-XXXX".

| Keep | Close | Reason |
|------|-------|--------|
| bd-eoj5 | bd-9zfh | bd-eoj5 is in bd-5p51 dep chain |
| bd-pvep | bd-5kwg | bd-pvep is in bd-5p51 dep chain |
| bd-g0gv | bd-3gxj | bd-g0gv is in bd-5p51 dep chain |
| bd-qfpl | bd-9qcf | bd-qfpl is in bd-5p51 dep chain |
| bd-fgsz | bd-5u12 | bd-fgsz is in bd-5p51 dep chain |
| bd-mo5y | bd-2nqp | bd-mo5y is in bd-5p51 dep chain |
| bd-e3jj | bd-9l6g | bd-e3jj is in bd-5p51 dep chain |
| bd-05mf | bd-d3j6 | bd-05mf is in bd-5p51 dep chain |
| bd-0uan | bd-pn9o | bd-0uan is in bd-5p51 dep chain |
| bd-70q9 | bd-pzy3 | bd-70q9 is in bd-5p51 dep chain |
| bd-292f | bd-gbd9 | bd-292f is in bd-5p51 dep chain |
| bd-1jh4 | bd-pzmw | bd-1jh4 is in bd-5p51 dep chain |

**Verification**: `br list --status=open | wc -l` decreases by 12.

**Risk**: None. Closing duplicates with explicit "Duplicate of" reason.

**Effort**: S — 12 `br close` commands.

**Depends on**: Nothing.

---

## Execution Order

```
1. FINDING-5  (dedup beads)          — housekeeping, clears noise
2. FINDING-1  (ADR-FERR-010)         — highest severity x leverage
3. FINDING-2  (u16 → u32)            — bundled with GAP-3 backpressure narrative
4. FINDING-3  (checkpoint format)    — small spec addition
5. FINDING-4  (observer semantics)   — ADR amendment
```

All five are spec-only changes (no code). Total estimated effort: ~2 hours.

---

## Output Checklist

- [x] Every finding traces to specific INV-FERR, ADR-FERR, or NEG-FERR
- [x] Every solution has a verification plan
- [x] No solution violates a settled ADR-FERR
- [x] Solutions ordered by severity x leverage
- [x] Dependency edges explicit (GAP-4 depends on GAP-1 for implementation, but spec changes are independent)
- [x] Risks are specific
- [x] Effort estimates calibrated (all S = < 1 hour each)
- [x] Analysis is COMPLETE — no TBD items
