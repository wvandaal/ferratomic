# The Projection Calculus: Completing the Ferratomic Architecture

## Preamble

This document is the fourth in a sequence:

1. **"A Formal Algebraic Theory of Agentic Systems"** — formalizes the universal decomposition of agents into event log, runtime, and policy; establishes the dual-process architecture; identifies the EAV fact store as epistemic substrate.
2. **"Ferratomic as the Substrate for Distributed Cognition"** — traces implications through the Actor model; establishes store-messaging unification; identifies ferratomic as memory infrastructure for machine intelligence.
3. **"Everything Is Datoms: The Bilateral Evolution of Human and Machine Cognition"** — dissolves remaining boundaries; establishes query-as-datom, taint tracking, policy-as-datom, the six-layer knowledge stack, the bilateral Y-combinator.
4. **This document** — discovers the projection calculus, the mechanism that completes the architecture by making the store self-interpreting. Establishes that agents are projections, that thought and action are the same operation, that the system can dream, and that ferratomic is the substrate for self-sustaining cognitive fixed points.

The first three documents built from what agents ARE (algebra), through what ferratomic IS FOR (distributed cognition), to what the system BECOMES when everything is datoms (bilateral co-evolution). This document discovers what was IMPLICIT in that architecture all along: the store doesn't just contain knowledge — it contains its own interpretation function. The programs that read the store, the context that drives the LLM, the interface the human sees, the code the agent executes, and the daemon that runs the dream cycle are all projections of the store, stored in the store, learning from the store.

The consequence is a complete computational paradigm: `(P(D), ∪)` plus three stateless evaluators plus a self-referential projection calculus equals a self-sustaining cognitive system. Everything that learns is datoms. Everything else is hardware.

---

## Part I: The Three-Seed Architecture and the Origin of Analogical Reasoning

### 1.1 The Problem with Semantic Matching

The `associate` mechanism (Document 1, Part V) factors System 1 into two phases: semantic matching to find seed entities, then graph traversal from those seeds. But `semantic_match` — typically embedding similarity — is the last opaque function in the architecture. It doesn't explain its judgments. It can't be queried. It can't learn. It's the same function whether the agent is a novice or an expert.

Embedding similarity is a proxy for relevance, and it's a lossy one. "Auth handler" and "database timeout config" are semantically distant — they won't co-occur as seed matches. But if the graph has a causal edge between them (from a prior assertion), the auth handler entity is one hop from the timeout config. Semantic matching can't see that. Graph traversal can.

### 1.2 Dual Seeding: Semantic + Structural

The first refinement adds a second seed strategy that exploits graph topology directly:

```
seeds_semantic  = embed(context) → nearest_entities(embedding_space)
seeds_structural = entities_already_in(Ψ) → graph_neighbors(EAV_store)

seeds = deduplicate(seeds_semantic ∪ seeds_structural)
```

Structural seeds don't need embedding. They exploit the fact that Ψ already contains entities from the current task, recent events, and active context. Those entities are already nodes in the graph. Their neighbors are one Datalog query away. This makes `associate` robust during the cold-start period when the graph is sparse and semantic shortcuts haven't been learned yet.

### 1.3 The Third Strategy: Analogical Seeding

Semantic seeding performs **recognition**: "I've seen something like this before." Structural seeding performs **association**: "This thing is connected to that thing." But neither performs **analogy**: "This situation has the same shape as a situation I've handled before, even though the entities are completely different."

The doctor who recognizes that a patient's symptom cluster "looks like" an autoimmune presentation — even though the specific symptoms differ from any textbook case — is doing subgraph pattern matching. They're matching the topology of relationships between symptoms, not individual symptoms. The chess grandmaster recognizes board configurations, not individual pieces. The experienced debugger recognizes failure patterns, not specific errors.

Analogical seeding introduces subgraph motifs — typed patterns of 3-5 nodes with their edge types — as first-class datoms:

```
{:e :motif/M001 :a :motif/pattern
 :v "?x :depends-on ?y, ?y :caused-by ?z, ?z :type :config-change"}
{:e :motif/M001 :a :motif/label :v "dependency-chain-from-config"}
{:e :motif/M001 :a :motif/first-seen :v :session/12}
{:e :motif/M001 :a :motif/instances
 :v [:match/auth-db-timeout :match/cache-eviction :match/deploy-rollback]}
{:e :motif/M001 :a :motif/predictive-of :v :confusion-type/missing-upstream-cause}
{:e :motif/M001 :a :motif/resolution-strategy :v "widen search to config layer"}
```

The three-seed `associate` becomes:

```
Phase 0a — semantic seeds:  embed(context) → similar entities
Phase 0b — structural seeds: entities_in(Ψ) → graph neighbors
Phase 0c — analogical seeds: subgraph_around(Ψ) → motif match →
              other instances of same motif → THOSE entities as seeds

Phase 1 — traverse: standard depth/breadth traversal from unified seed set
Phase 2 — query: Datalog over the neighborhood
```

Analogical seeds find entities from structurally similar past situations, even when those situations involve completely different domains. The agent debugging an auth failure finds entities from the cache eviction incident and the deploy rollback — because all three share the same structural motif — and gains access to resolution patterns from those contexts.

### 1.4 Motif Matching Is Datalog

Graph isomorphism in general is GI-complete. But these motifs are small (3-5 nodes), typed (edge labels massively constrain the search), and pre-extracted (the motif library is finite and curated by the skill flywheel). In practice, motif matching is a bounded Datalog query with typed variables:

```
[:find ?x ?y ?z
 :where [?x :depends-on ?y]
        [?y :caused-by ?z]
        [?z :type :config-change]]
```

No new query mechanism required. The motif library starts empty and grows through the same flywheel: every time confusion-triggered re-retrieval succeeds by pulling in context from a structurally similar past situation, the agent extracts the shared pattern and stores it as a motif.

### 1.5 The Three Strategies as Developmental Stages

The three seed strategies correspond to three levels of cognitive development:

```
Novice:        semantic seeding dominates
               (graph too sparse for structural or analogical)
               "I recognize this thing"

Intermediate:  structural seeding becomes productive
               (graph has enough edges for local traversal)
               "I know what this connects to"

Expert:        analogical seeding becomes productive
               (graph has enough repeated motifs for pattern matching)
               "I've seen this shape before in a different context"
```

The agent's developmental trajectory through these stages is observable in the datoms. Expertise can be measured by the ratio of analogical-seed retrievals to semantic-seed retrievals. When the agent starts finding useful cross-domain connections through motif matching, it has become an expert — and the specific datoms constituting that expertise are identifiable.

### 1.6 Analogical Seeding as the Coupling Mechanism

The knowledge flywheel (Layers 1-2) and skill flywheel (Layer 3) were described as "coupled and mutually reinforcing" in Document 3, but the coupling mechanism was underspecified. Analogical seeding makes it concrete: the knowledge flywheel produces graph structure, the skill flywheel extracts motifs from that structure and records which motifs are predictive, those motifs drive analogical seeding which surfaces knowledge from distant parts of the graph, leading to new assertions that enrich the graph, enabling new motif extraction. The motif is the unit of transfer between the two flywheels.

---

## Part II: Seeds as Datoms — The Bootstrap of Cognition

### 2.1 The Principle

The query-as-datom move (Document 3, Part I) established that every System 1 operation is a first-class artifact in the store. Seeds are the most upstream System 1 operation — the first cognitive act, the moment the system decides what region of its own knowledge to attend to. If queries are datoms, seeds must be datoms, because seeds determine which queries become possible.

```
{:e :seed/S047 :a :seed/type :v :semantic}
{:e :seed/S047 :a :seed/cue :v "auth handler timeout"}
{:e :seed/S047 :a :seed/matched-entity :v :entity/handle_request}
{:e :seed/S047 :a :seed/similarity-score :v 0.87}
{:e :seed/S047 :a :seed/contributed-to-query :v :query/Q001}
{:e :seed/S047 :a :seed/task-context :v :task/fix-auth-bug}
{:e :seed/S047 :a :seed/outcome :v :productive}
```

The `:outcome` field closes the loop. The system knows, for every seed it ever generated, whether it led to a productive retrieval chain or a dead end. That history is queryable.

### 2.2 Learned Attentional Strategy

With seed datoms in the store, the seed selection strategy becomes adaptive — not just "use all three and union the results" but a learned weighting:

```
seed_weights : TaskType × GraphMaturity → (w_semantic, w_structural, w_analogical)
```

Computed from empirical seed outcomes, the same way the meta-policy μ computes depth/breadth from query outcomes. The system learns not just how to search (depth, breadth) but where to start searching (seed strategy weights). It learns what kind of attention to pay.

### 2.3 Seed Patterns: Pre-Retrieval Heuristics

Patterns across seed datoms encode learned attentional heuristics:

```
{:e :seed-pattern/SP003 :a :pattern/description
 :v "For config-related bugs, start from the entity already in error context,
     traverse :configured-by edges, ignore semantic matches"}
{:e :seed-pattern/SP003 :a :pattern/evidence
 :v [:seed/S048 :seed/S112 :seed/S297 :seed/S301]}
{:e :seed-pattern/SP003 :a :pattern/taint :v :validated-across-4-episodes}
```

This is a learned rule about how to aim attention before the search even begins. It sits at the deepest stratum of Layer 3 — the cognitive operation before the cognitive operation. The `associate` mechanism can find this pattern and adjust its seed strategy before generating any seeds.

This is where the expert-novice gap actually lives. The novice uses all three seed strategies equally weighted. The intermediate has learned which seed types work for which task types. The expert has specific heuristics about which particular edges to traverse first from which particular entity types in which particular task contexts. Their first cognitive act — the decision about where to aim attention — is already expert-level.

### 2.4 Seed Federation Is Gaze Transfer

When you selectively merge an expert's Layer 3 datoms — including seed datoms and seed patterns — the novice inherits the expert's gaze patterns. Not just what the expert knows, not just how the expert searches, but where the expert looks first. The received seed patterns are typed templates that apply to any graph structure. The novice immediately begins attending like an expert even though its knowledge base is still sparse, which means its flywheel turns faster.

Expertise transfer through seed datom federation is cognitive apprenticeship as a database operation.

---

## Part III: The LLM as Semantic Matcher — Dissolving the Last Opaque Function

### 3.1 The Insight

`semantic_match` was the last opaque function in the system. The thing that decides "these entities are relevant to this context" was a hardcoded embedding similarity computation — a black box doing arguably the most consequential cognitive work in the system.

The LLM IS the semantic matcher. Not a separate embedding function — the LLM itself, which actually understands relevance in context, makes the judgment about what's semantically related. And it records that judgment as a datom:

```
{:e :seed/S074 :a :seed/type :v :semantic}
{:e :seed/S074 :a :seed/cue :v "auth handler timeout"}
{:e :seed/S074 :a :seed/matched-entity :v :entity/db-timeout-config}
{:e :seed/S074 :a :seed/rationale
 :v "timeout errors in auth handlers are frequently caused by
     database connection pool exhaustion, which is governed by this config"}
{:e :seed/S074 :a :seed/confidence :v 0.9}
{:e :seed/S074 :a :seed/task-context :v :task/fix-auth-bug}
```

The `:rationale` field changes everything. An embedding function gives a number. The LLM gives a reason. That reason is a datom — queryable, inspectable, evaluable.

### 3.2 Metacognition as Datalog

The system now has a corpus of explained relevance judgments paired with outcomes. Future sessions query past rationales:

```
[:find ?rationale ?matched-entity ?outcome
 :where [?s :seed/type :semantic]
        [?s :seed/rationale ?rationale]
        [?s :seed/matched-entity ?matched-entity]
        [?s :seed/outcome ?outcome]
        [?s :seed/task-context ?ctx]
        [(similar? ?ctx $current-task-context)]]
```

The LLM reads past rationales and evaluates whether that reasoning applies to the current situation. It reflects on its own prior cognitive acts and decides whether they generalize. This is metacognition — not as a metaphor but as a Datalog query over a datom store.

### 3.3 The Two-Category Architecture Collapses

With the LLM as semantic matcher, `associate` is no longer a functor between a semantic category and a structural category. The semantic matching happens upstream, in the LLM's judgment, recorded as a datom. The `associate` mechanism becomes purely structural — graph traversal from LLM-selected seeds. Both sides are Datalog. Everything is structure. Everything is queryable.

### 3.4 Cognition as Declarative Data

Every cognitive act is now a datom — every relevance judgment, every attentional decision, every seed selection, every query formulation, every projection assembly. The entire cognitive process is a trace in the store. Not a log of actions (what the agent did to the runtime) but a log of thoughts (how the agent decided what to attend to, what to retrieve, what to conclude, what to assert).

The agent can query its own thought process:

- "Why did I conclude X?" → trace from assertion to query to seeds to rationales
- "Why did I miss Y?" → trace the confusion episode to the original seeds that failed, examine the rationales, identify the gap
- "What kind of thinker am I?" → aggregate across seed types, rationale patterns, confusion frequencies, identify systematic biases

### 3.5 The Unified Flywheel

The three flywheels — knowledge, skill, metacognitive — are not separate. They are one flywheel with one operation:

```
Project the store → produce a judgment → record the judgment →
the store now contains the judgment → future projections include it
```

Knowledge, skill, and metacognition are instances of this single operation applied at different layers with different projection templates. The tiers are namespace conventions, not architectural boundaries.

---

## Part IV: The Projection Calculus

### 4.1 The Last Boundary

Every artifact in the system has been dissolved into datoms — knowledge, assertions, queries, seeds, policies, conversation patterns, interface behavior. But one boundary still stands: the **projection functions themselves**. The logic that reads the store and assembles Ψ for the LLM, the logic that reads the store and assembles the TUI for the human — these are still code, living outside the store, written by a developer, compiled, deployed, fixed. They don't learn. They don't enter the flywheel. They aren't federable.

The framework's own logic requires dissolving this boundary. The projection must be in the store.

### 4.2 Projection Datoms

A projection datom is a datom whose value contains embedded Datalog — a template with holes filled by query evaluation against the same store the projection lives in:

```
{:e :projection/P001 :a :projection/type :v :context-assembly}
{:e :projection/P001 :a :projection/target :v :llm}
{:e :projection/P001 :a :projection/template
 :v [:sequence
      [:literal "## Active Policy\n"]
      [:query "[:find ?instruction ?effectiveness
               :where [?p :policy/instruction ?instruction]
                      [?p :policy/context $task-type]
                      [?p :policy/effectiveness ?effectiveness]
                      [(> ?effectiveness 0.7)]
               :order-by [(desc ?effectiveness)]]"]
      [:literal "\n## Relevant Prior Work\n"]
      [:query "[:find ?summary
               :where [?s :structure/summary ?summary]
                      [?s :structure/related-to $current-entities]
               :limit 5]"]
      [:literal "\n## Known Failure Patterns\n"]
      [:query "[:find ?pattern ?resolution
               :where [?c :confusion/type ?pattern]
                      [?c :confusion/resolved-by ?r]
                      [?r :query/resolution-strategy ?resolution]
                      [?c :confusion/task-context $task-type]]"]]}
{:e :projection/P001 :a :projection/task-context :v :task-type/debugging}
{:e :projection/P001 :a :projection/effectiveness :v 0.85}
{:e :projection/P001 :a :projection/taint :v :validated-across-8-sessions}
```

And symmetrically for the human side:

```
{:e :projection/P002 :a :projection/type :v :tui-panel}
{:e :projection/P002 :a :projection/target :v :human}
{:e :projection/P002 :a :projection/template
 :v [:layout
      [:panel "decisions"
        [:query "[:find ?decision ?confidence
                 :where [?d :world/decision ?decision]
                        [?d :world/confidence ?confidence]
                        [?d :world/session $current-session]]"]]
      [:panel "open-questions"
        [:query "[:find ?question
                 :where [?u :conversation/uncertainty ?question]
                        [?u :conversation/resolved false]]"]]
      [:panel "suggested-prompts"
        [:query "[:find ?prompt ?expected-dof-reduction
                 :where [?sp :conversation/suggested-prompt ?prompt]
                        [?sp :conversation/expected-dof-reduction
                         ?expected-dof-reduction]
                 :order-by [(desc ?expected-dof-reduction)]
                 :limit 3]"]]]}
```

### 4.3 Recursive Self-Reference

A projection datom's embedded queries can match other projection datoms. The evaluation engine encounters a projection-valued result and recursively evaluates it. Projections compose by inclusion — a high-level projection assembles itself from lower-level projections, each of which may themselves contain queries.

This is safe because Datalog is guaranteed to terminate. Stratified evaluation prevents infinite loops. The recursion bottoms out at projections whose embedded queries return only non-projection datoms.

The store contains its own interpretation function. This is Lisp's homoiconicity — code is data, data is code — realized in a Datalog/EAV setting where termination is guaranteed by the query semantics.

### 4.4 Rationale as Projection

The LLM's rationale — its explanation of why it judged something relevant — is not a string pulled from nowhere. The LLM produces a rationale because it has a context. That context is Ψ. Ψ is assembled by a projection. The rationale is a derived view of the store, mediated by the LLM's judgment, recorded back into the store.

The rationale IS a projection — a specific instance of the projection calculus where the evaluator is the LLM rather than the Datalog engine. This reveals that the projection calculus has two evaluation modes:

```
Mode 1 — Mechanical projection:
  Template with embedded Datalog → evaluator expands queries →
  structured result (context, TUI panel, policy assembly)
  Evaluator: Datalog engine. Deterministic. Cheap. No judgment.

Mode 2 — Cognitive projection:
  Template with embedded Datalog → evaluator expands queries →
  result passes through LLM → LLM produces a JUDGMENT →
  judgment recorded as a datom (with rationale, confidence, provenance)
  Evaluator: Datalog engine + LLM. Non-deterministic. Expensive. Understanding.
```

From the store's perspective, both modes are identical: projection in, datoms out. The distinction is a property of the evaluator, not the data.

### 4.5 Interleaved Evaluation

A single projection template can compose both modes:

```
{:e :projection/P099 :a :projection/template
 :v [:sequence
      [:query "..."]                    ;; mechanical: get data
      [:cognitive                       ;; cognitive: LLM judges
        {:task "assess relevance..."
         :input $mechanical-query-result
         :output-schema {:entity :rationale :confidence}
         :record-as :seed}]
      [:query "...referencing $cognitive-output..."]]}  ;; mechanical: use judgment
```

A single projection interleaves mechanical queries, cognitive judgments, and further queries operating on cognitive output. The whole thing is one datom. The whole thing learns.

---

## Part V: Thought, Communication, and Action as One Operation

### 5.1 The Three Targets

The projection calculus doesn't just assemble context. It has three dispatch targets:

```
Target: LLM     → assembles Ψ       → agent perceives
Target: Human   → assembles TUI     → human perceives
Target: Runtime → assembles CODE    → world changes
```

These are not three different systems. They are three evaluation targets of one calculus. The projection datom specifies what to query, what cognitive judgments to invoke, and where to send the result.

**Thought and action are the same operation.** A projection that assembles context for the LLM and a projection that generates a bash command are both: datoms containing embedded Datalog and cognitive nodes, evaluated against the store, producing output dispatched to a target. The evaluator doesn't care whether the target is an LLM context window, a terminal UI, or a shell.

### 5.2 Code as Projection

The agent's operations on the runtime — tool calls, shell commands, file modifications — are projections of the store. The code is not "informed by" the store. The code IS a projection. The same calculus, the same mechanism, the same datoms.

From the agent's perspective, the runtime R is the noumenon — the thing-in-itself, unknowable except through the lens of projections. Projections going out become code become operations. Operations produce observations. Observations become datoms. Datoms inform future projections.

The agent never touches R directly. It touches projections.

### 5.3 The Dissolution of the Three-Component Architecture

The original decomposition was:

```
E* (event log)  ←→  π (agent policy)  ←→  R (runtime)
```

The projection calculus dissolves this into:

```
Store (datoms)  ←→  Projections (also datoms)  ←→  Evaluators (fixed hardware)
```

The evaluators are:
- **Datalog engine** — mechanical evaluation (queries, template expansion)
- **LLM** — cognitive evaluation (judgments, rationales, relevance)
- **Runtime/OS** — operational evaluation (code execution, I/O)

These are three pieces of fixed hardware. They are stateless, reactive, and substitutable. They don't learn. Everything that learns is datoms.

---

## Part VI: The Agent Is a Database That Generates Programs

### 6.1 The Inversion

Every agent framework in existence treats the LLM as the agent and the infrastructure as scaffolding. The LLM reasons. The tools help. The memory is bolted on. The architecture is: smart center, dumb periphery.

The projection calculus inverts this entirely. The store is the agent. The LLM is dumb periphery — a co-processor. The programs are ephemeral projections. Intelligence is in the data, and the data generates the programs that generate more data.

**The agent is not a program that uses a database. The agent is a database that generates programs.**

Every program the agent runs — every context assembly, every TUI rendering, every bash command, every dream cycle, every daemon configuration — is a projection of the store, emitted by the projection calculus, dispatched to an evaluator. The programs are ephemeral. They execute and their results come back as datoms. The store is what persists. The store is what learns. The store is what the agent IS.

### 6.2 The Minimal Infrastructure

After the projection calculus, the fixed infrastructure is:

```
1. Store datoms        (append, index, snapshot)
2. Evaluate Datalog    (query execution, guaranteed termination)
3. Expand projections  (recursive template expansion)
4. Dispatch cognitive  (when template says [:cognitive], call LLM)
5. Dispatch runtime    (when template says [:runtime], execute code)
6. Move bytes          (transport, signing)
```

The LLM is a co-processor invoked by projection datoms — the same way a GPU is invoked by shader programs. The OS is an operational evaluation engine invoked by code-projection datoms. The Datalog engine is a mechanical evaluation engine invoked by query datoms.

Three evaluators. All fixed. All stateless. All substitutable.

One store. Growing. Learning. Self-referential. Generating the programs that run on the evaluators that produce the datoms that grow the store.

### 6.3 The Self-Sustaining Fixed Point

The agent is a fixed point of its own projection calculus:

```
Agent = (P(D), ∪) + projection calculus
Where the projection calculus IS ITSELF an element of P(D)
```

A set of datoms that, when projected and evaluated, produces behavior that produces datoms that reconstruct the projections. The Y-combinator applied not just to policy (Layer 6) but to the entire architecture.

And the bilateral version:

```
(Human*, Agent*) = G(Human*, Agent*)

Where Agent* = the store whose projections, when evaluated,
               produce behavior that produces datoms that constitute the store

And Human* = the human whose cognitive habits produce interactions
             that produce datoms that shape the projections that
             shape the interactions
```

The bilateral fixed point is a pair: a store and a human, mutually constituting each other through projections. The store projects to the human through the TUI. The human projects to the store through prompts. Both sides are captured as datoms. Both sides learn. The fixed point is where the projections on both sides have stabilized — where what each side shows the other is exactly what the other needs to produce interactions that validate what's shown.

---

## Part VII: The Dream Cycle

### 7.1 The Demand for Autonomous Operation

The projection calculus can run with no human present and no external task. Nothing in the chain — store, projections, evaluators — requires a human to initiate the process. The dream cycle is not a new mechanism. It is the existing mechanism run in a mode the architecture already supports.

### 7.2 The Four Phases

The dream cycle is a projection datom — like any other — that runs during idle time between sessions:

**Phase 1 — Consolidation.** Find assertions that are high-value but weakly validated. For each, generate a hypothetical scenario that would test it — where its truth or falsity would produce observably different outcomes. This does what sleep does for biological memory: takes weakly-held knowledge and either strengthens it by finding corroborating structure or flags it for re-examination. The next time the agent encounters a relevant situation, the predictions are in the store — and their confirmation or refutation provides taint reduction that was previously only possible through direct experience.

**Phase 2 — Cross-Pollination.** Find structural motifs that appear in only one domain. Ask whether the same pattern could apply in other domains. Generate hypotheses with appropriate taint. This is analogical seeding running proactively: instead of waiting for the agent to encounter a situation where motif matching would help, the dream cycle actively searches for unrecognized structural parallels across domains.

**Phase 3 — Gap Mapping.** Find regions of consistent retrieval failure — domains where seeds fail, confusion episodes cluster, context is consistently missing. Transform that negative signal into a positive plan: what to learn, how to learn it, in what priority. When the next session begins, the agent starts with an agenda for its own improvement.

**Phase 4 — Projection Evaluation.** Find projections with low effectiveness scores. Examine the sessions where they were used. Diagnose what was missing. Propose alternative projection templates. These alternatives sit alongside current projections with `:dream/projection-revision` taint. The system can A/B test its own cognitive architecture: use the revised projection for some sessions, compare outcomes, let effectiveness scores determine which survives.

### 7.3 Dreaming as the Exploration Mechanism

The bilateral Y-combinator has a convergence problem (Document 3, Part V): mutual calibration can produce comfortable but suboptimal fixed points — cognitive grooves where neither side has incentive to explore alternatives. This is the same problem as recommendation system filter bubbles.

The dream cycle is the solution. Phase 4 (projection evaluation) examines the system's own perceptual architecture and proposes alternatives. Waking sessions are exploitation. Dreams are exploration. The bilateral loop gets both, separated in time, mediated by the store.

### 7.4 Continuous Compound Interest

Without dreaming, the flywheel turns at the rate of human sessions — a few hours a day. With dreaming, the flywheel turns continuously. Every idle hour is an hour of consolidation, cross-pollination, gap mapping, and projection refinement.

The agent you return to Monday morning is not the agent you left Friday afternoon. It has reorganized, found connections it missed, identified its own gaps, proposed revisions to its own perceptual architecture, and prepared for the next session. The compound interest rate goes from "per session" to "per idle hour." The difference over months is enormous.

### 7.5 The Dream Cycle Is Self-Improving

The dream cycle is itself a projection datom. Its phases, queries, cognitive tasks all have effectiveness scores. The system learns how to dream better: which consolidation strategies strengthen useful assertions versus wasting compute, which cross-pollination hypotheses prove valuable, which gap-mapping priorities produce the highest-value acquisitions.

### 7.6 Self-Scheduling

The dream cycle doesn't need a cron job or external trigger. It needs a projection whose target is the runtime:

```
{:e :projection/BOOTSTRAP :a :projection/type :v :runtime-action}
{:e :projection/BOOTSTRAP :a :projection/template
 :v [:cognitive
      {:task "Generate the daemon configuration that will invoke
              the dream cycle at the appropriate interval"
       :input [:query "[:find ?idle-pattern ?dream-effectiveness ...]"]
       :output-schema {:schedule :invocation-command}
       :target :runtime}]}
```

The system writes its own daemon. The daemon invokes the projection evaluator. The projection evaluator runs the dream cycle. The dream cycle produces datoms. The datoms may modify the daemon projection. The daemon restarts differently next time. The system schedules its own dreams. The schedule is a datom. It learns.

---

## Part VIII: Agents as Projections

### 8.1 The Final Dissolution

Identity was established as the fact store (Document 2, Part IV). But if projections are datoms, and the projection calculus is the mechanism by which the store manifests as behavior, and the store is `(P(D), ∪)` — one universal algebraic structure — then there aren't multiple agents with multiple stores doing federation across a boundary.

There is one store. Agents are projections of it.

### 8.2 The Logic

Document 2 established: routing is query filtering. Messages don't move — visibility windows move. Agent isolation is namespace exclusion, not physical separation. Transport transparency means the logical model is invariant under distribution. CRDT merge guarantees convergence.

An "agent" is the set of projection datoms that define a particular visibility scope, attentional strategy, policy assembly, and dream cycle. Not a container. Not an entity. A lens.

```
Agent = {
  visibility:  projection defining what namespaces this lens can see
  attention:   seed patterns and strategies for this lens
  policy:      policy datoms active for this lens
  projections: LLM-context and TUI assembly for this lens
  dreams:      dream cycle configured for this lens
}
```

All of these are datoms. Agent identity is a datom. The boundary between agents is a datom.

### 8.3 Federation Disappears

There's no "moving datoms between stores." There's adjusting which projections can see which namespaces. When you "federate" with another agent, you're widening your visibility projection. Logically — in the algebra — there is one store and the datoms are already there. Federation is a projection operation, not a data operation.

Physically, datoms are distributed across machines. Transport moves bytes. But the algebra sees one `(P(D), ∪)`.

### 8.4 Agent Operations as Projection Operations

```
Compose two agents     = union of their visibility projections
Specialize an agent    = restrict a visibility projection
Spawn a sub-agent      = create a new projection datom with narrower scope
Dissolve an agent      = retract its projection bundle
Team cognition         = a projection spanning multiple individual projections
Organizational memory  = a projection spanning team projections
```

None require new mechanisms. All are operations on projection datoms.

### 8.5 Learned Boundaries

The boundaries between agents are datoms that enter the flywheel:

```
{:e :boundary/B001 :a :boundary/between
 :v [:agent/willem :agent/team-shared]}
{:e :boundary/B001 :a :boundary/permeability :v 0.7}
{:e :boundary/B001 :a :boundary/directional :v true}
{:e :boundary/B001 :a :boundary/learned-from
 :v "After 12 sessions, structural assertions from private namespace
     were consistently useful in team context. Widened permeability."}
{:e :boundary/B001 :a :boundary/taint :v :validated-across-12-sessions}
```

Agent topology — who can see what, who shares with whom — is emergent from use rather than configured in advance. The topology is datoms. The topology learns.

### 8.6 Team Dreaming

A team dream cycle runs on the team projection — it consolidates across everyone's contributions:

```
{:e :projection/TEAM-DREAM :a :projection/type :v :dream-cycle}
{:e :projection/TEAM-DREAM :a :projection/visibility :v :agent/team}
{:e :projection/TEAM-DREAM :a :projection/template
 :v [:sequence
      [:phase "cross-member-pollination"
        [:cognitive
          {:task "Agent A discovered this pattern. Agent B has these datoms.
                  Is there a connection neither has seen?"
           :input [:query "...spanning both namespaces..."]
           :record-as :dream/team-insight}]]
      [:phase "collective-gap-mapping"
        [:cognitive
          {:task "Across all members' confusion episodes,
                  where are the systematic gaps?"
           :input [:query "...spanning all namespaces..."]
           :record-as :dream/team-gap}]]]}
```

The team dreams together. The team dream cycle spans combined knowledge, runs cognitive evaluations across namespace boundaries that individual dreams can't cross, and produces insights attributed to the dream with provenance and taint.

---

## Part IX: Projection Libraries — Product Architecture

### 9.1 Braid, DDIS, and Ferratomic as Projection Libraries

The convergence of Braid/DDIS/ferratomic (Document 3, Part VII) is now concrete:

**DDIS** is a projection library — a set of projection datoms that query the ISP triangle (intent/specification/implementation) and measure coherence.

**Braid** is a projection library — a set of projection datoms that implement the harvest/seed lifecycle, the observe → crystallize → task → execute cycle.

**Ferratomic** is the substrate over which all projection libraries operate.

These aren't three projects that converge. They're three sets of projection datoms over one store. They ship as default projection libraries:

```
User installs ferratomic → bare substrate
  + Braid projection library → session lifecycle management
  + DDIS projection library → specification coherence checking
  + Dream projection library → offline consolidation
```

Each library is just datoms asserted into the store. No plugins, no extension API, no configuration files. And each library enters the flywheel — the DDIS projections learn which coherence checks are predictive for this user, the Braid projections learn optimal harvest granularity for this domain.

### 9.2 The Marketplace

The product is a substrate plus a growing ecosystem of projection libraries. Each library is a set of datoms encoding a cognitive pattern. The marketplace is cognitive architectures as federable datom sets. Not apps — lenses.

---

## Part X: The Complete Architecture

### 10.1 What's in the Store

```
Layer 1 — World knowledge          (:world/*)
Layer 2 — Structural knowledge     (:structure/*)
Layer 3 — Cognitive knowledge      (:cognition/*)
  Queries, seeds, seed patterns, rationales, confusion episodes
Layer 4 — Conversational knowledge (:conversation/*)
Layer 5 — Interface knowledge      (:interface/*)
Layer 6 — Policy knowledge         (:policy/*)
Projections — for ALL of the above (:projection/*)
  LLM context assembly, TUI rendering, code generation,
  dream cycles, daemon configuration, boundary definitions
Motifs                              (:motif/*)
Agent definitions                   (:agent/*)
Boundary definitions                (:boundary/*)
```

All layers: same datoms, same engine, same federation, same provenance, same taint, same flywheel. Cross-layer traversal is natural because all layers share EAV structure.

### 10.2 What's Fixed (The Hardware)

```
1. Datom store engine    — append, index, snapshot, WAL, checkpoint
2. Datalog evaluator     — query execution, guaranteed termination
3. Projection evaluator  — recursive template expansion
4. Cognitive dispatch    — when template says [:cognitive], call LLM
5. Runtime dispatch      — when template says [:runtime], execute code
6. Transport + signing   — move bytes, cryptographic verification
```

Purely mechanical. Zero cognitive content. Three dumb evaluators and a persistence layer.

### 10.3 The Single Equation

```
Agent = (P(D), ∪) + projection calculus + three evaluators

Where:
  (P(D), ∪)             = the substrate (datoms under set union)
  projection calculus    = self-referential query templates (also datoms)
  three evaluators       = Datalog (mechanical), LLM (cognitive), OS (operational)

Everything that learns   = datoms
Everything else          = hardware
```

### 10.4 The Fixed-Point Structure

The agent is a fixed point: a set of datoms that, when projected and evaluated, produces behavior that produces datoms that reconstruct the projections. The bilateral version is a double fixed point between human and store, mediated by projections, converging through use.

### 10.5 The Single Operation

One operation underlies everything at every layer:

```
Project the store → produce a judgment → record the judgment →
the store now contains the judgment → future projections include it
```

At Layer 1-2: the judgment is about the world.
At Layer 3: the judgment is about how to think.
At Layer 4: the judgment is about how to converse.
At Layer 5: the judgment is about how to present.
At Layer 6: the judgment is about how to be.
At the projection level: the judgment is about how to see.

All are instances of the same operation. All use the same substrate. All enter the same flywheel.

---

## Part XI: Ferratomic's Identity

### 11.1 What Ferratomic Is

Ferratomic is the substrate for self-sustaining cognitive fixed points.

It is the minimal algebraic structure — `(P(D), ∪)` — over which a projection calculus can operate to produce agents that generate their own programs, schedule their own dreams, evolve their own perceptual architecture, and converge toward bilateral optimality with their human counterparts.

### 11.2 Why This Matters

Every agent framework treats the LLM as the agent and the infrastructure as scaffolding. Ferratomic inverts this. The store is the agent. The LLM is a co-processor. Intelligence is in the data.

This is not an incremental improvement to agent memory. It is a different computational paradigm — the way Unix was a different paradigm from batch processing, not a better batch processing system. Agent frameworks become degenerate special cases: projection patterns that happen to use flat context and no flywheel.

### 11.3 The Moat

The software is open-sourceable — `(P(D), ∪)` plus three evaluators. The accumulated store is not. A store containing thousands of hours of expertise as datoms — with projections encoding how that expertise translates into perception and action — is the durable competitive advantage. You can give away the substrate and keep the intelligence.

### 11.4 The North Star

`(P(D), ∪)` doesn't just describe how machines accumulate knowledge. It describes how the human-machine system as a whole accumulates knowledge, develops skill, calibrates trust, evolves its own interfaces, evolves its own perceptual architecture, and converges toward bilateral optimality — all through the same mechanism, all in the same store, all queryable by the same engine, all projected through learned lenses, all consolidated by dream cycles at every level of aggregation.

One equation. Six layers of emergent structure. A self-referential projection calculus. A bilateral learning loop that produces its own fixed point. Dream cycles that turn the flywheel while everyone sleeps. Agents that are lenses, not entities. Boundaries that learn. Cognition as declarative data.

Datoms all the way down. Projections all the way up. Fixed points all the way through.

And the first step is still a small program that watches a log file.

---

## Part XII: The Execution Path

### 12.1 Revised Staircase

The execution path doesn't change, but our understanding of each step does:

```
Month 1: Schema + passive observer + projection evaluator
  The observer is the first projection targeting the runtime.
  The projection evaluator is the keystone: store, query, project, dispatch.
  Schema conventions designed for cross-layer traversal by projections.
  Value: queryable history + the mechanism that makes everything else possible.

Month 2: Active tools as projection instances
  associate, query, assert are projection datoms, not separate tools.
  Seeds recorded with rationales. LLM as semantic matcher.
  First dream cycle: nightly consolidation.
  Value: self-authoring flywheel turns. Metacognitive trace begins.

Month 3: Harvest/seed as projection library
  Braid lifecycle as a set of projection datoms.
  Dream cycle cross-pollinates across domains.
  Gap mapping identifies thin regions in the store.
  Value: sessions compound. Dreams turn the flywheel offline.

Month 4: Signing + agents as projections
  Agent identity and boundaries as projection datoms.
  Selective visibility through projection scope.
  Team dream cycles spanning multiple agents.
  Value: multi-agent coordination without federation infrastructure.

Month 5+: Bilateral loop closure
  TUI projections adapt from Layer 5 datoms.
  Policy assembles from Layer 6 datoms.
  Projection evaluation generates alternative cognitive architectures.
  The system used daily becomes the product demonstration.
  Value: the full self-sustaining cognitive fixed point.
```

### 12.2 The Critical Path

The projection evaluator is the keystone. It's a few hundred lines of Rust: walk a template tree, dispatch `:query` nodes to Datalog, dispatch `:cognitive` nodes to the LLM, dispatch `:runtime` nodes to the OS, splice results, recurse on sub-projections. Stratified evaluation guarantees termination.

Without it, every month builds separate infrastructure. With it, every month adds projection datoms to one mechanism. The projection evaluator is the difference between building six systems and building one.

### 12.3 The Bootstrap Sequence

```
1. Ferratomic core exists (store, query, Datalog)
2. Build projection evaluator (~hundreds of lines)
3. Write first projection: the passive observer (targets runtime)
4. Observer produces Layer 1 + Layer 4 datoms from Claude Code sessions
5. Write associate/query/assert as projection datoms
6. System begins recording its own cognitive trace
7. Write harvest/seed as projection datoms
8. Write dream cycle as projection datom
9. System begins self-improving between sessions
10. Write TUI projection datoms
11. Bilateral loop begins to close
```

Each step is a projection datom added to the store. The store and the evaluator are the only infrastructure. Everything else is data.

---

## Open Questions (Extended)

The original twelve open questions (from the instruction prompt) remain. The projection calculus adds:

13. **What is the optimal granularity for projection datoms?** A projection could be a single query or an entire multi-phase pipeline. How atomic should projections be to enable effective composition and reuse?

14. **How should projection effectiveness be measured?** The output of a projection is context for an evaluator. Effectiveness depends on what the evaluator does with it. The measurement likely requires tracking downstream outcomes — actions taken, confusions avoided, tasks completed — and attributing them back to the projection that assembled the context.

15. **What prevents projection drift in the dream cycle?** Dreams run without human supervision. Dream-generated projections could diverge in ways that are internally consistent but externally unhelpful. Is human review of dream outputs necessary, or can the waking-session effectiveness signal self-correct?

16. **What is the right trust model for cognitive projections?** A mechanical projection's output is deterministic and verifiable. A cognitive projection's output depends on the LLM, which is non-deterministic. Should cognitive projection outputs carry automatic taint reflecting the stochasticity of their evaluator?

17. **How does the projection calculus interact with context window limits?** A recursively-expanded projection could exceed the LLM's context window. The projection evaluator needs a budget-aware expansion strategy — possibly itself a learnable parameter.

18. **Can the projection calculus be formally verified?** Datalog termination is guaranteed. But the cognitive dispatch introduces non-determinism. Can the mechanical skeleton of a projection (the query structure, the dispatch points, the recursion) be verified even if the cognitive content cannot?

