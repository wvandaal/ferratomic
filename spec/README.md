# Ferratomic Specification

> **These files are the CANONICAL specification.**

Formal specification using DDIS methodology (INV/ADR/NEG).
89 invariants (incl. 025b, 045a, 045c, 046a, 086), 32 ADRs, 7 negative cases, 2 coupling invariants.

**Implementation note**: Spec Level 2 contracts use `BTreeSet`/`BTreeMap` as conceptual
illustrations. The actual implementation uses `im::OrdSet`/`im::OrdMap` per ADR-FERR-001.
The `IndexBackend` trait (INV-FERR-025) allows swapping backends.

Every invariant follows the Level 0/1/2 format:
- **Level 0**: Algebraic law (formal predicate)
- **Level 1**: State invariant (natural language)
- **Level 2**: Implementation contract (Rust + Kani/proptest)
- **Falsification**: What violates this invariant
- **proptest strategy**: How to test with property-based testing
- **Lean theorem**: Machine-checked proof (where applicable)

## Modules

| File | Section | INV-FERR | Focus |
|------|---------|----------|-------|
| [00-preamble.md](00-preamble.md) | §23.0 | — | Overview, crate structure, Lean foundation, Stateright model |
| [01-core-invariants.md](01-core-invariants.md) | §23.1 | 001-012 | CRDT semilattice, indexes, snapshots, WAL, schema, identity |
| [02-concurrency.md](02-concurrency.md) | §23.2 | 013-024 | Checkpoint, recovery, HLC, sharding, append-only, atomicity, backpressure, substrate |
| [03-performance.md](03-performance.md) | §23.3 | 025-032 | Index backend, write amplification, tail latency, cold start, LIVE resolution, genesis |
| [04-decisions-and-constraints.md](04-decisions-and-constraints.md) | §23.4-23.7 | 033-036 | ADR-FERR-001..007,010, NEG-FERR-001..005, cross-shard query, partition tolerance |
| [05-federation.md](05-federation.md) | §23.8, §23.10 | 037-044, 051-055, 060-063, 025b, 086 | Federated query, selective merge, transport, VKN, **Phase 4a.5**: store identity, causal predecessors, merge receipts, provenance lattice, universal index algebra, canonical datom format (086), ADR-FERR-034 (Database-layer signing), ADR-FERR-035 (TxId-based entity), ADR-FERR-036 (store fingerprint in signing message) |
| [06-prolly-tree.md](06-prolly-tree.md) | §23.9 | 045-050, 045a, 045c | Canonical datom key encoding (S23.9.0), chunk addressing, leaf chunk codec conformance trait (045c — load-bearing accretive lever for trait-based DI), deterministic chunk serialization (045a — DatomPair reference codec), history independence, O(d) diff, block store, substrate independence, RootSet manifest model |
| [07-refinement.md](07-refinement.md) | §23.11 | CI-FERR-001..002 | Lean-Rust coupling invariant, refinement tower |
| [08-verification-infrastructure.md](08-verification-infrastructure.md) | §23.12 | 056-059 | Fault injection, soak testing, metamorphic testing, optimization preservation, self-monitoring convergence (B17 → M(S) ≅ S) |
| [09-performance-architecture.md](09-performance-architecture.md) | §23.13 | 070-085 | Zero-copy cold start, sorted-array backend, positional content addressing, permutation index fusion, homomorphic fingerprint, LIVE-first checkpoint, interpolation search, SoA columnar (078), chunk fingerprints (079), incremental LIVE (080), TxId temporal permutation (081), entity RLE (082), graph adjacency (083), WAL dedup Bloom (084), attribute interning (085), NEG-FERR-007 (FM-Index NO-GO), ADR-FERR-030..033, **§Wavelet** (rank/select contract, symbol encoding, construction/query algorithms, performance budgets — Phase 4b primary backend per bd-4vwk) |

## Reading Order for Implementing Agents

- **Phase 1 (Lean proofs)**: 00-preamble + 01-core-invariants
- **Phase 2 (tests)**: All modules (write tests for every INV)
- **Phase 3 (types)**: 01-core-invariants (type contracts)
- **Phase 4a (MVP)**: 01 + 02 + 03
- **Phase 4a.5 (federation foundations)**: 05-federation §23.8.5 (INV-FERR-060..063, 025b, ADR-FERR-021..029)
- **Phase 4b (prolly tree + hardening)**: 06-prolly-tree + 08-verification-infrastructure (INV-FERR-056, ADR-FERR-011..014, NEG-FERR-006) + 09-performance-architecture §Wavelet (ADR-FERR-030, rank/select, wavelet algorithms)
- **Phase 4c (federation)**: 05-federation + 06-prolly-tree + 08-verification-infrastructure (INV-FERR-057, INV-FERR-059)
- **Phase 4d (datalog)**: 04-decisions-and-constraints (§23.6) + 08-verification-infrastructure (INV-FERR-058)

## Traces

Every INV-FERR traces to SEED.md (the foundational design document).
See 00-preamble.md §23.0.3 for the complete INV-STORE → INV-FERR mapping.
