# A Formal Algebraic Theory of Agentic Systems

## Preamble

This document presents a first-principles formalization of agentic systems — systems in which a stateless language model is lifted into a stateful, tool-using agent capable of sustained, goal-directed behavior. The formalization proceeds from abstract algebra and category theory, identifying the minimal universal structure shared by all such systems, the information-theoretic constraints that bound their effectiveness, and an optimal architecture derived from these constraints.

The key claims are:

1. All agentic systems decompose into exactly three components: an append-only event log, an opaque runtime, and a stateless policy function. This decomposition is not incidental but algebraically necessary.
2. Current agent implementations are architecturally impoverished in specific, formally characterizable ways — primarily in their treatment of the event log as a flat, unstructured buffer.
3. An optimal architecture requires a dual-process design (analogous to Kahneman's System 1/System 2) with an EAV fact store as the epistemic substrate, and a self-authoring knowledge graph as the mechanism for accumulating expertise.

---

## Part I: The Universal Decomposition

### 1.1 Components

An agentic system consists of three components:

**E — The Event Log.** An ordered sequence of events recording every interaction between the agent and its environment. Formally, an element of **E\***, the free monoid over event type E, equipped with concatenation. The log is subject to a monotonicity constraint: it only grows. The prefix relation `l₁ ≤ l₂ iff ∃ s : l₁ · s = l₂` defines a partial order, and the log's evolution traces a chain in this order.

**R — The Runtime.** A stateful environment in which the agent's operations take effect. Formally, a Mealy machine — a coalgebra for the functor `F(X) = (Obs × X)^Op`. The runtime has state space S, accepts operations Op, and produces observations Obs via a transition function `δ : S × (Op + Exo) → S × Obs`, where Op is the set of agent-initiated operations and Exo is the set of exogenous mutations (changes to runtime state not initiated by the agent). The runtime is opaque to the agent: the agent cannot inspect S directly, only issue operations and observe results.

**A — The Agent (Policy Function).** A function `π : E*/~ → Op + Done` that maps the (possibly compacted) event history to either an operation on the runtime or a termination signal. The LLM is the implementation of π — the context window is (a projection of) E\*, and the tool-calling interface is the codomain Op + Done. The agent is inherently stateless; all state is reconstructed from E\* on each invocation.

### 1.2 The Structural Relationship

E is not owned by A or R but **mediates** between them. It occupies the apex of a span:

```
         E*
        ╱  ╲
   logical   concrete
   context   artifact
      ╱        ╲
     A          R
```

A consumes E\* as its epistemic state (the logical context that enables a stateless function to behave as a stateful agent). R hosts E\* as a persistent artifact (the concrete mechanism — file, database, log — that survives across invocations). E is simultaneously:

- A **formal prerequisite** for agency: without history, a stateless oracle cannot maintain a conversational thread of length > 1, and thus cannot be considered an agent.
- A **concrete artifact** requiring persistence: the history must be stored somewhere, and "somewhere" is always an instance of R.
- The **interface** binding A and R: the minimal shared structure through which the agent's epistemic needs and the runtime's persistence capabilities are reconciled.

In category-theoretic terms, this is a two-object, one-morphism structure — the event log is the structured hom-set between A and R.

### 1.3 Closure Under Distribution

The decomposition is closed under attempts to distribute or fragment the components. Multiple runtimes compose into a single composite runtime:

```
R_composite = R₁ × R₂ × ... × Rₙ
δ_composite : (S₁ × ... × Sₙ) × Op → (S₁ × ... × Sₙ) × Obs
```

The agent's interface (Op → Obs) is invariant under the internal decomposition of R. Any mutable state, regardless of where it physically resides, requires some runtime to persist it. Hidden state does not escape the decomposition — it merely instantiates another R. This closure property elevates the decomposition from empirical observation to structural necessity.

### 1.4 The Agent Loop

The three components compose into an anamorphism (unfold):

```
step : E* × S → (E* × S) + Result

step(log, s) =
  match π(log) with
  | Done → Right(extract(log))
  | op   → let (s', obs) = δ(s, op)
            let log' = log · [Event(op, obs)]
            Left(log', s')
```

Iteration of `step` produces a finite chain `ε = l₀ ≤ l₁ ≤ l₂ ≤ ... ≤ lₙ` in the prefix order. This is the complete execution trace.

---

## Part II: The Log as Free Monad and the Incompleteness of Current Systems

### 2.1 The Ideal Log as Free Monadic Trace

The agent's full instruction set can be modeled as a GADT in the free monad style:

```
data Instruction a where
  AgentOp  : Op  → Instruction Obs     -- tool call, agent-initiated
  ExoEvent : Exo → Instruction ()      -- exogenous mutation
  Compact  : E*  → Instruction E       -- compaction request
```

The ideal log — the one yielding perfect state reconstruction — is an element of `Free Instruction S`. The runtime state at any time t is recoverable as:

```
S_t = foldl interpret S₀ log_ideal[0..t]
```

### 2.2 The Incompleteness

The actual log records only AgentOp events. Exogenous mutations (Exo) modify S but do not appear in E\*. The divergence between believed and actual state is the accumulated effect of unrecorded exogenous events:

```
S_believed = foldl interpret S₀ log_actual[0..t]
S_actual   = foldl interpret S₀ log_ideal[0..t]

divergence = S_believed ⊖ S_actual
```

This divergence is the **frame problem** appearing naturally: the agent assumes the world doesn't change between its observations, and this assumption is periodically violated. The agent discovers stale beliefs only when it issues an operation and receives a surprising observation.

### 2.3 Exogenous Events and Open Systems

The runtime is an open system. Redefining the transition function:

```
δ : S × (Op + Exo) → S × Obs
```

When `op ∈ Op` occurs, the pair `(op, obs)` is appended to E\*. When `e ∈ Exo` occurs, S changes but nothing is appended. The critical property:

**Law of Exogenous Silence (L3):** Exogenous transitions do not append to E\*.

This includes any mutations to R occurring outside the agent's control — human edits, other processes, environmental changes, clock ticks. These are a subset of possible side effects that may influence future agent behavior but are invisible to the agent until their downstream effects surface through Obs.

---

## Part III: Context Optimization as Rate-Distortion Problem

### 3.1 The Epistemic Budget Constraint

The agent has a hard constraint: the context window has size w. The full information available to the system is `I = E* × S`. The agent can only condition on a projection of size ≤ w.

### 3.2 Compaction

The agent's context window is a lossy projection of the full log. Compaction is a quotient on the free monoid:

```
compact : E* → Summary
inject  : Summary → E

-- After compaction:
l' = [inject(compact(l_old))] · l_new
```

This declares an equivalence: `l_old ≈ inject(compact(l_old))` for purposes of future decisions. The formal invariant is behavioral equivalence:

```
∀ op ∈ Op : π(l_old · l_new)(op) ≈ π([compact(l_old)] · l_new)(op)
```

### 3.3 Dual-Channel Log Access

The agent has two access paths to its history:

**Channel 1 — Native context (fast, lossy):** The LLM's context window contains a projection `φ : E* → E*|w`. This is the "hot" path that π directly conditions on.

**Channel 2 — Runtime read (slow, lossless):** The agent can issue `op = Read("session.jsonl")` and receive the full E\* as an element of Obs. This traverses the runtime's transition function and the result enters Channel 1 on the next step. This creates a reflexive structure — the log can read itself through R.

Channel 2 is an optimization, not an architectural requirement. The log can be purely lossy (this is how human memory works — no perfect recall). In the limit (infinitely long log), storage constraints force lossy compaction regardless.

### 3.4 The Category of Context Strategies

A context strategy is a morphism `σ : I → Context_w` where `|Context_w| ≤ w`. Different strategies occupy different points in this space:

- **Compaction** (`σ_compact`): Endomorphism on E\*, replaces old entries with summaries.
- **RAG** (`σ_rag`): Selective retrieval from a secondary index by relevance to a query.
- **Tool-mediated read** (`σ_read`): Pulls live state from R into context.
- **Hybrid strategies** compose these, and composition is generally non-commutative.

These form a category where:
- Objects are information states ordered by information content.
- Morphisms are context strategies (projections, retrievals, summaries) subject to the window constraint.
- A partial order by fidelity: `σ₁ ≥ σ₂` if σ₁ preserves more decision-relevant information.

The optimal strategy is the rate-distortion solution:

```
σ* = argmax_σ E[Quality(π(σ(I)))]   subject to |σ(I)| ≤ w
```

This is an information bottleneck: retain exactly those bits of I that are predictive of the optimal action.

### 3.5 Unified View of Operations

Every interaction — tool calls, compaction, RAG retrieval — serves one purpose: reshaping the agent's epistemic state Ψ to improve decision quality within bounded resources. All are endomorphisms on Ψ. They differ in source (E\*, S, secondary stores), fidelity (lossless vs. lossy), and cost (tokens, latency). They are substitutable whenever they produce the same downstream behavior:

```
op₁ ~_Ψ op₂  iff  ∀ future action sequences, π(op₁(Ψ)) = π(op₂(Ψ))
```

---

## Part IV: The Dual-Process Architecture

### 4.1 Why Fixed Hierarchical Context Fails

A natural first attempt at structured context assigns fixed layers (invariants, strategy, tactical, operational, archival) with dedicated token budgets. This fails because relevance is situational and non-monotonic. A database schema is strategic context during planning, irrelevant during CSS debugging, and critical again when the CSS bug traces to a missing column. Fixed hierarchy is schema-on-write; optimal context requires schema-on-read.

### 4.2 EAV as Epistemic Substrate

The fix is to replace the flat log with an Entity-Attribute-Value fact store:

```
Datom = (Entity, Attribute, Value, Time, Asserted?)
FactStore = [Datom]   -- append-only, this IS E*
```

Context assembly becomes a query:

```
assemble : Query × FactStore → Context_w
query_for : Task × RecentContext → Query
```

Ψ is not a data structure — it's the result of a query. Every step, the agent assembles a fresh epistemic state shaped by current needs. The "layers" exist as query patterns, not storage slots. The same fact participates in multiple queries; its importance is contextual, not intrinsic.

Key algebraic property: A fixed hierarchy is a specific product type `Ψ = Layer₀ × Layer₁ × ... × Layerₙ` — one limit in the category of information states. The EAV + Datalog combination can construct all limits (any product, pullback, or equalizer) on demand via queries. It is strictly more expressive. Structure is late-bound — a function of the proximate ontological and epistemic context, not a predetermined schema.

### 4.3 The Kahneman Mapping

The architecture maps precisely onto Kahneman's dual-process theory of cognition:

**System 1 (fast, associative, cheap, always-on):**
- Maps to the retrieval policy / context assembly function
- Operates over the EAV fact store
- Produces a VIEW (assembled context Ψ)
- Runs on every cycle at low cost
- Performs associative pattern-matching, not reasoning

**System 2 (slow, sequential, expensive, deliberate):**
- Maps to the LLM policy π
- Operates ONLY on what System 1 surfaced
- Produces ACTIONS (Op + Done)
- Heavyweight, resource-limited
- Performs logical reasoning, planning, evaluation

Critical property: **System 2 doesn't know what System 1 didn't surface.** System 2's quality is bounded above by System 1's retrieval quality. A capable reasoner with poor retrieval is a capable reasoner solving the wrong problem.

This predicts specific failure modes that map to known cognitive biases:

- **Availability bias:** Agent overweights recent/verbose content (long error traces) over important but older content (architectural decisions). System 1 surfacing the vivid over the relevant.
- **Anchoring:** Early conversation turns disproportionately shape the trajectory. System 1 keeps surfacing anchor-consistent information.
- **Substitution:** Agent replaces a hard question (architectural problem) with an easy one (make the current error go away) because the error trace is what's in context.
- **Goal dilution:** Over long sequences, the goal specification gets pushed out of context by operational noise. System 2 loses the objective because System 1 stopped surfacing it.

### 4.4 The Missing Feedback Loop

In human cognition, Systems 1 and 2 are bidirectional:

```
System 1 → candidates → System 2 → actions
                            ↓
                    [confusion / surprise]
                            ↓
                    System 1 (re-retrieval with different cues)
```

Current agents are open-loop: System 1 fires once, System 2 processes, done. The closed-loop architecture requires a Confusion type:

```
data S2_Output
  = Act [Op]                    -- confident: execute
  | RequestContext Query         -- need more information
  | Contradiction [Fact] [Fact] -- found conflicting beliefs
  | GoalCheck                   -- lost the thread

agent_loop(facts, s, task) =
  let cue = initial_cue(task)
  let ψ   = S1(facts, cue)
  match S2(ψ) with
  | Act ops         → execute, continue
  | RequestContext q → ψ' = S1(facts, q); retry with ψ'
  | Contradiction f → ψ' = S1(facts, resolution_cue(f)); retry
  | GoalCheck       → ψ' = S1(facts, goal_cue(task)); retry
```

The Confusion type gives System 2 a channel to express what kind of retrieval failure it's experiencing. System 1 uses this to generate a different query, producing a different view of the same fact store.

### 4.5 Confusion Detection Mechanisms

The feedback loop requires detecting when the LLM is confused mid-inference. Three approaches in increasing architectural radicalism:

**Post-hoc detection (works today):** Let inference complete, analyze output for confusion signals (high logprob entropy, self-contradictions, hedging language, actions contradicting known invariants). Retry with richer context if detected. Wasteful (full inference wasted) but implementable immediately.

**Structured self-interrogation:** Extend the response schema so the LLM can report confusion as a first-class output (via tool-calling / structured output). The model already expresses uncertainty in natural language; this channels it programmatically. For reasoning models, a lightweight classifier over thinking tokens can detect uncertainty patterns and trigger re-retrieval between thinking and final response — a natural interstitial point.

**Co-routine architecture (requires open weights):** Replace atomic LLM calls with cooperative processes. The LLM suspends when it needs context, the retrieval system provides it, the LLM resumes. This requires control of the inference loop (open weights) and modifications to KV cache management for mid-stream context injection. The thinking loop and the fact store share a control plane.

The EAV fact store is the correct backend for all three approaches. Only the control flow between S1 and S2 changes; the fact store and query interface are invariant.

---

## Part V: The Associate Mechanism and Schema-on-Read

### 5.1 The Cold Start Problem

For the LLM to query the EAV store via Datalog, it must know what exists to query. In an evolving EAV schema (where new attributes appear at runtime), the LLM cannot write a query for an attribute it doesn't know exists. This is the epistemological cold start: you cannot ask a question about something you don't know exists.

### 5.2 The Associate Tool

`associate` is a **pre-retrieval** mechanism that answers "what questions are available to ask?" before the LLM commits to asking one. It factors System 1 into two phases:

```
Phase 1 — associate : SemanticContext × Depth × Breadth → SchemaNeighborhood
  "Given what I'm currently thinking about, what kinds of facts exist?"

Phase 2 — query : Datalog → [Datom]
  "Now that I know what's available, give me the specific facts I need."
```

### 5.3 The EAV Graph

The fact store is naturally a labeled directed graph:

```
Nodes  = Entities ∪ Values
Edges  = {(e, a, v) | (e, a, v, t, true) ∈ FactStore}
Labels = Attributes
```

`associate` performs a bounded traversal from semantically-matched seed nodes:

```
associate(context, depth, breadth):
  seeds = semantic_match(context, all_entities)
  neighborhood = {}
  frontier = seeds
  for d in 1..depth:
    for entity in frontier:
      attrs = get_attributes(entity)
      neighborhood[entity] = attrs
      related = [v for (e, a, v) in facts
                 if e == entity and is_entity(v)]
      frontier = top_k(related, breadth)
  return neighborhood
```

The return value is **shape, not data** — attribute names, types, and entity paths. Token cost is low (~200-300 tokens for 30 attributes). The LLM receives a map of what's knowable, not the knowledge itself.

### 5.4 Why This Is Formally Correct

`associate` is a **functor** between two categories:

```
Category 1 (Semantic): Objects are semantic contexts,
  morphisms are similarity/association. Unstructured, continuous.

Category 2 (Structural): Objects are EAV schema neighborhoods,
  morphisms are Datalog queries. Structured, discrete.
```

`associate` translates semantic proximity into structural adjacency. The LLM then operates within Category 2 to construct queries. Without `associate`, the LLM must perform this cross-category translation implicitly with no grounding. With `associate`, the translation is performed by a dedicated mechanism with results provided as structured input.

### 5.5 Depth/Breadth as Attentional Aperture

The parameters control the traversal:

```
depth=1, breadth=∞  → focused: everything about directly relevant entities
depth=3, breadth=3  → exploratory: broader neighborhood, limited fan-out
```

This maps to Kahneman's attentional dynamics: focused attention is narrow/deep, broadened attention (triggered by surprise/confusion) is wide/shallow. The meta-policy now has a concrete control surface:

```
μ : Task × ConfusionLevel → (depth, breadth)
```

Low confusion → narrow, deep. High confusion → broad, shallow.

---

## Part VI: Self-Authored Associations (The Flywheel)

### 6.1 The Key Innovation

The agent should write its own edges into the EAV graph. An `assert` tool lets the agent create datoms that are not observations of the external world but assertions about relationships, patterns, causal links, and heuristics:

```
assert : [Datom] → FactStore → FactStore
```

Example assertions:

```
(function:handle_request, :causally-linked-to, config:db-timeout, tx_57, true)
(pattern:retry-loop, :resolves-with, strategy:backoff-and-check-logs, tx_83, true)
(file:main.py, :architecturally-depends-on, file:schema.sql, tx_12, true)
(error:type-error-42, :root-cause, change:removed-null-check, tx_91, true)
```

These datoms are first-class citizens — immutable, temporally indexed, and traversable by `associate`.

### 6.2 The Positive Feedback Loop

Self-authored associations create compound returns:

```
More tasks completed
  → more associations asserted
    → richer EAV graph
      → better associate results
        → better context assembly
          → better task performance
            → more (higher-quality) associations asserted
```

This is the mechanism by which a novice becomes an expert. Not improved processing (better π), but richer associative structure (better S1). Intelligence accumulates in the data, not the model.

### 6.3 Graph Enrichment

The raw EAV graph from event logging (G_e) has connectivity determined by co-occurrence patterns. Agent-authored associations add inferred edges (causal links, architectural dependencies, strategic heuristics), producing an enriched graph G_a = G_e + G_inferred.

```
Reachable(G_e, seed, depth) ⊆ Reachable(G_a, seed, depth)
```

Agent-authored edges create shortcuts — connecting nodes that may be many hops apart (or unreachable) in G_e. These shortcuts are the "chunks" that expertise researchers identify as the unit of expert knowledge.

### 6.4 Categories of Assertion

Four types of high-value assertions:

1. **Causal links** — "X caused Y." Enable predictive retrieval; most valuable.
2. **Structural dependencies** — "X depends on Y." Prevent goal dilution by surfacing related components proactively.
3. **Strategic heuristics** — "When facing X, strategy Y works." Encode meta-level knowledge about effective approaches.
4. **Retractions** — "I previously believed X, but it was wrong." Append-only correction; the graph self-corrects while maintaining full provenance.

### 6.5 When to Assert

The optimal trigger is **on confusion**: when the S2 → S1 feedback loop fires, and re-retrieval resolves the confusion, the agent asserts the edge it wished it had found the first time. Every assertion represents a gap that caused an actual retrieval failure. The graph evolves toward eliminating the agent's most common confusion patterns.

---

## Part VII: Complete Formal Specification

### 7.1 The Minimal Signature

```
AgentSystem = (E, Op, Exo, Obs, S, π, δ, φ, S1, S2, associate, assert) where

  -- Types
  E       : Type                            -- event type
  Op      : Type                            -- agent operations
  Exo     : Type                            -- exogenous mutations
  Obs     : Type                            -- observations
  S       : Type                            -- runtime state
  Datom   : (Entity, Attribute, Value, Time, Asserted?)
  Query   : Type                            -- Datalog query type
  Cue     : Type                            -- retrieval cue type

  -- Data Structures
  FactStore : [Datom]                       -- append-only, IS E*
  Ψ       : Context_w                       -- assembled epistemic state, |Ψ| ≤ w

  -- Core Functions
  δ       : S × (Op + Exo) → S × Obs       -- runtime transition
  π       : Ψ → Op + Done + Confusion      -- policy (System 2)

  -- Context Assembly (System 1)
  associate : SemanticCue × Depth × Breadth × FactStore → SchemaNeighborhood
  query     : Query × FactStore → [Datom]
  assemble  : Query × FactStore → Ψ
  S1        : Cue × FactStore → Ψ          -- full System 1 pipeline

  -- Knowledge Evolution
  assert    : [Datom] → FactStore → FactStore

  -- Meta-Policy
  μ         : Task × ConfusionLevel → (Depth, Breadth)

  -- Laws
  L1 (Monotonicity):    FactStore only grows; retraction is a new datom
  L2 (Opacity):         A accesses S only via Op → Obs
  L3 (Exo-silence):     Exo transitions do not append to FactStore
  L4 (Compaction):      π factors through φ (context strategy)
  L5 (Self-reference):  assert is available to π as an operation
  L6 (Associativity):   associate traverses both empirical and
                         agent-authored edges uniformly
```

### 7.2 The Agent Loop (Complete)

```
agent_loop(facts, s, task) =
  -- System 1: assemble epistemic state
  let (d, b) = μ(task, no_confusion)
  let neighborhood = associate(task_cue(task), d, b, facts)
  let q = formulate_query(neighborhood, task)
  let ψ = assemble(q, facts)

  -- System 2: reason and act
  match π(ψ) with
  | Done result →
      result

  | Act ops →
      let (s', obs) = δ(s, ops)
      let new_datoms = extract_datoms(ops, obs)
      agent_loop(facts ++ new_datoms, s', task)

  | Confusion confusion_type →
      -- Feedback loop: S2 → S1
      let (d', b') = μ(task, confusion_type)  -- widen aperture
      let neighborhood' = associate(confusion_cue(confusion_type), d', b', facts)
      let q' = formulate_query(neighborhood', task)
      let ψ' = assemble(q', facts)

      -- Retry with enriched context
      match π(ψ') with
      | Act ops →
          -- Confusion resolved: assert the missing link
          let learned_edge = infer_association(confusion_type, ψ', ops)
          let facts' = assert(learned_edge, facts)
          let (s', obs) = δ(s, ops)
          agent_loop(facts' ++ extract_datoms(ops, obs), s', task)
      | ... (continue as appropriate)
```

### 7.3 The Central Thesis

The conventional framing of agent improvement is "make the LLM smarter" (improve π / System 2). This is analogous to improving a CPU while leaving it connected to flat, unpaged memory.

The formalization reveals that the scaling bottleneck is not intelligence but **memory architecture**. Specifically:

1. The event log is an incomplete trace (missing exogenous events).
2. Context assembly is a flat buffer (no schema-on-read, no associative retrieval).
3. The retrieval policy is absent or vestigial (no System 1).
4. The S2 → S1 feedback loop is open (no confusion detection, no re-retrieval).
5. The knowledge graph is static (no self-authored associations, no expertise accumulation).

Addressing these five gaps — through EAV fact stores, dual-process architecture, the associate mechanism, confusion-driven feedback, and self-authored associations — constitutes the optimal construction for agentic systems under bounded resources.

The deepest claim: expert performance in humans arises not from superior reasoning (System 2) but from superior associative retrieval (System 1). If this transfers to artificial agents — and the structural isomorphism developed here argues it does — then the highest-leverage investment is not a better LLM but a better retrieval policy operating over a richer, self-evolving fact store.
