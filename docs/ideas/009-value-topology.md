# The Value Topology: Information-Theoretic Foundations for Knowledge Assessment

## Preamble

This document is the seventh in a sequence:

1. **"A Formal Algebraic Theory of Agentic Systems"** — the universal decomposition, dual-process architecture, EAV fact store.
2. **"Ferratomic as the Substrate for Distributed Cognition"** — Actor model isomorphism, store-messaging unification, ferratomic as memory infrastructure.
3. **"Everything Is Datoms"** — query-as-datom, taint tracking, policy-as-datom, the six-layer stack, the bilateral Y-combinator.
4. **"The Projection Calculus"** — the self-referential projection mechanism, dream cycles, agents as projections, code as projection, self-sustaining cognitive fixed points.
5. **"From Projections to Practice"** — differential dataflow validation, Claude Code codebase analysis, S-expression syntax, the McCarthy completion.
6. **"The Agentic Operating System"** — event-driven architecture, System 1 as state monad returning comonad, situations replacing conversations, the LLM as co-processor.
7. **This document** — discovers the information-theoretic structure of value in the datom store. Establishes that datom value follows power laws at every level of meta-assessment, that value is contextual (a property of datom-projection pairs, not datoms alone), that the value gradient field constitutes an optimal curriculum, and that the engine's role is to make value COMPUTABLE — not to compute it.

Documents 1-6 established what the system is, how it works, and what it becomes. This document establishes HOW THE SYSTEM KNOWS WHAT MATTERS — the formal structure of value assessment over `(P(D), ∪)`, and the clean separation between the engine (which provides structural primitives) and the application layer (which interprets value).

---

## Part I: The Information-Theoretic Foundations

### 1.1 Not All Datoms Are Created Equal

The store is `(P(D), ∪)`. Every datom is structurally equal — same type, same merge semantics, same CRDT participation. But informationally, datoms are wildly unequal. Some datoms are load-bearing — remove them and entire proof chains collapse. Others are noise — they consume storage and index space but never participate in a useful derivation.

The question this document addresses: can we KNOW which datoms are valuable, and can that knowledge be used to make the system smarter?

The answer: yes, value is computable from four independent dimensions, follows power laws at every level of meta-assessment, and — critically — is an APPLICATION-LAYER concern built ON the engine, not IN it.

### 1.2 Shannon Entropy of the Datom Set

Consider the store as a probability distribution over possible queries. Each datom contributes to some set of query results. The information content of a datom d is:

```
I(d) = -log₂ P(d is accessed in the next query)
```

High-access datoms have low information content per access (they're "expected" — like common words in a language). Low-access datoms have high information content per access (when accessed, they resolve significant uncertainty).

This is the Shannon paradox of knowledge systems: the most frequently accessed datoms are the least informative per access but the most valuable in aggregate. The rarely-accessed datom that resolves a critical confusion is maximally informative per access but may never be accessed.

The optimal retrieval strategy maximizes the mutual information between the retrieved set and the task goal:

```
I(Retrieved; Goal) = H(Goal) - H(Goal | Retrieved)
```

"How much does knowing these datoms reduce my uncertainty about the goal?" This is the formal characterization of what `associate` (Document 1, Part V) should optimize.

### 1.3 The Zipf Distribution

Datom access follows Zipf's law — the k-th most accessed datom is accessed with frequency proportional to 1/kᵅ where α ≈ 1. This is universal across natural language, web page access, database queries, and knowledge retrieval.

The consequence: the top 20% of datoms serve ~80% of queries (Pareto). But the interesting structure is in the TAIL. The long tail contains datoms that are rarely accessed but occasionally critical. These are the "expert knowledge" datoms — the ones that distinguish a novice from an expert.

The expert-novice gap from Document 4 (§1.5) maps directly onto the Zipf distribution:

```
Novice:        queries hit the head (popular, well-connected datoms)
Intermediate:  queries reach the body (moderately connected datoms)
Expert:        queries reach the tail (rare but critical datoms)
```

The skill flywheel (Document 3) is the mechanism by which the system learns to reach deeper into the tail. Analogical seeding (Document 4, §1.3) is the mechanism for jumping between tail regions via structural similarity.

### 1.4 Kolmogorov Complexity and Incompressible Knowledge

Some datoms are "compressible" — they can be derived from other datoms via Datalog rules. A derived datom's Kolmogorov complexity relative to the store is the length of the shortest derivation:

```
K(d | S) = min { |proof| : proof(S) ⊢ d }
```

A datom with K(d | S) = 0 is a theorem — derivable from what's already known. Storing it explicitly is redundant (though valuable as a cache for query performance).

A datom with K(d | S) = ∞ is an axiom — not derivable from anything in the store. It represents irreducible observation. These are the datoms with provenance `:provenance/observed`.

The value hierarchy by Kolmogorov complexity:

```
Axioms (K = ∞):     Irreducible observations. Cannot be re-derived if lost.
                    Maximum preservation priority.

Short derivations:  Easily re-derivable. Low preservation priority but
                    high caching value (save re-computation).

Long derivations:   Expensive to re-derive. High preservation AND
                    caching value. These are the "insights" — conclusions
                    that require many inference steps from axioms.

Noise (K ≈ 0):     Trivially derivable or never accessed.
                    Candidates for deprioritization.
```

Connection to the proof-theoretic interpretation: the most valuable datoms are the ones whose proofs are longest. Short proofs mean the conclusion is "obvious" — close to the axioms. Long proofs mean the conclusion required deep reasoning. Deep reasoning is expertise. The long-proof datoms ARE expertise, reified as datoms.

---

## Part II: Network-Theoretic Structure

### 2.1 The Entity Graph as a Scale-Free Network

The entity graph (entities connected by `Value::Ref` edges) is almost certainly a scale-free network — a few entities are highly connected (hubs) and most have few connections. This follows from preferential attachment: entities that are already well-connected attract more references.

In scale-free networks, the degree distribution follows P(k) ~ k^(-γ), typically with γ ∈ [2, 3].

The structural consequence: hub entities are disproportionately valuable. Removing a hub disconnects large portions of the graph. These are the "keystone datoms" — analogous to keystone species in ecology.

For ferratomic, the hub entities are:
- Schema-defining entities (db/*, lattice/*) — every typed datom references them
- Transaction entities (tx_entity) — every datom's tx field links to one
- High-reference entities (frequently cited via Value::Ref)
- Store identity entity — the root of trust (INV-FERR-060)

The VAET index provides hub detection: `VAET.count(entity)` is the in-degree. Entities with high in-degree are hubs. This is available NOW — no new engine primitive needed.

### 2.2 Betweenness Centrality and Information Bridges

Some datoms are valuable not because they're frequently accessed but because they're bridges — they connect otherwise disconnected graph regions. These have high betweenness centrality: many shortest paths between other entities pass through them.

In the proof-theoretic interpretation, bridge datoms are LEMMAS — intermediate results that multiple different proofs depend on. A lemma supporting 50 conclusions is more valuable than a theorem supporting only itself.

The causal predecessor DAG (INV-FERR-061) adds a temporal dimension: a transaction that is a predecessor of many downstream transactions is a temporal keystone — its causal influence propagates widely.

### 2.3 Metcalfe's Law and the Logarithmic Correction

The classic Metcalfe's law says network value ∝ n². The Briscoe-Odlyzko-Tilly logarithmic correction gives:

```
V(n) = n · log(n)
```

Not all connections are equally valuable — the value of each connection follows Zipf (the k-th most valuable connection has value ∝ 1/k). For ferratomic stores:

```
V(store) = |D| · log(|D|)
```

The marginal value of adding a datom INCREASES as the store grows (log is monotonically increasing). This is the formal basis for the compound interest argument in GOALS.md §5 — knowledge accumulation has increasing marginal returns.

But marginal COST also increases (larger stores are slower to query, more expensive to merge). The optimal store size is where marginal value equals marginal cost. The performance architecture (Phase 4b: prolly tree, wavelet matrix, O(log n) everything) is what keeps the cost curve flat enough that the value curve stays above it at 100M+ datoms.

---

## Part III: Bayesian Valuation

### 3.1 Datoms as Evidence

Each datom is evidence updating the posterior distribution over possible world states:

```
P(world | datoms) ∝ P(datoms | world) · P(world)
```

The value of datom d is the KL divergence between the posterior with d and without d:

```
V(d) = D_KL(P(world | D ∪ {d}) || P(world | D))
```

This is information gain — how much the datom changes the system's beliefs. A datom that dramatically shifts the posterior is highly valuable. A datom that barely changes it is noise.

The provenance lattice (Observed > Derived > Inferred > Hypothesized) maps onto evidence strength:

```
Observed:     high likelihood ratio, strong posterior update
Derived:      medium likelihood (depends on derivation quality)
Inferred:     weak likelihood (speculative connection)
Hypothesized: prior probability only (minimal evidence)
```

### 3.2 The Value of Being Wrong

Counterintuitively, datoms that are WRONG can be more valuable than datoms that are right.

A wrong datom with high provenance (an observation that turned out to be incorrect) is extremely informative — it reveals a failure mode in the observation process. The RETRACTION is the valuable part. The (assert, retract) pair together constitute a proof that the observation process is unreliable in specific circumstances.

In Bayesian terms, a disconfirmed hypothesis has higher information gain than a confirmed one (assuming the hypothesis was initially probable). This is why the scientific method values falsification over confirmation.

For ferratomic: retraction datoms are disproportionately valuable. They're rarer than assertions. When they occur, they signal important state changes. The LIVE resolution system (INV-FERR-029/032) gives retractions special treatment — they override assertions. This is correct: retractions carry more information per datom than assertions.

### 3.3 The Taint System as Bayesian Updating

Document 3's taint tracking is Bayesian updating:

```
Initial assertion:        prior (single-observation taint)
Corroborating evidence:   posterior update (taint reduction)
Contradicting evidence:   posterior update (taint increase)
Cross-context validation: strong posterior (validated-across-N-contexts)
```

The taint level IS the posterior probability. Evidence datoms (the ones that cause taint reduction) are the most valuable datoms in the store — they're the ones that turn hypotheses into validated knowledge.

---

## Part IV: Thermodynamic Analogies

### 4.1 The Store as a Thermodynamic System

The append-only store has a deep structural analogy with thermodynamics:

```
Datoms       ↔ Microstates
Store state  ↔ Macrostate
Entropy      ↔ Number of equivalent stores (same LIVE view, different histories)
Temperature  ↔ Rate of new datom arrival
```

The second law applies: the number of datoms that don't contribute to the LIVE view (historical datoms, retracted values, superseded assertions) grows monotonically. The ratio of "live" datoms to total datoms decreases over time. The store becomes increasingly "cold" — most of its mass is historical.

This is not a problem — it's a feature. The historical datoms are the proof chain. They're the evidence supporting the current live state. Discarding them would be like discarding the experimental record and keeping only the conclusions.

But it means the LIVE view is a low-entropy projection of a high-entropy store. LIVE resolution (INV-FERR-029) computes the macrostate from microstates. The projection calculus (Document 4) generates different macrostates from the same microstates — different views, different temperatures, different coarse-graining.

### 4.2 Free Energy and Actionable Information

In statistical mechanics, free energy is the portion of total energy convertible to useful work:

```
F = E - TS
```

For the datom store:

```
Actionable_information = Total_information - Entropy × Access_cost
```

A datom has high "free energy" if it has high information content AND low access cost (well-indexed, frequently traversed paths, central in the graph). A datom has low free energy if it's highly informative but buried in the tail (high access cost).

The performance architecture IS the mechanism for reducing access cost. Interpolation search (INV-FERR-077) reduces point lookup cost. Eytzinger layout (INV-FERR-071) reduces cache misses. The sorted-array backend reduces scan cost. Each optimization increases the free energy of the store by reducing the TS term — making more information actionable.

The wavelet matrix (ADR-FERR-030) is the endgame: ~5 bytes/datom means the entire store fits in memory at 100M datoms. When access cost approaches zero, free energy approaches total information. Every datom becomes actionable.

---

## Part V: The Four-Dimensional Value Function

### 5.1 Synthesis

Combining all four frameworks, datom value is a function of four dimensions:

```
V(d) = f(
    structural_centrality,    // network position (hub, bridge, leaf)
    information_gain,         // Bayesian posterior shift
    derivation_depth,         // Kolmogorov complexity (proof length)
    access_frequency          // empirical query participation
)
```

These dimensions interact non-trivially:

- **High centrality + high information gain = KEYSTONE FACT.** A hub that resolves uncertainty. Example: discovering that two previously unconnected systems share a common failure mode.

- **High derivation depth + low access frequency = LATENT EXPERTISE.** Deep insight that hasn't been needed yet. Example: a cross-domain structural motif (Document 4, §1.3) that hasn't been triggered by any current task.

- **High access frequency + low information gain = COMMON KNOWLEDGE.** Frequently consulted but unsurprising. Example: the store's own genesis schema. Essential but not informative.

- **Low centrality + high information gain = ORPHAN SIGNAL.** An important fact that isn't connected to anything. Example: an observation from a rare failure mode that hasn't been linked to root causes. Maximum opportunity for value creation through linkage.

### 5.2 The Value Topology Is Queryable

All four dimensions are computable from the datoms themselves:

```
Structural centrality:  VAET in-degree (available now in engine)
                       Betweenness (computable via graph traversal, R27 GraphIndex)
                       PageRank (computable via iterative Datalog in Phase 4d)

Information gain:       Provenance × taint level (available from D12 + taint system)
                       Evidence count (queryable from taint-reduction datoms)
                       Retraction status (LIVE view, available now)

Derivation depth:       tx/derivation-input chain length (available after D20)
                       Predecessor DAG depth (available from INV-FERR-061)

Access frequency:       Query-as-datom history (Document 3, Layer 3)
                       Seed outcome tracking (Document 4, §2.1)
```

The value of each datom can itself be stored as a datom:

```
{:e datom-entity :a :meta/structural-centrality :v 0.87}
{:e datom-entity :a :meta/information-gain :v 0.34}
{:e datom-entity :a :meta/derivation-depth :v 5}
{:e datom-entity :a :meta/access-frequency :v 0.02}
{:e datom-entity :a :meta/composite-value :v 0.71}
```

Value assessment enters the flywheel. Value is federable. An expert's value assessments transfer to a novice. The store knows what it knows AND knows how valuable what it knows is.

### 5.3 The Dream Cycle as Value Maximizer

Document 4's dream cycle maps directly onto value topology optimization:

```
Phase 1 (Consolidation):   Increase information gain of weakly-proved datoms
                           → moves datoms from "hypothesized" to "validated"
                           → increases V by strengthening the Bayesian term

Phase 2 (Cross-pollination): Increase structural centrality of isolated datoms
                           → creates bridges between disconnected regions
                           → increases V by strengthening the network term

Phase 3 (Gap mapping):     Identify regions of low access frequency
                           → these are the "unknown unknowns"
                           → guides acquisition of high-information-gain datoms

Phase 4 (Projection eval): Optimize the value function itself
                           → learn which dimension combination best predicts
                             task-relevant value
                           → meta-optimization of the value topology
```

Each phase targets a different dimension. Together, they perform gradient ascent on the composite value landscape.

### 5.4 Value-from-Cascade (A Posteriori Grounding)

The four a priori dimensions (§5.1) are *predictors* of a single a posteriori ground truth: the expected size of the truth-maintenance cascade that a datom's retraction would trigger under the current rule set (D20).

```
V_post(d) := E[ |cascade(retract(d))| | current rule set ]
```

Structural centrality, information gain, derivation depth, and access frequency are all estimators of this quantity — each captures a different slice of "how much would the system move if this datom went away." A high-centrality datom in a dense region with many downstream derivations is predicted to trigger a large cascade; a leaf datom with no dependents is predicted to trigger none. `V_post` is the thing the four dimensions are *trying to predict*, and the four-dimensional function in §5.1 is validated by how well it tracks observed cascade sizes over time.

This reframes the engineering picture. The four a priori metrics are cheap to compute (available at write time). `V_post` is expensive to compute (requires simulating a retraction) but is the ground truth. The right architecture is: compute the cheap metrics always, compute `V_post` occasionally during dream cycles, and use the `V_post` measurements to *refit* the weights in the composite function over time. Value assessment becomes a supervised learning problem where the labels come from actual cascade observations.

`V_post` is directly measurable in `bd-imwb`'s cascade debt simulator without any new infrastructure: the simulator already generates retraction events and records the resulting cascade sizes. Fitting the per-datom distribution of `E[|cascade|]` against each datom's a priori dimension vector gives the first validation of whether the §5.1 composite is predictive or only plausible. If the a priori metrics correlate poorly with `V_post`, §5.1 is wrong as stated and needs revision before any meta-value datoms get stored in production.

---

## Part VI: Value Is Contextual

### 6.1 Value Is Not a Property of the Datom

Everything above treats value as a property of individual datoms. But value is contextual — a datom's value depends on what task is active, what the system's current uncertainty is, and what other datoms are in the store.

The same datom can be worthless in one context and critical in another. The "auth handler connects to database timeout config" edge is noise when writing documentation. It's the most valuable datom in the store when debugging an auth failure.

Value is not a property of the datom. Value is a property of the (datom, context) pair. The datom is fixed (append-only). The context changes (each task, each query, each projection). Value is the relationship between them.

This is why the projection calculus (Document 4) is the right architecture. Each projection defines a context. The value of each datom is relative to the projection. Different projections assign different values. The same store, through different projections, reveals different value landscapes.

### 6.2 The Quantum Analogy

In quantum mechanics, an observable doesn't have a definite value until measured. The wavefunction contains all possible values as superpositions. Measurement collapses the superposition to a definite outcome.

A datom doesn't have a definite value until projected. The store contains all possible value assignments as latent structure. A projection — a specific query in a specific context for a specific task — collapses the latent value structure into a definite ranking. Different projections produce different rankings from the same store.

The LIVE view is one measurement. A Datalog query is another. The `associate` mechanism is another. Each is a different "observable" applied to the same "quantum state." And just as in quantum mechanics, measurement changes the state — query-as-datom means each query modifies the store, changing the latent value structure for future queries.

The bilateral Y-combinator (Document 3, Part V) is the system reaching a stationary state — a fixed point where projections produce results that, when fed back through the store, reproduce the same projections. This is the eigenstate of the value operator.

---

## Part VII: Recursive Pareto and the Meta-Value Hierarchy

### 7.1 The Value of Value Assessment

Value-assessment datoms are themselves datoms with computable value. The most valuable prediction datoms are the ones that correctly predicted value changes. The most valuable gradient datoms are the ones that pointed toward regions where actual learning occurred.

This creates a recursive structure:

```
Level 0: Datoms (facts about the world)
Level 1: Value datoms (facts about the value of facts)
Level 2: Meta-value datoms (facts about the value of value assessments)
Level 3: Meta-meta-value datoms (facts about the value of meta-assessments)
```

Each level is a datom in `(P(D), ∪)`. Each merges by set union. Each converges. The recursion doesn't diverge because each level is sparser than the one below (power law at every level — recursively Pareto).

### 7.2 The Fixed-Point Value Oracle

The recursive value assessment converges to a fixed point:

```
V₀(d) = access_frequency(d)
V₁(d) = V₀(d) + Σ prediction_accuracy(d)
V₂(d) = V₁(d) + Σ V₁(meta_assessments_of_d)
...
V∞(d) = fixed point of the recursive value operator
```

By Kleene's fixed-point theorem, if the value operator is continuous on the lattice of value assignments (which it is — corrections decrease geometrically due to power-law sparsity), the sequence converges to a unique fixed point.

V∞ is the TRUE VALUE of each datom — accounting for all levels of meta-assessment simultaneously. It's computable by iterating the operator until convergence, exactly like PageRank. O(k · |edges|) per iteration, converging in ~50 iterations for practical graphs. At 100M datoms, feasible as a nightly dream cycle computation.

### 7.3 The Recursively Pareto Distribution

The value distribution is power-law at EVERY level:

```
Level 0: ~20% of datoms serve ~80% of queries
Level 1: ~20% of value assessments explain ~80% of retrieval success
Level 2: ~20% of meta-assessments explain ~80% of value assessment accuracy
```

At each level, the most valuable datoms are the ones that assess value at the level below. Layer 3 (skill datoms) is more valuable per datom than Layer 1 (world knowledge). Layer 6 (policy datoms) is more valuable per datom than Layer 3. The projection that selects which projections to use — the meta-projection — is the most valuable artifact in the entire store.

---

## Part VIII: The Value Gradient Field

### 8.1 The Optimal Curriculum

If the value topology is a continuous surface (by interpolating between discrete datoms), the surface has a gradient — the direction of steepest value increase:

```
∇V(position) = direction where adding a new datom would create the most value
```

The gradient field over the store is a map of optimal learning. At every point in the knowledge space, the gradient says: "learn THIS next." Not because it's interesting (curiosity), not because a teacher says so (supervised). Learn this because it maximally increases the total value of everything already known.

The dream cycle Phase 3 (gap mapping) is a noisy approximation of the gradient field — it identifies regions of consistent retrieval failure, which are regions where the gradient is steep.

### 8.2 The Gradient Field Is Federable

The gradient field — the map of "what should I learn next" — is itself a set of datoms. It's federable. An expert's gradient field can be transferred to a novice via selective merge:

```
selective_merge(
  novice_store,
  expert_store,
  And(Namespace(":prediction/"), Namespace(":meta/value-gradient"))
)
```

The novice receives the expert's map of optimal learning. Not the expert's knowledge. Not the expert's retrieval strategies. The expert's assessment of WHERE KNOWLEDGE IS MISSING AND WHAT WOULD BE MOST VALUABLE TO ACQUIRE.

This is beyond knowledge transfer. Beyond skill transfer. This is curiosity transfer — the ability to transfer the expert's sense of what questions are worth asking.

### 8.3 Predictive Datoms

The gradient field enables a radical capability: predictive assertions. Datoms that represent the expected value of future observations:

```
{:e :prediction/P001 :a :prediction/pattern :v "if we observe X, datom D becomes critical"}
{:e :prediction/P001 :a :prediction/expected-value-delta :v 0.73}
{:e :prediction/P001 :a :prediction/target-datom :v <ref to D>}
{:e :prediction/P001 :a :prediction/activating-pattern :v {:attribute "error/type" :value "timeout"}}
```

"I predict that IF this pattern is observed in the future, THEN this existing datom becomes valuable." The prediction is a datom — append-only, signed, federable, taintable. When the activating pattern arrives, the prediction is confirmed (taint reduction). When it doesn't, the prediction ages.

This is System 1 that ANTICIPATES what it will need to know — a knowledge system that prepares for questions before they're asked.

---

## Part IX: The C8-Compliant Architecture

### 9.1 The Engine/Application Separation

The value topology is an APPLICATION-LAYER concern, not an engine primitive. This follows from C8 (Substrate Independence): "If someone used Ferratomic for a game engine, would a built-in value topology make sense?" No — value is domain-specific.

The engine provides STRUCTURAL PRIMITIVES that enable value computation:

| Engine Primitive | Value Dimension It Enables | Phase |
|-----------------|---------------------------|-------|
| VAET index | Structural centrality (in-degree) | Available now |
| DatomAccumulator (R21) | Structural counters (entity count, retraction ratio) | Phase 4b |
| Derivation chains (D20) | Derivation depth (proof length) | Phase 4d |
| GraphIndex (R27) | Graph metrics (betweenness, connected components) | Phase 4d |
| Datalog with recursion | Iterative computation (PageRank, fixed-point value) | Phase 4d |
| Projection calculus | Standing value projections, value-weighted retrieval | Phase 4d |
| Store fingerprint (D17) | Convergence verification of value assessments | Phase 4a.5 |

The engine makes value COMPUTABLE. The application COMPUTES it.

### 9.2 The Associate Mechanism as Value-Weighted Traversal

R10 (associate, Phase 4d) should accept an optional weight function:

```rust
pub fn associate(
    seeds: &[EntityId],
    depth: usize,
    breadth: usize,
    weight_fn: Option<&dyn Fn(EntityId) -> f64>,
) -> Vec<Datom>
```

Without weights: uniform BFS (novice mode). With weights: value-weighted BFS (expert mode). The weight function is application-provided. The traversal is engine-provided.

The expert's System 1 doesn't just traverse differently — it EVALUATES differently. The expert has calibrated value weights from experience. Transferring calibrated weights (via federation of Layer 3 datoms) is instant expertise transfer.

### 9.3 DatomAccumulator Metrics

R21 (DatomAccumulator, Phase 4b) should maintain these structural counters, all O(1) incremental:

- `entity_count`: total distinct entities
- `datom_count`: total datoms
- `datoms_per_attribute`: HashMap<Attribute, usize>
- `retractions_per_attribute`: HashMap<Attribute, usize> — retraction ratio = change signal
- `datoms_per_entity`: HashMap<EntityId, usize> — entity documentation density

These are engine-level structural metrics, not value judgments. Applications use them as inputs to value functions.

---

## Part X: Implications for the Ferratomic Roadmap

### 10.1 What's Already Planned and Validated

The value topology analysis validates the existing roadmap:

- **D17 (store fingerprint)**: O(1) convergence check enables convergence of value assessments
- **D20 (proof-producing evaluator)**: derivation chains enable computation of Kolmogorov complexity
- **R21 (DatomAccumulator)**: structural counters enable the centrality dimension
- **R27 (GraphIndex)**: entity graph traversal enables betweenness and PageRank
- **Phase 4d Datalog**: iterative computation enables the fixed-point value oracle

### 10.2 What's New

One design principle and two design notes:

**Principle**: The engine provides the physics; the application provides the interpretation. Value assessment is always application-layer. If a future proposal wants to add value computation to the engine, apply the C8 test.

**R10 design note**: Associate should accept an optional weight function for value-weighted traversal.

**R21 design note**: Accumulators should include retraction counts and per-entity datom density.

### 10.3 The Compound Interest Structure

The value topology reveals why ferratomic's compound interest argument (GOALS.md §5) is formally sound:

1. Marginal value of new datoms increases (n · log(n))
2. The skill flywheel (Layer 3) reaches deeper into the Zipf tail over time
3. Value assessment datoms (meta-knowledge) are more valuable per datom than content datoms
4. The recursive Pareto structure means each meta-level amplifies the value of levels below
5. The gradient field guides learning toward maximum value gain

The system's intelligence grows super-linearly with the store size. Each new datom is more valuable than the last because it connects to a richer graph. Each new retrieval episode improves the retrieval strategy. Each dream cycle strengthens the most important connections and identifies the most valuable gaps.

This is why `(P(D), ∪)` — the simplest possible algebraic structure with these properties — is the right foundation. The value topology IS the compound interest mechanism, realized as datoms about datoms in the same substrate as everything else.
