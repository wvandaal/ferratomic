# Implementation Risk Vectors: An Honest Engineering Reality Check

## Preamble

This document is the eleventh in a sequence:

1. **"A Formal Algebraic Theory of Agentic Systems"** — universal decomposition, EAV fact store as epistemic substrate.
2. **"Ferratomic as the Substrate for Distributed Cognition"** — Actor model isomorphism, store-messaging unification.
3. **"Everything Is Datoms"** — query-as-datom, taint tracking, the bilateral Y-combinator.
4. **"The Projection Calculus"** — self-referential projections, dream cycles, agents as projections.
5. **"From Projections to Practice"** — differential dataflow, the McCarthy completion.
6. **"The Agentic Operating System"** — event-driven architecture, situations replacing conversations.
7. **"The Value Topology"** — power laws, four-dimensional value, the gradient field.
8. **"Epistemic Entropy"** — the two entropies, knowledge metabolism.
9. **"Reflective Rules"** — rules-as-datoms with CRDT convergence.
10. **"Grown, Not Engineered"** — the year-by-year trajectory of intelligence growing on the substrate.
11. **This document** — the engineering reality check. Documents 9-10 articulated a vision that depends on a chain of risky technical bets actually working. This document inventories those bets, calibrates probabilities, separates build risk from outcome risk, and proposes fail-fast experiments to derisk the speculative parts before committing to them.

Documents 1-8 established the substrate. Documents 9-10 established the speculative arc. This document grounds the arc in engineering reality — what could go wrong, how likely each failure is, and what cheap experiments we can run NOW to validate the riskiest assumptions before they become load-bearing.

---

## Part I: Why This Document Exists Now

### 1.1 The Speculative Arc Has Outrun the Verified Substrate

We have a working algebraic foundation: `(P(D), ∪)` as a G-Set CRDT semilattice, content-addressed identity, append-only history, Lean-proven invariants. That's grounded engineering. Phase 4a is closing on schedule.

We also have a vision that extends far beyond the substrate: reflective rules that converge under CRDT semantics, projection calculus with cognitive nodes, year-5 self-bootstrapping epistemology, an entire trajectory of intelligence "grown" on top of the substrate. That's mostly speculation backed by intuition.

Between the two there is a credibility gap. If we keep accumulating speculative documents without validating that the speculation can actually be built, the project starts to look like vaporware to anyone reading the docs cold. Worse, we start believing our own marketing — making design decisions today that depend on capabilities we have not yet proven possible.

This document closes the gap by being honest about what we know, what we hope, and what we have not yet tested.

### 1.2 The Cost of Optimism Without Calibration

Every risk we don't name is a risk that ambushes us at the worst possible moment. The Phase 4a gate is closing in days. Phase 4a.5 (federation foundations) is fully scoped with 24 lab-grade beads. Phase 4b (performance at scale), 4c (federation in production), and 4d (Datalog evaluator with proof production) are sketched but not rigorously assessed for feasibility.

If we discover at Phase 4d that proof-producing Datalog is harder than anticipated, we lose six months. If we discover that reflective rules don't actually converge productively, the entire arc above doc 011 collapses. If we discover that projection calculus with cognitive nodes can't hit reasonable latency, the user-facing value of the system is compromised.

These are not hypothetical. They are the actual risks. Naming them is the first step to managing them.

### 1.3 The Cost of Pessimism Without Action

The opposite failure mode is naming risks and then doing nothing about them. Risk catalogs that sit in documents and never produce experiments are worse than useless — they create a false sense of having addressed the issue.

This document is structured to prevent that failure mode. Every risk in Part IV-VIII is paired with a mitigation status (designed-for vs needs design). Every category is followed by Part IX, which translates risks into concrete fail-fast experiments — small, cheap, time-boxed validations we can run NOW to derisk the speculative phases before committing to them.

The output of this document is not just a catalog. It is a set of beads that get filed and executed.

---

## Part II: Calibrated Probabilities

### 2.1 The Phase-By-Phase Confidence Vector

These are honest estimates. They represent my best calibration, not aspirations.

| Phase | Description | Build Confidence | Outcome Confidence |
|-------|-------------|------------------|-------------------|
| 4a | ferratomic-store complete + perf substrate | 95% | 95% |
| 4a.5 | Federation foundations (signing, frontier, filters) | 85% | 85% |
| 4b | Performance at scale (validated against 100M datoms) | 70% | 65% |
| 4c | Federation in production (real network, real adversaries) | 50% | 45% |
| 4d | Datalog evaluator with proof production | 40% | 40% |
| RR-tech | Reflective rules technical machinery | 60% | — |
| RR-out | Reflective rules converging productively | — | 25% |
| Y5 | Year-5 self-bootstrapping epistemology emerges | — | 10% |

**Build confidence** = "given infinite patience, can we make this code pass our quality gates?"
**Outcome confidence** = "given that we built it, will it actually do what we hoped?"

These are independent. Phase 4d might compile, pass tests, satisfy specs, AND still produce derivations slowly enough to be useless. Reflective rules might work mechanically AND still descend into chaos as the rule library grows.

### 2.2 Build Risk vs Outcome Risk

The most important distinction in this table is the separation of build risk from outcome risk. This is a habit we should adopt across all our planning.

**Build risk** is what engineers traditionally measure: can we make the code work? It's bounded by our technical capability, the maturity of available libraries, and our willingness to grind. Build risk decreases with effort and experience. Build risk on `(P(D), ∪)` is near-zero — we know how to build a G-Set CRDT.

**Outcome risk** is whether the working code actually delivers the intended value. It's bounded by empirical reality, user behavior, and emergent properties we can only observe at scale. Outcome risk does NOT decrease with engineering effort alone — it requires actual deployment, actual users, actual data.

This matters because most of the speculative parts of Ferratomic have tractable build risk but unknown outcome risk. We probably CAN build reflective rules. We don't know if reflective rules actually converge productively at year-5 scale.

Calibrating these separately lets us make smarter trade-offs. If build risk is low and outcome risk is high, build the minimum viable version and run it as an experiment. Don't build the gold-plated version of something we don't know will work.

### 2.3 The Compounding Problem

Notice that the multiplied probability is small. Phase 4a × 4a.5 × 4b × 4c × 4d × RR-tech × RR-out × Y5 = roughly 0.003. That means there is a ~0.3% chance the entire speculative arc plays out exactly as envisioned.

This sounds bad. It is not necessarily bad. Compounding probabilities undersell the value of partial success:

- If we ship through Phase 4c (federation in production) and stop there, we still have a unique product: a formally verified distributed embedded datom database with cryptographic provenance. That alone is worth shipping.
- If we ship through Phase 4d (Datalog evaluator) and stop there, we have something better: a formally verified distributed embedded datom database with proof-producing Datalog. There is no production system today that has all these properties.
- If reflective rules technical machinery works (RR-tech = 60%) but the year-5 vision (Y5 = 10%) does not, we still have built something useful: a substrate for self-modifying logic that converges under CRDT semantics. The fact that the rule library doesn't bootstrap fully autonomously doesn't invalidate it as a tool.

The right framing is not "what is the probability the entire vision plays out?" It is "what is the probability of partial success at each level, and what is the value of that partial success?"

### 2.4 The Asymmetry

The expected value calculation is asymmetric in our favor.

Downside: we ship a formally verified embedded datom database with no users, and the speculative arc above it is a documentation curiosity. This is a real outcome. The cost is the time we spent building it versus alternative things we could have built. That's a finite, recoverable cost.

Upside: we ship the substrate for distributed cognition and the speculative arc actually plays out. The cost is the same. The value is approximately infinite, because no other system in the world has all these properties combined.

The asymmetric payoff justifies the project even at low probabilities. But asymmetric payoffs only matter if you actually run the experiments — if you stay in the speculative-document phase forever, the asymmetric upside collapses to zero because you never tested it.

This document is the bridge from speculation to test.

---

## Part III: The Risk Taxonomy

Risks are not all the same kind of thing. Conflating them leads to bad mitigation strategies. Five distinct categories, each requiring a different response:

| Category | What it is | Response strategy |
|----------|------------|-------------------|
| Performance | Things that work but are too slow | Profile, benchmark, optimize iteratively |
| Correctness | Things that produce wrong answers | Tests, proofs, fault injection, formal verification |
| Scaling | Things that work at small N but fail at large N | Validate at target scale, not extrapolated from small |
| Novelty | Things that have never been built before | Prototype, fail fast, accept research-level uncertainty |
| Social | Things that depend on humans behaving in particular ways | Test with real users, accept empirical reality |

Performance risks are tractable. We have the tools. Correctness risks are tractable. We have the methodology. Scaling risks are tricky but bounded. Novelty risks are the dangerous ones — we can't profile them away because we don't know if they exist. Social risks are the most dangerous because we have the least control.

The next five sections enumerate risks in each category, with a status marker for each:
- **DESIGNED-FOR**: We have an explicit plan in the spec or docs.
- **PARTIALLY DESIGNED**: We have a sketch but not a rigorous design.
- **NEEDS DESIGN**: We have not addressed this yet.
- **EMPIRICAL**: Cannot be derisked through design — must be tested.

---

## Part IV: Performance Risks

### 4.1 Datalog Evaluator: Reflective Rule Loading Cost

Every evaluation cycle starts by querying the store for `(_, :rule/active, true)` and parsing each rule's head clause text.

- At small scale (50 rules): trivially fast (~1ms).
- At year-5 scale (thousands of derived rules across multiple trust tiers): rule loading itself becomes a significant fraction of evaluation time.

**Mitigation**: Cache the parsed rule set. Invalidate only when the LIVE state of `:rule/active` datoms changes. The fingerprint over the rule subset gives O(1) cache validation.

**Status**: PARTIALLY DESIGNED. The fingerprint mechanism exists for the global store but has not been specialized for rule subsets. INV needed: "rule subset fingerprint is stable under reordering, equal under equality."

### 4.2 Datalog Evaluator: Recursive Rule Evaluation Explosion

Datalog with recursion is decidable (unlike Prolog) but can have very large fixpoints. A naive evaluator runs to fixpoint on every query. At 100M datoms with thousands of rules, naive evaluation is impossible.

**Mitigation**: Incremental view maintenance via differential dataflow (DDlog/DBSP-style, doc 007). Only re-derive what changed since the last evaluation. Incremental evaluation is O(delta) instead of O(store), where delta is typically tiny per transaction.

**Status**: PARTIALLY DESIGNED. Doc 007 sketches the approach. No spec for incremental evaluation primitives. No INV for "incremental evaluation is observationally equivalent to fixpoint."

### 4.3 Datalog Evaluator: Stratification Verification Cost

CALM-classified queries (R08, R09) need to be checked for monotonicity. Non-monotone queries require coordination barriers. This check is per-query and not free.

**Mitigation**: Cache the classification result with the query plan. Invalidate only when rules change.

**Status**: NEEDS DESIGN. The classification algorithm exists in literature; the caching policy has not been specced.

### 4.4 Datalog Evaluator: Rule Combinatorial Explosion

With reflective rules, the system can derive new rules by combining existing rules. The space of possible rule combinations is exponential. The dream cycle's Phase 5 (rule evolution) must avoid blindly enumerating combinations.

**Mitigation**: The value topology (doc 009) provides the gradient — only explore combinations in regions of high value-density. Beam search guided by value scores, not exhaustive enumeration.

**Status**: NEEDS DESIGN. Doc 009 establishes the value gradient field; doc 011 establishes reflective rules; the combination — value-gradient-guided rule evolution — has not been specced.

### 4.5 Datalog Evaluator: Truth Maintenance Cascade Size

Retracting a heavily-used premise cascades through every derivation that depends on it. At scale, a single retraction could taint millions of conclusions.

**Mitigation**: Incremental cascade processing (don't compute the full transitive closure synchronously). Value-weighted prioritization (taint high-value conclusions first, defer low-value ones to the dream cycle).

**Status**: PARTIALLY DESIGNED. D20 (truth maintenance) specifies the cascade as an INV but does not bound its synchronous cost. Need backpressure design.

### 4.6 Projection Calculus: Recursive Projection Expansion

A projection datom contains queries that may return more projection datoms. The evaluator must expand them recursively. Termination depends on the projection graph being acyclic at evaluation time.

**Mitigation**: Depth limit on projection expansion. Cycle detection in the projection dependency graph. Static analysis of projection definitions to flag potential cycles.

**Status**: NEEDS DESIGN. Doc 006 §4.3 says "Datalog's stratified semantics guarantees termination" but this only holds for the QUERY layer, not the projection composition layer.

### 4.7 Projection Calculus: Mode Dispatch Overhead

Projections have two evaluation modes (mechanical via Datalog, cognitive via LLM). Each mode switch is expensive — Datalog evaluation is microseconds, LLM calls are seconds.

**Mitigation**: Parallelize independent cognitive nodes. Cache cognitive judgments by input hash. Batch LLM calls when possible. Most importantly: commit to "queries never wait for LLM" — cognitive evaluation happens entirely in the dream cycle, queries read cached results.

**Status**: NEEDS DESIGN. Doc 006 sketches both sync and async modes; we need to commit to async-only for production query paths.

### 4.8 Projection Calculus: Effectiveness Scoring Overhead

Each projection has an effectiveness score that updates with use. Updating the score is one transact per projection invocation. At high projection rates, the meta-transact overhead exceeds the projection itself.

**Mitigation**: Batch effectiveness updates in the dream cycle rather than per-invocation. The effectiveness score is converged, not real-time — there is no consumer that needs the current value with sub-cycle latency.

**Status**: PARTIALLY DESIGNED. The dream cycle is specced; the deferred-update batching is not.

### 4.9 Substrate: Ed25519 Verification at Scale

Every transaction is signed. At 10K tx/sec, the consumer side does 10K verifications/sec. Ed25519 verification is ~50µs on modern CPUs — that's 500ms of CPU per second per consumer for the single-sig path.

**Mitigation**: Ed25519 supports batch verification at ~3x throughput. Batch verification at the network layer where multiple transactions arrive together. For local verification (e.g., recovering the WAL), single-sig is fine.

**Status**: NEEDS DESIGN. Spec 05 mandates signing but does not specify the verification path's batching strategy.

### 4.10 Substrate: Index Lookup at 100M Datoms

We have a sophisticated index architecture (sorted vec, prolly tree, wavelet matrix, interpolation search, Eytzinger layout). The theoretical cost model says we should hit O(log N) for point queries and O(log N + k) for range queries. We have not validated this at 100M datoms.

**Mitigation**: Validate. This is not a design problem — it's an empirical question. See fail-fast experiment §9.1.

**Status**: EMPIRICAL.

### 4.11 Substrate: WAL Fsync Throughput

Every committed transaction requires an fsync to the WAL. fsync latency on commodity SSDs is ~1ms. That bounds transaction throughput at ~1000 tx/sec single-threaded, regardless of CPU cost.

**Mitigation**: Group commit (batch multiple transactions into a single fsync). Async commit (return to caller before fsync, with explicit durability point). Configurable durability (some workloads prefer throughput over per-transaction durability).

**Status**: PARTIALLY DESIGNED. The WAL spec mentions group commit but does not specify the batching strategy.

---

## Part V: Correctness Risks

### 5.1 HLC Clock Skew at Scale

HLC depends on physical clocks being roughly synchronized across federated nodes. If a node's clock is far ahead of the rest of the cluster, its TxIds will have artificially high physical timestamps, which violates the implicit assumption that "later TxIds correspond to later wall-clock events."

**Worst case**: A malicious or buggy node sets its clock to year 2099. Every transaction it produces has a TxId far in the future. When other nodes merge with this node, their HLCs jump forward, polluting their own future TxIds.

**Mitigation**: Bounded skew tolerance — reject transactions whose physical timestamp is more than N seconds in the future. NTP synchronization as a hard requirement. Detection: alert on HLC jumps that exceed the skew tolerance.

**Status**: PARTIALLY DESIGNED. INV-FERR-001 (Monotonic Tick) is in the spec; the bounded-skew defense is mentioned but not specced.

### 5.2 Truth Maintenance Under Concurrency

Truth maintenance (D20) operates on the dependency graph of derived conclusions. Under concurrent transactions, two writers could simultaneously retract premises that affect overlapping derivation sets. The cascade processing must handle interleaved retractions without losing derivations.

**Mitigation**: Truth maintenance operates in the dream cycle, not in the transact path. Concurrent transactions write to the WAL; the dream cycle processes cascades sequentially. The CRDT semantics ensures that the final cascade output is independent of the order in which transactions were committed.

**Status**: NEEDS DESIGN. D20 specifies the cascade but not the concurrency model.

### 5.3 Signature Replay Attacks

A signed transaction is just bytes. Anyone who has seen a signed transaction can replay it to other nodes. If the transaction is idempotent under the CRDT semantics (which it is — set union), replay is harmless. But replay can be used to "boost" the apparent activity of a particular agent or to influence value-topology calculations.

**Mitigation**: Include a TxId in the signed payload (already in the design). Reject replays at the network layer by tracking seen TxIds in a Bloom filter. The replay protection is not a correctness issue (CRDT handles it) but a denial-of-service issue.

**Status**: PARTIALLY DESIGNED. The TxId is in the signing message (D15-D19); the replay defense at the network layer is not specced.

### 5.4 CRDT Convergence Under Network Partitions

`(P(D), ∪)` is convergent under arbitrary partition patterns by construction. This is mathematically guaranteed. The risk is not in the algebra — it is in the implementation: if the merge code has a bug, convergence could fail in practice even when the math says it should hold.

**Mitigation**: Stateright model (already in the design). Property tests with adversarial network schedules. Lean proof of merge commutativity, associativity, idempotence (already in the design). Fault injection at the storage layer.

**Status**: DESIGNED-FOR. This is one of our strongest risk areas because the verification methodology is mature.

### 5.5 Reflective Rule Soundness

Reflective rules can derive new rules from existing rules. A bug in the rule derivation logic could introduce unsound rules — rules whose conclusions don't actually follow from their premises. This pollutes the entire rule library.

**Mitigation**: Every derived rule must carry a derivation tree (proof object) that can be verified by re-evaluating the source rules against the source datoms. This makes rule soundness checkable — not just at derivation time, but anytime a consumer uses the rule.

**Status**: PARTIALLY DESIGNED. Doc 011 mentions proof carrying; the verification protocol is not specced.

---

## Part VI: Scaling Risks

### 6.1 Storage Cost at 100M+ Datoms

We have not modeled the full storage footprint. Per-datom overhead includes:
- The datom itself (entity, attribute, value, tx, op): variable, but ~100-200 bytes typical
- Index entries (EAVT, AEVT, AVET, VAET): 4 entries per datom
- Ed25519 signature on the containing transaction: 64 bytes amortized across the transaction's datoms
- Content hash (BLAKE3): 32 bytes per content-addressed entity
- WAL frame overhead: ~50 bytes per frame

At 100M datoms with ~256 bytes/datom average (including all overhead), that's ~25GB. Tractable, but Ed25519 sigs alone account for ~6.4GB if amortized across single-datom transactions.

**Mitigation**: Larger transactions amortize signature cost better. Compression (LZ4, zstd) on cold data. Tiered storage (hot in memory, warm on SSD, cold on S3). Most importantly: actually model the cost spreadsheet so we know what we're getting into.

**Status**: NEEDS DESIGN. We have not built the storage cost model.

### 6.2 Index Sizes at 100M+ Datoms

The four indexes (EAVT, AEVT, AVET, VAET) each store one entry per datom. Plus the prolly tree, wavelet matrix, sorted vec, and Bloom filter overhead.

**Mitigation**: The wavelet matrix compression is the main lever. We've designed for it but not validated it at scale.

**Status**: EMPIRICAL. See fail-fast experiment §9.1.

### 6.3 Rule Library Size

By year 5, the rule library could contain thousands of rules across multiple trust tiers. Each rule is a set of datoms (head, body clauses, metadata, derivation tree). A rule with a 5-clause body is ~30 datoms. A rule library of 10,000 rules is 300,000 datoms — small compared to the data store, but non-trivial.

**Mitigation**: Most rule library overhead is in the derivation trees, which are append-only and can be archived. The "active" rule subset is much smaller and can be cached in memory.

**Status**: PARTIALLY DESIGNED. Doc 011 mentions trust tiers and rule lifecycle; the storage tiering for rule history is not specced.

### 6.4 Cascade Debt Accumulation

Truth maintenance defers cascade processing to the dream cycle. If the rate of retractions exceeds the rate at which the dream cycle can process cascades, the debt grows without bound.

**Worst case**: A high-volume retraction stream overwhelms the dream cycle. The "current truth" view of the database is stale — it claims derivations that have already been invalidated by upstream retractions but not yet cascaded.

**Mitigation**: Backpressure on retractions. Priority-based cascade processing (high-value derivations cascade first, low-value ones can be stale longer). Explicit "stale tolerance" parameter for query consumers (some queries accept stale-by-N-seconds results, others require fresh).

**Status**: NEEDS DESIGN. The cascade debt problem has not been formally identified in the spec; the backpressure mechanism does not exist.

### 6.5 Federation Mesh Size

In federation, every node maintains a frontier (vector clock) over every other node's TxIds. With N nodes, the frontier has N entries. For a small mesh (N < 100), this is trivial. For a large mesh (N > 10,000), the frontier itself becomes a scaling problem.

**Mitigation**: Hierarchical federation (nodes are organized into clusters; clusters communicate via cluster-level frontiers). Sparse frontiers (only track nodes you've actually merged with).

**Status**: NEEDS DESIGN. Spec 05 assumes a single flat mesh.

---

## Part VII: Novelty Risks

### 7.1 Proof-Producing Datalog Evaluator (Phase 4d)

There is no production system today that has a Datalog evaluator producing first-class proof objects for every derivation. DDlog had the closest approach but did not surface proofs. Datafrog and Crepe are good Datalog implementations but don't produce proofs.

**What's novel**: Every derivation carries a derivation tree. The tree is queryable as datoms (since "everything is datoms"). Consumers can verify any conclusion by walking its derivation tree. Truth maintenance uses the tree to invalidate derivations when premises change.

**Risk**: We don't know if the proof-tree overhead is bearable at scale. Every fact derived by a 10-clause rule needs a 10-node tree. At 100M derived facts, that's 1B tree nodes. Even at 32 bytes/node, that's 32GB.

**Mitigation**: Proof trees can be compressed (most derivations share structure). Proof trees can be archived to cold storage. Proof trees can be regenerated on demand from the source rules and datoms (recompute instead of store).

**Status**: NEEDS DESIGN. Doc 005 mentions derivation trees; doc 007 mentions DBSP-style incremental evaluation; the synthesis (proof-producing incremental evaluation) is unspecified.

### 7.2 Reflective Rules with CRDT Convergence (Phase 4c-4d boundary)

There is no production system today that combines self-modifying logic with CRDT semantics. The closest precedents are theorem provers with reflection (Coq, Agda) but those don't operate distributed; and CRDT databases (Riak, Automerge) but those don't support self-modifying logic.

**What's novel**: Rules are stored as datoms. Rule modifications go through the same signed transaction path as data modifications. The merge of two divergent rule libraries is well-defined (set union). The system can reason about its own rules and modify them.

**Risk**: We don't know if the algebraic guarantees actually hold for the implementation. The math says CRDT merge is convergent. The implementation might have bugs that violate convergence in subtle ways. And even if the implementation is correct, the OUTCOME is unknown — does the rule library converge productively, or does it descend into chaos?

**Mitigation**: Stateright model checking. Lean proofs of merge properties. Most importantly: small-scale empirical validation (50-rule prototype, see §9.3).

**Status**: PARTIALLY DESIGNED + EMPIRICAL.

### 7.3 Projection Calculus Homoiconicity (Phase 4d+)

Projections contain queries. Queries can return projection datoms. This is homoiconic — the language describes itself. Lisp had this; Datalog typically does not.

**What's novel**: Projections compose recursively. A projection can be used to compute the input to another projection. The system can reason about projections as data.

**Risk**: Recursive composition is hard to bound. Cycle detection is non-trivial. Termination depends on the projection graph being acyclic at evaluation time, which is a runtime property, not a static one.

**Mitigation**: Static analysis where possible. Runtime cycle detection where static analysis fails. Depth limits on recursive expansion. See §9.4.

**Status**: NEEDS DESIGN.

### 7.4 Truth Maintenance + CRDT Semantics

Classical truth maintenance (TMS, ATMS) assumes a centralized knowledge base. A retraction propagates through the dependency graph and invalidates derivations. In a distributed CRDT setting, retractions are themselves datoms — they don't actually remove anything from the store, they just mark something as retracted.

**What's novel**: Truth maintenance over a grow-only store. The "current truth" is a derived view (the LIVE projection), not a mutation of the store.

**Risk**: We don't know if the LIVE projection is efficient at scale. Computing "what is currently true" by traversing all datoms and applying retractions could be O(N) per query.

**Mitigation**: LIVE bitvector (already in the design) tracks which datoms are currently live. Updated incrementally as retractions arrive. Queries check the bitvector instead of traversing all datoms.

**Status**: DESIGNED-FOR (LIVE bitvector exists). The interaction with the cascade is PARTIALLY DESIGNED.

---

## Part VIII: Social Risks

### 8.1 Federation Byzantine Tolerance

`(P(D), ∪)` is convergent under crash faults. Under Byzantine faults, a node with valid keys can sign and flood semantically poisonous datoms. Ed25519 prevents impersonation. It does not prevent authorized poisoning.

**Worst case**: A trusted node is compromised. The attacker uses the node's signing key to flood the federation with garbage datoms — millions of fake derivations, fake retractions, fake rules. The CRDT merges them all (because they're cryptographically valid). The store grows unboundedly with garbage.

**Mitigation**: Content moderation layer. Per-node rate limits. Reputation scoring. Manual review for high-impact datoms (rule additions, schema modifications). Signed rejections that can be applied retroactively.

**Status**: NEEDS DESIGN. We have no content moderation in the current design.

### 8.2 Rule Namespace Collision

Doc 011's bootstrap sequence assumes thousands of derived rules across multiple trust tiers. Without strong namespacing, name collisions become a real issue. `policy/admit-immediately` could mean different things in different rule subtrees.

**Mitigation**: Hierarchical rule namespacing from day one. Rules are addressed by `(authority, namespace, name)`, not just name. This is easy to add now, painful to retrofit.

**Status**: NEEDS DESIGN.

### 8.3 Lean Proof Maintenance Tax

Every code change that touches a proven invariant requires the Lean proof to be updated. With 200+ INV proofs at year 5, this is a continuous tax on dev velocity.

**Mitigation**: Keep proofs at the spec level, not the implementation level. The spec changes less than the implementation. Implementation changes that don't affect the spec don't require proof updates.

**Status**: PARTIALLY DESIGNED. Most of our INV proofs already operate at the spec level. Some operate at the implementation level (specifically the integration tests). We should audit which is which and migrate where appropriate.

### 8.4 Emergence Safety

If reflective rules actually work, we'll see behavior we don't understand. The system might converge to local optima we can't reason about. Doc 011's safety argument assumes humans can audit the rule library, but at year 5 scale no human reads thousands of rules.

**Mitigation**: Meta-rules that constrain rule evolution. Rule additions require justification (signed by an agent that can be held accountable). High-impact rule changes require multi-party approval. Emergency "rollback to baseline" capability that disables all derived rules and reverts to the human-curated baseline.

**Status**: NEEDS DESIGN. This is the most underspecified risk in the document.

### 8.5 Application-Layer Ergonomics

The substrate is rigorous, but the application layer (projections, schemas, queries) might be too complex for non-experts. If the only people who can use Ferratomic are the people who built it, the system has failed regardless of how correct the substrate is.

**Mitigation**: Tutorial documentation. High-level abstractions over the raw substrate (e.g., a "schema builder" DSL that compiles to datoms). Worked examples for common use cases. Client libraries in multiple languages.

**Status**: NEEDS DESIGN. We have not started on application-layer ergonomics.

### 8.6 Trust in Self-Modifying Logic

Even if reflective rules work mathematically and the implementation is bug-free, users may not trust a system that modifies its own logic. The history of expert systems is littered with technically successful systems that were rejected because users could not understand or audit their behavior.

**Mitigation**: Explainability tooling. Every derivation must be queryable: "why did the system conclude X?" returns the derivation tree. Every rule must have a human-readable description. The rule library must be browsable, not just queryable.

**Status**: NEEDS DESIGN.

---

## Part IX: Fail-Fast Experiments

This is the actionable part of the document. Each experiment is a small, time-boxed validation we can run NOW to derisk one or more of the risks above. The goal is not to fully solve any of them — it is to learn enough to either commit harder to the design or pivot away from it.

Each experiment specifies: goal, hypothesis, methodology, success criteria, time budget, and which risks it derisks. All six are filed as beads (label `experiment`):

| § | Experiment | Bead | Time | Phase Derisked |
|---|------------|------|------|---------------|
| 9.1 | Index scaling at 100M datoms | bd-snnh | 2 days | 4b |
| 9.2 | Ed25519 verification throughput | bd-0lk8 | 1 day | 4a.5 + 4c |
| 9.3 | Reflective rule library hand-build | bd-lfgv | 2 days | 4d |
| 9.4 | Projection calculus cost model | bd-59dc | 2 days | 4d |
| 9.5 | Cascade debt simulation | bd-imwb | 1 day | 4d |
| 9.6 | Storage footprint cost model | bd-lzy2 | 1 day | 4b + 4c |

Total budget: 9 days of focused work, derisks 4 phases of speculative commitment.

### 9.1 Index Scaling Validation at 100M Datoms

**Goal**: Validate that our index architecture (sorted vec + prolly tree + wavelet matrix + interpolation search + Eytzinger layout) actually delivers the theoretical performance at production scale.

**Hypothesis**: Point query latency stays under 10µs at 100M datoms. Range scan throughput stays above 10M datoms/sec. Index build time is bounded by memory bandwidth, not CPU.

**Methodology**:
1. Generate a synthetic 100M-datom dataset with realistic value distributions (Zipf attribute distribution per doc 009).
2. Build all four indexes from scratch. Measure build time, memory usage, on-disk size.
3. Run a benchmark suite: 1M point queries (random distribution), 1K range scans (varying selectivity), 100 full scans.
4. Repeat at 10M, 50M, 100M to validate the scaling curve.
5. Compare measured cost against theoretical model.

**Success criteria**:
- Point query latency at 100M: <10µs (P50), <100µs (P99)
- Range scan throughput at 100M: >10M datoms/sec
- Index size on disk: <2x raw datom size
- Theoretical model predicts measurements within 30%

**Failure response**: If point query latency exceeds 100µs, the index architecture needs revision. If the theoretical model is off by more than 2x, our cost models for downstream phases (4b, 4c, 4d) are unreliable and need rebuilding.

**Time budget**: 2 days.

**Risks derisked**: §4.10 (index lookup at scale), §6.1 (storage cost), §6.2 (index sizes).

### 9.2 Ed25519 Verification Throughput

**Goal**: Validate that Ed25519 signature verification is not the bottleneck at our target transaction rates.

**Hypothesis**: Single-sig verification on target hardware achieves >20K/sec. Batch verification achieves >60K/sec. Both numbers are well above our 10K tx/sec target.

**Methodology**:
1. Generate 1M signed transactions of varying sizes (1, 10, 100, 1000 datoms each).
2. Run single-sig verification benchmark: measure throughput for each transaction size.
3. Run batch verification benchmark: measure throughput with batch sizes 10, 100, 1000.
4. Profile CPU usage during verification.
5. Test with multiple parallel verifier threads to measure scaling.

**Success criteria**:
- Single-sig: >20K verifications/sec on target CPU
- Batch (size 100): >60K verifications/sec
- Linear scaling with cores up to 8 cores
- CPU usage during verification: <50% of available

**Failure response**: If single-sig is below 10K/sec, the federation transport layer must batch aggressively. If batch verification doesn't scale linearly, we need a different verification library.

**Time budget**: 1 day.

**Risks derisked**: §4.9 (Ed25519 at scale), §5.3 (signature replay).

### 9.3 Reflective Rule Library Hand-Build

**Goal**: Validate that the reflective rules design actually composes — that we can hand-write a non-trivial rule library and walk through its evolution by hand.

**Hypothesis**: A 50-rule library across 3 trust tiers can be hand-written without contradictions. Truth maintenance for 10 retractions completes without infinite cascades. Rule derivation produces sensible new rules from existing rules.

**Methodology**:
1. Pick a domain we understand deeply (Ferratomic's own bug triage process).
2. Hand-write 50 rules covering: bug classification, priority assignment, dependency detection, sprint allocation.
3. Stratify into 3 trust tiers: baseline (engineer-written), validated (proven against historical bugs), provisional (recently added, not yet validated).
4. Apply 10 retractions of varying scope (small: retract one fact; medium: retract a high-frequency premise; large: retract a baseline rule).
5. Manually walk through the cascade for each retraction. Note any contradictions, infinite loops, or unexpected derivations.
6. Attempt to derive 5 new rules from the existing 50 by composition. Note whether the derivation rules feel principled or ad hoc.

**Success criteria**:
- 50 rules written without internal contradictions
- All 10 retraction cascades terminate within 100 derivations
- 5 derived rules feel principled and useful
- Trust tiers can be enforced by inspection

**Failure response**: If we cannot hand-write 50 consistent rules, the reflective rules design has a flaw we need to fix before building tooling. If retractions cascade unboundedly, truth maintenance needs a different design. If rule derivation feels ad hoc, doc 011 needs rethinking.

**Time budget**: 2 days.

**Risks derisked**: §4.4 (rule combinatorial explosion), §5.5 (rule soundness), §7.2 (reflective rules + CRDT), §8.4 (emergence safety).

### 9.4 Projection Calculus Cost Model

**Goal**: Validate that the projection calculus can deliver acceptable user-facing latency under the "queries never wait for LLM" model.

**Hypothesis**: Projections with cached cognitive results return in <10ms. Cache miss rate is bounded by the rate of new contexts (typically <1% in steady state). Dream cycle compute budget for projection refresh is <10% of total system compute.

**Methodology**:
1. Mock a projection with 5 cognitive nodes and 10 mechanical nodes.
2. Build a cache layer for cognitive results (input hash → result).
3. Run 1000 queries against the projection with realistic input variation.
4. Measure: query latency (P50, P99), cache hit rate, dream cycle compute required to maintain freshness.
5. Vary the rate of new contexts (10/min, 100/min, 1000/min) and measure cache miss latency.

**Success criteria**:
- P50 query latency with cache hit: <10ms
- P99 query latency with cache hit: <50ms
- Cache hit rate in steady state: >95%
- Dream cycle compute to maintain freshness: <10% of total

**Failure response**: If query latency exceeds 50ms even with cache hits, the mechanical layer is too slow. If cache miss rate exceeds 10%, the cache key is wrong. If dream cycle compute exceeds 10%, projection refresh is too expensive.

**Time budget**: 2 days.

**Risks derisked**: §4.6 (recursive projection expansion), §4.7 (mode dispatch), §4.8 (effectiveness scoring).

### 9.5 Cascade Debt Simulation

**Goal**: Determine how truth maintenance behaves under sustained retraction load.

**Hypothesis**: With incremental cascade processing and value-weighted prioritization, the cascade debt stabilizes (does not grow without bound) at retraction rates up to 100/sec.

**Methodology**:
1. Build a mock dependency graph with 1M derivations.
2. Generate retraction load at varying rates (10/sec, 100/sec, 1000/sec).
3. Measure: cascade processing rate, cascade debt over time, query result staleness.
4. Test with and without value-weighted prioritization.
5. Identify the breaking point where debt grows unboundedly.

**Success criteria**:
- At 100 retractions/sec: cascade debt stays bounded (oscillates around steady state)
- Query result staleness for high-value derivations: <1 second
- Query result staleness for low-value derivations: <60 seconds

**Failure response**: If cascade debt grows at 100/sec, we need backpressure (reject retractions when debt is high). If staleness exceeds tolerances, we need a different cascade strategy entirely.

**Time budget**: 1 day.

**Risks derisked**: §4.5 (truth maintenance cascade), §6.4 (cascade debt accumulation).

### 9.6 Storage Footprint Cost Model

**Goal**: Build the spreadsheet model for total storage cost at 100M datoms, including all overhead.

**Hypothesis**: Total storage at 100M datoms is between 20GB and 50GB depending on signing strategy and compression.

**Methodology**:
1. Enumerate all storage components: datoms, indexes, signatures, content hashes, WAL frames, derivation trees, rule library, LIVE bitvector, fingerprints, checkpoint metadata.
2. Estimate per-component bytes per datom under realistic assumptions.
3. Build a spreadsheet that outputs total storage as a function of datom count, average transaction size, average rule complexity, and compression ratio.
4. Validate the model against actual storage from a 1M-datom prototype.
5. Project to 100M, 1B.

**Success criteria**:
- Model exists, is documented, and is validated against prototype within 20%.
- Storage at 100M datoms: <50GB.
- Identifies the 3 biggest storage cost drivers.

**Failure response**: If storage exceeds 50GB at 100M, we need a compression strategy. If the biggest cost drivers are surprising, our intuitions about the system are wrong and need updating.

**Time budget**: 1 day.

**Risks derisked**: §6.1 (storage cost), §6.3 (rule library size).

---

## Part X: The Honest Conclusion

### 10.1 What We Know

We can build a formally verified embedded datom database. The substrate is solid. Phase 4a is closing. Phase 4a.5 is fully scoped with 24 lab-grade beads. We have the methodology (DDIS, Lean, proptest, fault injection) to make Phases 4b and 4c work. Build risk through Phase 4c is low.

### 10.2 What We Hope

We hope Phase 4d (proof-producing Datalog) works. We have strong intuitions and partial precedents (DDlog, DBSP). We do not have a working prototype. The fail-fast experiments in §9 will tell us within a week whether the intuitions are sound.

We hope reflective rules converge productively. The math says they should. The empirical question — does the rule library actually grow into something useful, or descend into chaos? — can only be answered by running it.

We hope the year-5 vision plays out. This is the most speculative claim in the project. It requires everything else to work AND for emergent behavior to be beneficial AND for users to trust a self-modifying system AND for the application layer to be ergonomic enough for non-experts AND for the federation to scale AND for the economics to work.

### 10.3 What We Should Do

**This week**: Run experiments §9.1 (index scaling) and §9.2 (Ed25519 throughput). These are the cheapest and most informative. They derisk Phase 4b directly.

**Next two weeks**: Run experiments §9.3 (rule library hand-build) and §9.6 (storage footprint). These derisk Phase 4d and the year-5 vision at low cost.

**Month two**: Run experiments §9.4 (projection calculus cost) and §9.5 (cascade debt simulation). These are the more involved experiments and require infrastructure we don't yet have.

**Throughout**: Update this document with results. Move risks from "needs design" to "designed for" as we close them. Move risks from "designed for" to "validated empirically" as experiments succeed. Move risks from "validated empirically" to "shipped" as we cross phase gates.

### 10.4 The Calibration Habit

The single most important practice this document establishes is the habit of calibrated probabilities. Not every claim deserves equal confidence. The substrate is at 95% — say so. The year-5 vision is at 10% — say so. Hiding the difference behind uniformly confident prose is worse than admitting the difference.

This habit should propagate. When we file beads, write specs, or make design decisions, we should attach a confidence number. Beads marked at 95% confidence get one kind of review (sanity check). Beads marked at 25% confidence get a different kind (challenge the assumptions). Beads marked at 10% confidence don't get implemented — they get experiments first.

### 10.5 The Asymmetric Bet

The compounding probability through the full vision is small (~0.3%). The expected value is still strongly positive because the payoff is unique and the cost is bounded. There is no other system in the world that combines formal verification, content-addressed identity, append-only history, CRDT semantics, signed transactions, proof-producing Datalog, and self-modifying logic. If any subset of these works, it is shippable. If the full set works, it is transformative.

We are betting on the asymmetric upside. The risks in this document are the ways the bet could fail. Naming them is how we manage them. Running the experiments in §9 is how we increase our confidence — or update our priors when reality disagrees with our hopes.

The substrate is grounded. The vision is speculative. The bridge between them is empirical validation. This document is the map. The next step is to walk it.
