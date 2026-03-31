# Cleanroom Audit: Ferratomic Phase 4a

> **Date**: 2026-03-31
> **Auditor**: Claude Opus 4.6 (8 adversarial subagents, /effort max)
> **Scope**: Full 8-phase cleanroom review per `docs/prompts/lifecycle/06-cleanroom-review.md`
> **Methodology**: Cleanroom software engineering -- formal methods, spec-driven design, abstract algebra, formal verification
> **Standard**: Zero-defect, lab-grade, production-ready Rust
> **Build state at audit time**: Compiles (1 dead-code warning). Clippy FAILS (134 `unwrap_used` in test code). Fmt FAILS (1 file diff). Tests: **1 FAILURE** -- `inv_ferr_008_wal_roundtrip` in `proptest_wal` (DEFECT CR-006: serde_json used to deserialize bincode payload; took 437s to fail).

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Phase 1: Algebraic Correctness](#2-phase-1-algebraic-correctness)
3. [Phase 2: Invariant Integrity](#3-phase-2-invariant-integrity)
4. [Phase 3: Type-Theoretic Analysis](#4-phase-3-type-theoretic-analysis)
5. [Phase 4: Performance](#5-phase-4-performance)
6. [Phase 5: Test Adequacy](#6-phase-5-test-adequacy)
7. [Phase 6: Error Handling](#7-phase-6-error-handling)
8. [Phase 7: Documentation](#8-phase-7-documentation)
9. [Phase 8: Defect Register (Consolidated)](#9-phase-8-defect-register-consolidated)
10. [Spec Drift and Coverage Gaps](#10-spec-drift-and-coverage-gaps)
11. [WAL and Crash Recovery Deep Audit](#11-wal-and-crash-recovery-deep-audit)
12. [Concurrency Deep Audit](#12-concurrency-deep-audit)
13. [Design Decisions](#13-design-decisions)
14. [Architecture C: Two-Tier Type System for Deserialization Security](#14-architecture-c-two-tier-type-system-for-deserialization-security)
15. [Empirically Grounded Implementation Plan](#15-empirically-grounded-implementation-plan)

---

## 1. Executive Summary

8 audit phases complete. 8 Opus agents performed adversarial line-by-line review across ~90 files (spec, types, core, verification). All agents operated in discovery mode (high DoF), assuming bugs exist.

| Severity | Count | Breakdown |
|----------|-------|-----------|
| **CRITICAL** | 6 | Recovery loses schema (2), NaN deserialization bypass, EntityId deserialization bypass, JSON checkpoint unscalable, WAL proptest wrong deserializer |
| **HIGH** | 18 | Checkpoint not atomic, no parent dir fsync (WAL+checkpoint), observer delivery fails transact, commit_unchecked pub, WAL OOM, recovery swallows errors, HLC not wired, TxId u32 vs u16, LIVE index missing, epoch/physical conflation, schema merge lies |
| **MEDIUM** | 24 | Lock poison misreported, observer TOCTOU, CRC32 weak, index count mismatch, BigInt not arbitrary, verify_bijection count-only, observer full scan O(n), datom 5x clone, various test gaps |
| **MINOR** | 12 | Style, allocation, doc nits, generator distribution |
| **TOTAL** | **60** | |

**Clean phases**: None. All 8 review phases produced findings.

---

## 2. Phase 1: Algebraic Correctness

The core algebraic structure: `Store = (P(D), U)` -- a G-Set CRDT semilattice. Three laws must hold:
- L1: `merge(A, B) = merge(B, A)` (commutativity, INV-FERR-001)
- L2: `merge(merge(A,B),C) = merge(A,merge(B,C))` (associativity, INV-FERR-002)
- L3: `merge(A, A) = A` (idempotency, INV-FERR-003)
- L4/L5: `S <= apply(S, d)` and `|transact(S,T)| > |S|` (monotonic growth, INV-FERR-004)

### Positive Findings (Things That Are Correct)

1. **Datom set merge IS commutative**: `im::OrdSet::union` is true set union -- `A.union(B) == B.union(A)` by the OrdSet invariant. L1 holds for the datom set itself.
2. **Datom set merge IS associative**: `im::OrdSet::union` is associative. L2 holds.
3. **Datom set merge IS idempotent**: `A.union(A) == A`. L3 holds.
4. **Epoch merge IS commutative**: `max(a,b) == max(b,a)`. The epoch component satisfies L1.
5. **Monotonic growth holds for `transact`**: `transact` inserts datoms (set grows), epoch is incremented via `checked_add`, and `create_tx_metadata` guarantees at least 2 new datoms per transaction. L4/L5 satisfied.
6. **Datom Ord implementation is correct**: `Datom` derives `Ord` which gives lexicographic ordering on fields in declaration order (entity, attribute, value, tx, op). This matches the EAVT primary index order.
7. **Index rebuild is deterministic**: `Indexes::from_datoms` iterates the input and inserts into freshly constructed maps. Given the same datom set, the same indexes are produced.
8. **No integer overflow in epoch**: `transact` uses `checked_add(1)` and returns `InvariantViolation` on overflow.
9. **Schema merge IS commutative** (after re-analysis): The "keep min" resolution produces `min(A,B) = min(B,A)`.
10. **Content hash is collision-resistant across Value variants**: Discriminant tag system in `hash_value` prevents cross-variant collisions.

### Defects

### [MAJOR] DEFECT-P1-001: Schema conflict during merge is swallowed in release builds

**Location**: `ferratomic-core/src/store/apply.rs:92-96`
**Traces to**: INV-FERR-043 (schema compatibility)
**Evidence**: When `from_merge` encounters conflicting schema definitions, it fires `debug_assert!(false, ...)` which is a no-op in release builds. The `merge()` function in `merge.rs` (line 25-27) wraps `from_merge` but its doc comment says "Returns `FerraError::SchemaIncompatible` if the stores define the same attribute with different definitions" -- yet it always returns `Ok`. The `Semilattice::merge` trait impl also calls `Ok(Store::from_merge(a, b))` unconditionally.
**Expected**: INV-FERR-043 requires that conflicting schemas produce an error. The doc comment on `merge()` promises `FerraError::SchemaIncompatible` but the function never returns `Err`.
**Fix**: Either (a) `from_merge` should return `Result` and propagate the schema conflict as a real error, or (b) the doc comment and INV-FERR-043 should be relaxed to specify deterministic conflict resolution. The current state is a specification/implementation mismatch where the error path is documented but unreachable.

### [MAJOR] DEFECT-P1-002: `from_merge` uses `a.genesis_agent` unconditionally, breaking commutativity of the full Store struct

**Location**: `ferratomic-core/src/store/apply.rs:113`
**Traces to**: INV-FERR-001 (commutativity)
**Evidence**: Line 113: `genesis_agent: a.genesis_agent`. When merging store A (genesis_agent=X) with store B (genesis_agent=Y), `merge(A,B)` produces genesis_agent=X while `merge(B,A)` produces genesis_agent=Y. The `genesis_agent` field propagates into `create_tx_metadata` (line 200-226), which uses `agent.as_bytes()[0]` to construct the transaction entity ID. Different genesis_agent values produce different entity IDs for transaction metadata, creating state divergence after subsequent transacts on merged stores.
**Expected**: `merge(A,B)` and `merge(B,A)` should produce Stores that behave identically for all subsequent operations.
**Fix**: Use a deterministic resolution for `genesis_agent` in `from_merge`, e.g., `min(a.genesis_agent, b.genesis_agent)` by byte comparison.

### [MAJOR] DEFECT-P1-003: `create_tx_metadata` makes tx_entity depend on first byte of agent, not full agent

**Location**: `ferratomic-core/src/store/apply.rs:200-226`
**Traces to**: INV-FERR-014, INV-FERR-004
**Evidence**: Lines 201-203: `let tx_entity = EntityId::from_content(&format!("tx-{epoch}-{}", agent.as_bytes()[0]).into_bytes())`. Only the first byte of agent ID is used. Two agents whose first byte is identical will produce colliding `tx_entity` values at the same epoch.
**Expected**: Transaction entity IDs should be unique per transaction.
**Fix**: Use the full agent ID (or a hash of epoch + full agent) for `tx_entity` construction.

### [MINOR] DEFECT-P1-004: Generic recovery path `recover_checkpoint_plus_wal` does not advance epoch or evolve schema

**Location**: `ferratomic-core/src/storage.rs:493-502`
**Traces to**: INV-FERR-014, INV-FERR-009
**Evidence**: The generic `recover_checkpoint_plus_wal` uses raw `store.insert(&datom)` to replay WAL entries. The filesystem-specific `Database::recover` at `db/recover.rs:97-103` uses `store.replay_entry(entry.epoch, &datoms)` which correctly advances the epoch.
**Expected**: Both recovery paths should produce identical state.
**Fix**: Use `replay_entry` in `recover_checkpoint_plus_wal` instead of raw `insert`.

### [MINOR] DEFECT-P1-005: `replay_entry` does not invoke `evolve_schema`

**Location**: `ferratomic-core/src/store/apply.rs:55-61`
**Traces to**: INV-FERR-009, INV-FERR-014
**Evidence**: `replay_entry` inserts datoms and advances epoch but does not call `evolve_schema`. If the WAL contains schema-defining datoms, they enter the primary set but the schema doesn't know about them.
**Expected**: After WAL recovery, the store's schema should match what it was before the crash.
**Fix**: Add `evolve_schema(&mut self.schema, datoms)?` to `replay_entry`.

### [MINOR] DEFECT-P1-006: `full_delta_since` conflates `tx.physical()` with epoch

**Location**: `ferratomic-core/src/observer.rs:134-140`
**Traces to**: INV-FERR-011
**Evidence**: `full_delta_since` filters by `datom.tx().physical() > from_epoch`. The `tx.physical()` field is set to `self.epoch` in `Store::transact`, so `physical() == epoch` currently. But if HLC is wired in, `physical()` becomes wall-clock milliseconds and the filter breaks.
**Expected**: Filter by actual epoch, not `tx.physical()`.
**Fix**: Document the coupling or add a proper epoch-based filter.

### [STYLE] DEFECT-P1-007: `merge()` wrapper has misleading doc comment

**Location**: `ferratomic-core/src/merge.rs:22-27`
**Traces to**: INV-FERR-043
**Evidence**: Doc says "Returns `FerraError::SchemaIncompatible`" but body is `Ok(Store::from_merge(a, b))`.
**Fix**: Update doc or implement the error path.

---

## 3. Phase 2: Invariant Integrity

### [CRITICAL] DEFECT-P2-001: Recovery path in `storage.rs` uses `store.insert()` which skips epoch advance and schema evolution

**Location**: `ferratomic-core/src/storage.rs:494-502` and `ferratomic-core/src/storage.rs:530-537`
**Traces to**: INV-FERR-014, INV-FERR-009
**Evidence**: The generic `cold_start_with_backend` recovery path uses raw `store.insert(&datom)` in a loop. Meanwhile, the filesystem-specific `Database::recover_from_wal` correctly calls `store.replay_entry(entry.epoch, &datoms)?`.
**Consequences**: (1) Recovered database epoch stays at checkpoint epoch (or 0 for WAL-only). Next `transact()` produces epoch=1, violating INV-FERR-007 monotonicity. (2) Schema-defining transactions replayed through this path silently fail to install into the schema.
**Fix**: Replace `store.insert(&datom)` loops with `store.replay_entry(entry.epoch, &datoms)?`.

### [CRITICAL] DEFECT-P2-002: WAL fsync failure mapping -- poisoned mutex reported as Backpressure

**Location**: `ferratomic-core/src/db/transact.rs:92`
**Traces to**: INV-FERR-008
**Evidence**: `self.wal.lock().map_err(|_| FerraError::Backpressure)?`. A poisoned WAL mutex (previous panic) is reported as `Backpressure` -- callers retry endlessly against a permanently poisoned lock.
**Expected**: `FerraError::InvariantViolation` for poisoned mutex.
**Fix**: Change mapping to `InvariantViolation`.

### [HIGH] DEFECT-P2-003: `create_tx_metadata` uses `SystemTime::now()` -- non-deterministic

**Location**: `ferratomic-core/src/store/apply.rs:204-208`
**Traces to**: INV-FERR-031, INV-FERR-014
**Evidence**: Wall-clock timestamp embedded in `tx/time` datom. WAL replay is deterministic (stores post-stamp datoms), but a fresh `transact` replaying the same logical operations produces different datoms. The `#[allow(clippy::cast_possible_truncation)]` on `as i64` truncates u128 to i64.
**Fix**: Document intentional non-determinism. Use `i64::try_from(ms).unwrap_or(i64::MAX)`.

### [HIGH] DEFECT-P2-004: Checkpoint write is not atomic -- crash during write produces corrupt file

**Location**: `ferratomic-core/src/checkpoint.rs:183-201`
**Traces to**: INV-FERR-013
**Evidence**: `File::create(path)` truncates existing checkpoint immediately. Crash between truncation and `sync_all` loses both old and new.
**Fix**: Write to temp file, fsync, rename (atomic on POSIX), fsync parent directory.

### [HIGH] DEFECT-P2-005: WAL `Wal::create` does not fsync parent directory

**Location**: `ferratomic-core/src/wal/mod.rs:92-104`
**Traces to**: INV-FERR-008
**Evidence**: After creating WAL file, parent directory not fsynced. On crash, directory entry may be lost.
**Fix**: Open parent directory and `sync_all()` after file creation.

### [HIGH] DEFECT-P2-006: Observer notification error propagates as transact failure -- transaction IS committed

**Location**: `ferratomic-core/src/db/transact.rs:76`
**Traces to**: INV-FERR-011, INV-FERR-020
**Evidence**: WAL fsynced (Step 2), ArcSwap swapped (Step 3), then `notify_observers(...)?` returns error. Caller thinks transaction failed; may retry.
**Fix**: `let _ = self.notify_observers(...)` or return advisory-only error alongside receipt.

### [MEDIUM] DEFECT-P2-007: Observer catch-up race -- concurrent write between load and lock

**Location**: `ferratomic-core/src/observer.rs:89-116`
**Traces to**: INV-FERR-011
**Evidence**: Between write lock release and observer delivery, another transaction can advance the store. The `store` snapshot passed to `publish` may include datoms from a later transaction.
**Fix**: Pass `new_store` directly to `notify_observers` rather than re-loading from `self.current.load()`.

### [MEDIUM] DEFECT-P2-008: `verify_bijection` only checks cardinality, not datom identity

**Location**: `ferratomic-core/src/indexes.rs:330-333`
**Traces to**: INV-FERR-005
**Evidence**: Checks that all four indexes have the same count, not that they contain the same datoms. A bug that inserts different datoms into different indexes passes this check.
**Fix**: Full datom-set comparison in debug/canary mode.

### [MEDIUM] DEFECT-P2-009: `replay_entry` does not call `evolve_schema`

**Location**: `ferratomic-core/src/store/apply.rs:55-61`
**Traces to**: INV-FERR-014, INV-FERR-009
**Evidence**: Same as DEFECT-P1-005. Schema-defining transactions lost on ALL recovery paths.
**Fix**: Add `evolve_schema` call to `replay_entry`.

### [MEDIUM] DEFECT-P2-010: `Snapshot` does not capture secondary indexes

**Location**: `ferratomic-core/src/store/mod.rs:87-113`
**Traces to**: INV-FERR-006, INV-FERR-005
**Evidence**: `Snapshot` only carries primary datom set and epoch. Query engine (Phase 4d) would need O(n) index rebuild per snapshot.
**Fix**: Include `Indexes` clone in `Snapshot` (cheap via `im::OrdMap` structural sharing).

### [LOW] DEFECT-P2-011: `Observer::observe` silently returns stale snapshot on epoch regression

**Location**: `ferratomic-core/src/observer.rs:178-187`
**Traces to**: INV-FERR-011
**Fix**: Add debug_assert or return error when `current_epoch < self.last_epoch.load()`.

### [LOW] DEFECT-P2-012: No concurrent reader/writer test for INV-FERR-020

**Traces to**: INV-FERR-020
**Fix**: Add multi-threaded stress test.

### [LOW] DEFECT-P2-013: `from_merge` uses `debug_assert!(false)` for schema conflicts

**Location**: `ferratomic-core/src/store/apply.rs:92-95`
**Traces to**: INV-FERR-043
**Fix**: Use `log::warn!` or return error in release mode.

---

## 4. Phase 3: Type-Theoretic Analysis

### [CRITICAL] DEFECT-P3-001: NonNanFloat Deserialize bypasses NaN rejection

**Location**: `ferratom/src/datom/value.rs:60`
**Traces to**: INV-FERR-012
**Evidence**: `NonNanFloat` derives `Deserialize` which delegates to `OrderedFloat<f64>`, which accepts NaN. A corrupt checkpoint or WAL payload can inject NaN, breaking Eq/Hash determinism. The `new()` constructor gate is bypassed. `OrderedFloat` does NOT reject NaN during deserialization.
**Expected**: Deserialization must validate the same invariant as construction: NaN is rejected.
**Fix**: Manual `Deserialize` impl that rejects NaN (see Architecture C section).

### [CRITICAL] DEFECT-P3-002: EntityId Deserialize bypasses BLAKE3 provenance

**Location**: `ferratom/src/datom/entity.rs:14`
**Traces to**: INV-FERR-012
**Evidence**: `from_bytes` is test-gated, but derived `Deserialize` is always available. Any checkpoint/WAL/network payload can inject an `EntityId` with arbitrary 32 bytes that are not BLAKE3 of any content. This is the EXACT same bypass that `from_bytes` was gated to prevent, but the serde path was not guarded. In Phase 4c (federation), this becomes an active attack vector -- Byzantine peers can forge EntityIds.
**Expected**: Deserialization must go through a controlled trust boundary.
**Fix**: Two-tier type system (Architecture C) with `WireEntityId` -> verification -> `EntityId`.

### [HIGH] DEFECT-P3-003: `commit_unchecked` is pub, not cfg-gated

**Location**: `ferratomic-core/src/writer/mod.rs:239`
**Traces to**: INV-FERR-009
**Evidence**: `pub fn commit_unchecked(self) -> Transaction<Committed>` is available to all callers. Downstream crates can bypass schema validation entirely. Used in production code at `store/checkpoint.rs:97`. The typestate claim that `Committed` proves validation is violated.
**Fix**: Gate behind `#[cfg(any(test, feature = "test-utils"))]`.

### [HIGH] DEFECT-P3-004: Schema.define() silently overwrites conflicting definitions

**Location**: `ferratom/src/schema.rs:173`
**Traces to**: INV-FERR-009, INV-FERR-043
**Evidence**: `BTreeMap::insert` silently replaces existing entry. No check for conflicting ValueType or Cardinality on redefinition.
**Fix**: Return `Result` and reject conflicting redefinitions.

### [HIGH] DEFECT-P3-005: WriteLimiter counter can leak via mem::forget

**Location**: `ferratomic-core/src/backpressure.rs:57-65`
**Traces to**: INV-FERR-021
**Evidence**: `WriteGuard` relies on `Drop` for counter decrement. `std::mem::forget` (safe Rust) prevents Drop from running. After enough leaks, writes are permanently rejected.
**Fix**: Document RAII contract. Optionally add periodic health check.

### [HIGH] DEFECT-P3-006: Observer full_delta_since conflates epoch with TxId.physical

**Location**: `ferratomic-core/src/observer.rs:134-140`
**Traces to**: INV-FERR-011
**Evidence**: Same as DEFECT-P1-006. `physical()` happens to equal epoch now; breaks when HLC is wired in.
**Fix**: Filter by actual epoch.

### [MEDIUM] DEFECT-P3-007: Wildcard matches on Value enum in schema_evolution

**Location**: `ferratomic-core/src/schema_evolution.rs:124`
**Traces to**: Code quality standard
**Evidence**: `_ => continue` on `Value` enum. If a new variant is added, silently skips it.
**Fix**: Exhaustive match on all 11 variants.

### [MEDIUM] DEFECT-P3-008: `as` casts with potential truncation

**Locations**: `wal/recover.rs:51,132`, `checkpoint.rs:318`, `store/apply.rs:208`, `datom/mod.rs:129,179`, `clock/mod.rs:150`
**Traces to**: NEG-FERR-001
**Fix**: Use `try_from`/`try_into` for narrowing casts.

### [MEDIUM] DEFECT-P3-009: Index key types have pub fields

**Location**: `ferratomic-core/src/indexes.rs:100-118`
**Traces to**: INV-FERR-005
**Evidence**: `pub struct EavtKey(pub EntityId, pub Attribute, ...)`. Anyone can construct mismatched keys.
**Fix**: Fields `pub(crate)`, construct only through `from_datom`.

### [MEDIUM] DEFECT-P3-010: Attribute doc claims O(1) equality, it's O(n)

**Location**: `ferratom/src/datom/value.rs:20`
**Evidence**: Derived `PartialEq` on `Arc<str>` compares by content, not pointer.
**Fix**: Correct the doc comment.

### [MEDIUM] DEFECT-P3-011: FerraError Clone forces lossy io::Error conversion

**Location**: `ferratom/src/error.rs:21`
**Evidence**: `Clone` derive forces `From<io::Error>` to use `e.to_string()`, losing `ErrorKind`.
**Fix**: Consider `Arc<io::Error>` or drop Clone requirement.

### [LOW] DEFECT-P3-012: BigDec scale not encoded in type

**Location**: `ferratom/src/datom/value.rs:113-114`
**Evidence**: `BigDec(i128)` without scale field. Two datoms with same i128 but different schema-defined scales are `Eq`.
**Fix**: Document or add scale field.

### [LOW] DEFECT-P3-013: TxId placeholder (0,0,zero_agent) could collide with real TxId

**Location**: `ferratom/src/clock/txid.rs:94`
**Fix**: Use sentinel values (u64::MAX, u32::MAX) for placeholder.

---

## 5. Phase 4: Performance

### [CRITICAL] DEFECT-P4-001: Checkpoint uses JSON serialization -- INV-FERR-028 violated at scale

**Location**: `ferratomic-core/src/checkpoint.rs:101-102`
**Traces to**: INV-FERR-028 (cold start < 5s at 100M datoms)
**Evidence**: `serde_json::to_vec(&payload)`. At 100M datoms (~500 bytes/datom in JSON), checkpoint is ~50GB. Cannot parse in 5s. WAL already uses bincode.
**Fix**: Switch checkpoint to bincode.

### [MAJOR] DEFECT-P4-002: WAL recovery reads entire file into memory

**Location**: `ferratomic-core/src/wal/recover.rs:34-38`
**Traces to**: INV-FERR-028
**Evidence**: `self.file.read_to_end(&mut buf)`. 10GB WAL = 10GB allocation.
**Fix**: Streaming frame parser.

### [MAJOR] DEFECT-P4-003: CRC32 computed byte-by-byte without lookup table

**Location**: `ferratomic-core/src/wal/mod.rs:154-167`
**Traces to**: INV-FERR-026, INV-FERR-028
**Evidence**: 8 bit-shift iterations per byte. ~8x slower than lookup table. On 1GB WAL recovery, significant overhead.
**Fix**: Use `crc32fast` crate (hardware-accelerated on x86) or lookup table.

### [MAJOR] DEFECT-P4-004: Observer catch-up scans entire datom set O(n)

**Location**: `ferratomic-core/src/observer.rs:134-140`
**Traces to**: INV-FERR-011
**Evidence**: `full_delta_since` iterates ALL datoms. With 100M datoms, O(n) inside the observers mutex. Also clones `self.recent` VecDeque on every publish call (line 98).
**Fix**: Epoch-indexed structure. Avoid `recent` clone.

### [MAJOR] DEFECT-P4-005: Store::clone on every transact includes Schema clone

**Location**: `ferratomic-core/src/db/transact.rs:59-60`
**Traces to**: INV-FERR-027
**Evidence**: `Store::clone` clones `Schema` (HashMap) on the hot write path. Schema grows via evolution.
**Fix**: Wrap Schema in `Arc`.

### [MODERATE] DEFECT-P4-006: 5x datom cloning per insert

**Location**: `ferratomic-core/src/indexes.rs:258-267`
**Traces to**: INV-FERR-026
**Evidence**: 4 index inserts each clone the datom + 1 primary insert = 5 full clones. Each clone allocates for Arc<str> refcounting on Attribute and Value.
**Fix**: Use `Arc<Datom>` to share across indexes.

### [MODERATE] DEFECT-P4-007: `all_datoms.clone()` in Store::transact hot path

**Location**: `ferratomic-core/src/store/apply.rs:164`
**Evidence**: Clones entire stamped datom vector for receipt. Then iterates and clones each datom again for insert. 7*(N+2) full datom clones per transact.
**Fix**: Consume vector, use references for insertion.

### [MODERATE] DEFECT-P4-008: Checkpoint loads entire file into memory

**Location**: `ferratomic-core/src/checkpoint.rs:216`
**Traces to**: INV-FERR-028
**Evidence**: `std::fs::read(path)`. JSON checkpoint at 100M = 50GB+ in RAM.
**Fix**: Streaming deserialization (after switching to bincode).

### [MINOR] DEFECT-P4-009: Redundant observer datom Vec allocation per publish

**Location**: `ferratomic-core/src/observer.rs:90-93`
**Fix**: Use `Arc<[Datom]>` in ring buffer entries.

### [MINOR] DEFECT-P4-010: WAL writer allocates new Vec per frame

**Location**: `ferratomic-core/src/wal/writer.rs:65`
**Fix**: Reuse frame buffer stored in Wal struct.

---

## 6. Phase 5: Test Adequacy

### [CRITICAL] DEFECT-P5-001: WAL proptest uses serde_json to deserialize bincode payloads

**Location**: `ferratomic-verify/proptest/wal_properties.rs:51-53`
**Traces to**: INV-FERR-008
**Evidence**: WAL writes via `bincode::serialize` but proptest recovery deserializes with `serde_json::from_slice`. Either test never reaches this path or silently fails.
**Fix**: Replace `serde_json::from_slice` with `bincode::deserialize`.

### [MAJOR] DEFECT-P5-002: No test coverage for INV-FERR-033 through INV-FERR-055

**Traces to**: INV-FERR-033 through INV-FERR-055
**Evidence**: 23 invariants with zero test coverage. These are Phase 4c/4d invariants.
**Fix**: Add stub tests documenting pending implementation.

### [MAJOR] DEFECT-P5-003: No test for INV-FERR-043 (schema merge compatibility)

**Traces to**: INV-FERR-043
**Evidence**: No proptest or integration test generates stores with conflicting attribute definitions.
**Fix**: Add proptest with conflicting schemas.

### [MAJOR] DEFECT-P5-004: INV-FERR-027 read latency test measures average, not P99.99

**Location**: `ferratomic-verify/integration/test_thresholds.rs:165-215`
**Traces to**: INV-FERR-027
**Evidence**: Computes `avg_ns = elapsed.as_nanos() / lookup_count`. Spec says P99.99. Uses only 1,000 lookups (need >= 10,000 for P99.99).
**Fix**: Time individual lookups, sort, assert P99.99.

### [MODERATE] DEFECT-P5-005: Cold start threshold test uses 1K datoms

**Location**: `ferratomic-verify/integration/test_thresholds.rs:226-250`
**Traces to**: INV-FERR-028
**Fix**: Add release-mode benchmark at 100K+ datoms.

### [MODERATE] DEFECT-P5-006: Proptest atomicity assertion is tautological

**Location**: `ferratomic-verify/proptest/index_properties.rs:153-174`
**Traces to**: INV-FERR-006
**Evidence**: Compares pre-stamp datoms (with placeholder TxId) against post-stamp store. `visible_count == 0` is always true because stamped datoms have different TxIds.
**Fix**: Use `TxReceipt::datoms()` to get post-stamp datoms.

### [MODERATE] DEFECT-P5-007: HLC not wired into Database -- clock proptests don't verify end-to-end

**Traces to**: INV-FERR-015, INV-FERR-016
**Evidence**: `Store::transact` uses epoch counter, not HLC. Clock proptests test infrastructure that is not connected.
**Fix**: Wire HLC into transact (see Decision 2).

### [MODERATE] DEFECT-P5-008: No error path tests for WAL write failure

**Location**: `ferratomic-core/src/db/transact.rs:91-99`
**Traces to**: INV-FERR-008
**Fix**: Create `FailingWalBackend` and test atomicity on WAL failure.

### [MINOR] DEFECT-P5-009: Kani harnesses test BTreeSet, not Store

**Location**: `ferratomic-verify/kani/crdt_laws.rs:12-50`
**Traces to**: INV-FERR-001-003
**Fix**: Replace `BTreeSet::union` tests with `Store::from_merge`.

### [MINOR] DEFECT-P5-010: Value generator lacks edge cases

**Location**: `ferratomic-verify/src/generators.rs:46-62`
**Fix**: Add empty string, empty bytes, large-value generators.

---

## 7. Phase 6: Error Handling

### [HIGH] DEFECT-P6-001: `From<io::Error>` loses ErrorKind

**Location**: `ferratom/src/error.rs:244-248`
**Traces to**: INV-FERR-019
**Evidence**: `Self::Io(e.to_string())` discards `ErrorKind`. Callers can't distinguish `NotFound` from `PermissionDenied`.
**Fix**: Store `io::ErrorKind` or `Arc<io::Error>`.

### [HIGH] DEFECT-P6-002: Checkpoint write is not atomic

**Location**: `ferratomic-core/src/checkpoint.rs:183-201`
**Traces to**: INV-FERR-013
**Evidence**: Same as DEFECT-P2-004. `File::create` truncates immediately.
**Fix**: Write-to-temp-then-rename.

### [HIGH] DEFECT-P6-003: Observer delivery failure propagates as transact error

**Location**: `ferratomic-core/src/db/transact.rs:76`
**Traces to**: INV-FERR-011
**Evidence**: Same as DEFECT-P2-006. Transaction IS committed but caller gets Err.
**Fix**: Swallow or advisory-only error.

### [MEDIUM] DEFECT-P6-004: WAL mutex poison reported as Backpressure

**Location**: `ferratomic-core/src/db/transact.rs:92`
**Evidence**: Same as DEFECT-P2-002.
**Fix**: Map to `InvariantViolation`.

### [MEDIUM] DEFECT-P6-005: Recovery silently swallows errors, falls to genesis

**Location**: `ferratomic-core/src/storage.rs:452-477, 579-605`
**Traces to**: INV-FERR-014
**Evidence**: `if let Ok(result) = ...` discards errors. Transient I/O error -> silent data loss (genesis).
**Fix**: Distinguish corruption (fallthrough) from I/O errors (propagate).

### [MEDIUM] DEFECT-P6-006: Schema merge uses debug_assert, not runtime error

**Location**: `ferratomic-core/src/store/apply.rs:92-99`
**Traces to**: INV-FERR-043
**Evidence**: Same as DEFECT-P1-001.
**Fix**: Return error or log::warn.

### [MEDIUM] DEFECT-P6-007: Seek error discarded in InMemoryBackend

**Location**: `ferratomic-core/src/storage.rs:358`
**Evidence**: `let _ = cursor.seek(...)`.
**Fix**: Propagate with `?`.

### [MEDIUM] DEFECT-P6-008: Write lock poison also mapped to Backpressure

**Location**: `ferratomic-core/src/db/transact.rs:55-56`
**Traces to**: INV-FERR-007
**Evidence**: `try_lock().map_err(|_| FerraError::Backpressure)`. Poisoned vs WouldBlock not distinguished.
**Fix**: Match on TryLockError variant.

### [LOW] DEFECT-P6-009: FerraError Clone allows error duplication

**Location**: `ferratom/src/error.rs:21`
**Fix**: Audit whether Clone is needed; document trade-off.

### [LOW] DEFECT-P6-010: `create_tx_metadata` uses `unwrap_or_default()` on clock failure

**Location**: `ferratomic-core/src/store/apply.rs:205-208`
**Evidence**: Pre-epoch clock produces `Instant(0)` silently.
**Fix**: Diagnostic warning.

### [LOW] DEFECT-P6-011: `#[allow(clippy::cast_possible_truncation)]` on `as i64`

**Location**: `ferratomic-core/src/store/apply.rs:204-208`
**Fix**: Use `i64::try_from` with documented fallback.

---

## 8. Phase 7: Documentation

### [HIGH] DEFECT-P7-001: `merge::merge` documents SchemaIncompatible error it can never return

**Location**: `ferratomic-core/src/merge.rs:22-27`
**Fix**: Implement error or correct docs.

### [MEDIUM] DEFECT-P7-002: Checkpoint doc claims "consistency with WAL" -- WAL uses bincode

**Location**: `ferratomic-core/src/checkpoint.rs:27-28`
**Fix**: Correct doc.

### [MEDIUM] DEFECT-P7-003: Aspirational docs in snapshot.rs, transport.rs

**Locations**: `ferratomic-core/src/snapshot.rs:1-8`, `ferratomic-core/src/transport.rs:1-7`
**Fix**: Replace with factual "reserved" statements.

### [MEDIUM] DEFECT-P7-004: Three datalog modules contain TODO without tracking issue

**Locations**: `ferratomic-datalog/src/parser.rs:3`, `planner.rs:3`, `evaluator.rs:3`
**Fix**: Reference beads issue IDs.

### [MEDIUM] DEFECT-P7-005: lib.rs Quick Start example uses `.expect()`

**Location**: `ferratomic-core/src/lib.rs:149`
**Traces to**: NEG-FERR-001
**Fix**: Use `?` in example.

### [LOW] DEFECT-P7-006: `commit_unchecked` doc says "Testing only" but not cfg-gated

**Location**: `ferratomic-core/src/writer/mod.rs:233-245`
**Fix**: Gate or re-export via test-utils feature.

### [LOW] DEFECT-P7-007: CLAUDE.md does not state which phases are complete

**Fix**: Add Phase Status section.

### [LOW] DEFECT-P7-008: lib.rs docstring lists 6 indexes, code has 4

**Location**: `ferratomic-core/src/lib.rs:113`
**Fix**: Align docstring with implementation.

---

## 9. Phase 8: Defect Register (Consolidated)

### Consolidated by Priority (deduplicated across phases)

#### CRITICAL (6) -- Must Fix Before Phase 4a Gate

| ID | Title | Root Defect |
|----|-------|-------------|
| CR-001 | `replay_entry` skips `evolve_schema` -- schema lost on ALL recovery | P1-005, P2-001, P2-009 |
| CR-002 | Generic backend recovery uses `insert()` not `replay_entry()` -- epoch stuck | P1-004, P2-001 |
| CR-003 | NonNanFloat Deserialize bypasses NaN rejection | P3-001 |
| CR-004 | EntityId Deserialize bypasses BLAKE3 provenance | P3-002 |
| CR-005 | Checkpoint uses JSON -- INV-FERR-028 impossible at scale | P4-001 |
| CR-006 | WAL proptest uses wrong deserializer (serde_json vs bincode) | P5-001 |

#### HIGH (18) -- Should Fix Before Phase 4b

| ID | Title | Root Defect |
|----|-------|-------------|
| HI-001 | Checkpoint write not atomic (crash truncates old) | P2-004, P6-002 |
| HI-002 | No parent dir fsync after WAL creation | P2-005 |
| HI-003 | No parent dir fsync after checkpoint creation | WAL audit |
| HI-004 | Observer delivery failure propagates as transact error | P2-006, P6-003 |
| HI-005 | `commit_unchecked` is pub, not cfg-gated | P3-003 |
| HI-006 | No WAL payload size limit -- bincode can OOM | WAL audit |
| HI-007 | WAL recovery reads entire file into memory | P4-002 |
| HI-008 | Recovery silently falls to genesis on I/O errors | P6-005 |
| HI-009 | `from_merge` always returns Ok despite doc claiming SchemaIncompatible | P1-001, P7-001 |
| HI-010 | TxId logical u32 vs spec u16 (spec drift, not code bug) | Spec drift |
| HI-011 | Store::transact uses epoch counter as TxId.physical, not HLC | Spec drift |
| HI-012 | Observer full_delta_since compares physical() with epoch | P1-006, P3-006 |
| HI-013 | INV-FERR-029 LIVE View Resolution not implemented (Stage 0) | Coverage gap |
| HI-014 | `from_merge` genesis_agent breaks commutativity | P1-002 |
| HI-015 | Schema.define() silently overwrites conflicts | P3-004 |
| HI-016 | Observer registration TOCTOU -- missed epochs | CC audit |
| HI-017 | `From<io::Error>` loses ErrorKind | P6-001 |
| HI-018 | 4 indexes vs spec's 6 (Entity + LIVE missing) | Spec drift |

#### MEDIUM (24)

| ID | Title |
|----|-------|
| ME-001 | Write lock poison mapped to Backpressure |
| ME-002 | WAL mutex poison mapped to Backpressure |
| ME-003 | `verify_bijection` checks count only |
| ME-004 | CRC32 byte-by-byte, no lookup table |
| ME-005 | Observer catch-up scans entire datom set O(n) |
| ME-006 | 5x datom cloning per insert |
| ME-007 | Checkpoint loads entire file into memory |
| ME-008 | Snapshot lacks secondary indexes |
| ME-009 | Observer delivery outside write lock allows out-of-order epochs |
| ME-010 | transaction_count uses Relaxed ordering |
| ME-011 | WAL epoch monotonicity not enforced on append |
| ME-012 | last_synced_epoch never updated after fsync |
| ME-013 | WAL recovery truncation not fsynced |
| ME-014 | Generic backend recovery returns Database with no WAL |
| ME-015 | BigInt/BigDec as i128, not arbitrary precision |
| ME-016 | NEG-FERR-001 lints incomplete in ferratom + ferratomic-core |
| ME-017 | No INV-FERR-043 schema conflict proptest |
| ME-018 | P99.99 read latency test only measures average |
| ME-019 | Cold start threshold test uses 1K datoms |
| ME-020 | Proptest atomicity assertion is tautological |
| ME-021 | No error-path tests for WAL write failure |
| ME-022 | Seek error discarded in InMemoryBackend |
| ME-023 | Wildcard match on Value enum in schema_evolution |
| ME-024 | Index key types have pub fields |

#### MINOR (12)

| ID | Title |
|----|-------|
| MI-001 | `as` casts with potential truncation (6 locations) |
| MI-002 | Attribute doc claims O(1) equality, it's O(n) |
| MI-003 | BigDec scale not encoded in type |
| MI-004 | Dead code: Opening struct |
| MI-005 | Redundant observer datom Vec allocation per publish |
| MI-006 | WAL writer allocates new Vec per frame |
| MI-007 | receipt_datoms.clone() double-clones in transact |
| MI-008 | Kani harnesses test BTreeSet not Store |
| MI-009 | Value generator lacks edge cases |
| MI-010 | Checkpoint doc claims "consistency with WAL" |
| MI-011 | Aspirational docs in snapshot.rs, transport.rs |
| MI-012 | lib.rs Quick Start example uses .expect() |

---

## 10. Spec Drift and Coverage Gaps

### Spec Drift

| ID | Drift | Severity | Location |
|----|-------|----------|----------|
| SD-001 | TxId logical u32 vs spec u16 | HIGH | `ferratom/src/clock/txid.rs:78` |
| SD-002 | 4 indexes vs spec's 6 | MEDIUM | `ferratomic-core/src/indexes.rs` |
| SD-003 | BigInt/BigDec i128 vs "arbitrary precision" | MEDIUM | `ferratom/src/datom/value.rs:112` |
| SD-004 | Observer full_delta_since epoch/physical mismatch | HIGH | `ferratomic-core/src/observer.rs:134` |
| SD-005 | Flat error enum vs spec's 7-type hierarchy | LOW | `ferratom/src/error.rs` |
| SD-006 | NEG-FERR-001 lints incomplete | MEDIUM | All lib.rs files |
| SD-007 | Observer best-effort vs spec's at-least-once with retry | LOW | `observer.rs` |
| SD-008 | INV-FERR-005 cited but only 4 of 6 indexes | HIGH | `indexes.rs:1-8` |
| SD-009 | INV-FERR-015 cited but epoch used as physical | HIGH | `store/apply.rs:151` |
| SD-010 | lib.rs docstring lists 6 indexes, code has 4 | MEDIUM | `lib.rs:113` |
| SD-011 | INV-FERR-043 uses debug_assert, not runtime error | MEDIUM | `store/apply.rs:92` |

### Coverage Gap Matrix (abbreviated -- invariants with gaps)

| INV | Implemented | Tested | Proven | Model-Checked | Gap |
|-----|------------|--------|--------|---------------|-----|
| 021 | YES | **NO proptest** | NO | YES (Stateright) | Missing proptest |
| 022 | STUB only | NO | NO | NO | Full gap |
| 029 | **NO** | Manual only | NO | NO | Missing LIVE index |
| 032 | **NO** | Manual only | NO | NO | Missing LIVE resolution |
| 033-055 | NO | NO | NO | NO | Future phases |

---

## 11. WAL and Crash Recovery Deep Audit

### [CRITICAL] DEFECT-WAL-001: Recovery path divergence -- generic backend loses epoch

Same as CR-002. `storage.rs` generic backend uses `insert()` instead of `replay_entry()`.

### [CRITICAL] DEFECT-WAL-002: Schema evolution skipped on WAL replay

Same as CR-001. `replay_entry` never calls `evolve_schema`.

### [HIGH] DEFECT-WAL-003: No parent directory fsync after WAL file creation

Same as HI-002. On ext4/XFS, file metadata durability requires parent dir fsync.

### [HIGH] DEFECT-WAL-004: No parent directory fsync after checkpoint creation

Same as HI-003.

### [HIGH] DEFECT-WAL-005: Checkpoint write not atomic

Same as HI-001. Standard fix: write-to-temp-then-rename.

### [HIGH] DEFECT-WAL-006: No WAL payload size limit -- OOM on crafted frames

Same as HI-006. u32 frame length allows 4GiB payloads. `bincode::deserialize` has no default size limit.
**Fix**: `MAX_PAYLOAD_SIZE` constant + `bincode::options().with_limit()`.

### [HIGH] DEFECT-WAL-007: WAL recovery reads entire file into memory

Same as HI-007. 10GB WAL = 10GB allocation.

### [MEDIUM] DEFECT-WAL-008: CRC32 insufficient for safety-critical system

**Location**: `ferratomic-core/src/wal/mod.rs:149-167`
**Traces to**: INV-FERR-008
**Evidence**: 32-bit CRC gives 1 in ~4.3 billion collision probability. Checkpoint uses BLAKE3 (256-bit). The WAL uses a 2^224-times-weaker check.
**Fix**: Replace CRC32 with BLAKE3 truncated to 8 or 32 bytes. BLAKE3 hashes at >1 GiB/s.

### [MEDIUM] DEFECT-WAL-009: WAL epoch monotonicity not enforced on append

**Location**: `ferratomic-core/src/wal/writer.rs:24-28`
**Traces to**: INV-FERR-007
**Evidence**: Neither `append()` nor `append_raw()` verify `epoch > last_synced_epoch`.
**Fix**: Add assertion in `write_frame`.

### [MEDIUM] DEFECT-WAL-010: `last_synced_epoch` not updated after fsync

**Location**: `ferratomic-core/src/wal/writer.rs:53-57`
**Evidence**: Only set during recovery. After N appends+fsyncs, still 0.
**Fix**: Track pending epoch, update on successful fsync.

### [MEDIUM] DEFECT-WAL-011: Recovery truncation not fsynced

**Location**: `ferratomic-core/src/wal/recover.rs:49-53`
**Fix**: Add `self.file.sync_all()` after `set_len`.

### [MEDIUM] DEFECT-WAL-012: Generic backend silently swallows recovery errors

Same as HI-008. `if let Ok(result) = ...` discards errors.

### [MEDIUM] DEFECT-WAL-013: Filesystem cold_start has same silent swallowing

Same pattern but for filesystem path.

### [MEDIUM] DEFECT-WAL-014: Generic backend recovery returns Database without WAL

**Location**: `ferratomic-core/src/storage.rs:504-507`
**Traces to**: INV-FERR-008
**Evidence**: `Database::from_store(store)` constructs with `wal: Mutex::new(None)`. Subsequent transacts have no durability.
**Fix**: Extend `StorageBackend` for WAL attachment or document limitation.

### [LOW] DEFECT-WAL-015: CRC32 of empty input returns 0x00000000

**Fix**: Use stronger hash (DEFECT-WAL-008) or XOR with non-zero constant.

### [LOW] DEFECT-WAL-016: ext4 data=writeback can reorder data writes

**Fix**: Document mount option requirement.

### [LOW] DEFECT-WAL-017: Bincode deserialize can OOM on crafted collection length

**Fix**: Use `bincode::options().with_limit()`.

---

## 12. Concurrency Deep Audit

### [HIGH] DEFECT-CC-001: Observer registration TOCTOU -- missed transactions

**Location**: `ferratomic-core/src/db/mod.rs:192-202`
**Traces to**: INV-FERR-011
**Evidence**: Between `self.current.load()` (Step A) and observer mutex acquisition (Step B), a concurrent `transact()` can complete. New observer misses that epoch permanently.
**Fix**: Reload `self.current.load()` inside observer mutex scope.

### [MEDIUM] DEFECT-CC-002: Write lock poison masked as Backpressure

Same as ME-001. `try_lock().map_err(|_| Backpressure)` conflates `Poisoned` and `WouldBlock`.

### [MEDIUM] DEFECT-CC-003: WAL mutex poison also masked as Backpressure

Same as ME-002.

### [MEDIUM] DEFECT-CC-004: `transaction_count` uses Relaxed ordering

**Location**: `ferratomic-core/src/db/transact.rs:110`
**Fix**: Change to `AcqRel`.

### [MEDIUM] DEFECT-CC-005: Observer delivery outside write lock creates reordering window

**Location**: `ferratomic-core/src/db/transact.rs:70-76`
**Traces to**: INV-FERR-011
**Evidence**: After `drop(guard)`, transaction N+1 can deliver to observers before transaction N. Epoch N's `on_commit` is suppressed by the `epoch <= last_seen_epoch` check.
**Fix**: Document best-effort ordering, or assign sequence numbers under write lock.

### [MEDIUM] DEFECT-CC-006: WriteLimiter fetch_add/fetch_sub can spuriously reject under contention

**Location**: `ferratomic-core/src/backpressure.rs:96-99`
**Evidence**: Between `fetch_add` and `fetch_sub` in rejection path, counter is inflated. Other threads see inflated count and are spuriously rejected.
**Fix**: CAS loop instead of fetch_add/fetch_sub.

### [LOW] DEFECT-CC-007: WriteGuard Drop uses Release, not AcqRel

**Location**: `ferratomic-core/src/backpressure.rs:63`
**Fix**: Change to AcqRel for consistency.

### [LOW] DEFECT-CC-008: HybridClock busy-wait can live-lock in VMs

**Location**: `ferratom/src/clock/mod.rs:85-94`
**Evidence**: NTP step backward + `SystemTime::now()` not monotonic = infinite loop.
**Fix**: Bounded retry with forced physical increment.

### [LOW] DEFECT-CC-009: `mem::forget` on WriteGuard causes permanent counter leak

Same as DEFECT-P3-005. Theoretical; document RAII contract.

---

## 13. Design Decisions

Four design questions were raised during the audit and decided through first-principles analysis.

### Decision 1: TxId.logical field width -- u32 (keep current)

**Question**: Spec says u16, code uses u32. Which is correct?

**Analysis**: The logical counter serves *disambiguation* (events in the same physical millisecond), not *rate limiting*. The WriteLimiter (INV-FERR-021) is the intentional backpressure mechanism -- it operates at the Database level with configurable policy. The u16 overflow backpressure in the spec is an incidental consequence of a narrow counter, not a design requirement. In a single-writer architecture (Phase 4a Mutex), you can't produce 65K transactions in 1ms. In group-commit (Phase 4b), the HLC increments once per batch.

**Decision**: u32 is correct. The spec must be updated:
- Update INV-FERR-015 Level 2 to specify `logical: u32`
- Remove u16-overflow backpressure language from spec
- Add note that backpressure is handled by WriteLimiter (INV-FERR-021), not HLC overflow
- Busy-wait loop in `HybridClock::tick()` stays as safety valve for u32::MAX

### Decision 2: HLC wiring into transact -- Wire now, Phase 4a

**Question**: Is epoch-as-physical intentional or oversight?

**Analysis**: It was an oversight. The HLC exists to provide a total order with two properties: (1) causal consistency across distributed agents, (2) wall-clock proximity for time-range queries. Currently `TxId::with_agent(self.epoch, 0, agent)` defeats both: federation gets incomparable TxIds (independent epoch counters), time-range queries are impossible (physical is not a timestamp), and the observer fallback is broken (comparing physical with epoch by coincidence).

**Decision**: Wire HLC into `Store::transact` before Phase 4a gate closure. Implementation: Database owns HybridClock, ticks under write lock, passes resulting TxId to Store::transact. ~50 LOC change. Test impact: tests checking exact TxId values need updates; algebraic property tests should be invariant.

### Decision 3: LIVE Index -- Phase 4a (Stage 0)

**Question**: When should LIVE be implemented?

**Analysis**: No analysis needed. The project's methodology: `Stage 0 = required for Phase 4a`. `LIVE = Stage 0`. Therefore LIVE is required for Phase 4a. Without LIVE, the store is an append-only event log, not a database.

**Decision**: Implement `Store::live_resolve(entity, attribute) -> Option<Value>` and optionally a LIVE index as part of Phase 4a gate closure. INV-FERR-029 (LIVE derived view) + INV-FERR-032 (resolution rules: card-one = LWW, card-many = all non-retracted).

### Decision 4: EntityId deserialization security -- Architecture C (Two-Tier Types)

**Question**: How should EntityId deserialization handle the trust boundary?

**Analysis**: This was the deepest decision. See full analysis in Section 14 below.

**Decision**: Architecture C -- two-tier type system with wire types and core types. Full details in Section 14.

---

## 14. Architecture C: Two-Tier Type System for Deserialization Security

### The Threat Model

Ferratomic will be distributed across the network (Phase 4c): multiple collaborating users and agents sharing subsets of data via selective gossip, permissions, and blockchain-inspired provenance hashes (INV-FERR-051 through INV-FERR-055).

**Phase 4a (single node, trusted storage):**
```
EntityId sources:
  1. from_content()       -- verified by construction
  2. WAL deserialization   -- trusted via CRC integrity
  3. Checkpoint deser      -- trusted via BLAKE3 integrity
Adversary: none (local storage only)
```

**Phase 4c (distributed, adversarial peers):**
```
EntityId sources:
  1. from_content()        -- verified by construction
  2. WAL deserialization    -- trusted via CRC
  3. Checkpoint deser       -- trusted via BLAKE3
  4. Anti-entropy sync      -- UNTRUSTED (remote peer)
  5. Selective merge        -- UNTRUSTED (filtered subset from remote)
  6. Gossip protocol        -- UNTRUSTED (arbitrary network data)
  7. Light client queries   -- UNTRUSTED (response from remote)
Adversary: Byzantine peers, MITM, Sybil attacks
```

A Byzantine peer could forge an EntityId (send `[0xDEAD...; 32]` that is NOT BLAKE3 of anything), creating phantom entities that poison the Merkle tree and anti-entropy protocol.

### The Curry-Howard Argument

Types are propositions, constructors are proofs. If `EntityId` is the proposition "these 32 bytes are a BLAKE3 hash," then every constructor must be a proof:

| Constructor | Proof Type | Strength |
|-------------|-----------|----------|
| `from_content(bytes)` | Direct computation | Mechanically verified |
| Derived `Deserialize` | **None** | **Unproven** -- sorry axiom |

The derived `Deserialize` is a proof with no evidence -- a `sorry` in Lean terms. The Lean proofs have zero `sorry`. The Rust types should have zero unproven constructors by the same principle.

### Architecture C Design

```
Network receive --> WireDatom (Deserialize, unverified) --> verify(signature, merkle) --> Datom (trusted)
WAL recovery    --> bytes --> CRC verify --> StorageDatom --> trusted conversion --> Datom
Checkpoint      --> bytes --> BLAKE3 verify --> StorageDatom --> trusted conversion --> Datom
```

#### Tier 1: Core Types (ferratom crate)

These types have NO derived `Deserialize`. They can only be constructed through verified paths.

```rust
// EntityId -- NO Deserialize
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct EntityId([u8; 32]);

impl EntityId {
    /// Production constructor: BLAKE3 hash of content.
    pub fn from_content(bytes: &[u8]) -> Self { ... }

    /// Reconstruct from integrity-verified storage (WAL CRC, checkpoint BLAKE3).
    /// Caller MUST have verified source integrity before calling this.
    /// For network-received data (Phase 4c), use from_signed_transaction instead.
    pub(crate) fn from_trusted_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
}

// NonNanFloat -- NO derived Deserialize, custom impl rejects NaN
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize)]
pub struct NonNanFloat(OrderedFloat<f64>);

impl<'de> Deserialize<'de> for NonNanFloat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let f = OrderedFloat::<f64>::deserialize(d)?;
        if f.into_inner().is_nan() {
            Err(D::Error::invalid_value(
                Unexpected::Float(f64::NAN),
                &"a finite float (NaN rejected per INV-FERR-012)",
            ))
        } else {
            Ok(NonNanFloat(f))
        }
    }
}
```

#### Tier 2: Wire Types (new `ferratom::wire` module)

These types have `Deserialize`. They are the ONLY types that touch untrusted bytes.

```rust
// ferratom/src/wire.rs

/// Wire-format EntityId. NOT verified. Must be converted to EntityId
/// through a trust boundary before entering the Store.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WireEntityId(pub [u8; 32]);

/// Wire-format Value. May contain WireEntityId via Ref variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WireValue {
    Keyword(Arc<str>),
    String(Arc<str>),
    Long(i64),
    Double(NonNanFloat),  // Uses NonNanFloat's validating Deserialize
    Bool(bool),
    Instant(i64),
    Uuid([u8; 16]),
    Bytes(Arc<[u8]>),
    Ref(WireEntityId),
    BigInt(i128),
    BigDec(i128),
}

/// Wire-format Datom. All fields are unverified.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WireDatom {
    pub entity: WireEntityId,
    pub attribute: Attribute,  // Attribute is safe (just Arc<str>)
    pub value: WireValue,
    pub tx: TxId,             // TxId is safe (just integers)
    pub op: Op,               // Op is safe (just an enum)
}
```

#### Trust Boundary Conversion

```rust
// ferratom/src/wire.rs (continued)

impl WireEntityId {
    /// Convert to EntityId for data from integrity-verified local storage.
    /// CRC (WAL) or BLAKE3 (checkpoint) verification MUST have been performed
    /// on the source bytes before this call.
    pub fn into_trusted(self) -> EntityId {
        EntityId::from_trusted_bytes(self.0)
    }

    /// Convert to EntityId after cryptographic verification (Phase 4c).
    /// Signature and/or Merkle proof verification MUST have been performed.
    pub fn into_verified(self, _proof: &VerificationProof) -> Result<EntityId, FerraError> {
        // Phase 4c: verify signature chain, Merkle inclusion, trust gradient
        Ok(EntityId::from_trusted_bytes(self.0))
    }
}

impl WireValue {
    pub fn into_trusted(self) -> Value {
        match self {
            WireValue::Ref(wire_id) => Value::Ref(wire_id.into_trusted()),
            WireValue::Keyword(s) => Value::Keyword(s),
            WireValue::String(s) => Value::String(s),
            // ... all other variants map directly
        }
    }
}

impl WireDatom {
    pub fn into_trusted(self) -> Datom {
        Datom::new(
            self.entity.into_trusted(),
            self.attribute,
            self.value.into_trusted(),
            self.tx,
            self.op,
        )
    }
}
```

#### Integration with Storage Code

WAL recovery and checkpoint loading use wire types internally:

```rust
// In ferratomic-core/src/db/recover.rs
let wire_datoms: Vec<WireDatom> = bincode::deserialize(&entry.payload)?;
// CRC was already verified by Wal::recover() before this point
let datoms: Vec<Datom> = wire_datoms.into_iter().map(|wd| wd.into_trusted()).collect();
store.replay_entry(entry.epoch, &datoms)?;

// In ferratomic-core/src/checkpoint.rs
let payload: WireCheckpointPayload = serde_json::from_slice(payload_bytes)?;
// BLAKE3 was already verified by load_checkpoint() before this point
let datoms: Vec<Datom> = payload.datoms.into_iter().map(|wd| wd.into_trusted()).collect();
```

### Invasiveness Assessment

**New types: 4** (`WireEntityId`, `WireValue`, `WireDatom`, `WireCheckpointPayload`)

**Modified types: 2** (remove `Deserialize` from `EntityId`, custom impl for `NonNanFloat`)

**Types unchanged: 10** (`Datom`, `Value`, `Op`, `Attribute`, `TxId`, `AgentId`, `ValueType`, `Cardinality`, `ResolutionMode`, `AttributeDef`)

Note: `Datom` and `Value` LOSE their derived `Deserialize` because they contain `EntityId`. They don't need manual impls -- all deserialization goes through wire types.

`TxId`, `AgentId`, `Op`, `Attribute` KEEP their `Deserialize` because they don't contain `EntityId` and have no invariants that deserialization could violate.

**Deserialization callsites changed: ~10** (6 WAL, 1 checkpoint, 3 test) -- change from `Vec<Datom>` to `Vec<WireDatom>` + `.into_trusted()`.

**Performance impact: Zero.** Wire types are structurally identical to core types. `into_trusted()` is a field-by-field move with no allocation. The optimizer inlines and erases the conversion.

**Estimated LOC: ~200** (wire module ~100, conversion impls ~50, callsite changes ~50).

### Phase 4c Forward Compatibility

When federation arrives:

```rust
impl WireEntityId {
    /// Phase 4c: Convert after Ed25519 signature verification.
    pub fn into_verified(
        self,
        signature: &Ed25519Signature,
        signing_key: &PublicKey,
    ) -> Result<EntityId, VerificationError> { ... }

    /// Phase 4c: Convert after Merkle proof verification.
    pub fn into_merkle_verified(
        self,
        proof: &MerkleProof,
        trusted_root: &[u8; 32],
    ) -> Result<EntityId, VerificationError> { ... }
}
```

The federation transport MUST use `into_verified` or `into_merkle_verified`, not `into_trusted`. The type system enforces this: `into_trusted` is `pub(crate)` to ferratomic-core only. The federation crate cannot call it.

Every EntityId in the system has known provenance:
- `from_content` -- I computed the hash myself
- `into_trusted` -- I read it from my own integrity-verified storage
- `into_verified` -- A trusted agent signed it (Phase 4c)
- `into_merkle_verified` -- It's included in a verified Merkle tree (Phase 4c)

This is the trust gradient of INV-FERR-054, encoded in the type system.

---

## 15. Empirically Grounded Implementation Plan

### Phase 4a Gate Closure (Immediate)

Priority order based on defect severity and dependency:

**Batch 1: Critical recovery + deserialization fixes**

| Task | Defects Addressed | Estimated LOC |
|------|-------------------|---------------|
| Fix `replay_entry` to call `evolve_schema` | CR-001 | ~5 |
| Fix generic backend to use `replay_entry` | CR-002 | ~20 |
| Implement Architecture C wire types | CR-003, CR-004 | ~200 |
| Switch checkpoint to bincode | CR-005 | ~30 |
| Fix WAL proptest deserializer | CR-006 | ~5 |

**Batch 2: Durability fixes**

| Task | Defects Addressed | Estimated LOC |
|------|-------------------|---------------|
| Atomic checkpoint write (write-to-temp-then-rename) | HI-001 | ~30 |
| Parent directory fsync for WAL and checkpoint | HI-002, HI-003 | ~20 |
| Observer error should not fail transact | HI-004 | ~5 |
| Gate `commit_unchecked` behind test cfg | HI-005 | ~5 |
| Fix merge docstring and genesis_agent commutativity | HI-009, HI-014 | ~10 |
| Distinguish lock poison from backpressure | ME-001, ME-002 | ~15 |

**Batch 3: HLC + LIVE (architectural)**

| Task | Defects Addressed | Estimated LOC |
|------|-------------------|---------------|
| Wire HLC into Database::transact | HI-011, HI-012 | ~50 |
| Update spec INV-FERR-015 for u32 logical | HI-010 | spec edit |
| Implement LIVE view resolution | HI-013, HI-018 | ~150 |
| Add Entity and LIVE indexes | HI-018 | included above |

**Batch 4: Test gaps + quality**

| Task | Defects Addressed | Estimated LOC |
|------|-------------------|---------------|
| Fix tautological atomicity proptest | ME-020 | ~10 |
| Add schema conflict proptest | ME-017 | ~30 |
| Add WAL error-path tests | ME-021 | ~50 |
| Add P99.99 latency measurement | ME-018 | ~20 |
| Fix clippy (134 unwrap_used in tests) | build gate | ~50 |
| Fix fmt (1 file) | build gate | ~5 |

### Phase 4b Prerequisites (Before prolly tree work)

| Task | Defects Addressed |
|------|-------------------|
| WAL payload size limit + streaming recovery | HI-006, HI-007 |
| Recovery should propagate I/O errors | HI-008 |
| Replace CRC32 with BLAKE3 for WAL integrity | ME-004, WAL-008 |
| WAL epoch monotonicity enforcement | ME-011, ME-012 |
| Schema.define() conflict detection | HI-015 |
| Observer TOCTOU fix | HI-016 |

### Phase 4b Proper (Per existing beads)

- bd-85j.13: Prolly tree block store
- bd-85j.14: Entity-hash sharding
- bd-85j.12: Scaling benchmarks (1K-100M datoms)
- Performance fixes ME-005, ME-006, ME-007 addressed alongside benchmarks

### Phase 4c (Federation)

- bd-85j.15: Transport trait + implementations
- bd-85j.16: CRDT mesh federation
- Anti-entropy implementation (currently stub)
- Architecture C `into_verified` / `into_merkle_verified` constructors
- INV-FERR-051-055 implementation

---

*End of cleanroom audit. 60 defects identified across 8 phases. All 8 phases produced findings (no clean phases). Architecture C selected for deserialization security. Implementation plan ordered by severity and dependency.*
