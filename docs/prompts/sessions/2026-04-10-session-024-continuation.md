# Ferratomic Continuation — Session 024

> Generated: 2026-04-10
> Last commit: `dd0f103` "docs(beads): file bd-b7pfg — Attribute u16 length guard"
> Branch: main (synced with master)

## Read First

1. `QUICKSTART.md` — project orientation (updated by parallel session 023.5-023.7)
2. `AGENTS.md` — guidelines and constraints
3. `GOALS.md` §7 — Six-Dimension Decision Evaluation Framework (canonical)
4. `spec/README.md` — load only the spec modules you need
5. This continuation prompt

## Session Summary

### Completed (session 024, 2026-04-09 → 2026-04-10)

**Phase 4a.5 cleanroom refinement tower — 5 of 8 steps complete:**

1. **bd-k5bv C8 rename** (commit `0070040`): AgentId→NodeId, tx/agent→tx/origin across 81 files, 519 tests pass. 4 parallel Opus subagents + orchestrator. Bead closed.

2. **bd-bdvf.13 five-lens spec audit** (commit `c1fcf9f`): Lifecycle/17 audit of spec/05 §23.8.5. 6 MAJOR findings fixed inline (INV-061 proptest no-op, INV-062 sig/body mismatch, INV-086 weak proof sketch, INV-025b proptest gap, 5 missing Referenced-by headers, L6538 placeholder). 7 MINOR beads filed. Parent bd-bdvf also closed.

3. **Lean theorems** (commit `010138e`): 16 new theorems for INV-FERR-060/061/062/063 in Federation.lean. 1 sorry (predecessor_complete injectivity — bd-aqg9h). lake build 768/768.

4. **Level 2 types** (commit `ee8d7ab`): 4 new ferratom modules (filter.rs, signing.rs, provenance.rs, bundle.rs) + error.rs variants + genesis schema 19→25 attrs. 4 parallel Opus subagents. 5 beads closed (bd-tck2, bd-8f4r, bd-hcns, bd-37za, bd-1zxn).

5. **Proptest harnesses** (commit `3f109b5`): 19 property tests in federation_properties.rs. Generators for DatomFilter, ProvenanceType, TxSignature, TxSigner.

**Verification audit + remediation:**

6. **Lifecycle/18 verification audit** (commit `30b6b05`): Full Lean/Kani/Stateright/CI-FERR-001/catalog audit. 1 CI-FERR-001 stale count fixed (19→25). 3 drift beads filed.

7. **All findings resolved** (commits `be489da`, `eb560ef`): Catalog synced (70→78 entries, stage counts updated), 5 stale "19 axiomatic" references fixed, 7 MINOR spec findings fixed. Zero open findings.

8. **Canonical hash unification** (commit `c681e80`): The most accretive change — unified content_hash with canonical_bytes (INV-FERR-086 + INV-FERR-012). content_hash now streams the canonical byte format (u16 attr length, 0x01..0x0B tags) into BLAKE3. Eliminated three divergent encodings. spec/09 INV-FERR-074/079 updated from bincode→canonical_bytes. 4 new proptests (round-trip, determinism, injectivity, hash consistency). tx_id_canonical_bytes public API added (ADR-FERR-035).

9. **bd-b7pfg filed** (commit `dd0f103`): Lab-grade bead for Attribute u16 max length enforcement (Curry-Howard). Blocks entire Phase 4a.5 implementation chain. Target composite: 10.0.

### Session Stats

- **9 commits pushed** to main + master
- **17 beads closed** + 1 lab-grade bead filed
- **~7000 lines** of code, spec, Lean, and tests
- **Zero open audit findings**
- All pre-commit gates green on every commit

### Decisions Made (locked)

- **D-024-1**: content_hash unified with canonical_bytes. content_hash now IS BLAKE3(canonical_bytes layout). The old ad-hoc encoding (u64 attr length, tags 0..10) is eliminated. This is FROZEN — cannot change after signing ships.
- **D-024-2**: Attribute u16 length guard (bd-b7pfg) blocks all implementation. The type system must enforce the canonical format constraint before downstream code builds on it.
- **D-024-3**: Coordination with parallel session 023.5 agent is via git pull --rebase (Agent Mail MCP not configured). spec/05 is this session's territory; spec/06 is theirs.

### Bugs Found

- **spec/09 INV-FERR-074/079 used bincode::serialize, not canonical_bytes** — latent Tier 1 correctness bug. Fixed in commit c681e80. Would have caused signing verification failures when fingerprint enters the signing message (ADR-FERR-036).
- **content_hash/canonical_bytes tag divergence** — content_hash used tags 0..10, canonical_bytes used 0x01..0x0B. Different hashes for same datom. Eliminated by unification.

### Stopping Point

**Exactly where I stopped**: All 9 commits pushed, working tree clean (only other agent's unstaged changes in docs/prompts/lifecycle/README.md and ferratomic-verify/kani/error_exhaustiveness.rs). bd-b7pfg is filed and wired as blocker for bd-3t63, bd-6j0r, bd-mklv, bd-sup6, bd-r3um.

**The last thing I verified working**: `cargo check --workspace`, `cargo clippy`, `cargo test -p ferratom -p ferratomic-store --lib` (60/60), `cargo test --test proptest_federation` (19/19), `lake build` (768/768, 1 sorry tracked).

**The next thing to do**: Execute bd-b7pfg (Attribute u16 length guard), then start Phase 4a.5 implementation via the dependency chain.

## Next Execution Scope

### Primary Task

**bd-b7pfg**: Enforce Attribute u16 max length at construction.

**File**: `ferratom/src/datom/mod.rs` — the `Attribute` type is a newtype `struct Attribute(Arc<str>)` defined in the datom module alongside `Datom`, `EntityId`, `Value`, `Op`. The `From<&str>` impl is also in this file.

**What to do**: Add `Attribute::new(s: &str) -> Result<Self, FerraError>` with length validation (`s.as_bytes().len() > u16::MAX as usize` → Err). Keep `From<&str>` delegating with panic for structurally-impossible inputs (same pattern as `NonNanFloat` rejecting NaN). Replace `unwrap_or(u16::MAX)` with direct `as u16` cast in `canonical_bytes`, `canonical_value_hash`, `push_tag_u16_str`, and `hash_tag_u16_str`. ~80 `Attribute::from("...")` call sites compile unchanged (From trait preserved). Target composite: 10.0.

**Known parallel issue (deferred)**: `Value::Keyword(Arc<str>)` has the same u16 length prefix in `canonical_value_hash` with the same `unwrap_or(u16::MAX)`. Keywords are always attribute-name-length strings in Ferratomic (e.g., `"provenance/observed"`). A future bead could add `Keyword::new() -> Result` for full Curry-Howard coverage, but this is lower priority than the Attribute guard because Keywords don't have a standalone construction API — they're created inline as `Value::Keyword(Arc::from("..."))`.

After bd-b7pfg, the implementation chain unlocks:

```
bd-b7pfg (Attribute u16 guard)
  ↓
bd-3t63 (emit_predecessors) + bd-6j0r (sign_transaction)
  ↓
bd-mklv (genesis_with_identity)
  ↓
bd-sup6 (selective_merge with receipts)
  ↓
bd-7dkk + bd-h51f + bd-1rcm + bd-lifv (observer, transport)
  ↓
bd-hlxr (integration tests) + bd-r7ht (bootstrap B17)
  ↓
bd-r3um (Phase 4a.5 gate close)
```

### Ready Queue

```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context

- bd-b7pfg blocks ALL Phase 4a.5 implementation beads
- bd-3t63 (predecessors) and bd-6j0r (signing) are independent after bd-b7pfg
- bd-6j0r (signing) will need `ed25519-dalek` added as a dependency to `ferratomic-core/Cargo.toml`. The `TxSignature`/`TxSigner` newtypes exist in ferratom (no crypto deps), but actual sign/verify operations live in ferratomic-core per the spec.
- bd-mklv (genesis_with_identity) depends on BOTH bd-3t63 and bd-6j0r
- **Parallel agent context**: A separate agent ran sessions 023.5-023.7 concurrently, pushing ~15 commits to spec/06-prolly-tree.md and ferratomic-positional/src/codec.rs. Those commits are on main between `0070040` and `dd0f103`. They are NOT your work — treat them as the other agent's changes per CLAUDE.md. Their territory is spec/06 + ferratomic-positional. Your territory is spec/05 + ferratom + ferratomic-store + ferratomic-core.
- **The critical commit**: `c681e80` (canonical hash unification) is the most important commit from session 024. It changed `content_hash()` from an ad-hoc encoding (u64 attr length, tags 0..10) to streaming the INV-FERR-086 canonical format (u16 attr length, tags 0x01..0x0B). ALL fingerprints, signatures, and content hashes now use one unified encoding. This is FROZEN — do not change the streaming hash format.

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` by default
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target` (MUST set)
- Phase N+1 cannot start until Phase N passes isomorphism check
- Full defensive engineering standards: GOALS.md §6
- **NEW**: content_hash IS BLAKE3(canonical_bytes layout). Do NOT change the streaming hash format without updating the entire system (fingerprints, signing, chunk addressing all depend on it)
- **NEW**: Attribute length must be validated at construction before canonical_bytes/content_hash are used in production signing paths
- Six-Dimension Decision Framework (GOALS.md §7) consulted before non-trivial decisions
- Knowledge Organization Rule (AGENTS.md) enforced — prescriptive content in canonical sources only

## Stop Conditions

Stop and escalate to the user if:

- The Attribute::new() → Result change causes >5 compilation errors outside of Attribute::from() — indicates a deeper API coupling than expected
- Any existing content_hash-dependent test fails with wrong hash VALUES (not just equality) — indicates a consumer that hardcodes specific hashes
- A signed transaction or fingerprint from a pre-unification build is encountered — the hash format changed, old fingerprints are invalid
- canonical_bytes for a Value::Keyword produces different bytes than canonical_value_hash for the same Keyword — the streaming/buffered equivalence is broken
- The parallel agent pushes changes to ferratom/src/datom/mod.rs — file conflict on the canonical_bytes implementation
