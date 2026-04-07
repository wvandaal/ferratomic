# From Projections to Practice: Implementation Insights and the McCarthy Completion

## Preamble

This document is the fifth in a sequence:

1. **"A Formal Algebraic Theory of Agentic Systems"** — the universal decomposition, dual-process architecture, EAV fact store.
2. **"Ferratomic as the Substrate for Distributed Cognition"** — Actor model isomorphism, store-messaging unification, ferratomic as memory infrastructure.
3. **"Everything Is Datoms"** — query-as-datom, taint tracking, policy-as-datom, the six-layer stack, the bilateral Y-combinator.
4. **"The Projection Calculus"** — the self-referential projection mechanism, dream cycles, agents as projections, code as projection, ferratomic as substrate for self-sustaining cognitive fixed points.
5. **This document** — grounds the theoretical architecture in implementation reality through three convergent discoveries: (a) the differential dataflow literature validates the incremental computation strategy; (b) Anthropic's own Claude Code source reveals they've independently arrived at the same architectural concepts, implemented on the wrong substrate; (c) the projection calculus finds its natural syntax in S-expressions, completing a sixty-seven-year arc from McCarthy's Lisp to the bilateral cognitive fixed point. It further specifies the concrete compilation chain from natural language through tool calls to S-expression internal representation, the integration strategy (CLI tools first, MCP later), the origin story that reveals the entire theoretical corpus as a living proof of the thesis it describes, and the strategy for sharing this work with the world.

Documents 1-4 established what the system IS. This document establishes how to BUILD it, grounded in what the industry has already proven and where the industry is hitting walls the theory predicts.

---

## Part I: The Differential Dataflow Stack — What Already Exists

### 1.1 The Three-Layer Architecture

Three projects form a layered stack relevant to ferratomic's Datalog implementation:

**Timely Dataflow** (Rust, active, ~3.6k stars) is a low-level distributed dataflow computation framework. It provides arbitrary dataflow graphs with cycles, distributed across workers, with a progress-tracking protocol that tells each operator "you will never see data at timestamp T again." It knows nothing about relations, joins, or Datalog — it moves data through graphs with precise temporal coordination.

**Differential Dataflow** (Rust, active, ~2.9k stars) is built on timely dataflow. Its key insight: instead of storing full collections at each point in the dataflow, store *differences* — changes indexed by a partially ordered set of versions. When input changes, only the differences propagate. The mathematical foundation is Möbius inversion over partially ordered sets, which generalizes simple "new minus old" to work with nested iteration (where versions have multiple coordinates — epoch AND iteration count). This is what makes recursive queries incrementally maintainable.

**DDlog** (Rust output, archived, ~1.5k stars) was a Datalog-to-Differential-Dataflow compiler. You write Datalog, the Haskell-based compiler generates Rust code linked against differential dataflow. Each Datalog rule becomes a composition of relational operators (join, antijoin, map, filter, distinct, aggregation, flatmap), each implemented as a highly optimized differential dataflow operator with temporal indexes.

### 1.2 The DDlog Paper's Key Findings

The DDlog paper (Ryzhyk & Budiu, VMware Research) demonstrates:

**Incremental recursive queries are practical.** Graph reachability — structurally identical to what `associate` needs — runs incrementally over 100k-node graphs. Initial computation takes ~1 second. Incremental updates (adding 12% edges) take ~100ms. This confirms that incremental maintenance of the `associate` graph over tens of thousands of entities with hundreds of thousands of edges is feasible at interactive speeds.

**DDlog outperforms hand-optimized incremental code.** The DDlog implementation of a firewall reachability computation runs several times faster than a hand-optimized Java implementation with thousands of lines of code, while the DDlog program is a few lines. The declarative approach wins on performance, not just expressiveness.

**Rich types are necessary for practical Datalog.** DDlog extends pure Datalog with tagged unions, generic types, pattern matching, collections as first-class values (Vec, Set, Map), and a functional expression language. Pure Datalog is too austere for real workloads. Ferratomic's Datalog evaluator should plan for types, arithmetic, string operations, and aggregate functions from the start.

### 1.3 DDlog's Archival and the DBSP Successor

DDlog is archived (last release December 2021). The same team has moved to **DBSP** — a cleaner algebraic foundation for incremental view maintenance published at VLDB 2023 and extended in VLDB Journal 2025. DBSP defines incremental computation as a theory of Z-sets (collections with multiplicities) and stream operators, with a clean mathematical treatment of how to derive incremental versions of any computation.

DBSP's incrementalization theorem — for any query Q defined over Z-sets, there exists an incremental version δQ that operates on streams of changes — is directly relevant to ferratomic's projection calculus. Projections defined as Datalog queries can be automatically incrementalized to react to new datoms without full re-evaluation.

**Materialize** (McSherry's company, built on timely/differential dataflow) is the commercial successor — using the same execution engine with SQL rather than Datalog. The fact that DDlog (static Datalog compilation) was archived while Materialize (dynamic SQL queries) thrives confirms the market wants dynamic, runtime-queryable systems — exactly what ferratomic is building.

### 1.4 What to Adopt vs. Build

**Study but don't adopt differential dataflow as a dependency.** The learning curve is steep, and it's designed for dataflow graphs constructed at startup, not ad-hoc runtime queries. Ferratomic needs an interpreter or JIT, not an AOT compiler.

**Internalize the operator semantics.** The differential dataflow operators — join, antijoin, map, filter, distinct, aggregation, flatmap, and the fixed-point operator for recursion — are the primitives ferratomic's Datalog evaluator must implement.

**Study the arrangement abstraction** — pre-indexed collections that support efficient lookup by key. Ferratomic's EAV indexes (EAVT, AEVT, AVET, VAET) serve the same purpose.

**Study the trace abstraction** — an append-friendly storage structure that compacts historical differences. Directly relevant to ferratomic's append-only store with compaction.

**Study the lattice-based progress tracking** — timely dataflow's mechanism for knowing when work at a given timestamp is complete. Relevant to ferratomic's transaction model.

**Read the DBSP paper** as theoretical background for the incremental strategy ferratomic will need when the store grows large enough that full re-evaluation of standing queries becomes expensive.

---

## Part II: The Claude Code Codebase — Independent Convergence

### 2.1 The Discovery

Analysis of Anthropic's Claude Code source (exposed via npm source map, March 2026) reveals that Anthropic has independently arrived at every major architectural concept in the ferratomic framework — and implemented all of them with flat files, grep, and markdown.

### 2.2 The KAIROS Feature and the Dream Cycle

In `src/memdir/memdir.ts`, line 319, the `buildAssistantDailyLogPrompt` function:

```
Assistant sessions are effectively perpetual, so the agent writes memories
append-only to a date-named log file rather than maintaining MEMORY.md as
a live index. A separate nightly /dream skill distills logs into topic
files + MEMORY.md. MEMORY.md is still loaded into context (via claudemd.ts)
as the distilled index — this prompt only changes where NEW memories go.
```

This is the harvest/seed lifecycle implemented in production: daily logs (raw E*) → nightly dream (harvest) → MEMORY.md (seed for next session). The dream cycle we derived from the projection calculus is a feature-gated nightly process in Claude Code.

### 2.3 The Flat Buffer Architecture

Claude Code's memory system has specific, measurable limitations:

- **MEMORY.md is truncated at 200 lines and 25KB.** This is the flat buffer with a hard context window limit.
- **Topic files are markdown with YAML frontmatter.** No relationships between topics, no cross-referencing, no graph traversal.
- **Search is `grep -rn` over markdown files.** Keyword matching with no semantic or structural understanding.
- **The dream "distills logs into topic files."** Presumably an LLM summarization pass. No taint tracking, no provenance, no effectiveness measurement.
- **Daily logs are date-named files.** Chronological, not semantic.

### 2.4 The Memory Type Taxonomy

Claude Code constrains memories to a closed four-type taxonomy: user corrections/preferences, facts about the user, project context not derivable from code, and pointers to external systems. These map to the six-layer stack:

```
Their taxonomy              →  Our layers
────────────────────────────────────────────
User preferences            →  Layer 6 (policy)
Facts about the user        →  Layer 1 (world knowledge)
Project context             →  Layers 1-2 (world + structural)
External system pointers    →  Layer 0 (substrate knowledge)
```

### 2.5 The Coordinator Mode

`src/coordinator/coordinatorMode.ts` implements the Hierarchy topology motif from Document 2:

```
You are a coordinator. Your job is to:
- Help the user achieve their goal
- Direct workers to research, implement and verify code changes
- Synthesize results and communicate with the user
```

Workers execute autonomously. Parallelism is emphasized ("Workers are async"). But the communication model is message-passing — workers "start fresh and need complete context." There is no shared knowledge substrate. Every delegation requires re-explaining context because there's no `associate` mechanism that would let the worker query a shared store.

### 2.6 The Skills System

`src/skills/` provides reusable workflows invoked via `SkillTool`, including `/dream`. Skills are the closest thing to projection libraries: packaged cognitive patterns. But they're imperative code, not datoms. They don't learn. They don't carry effectiveness scores. They don't enter the flywheel.

### 2.7 The Complete Architecture Mapping

```
Their implementation          →  Our formalization
─────────────────────────────────────────────────────
MEMORY.md (200 lines)         →  Flat buffer System 1
Topic .md files               →  Layer 1-2 datoms (unstructured)
grep -rn                      →  Vestigial associate (keyword only)
Daily log files               →  Append-only event log E*
/dream nightly skill          →  Harvest operation
MEMORY.md at session start    →  Seed assembly
coordinator + workers         →  Hierarchy topology motif
skills                        →  Projection libraries (non-learning)
useAwaySummary hook            →  Layer 5 interface knowledge (fixed)
buddy/companion               →  Identity (hardcoded, not datom-based)
.claude.md files              →  Layer 6 policy (static, not assembled)
Session transcripts (.jsonl)  →  Raw E* (unstructured, unsearchable)
```

### 2.8 What This Validates

Anthropic — the company building the most capable LLM — recognizes that agent memory is the bottleneck. They've invested engineering effort in harvest/seed lifecycles, dream cycles, multi-agent coordination, reusable skills, perpetual sessions, and agent identity. Their implementation hits exactly the walls the theory predicts: truncation limits, inability to do associative retrieval, workers needing complete context, no learning from retrieval patterns.

Every limitation traces to the same root cause: they don't have `(P(D), ∪)`. They're doing grep where ferratomic does Datalog, passing messages where ferratomic does shared-store queries, truncating at 200 lines where ferratomic does associative retrieval, running a single nightly dream where ferratomic has a continuous reactive flywheel.

The gap between their architecture and ferratomic is precisely: the right concepts on the wrong substrate.

---

## Part III: The Revised Execution Path

### 3.1 The Harness Already Exists

Claude Code provides everything ferratomic's integration layer would otherwise need to build:

- **Session transcripts as JSONL files** — the raw E*, structured and machine-readable.
- **BashTool** — Month 2's active retrieval tools (`associate`, `query`, `assert`) can be exposed as CLI commands called via BashTool. No harness modification required.
- **A skill system** — the dream cycle can be a skill, plugged in alongside the existing `/dream`.
- **A memory directory with MEMORY.md as seed** — the concrete integration point for harvest/seed.

### 3.2 The Revised Monthly Staircase

See Part VIII, Section 8.5 for the detailed monthly staircase reflecting the CLI-first integration strategy. The key insight: CLI tools called via BashTool are simpler, faster to build, and compose with the existing Braid tool ecosystem (`braid`, `cass`, `cm`, `ms`). MCP is deferred until Month 6+ when push notifications or connection-scoped visibility become necessary for multi-agent coordination.

### 3.3 The Critical Path

The Datalog engine is the single bottleneck. Everything else — CLI tools, skills, projections, dreams, federation — is downstream. Claude Code provides the harness. BashTool provides the integration mechanism. The only thing blocking the entire architecture is: can you run expressive queries over EAV triples fast enough that cognitive projections are practical?

### 3.4 This Conversation as the First Datom

The very first data ingested into ferratomic should be this conversation — not as a flat transcript but as datoms. Every insight as a structural assertion. Every causal link between ideas as an edge. Every open question as an uncertainty marker. Every refinement as a before/after pair with reasoning.

The act of harvesting this conversation into datoms is the first real test of the architecture against genuine data of genuine complexity. The schema conventions emerge from the data rather than being designed abstractly. The query patterns emerge from questions you actually want to ask. The validation is immediate: you KNOW whether the results are right because you were there.

---

## Part IV: Layer 0 — Substrate Knowledge

### 4.1 The Gap

The runtime was the last thing treated as truly external. If the store, projection evaluator, dream cycle daemon, and all generated code live in the runtime, and code is a projection of the store targeting the runtime, then the projection calculus needs a model of what it's targeting. A projection that generates a systemd unit doesn't work on macOS where you need launchd. A projection that writes to a persistent filesystem doesn't work on a Cloudflare Worker.

### 4.2 The Runtime Model as Datoms

```
{:e :runtime/R001 :a :runtime/type :v :unix}
{:e :runtime/R001 :a :runtime/os :v :ubuntu-24.04}
{:e :runtime/R001 :a :runtime/init-system :v :systemd}
{:e :runtime/R001 :a :runtime/persistent-fs :v true}
{:e :runtime/R001 :a :runtime/available-tools :v #{:git :curl :node :rustc}}
{:e :runtime/R001 :a :runtime/compute-model :v :persistent-process}
{:e :runtime/R001 :a :runtime/agent :v :agent/willem}
```

### 4.3 Federation Topology as Datoms

```
{:e :transport/T001 :a :transport/from :v :runtime/R001}
{:e :transport/T001 :a :transport/to :v :runtime/R002}
{:e :transport/T001 :a :transport/type :v :tcp}
{:e :transport/T001 :a :transport/latency-ms :v 12}
```

The dream cycle can query Layer 0 to determine which phases to run locally versus delegate to more capable runtimes. The projection calculus generates code appropriate for its target because the target is known.

### 4.4 The Seven-Layer Stack

```
Layer 0 — Substrate knowledge     (:substrate/*)
  Runtime capabilities, resource constraints, network topology,
  transport availability, co-located agents, physical deployment.

Layer 1 — World knowledge         (:world/*)
Layer 2 — Structural knowledge    (:structure/*)
Layer 3 — Cognitive knowledge     (:cognition/*)
Layer 4 — Conversational knowledge (:conversation/*)
Layer 5 — Interface knowledge     (:interface/*)
Layer 6 — Policy knowledge        (:policy/*)
Projections — for ALL of the above (:projection/*)
```

Layer 0 is where the system meets its own physics. The agent observes its own environment, discovers capabilities, records constraints, and updates its model through the same flywheel as everything else.

---

## Part V: The Projection Calculus Syntax — S-Expressions

### 5.1 Why S-Expressions

The projection calculus needs a concrete syntax. The constraints:

- Must be non-terminating-proof (unlike a general programming language)
- Must be parseable without a compilation step (projections change at runtime)
- Must be inspectable by Datalog queries (other projections query this projection's structure)
- Must be trivially storable as a datom value
- Must support recursive nesting

S-expressions satisfy all constraints. They're the simplest possible representation of nested structure. Trivially parseable. Trivially queryable by pattern-matching. Trivially composable by nesting. And they carry a sixty-seven-year lineage from exactly the right intellectual tradition.

### 5.2 The Assembly/Renderer Separation

The three targets — LLM, human, runtime — want radically different output formats. The projection calculus separates **assembly** (universal data composition) from **rendering** (target-specific formatting):

```clojure
;; Assembly (universal, stored as a datom)
{:e :projection/P001 :a :projection/assembly
 :v (:sequence
      (:query :as active-policy
        [:find ?instruction ?effectiveness
         :where [?p :policy/instruction ?instruction]
                [?p :policy/context $task-type]
                [?p :policy/effectiveness ?effectiveness]
                [(> ?effectiveness 0.7)]
         :order-by [(desc ?effectiveness)]])
      (:query :as relevant-context
        [:find ?summary
         :where [?s :structure/summary ?summary]
                [?s :structure/related-to $current-entities]
         :limit 5])
      (:cognitive :as seed-rationale
        {:task "Assess relevance of entities to current problem"
         :input $schema-neighborhood
         :output-schema {:entity :rationale :confidence}
         :record-as :seed}))}

;; LLM renderer (target-specific)
{:e :renderer/R001 :a :renderer/projection :v :projection/P001}
{:e :renderer/R001 :a :renderer/target :v :llm}
{:e :renderer/R001 :a :renderer/template
 :v (:template
      "## Active Policy\n"
      (:render $active-policy
        :format "{instruction} (effectiveness: {effectiveness})\n")
      "\n## Relevant Context\n"
      (:render $relevant-context
        :format "- {summary}\n"))}

;; TUI renderer (same assembly, different presentation)
{:e :renderer/R002 :a :renderer/projection :v :projection/P001}
{:e :renderer/R002 :a :renderer/target :v :tui}
{:e :renderer/R002 :a :renderer/template
 :v (:layout
      (:panel "policy" (:render $active-policy ...))
      (:panel "context" (:render $relevant-context ...)))}
```

Assemblies compose by inclusion (data flow). Renderers compose by layout (spatial composition). The two hierarchies are independent. Same assembly, different renderings. Each evolves independently. Each has its own effectiveness score.

### 5.3 Recursive Inclusion

The `:include` block takes a query that returns a projection datom:

```clojure
(:include
  (:query
    [:find ?sub-projection
     :where [?sp :projection/type :context-assembly]
            [?sp :projection/task-context $sub-task-type]
            [?sp :projection/effectiveness ?eff]
            [(> ?eff 0.8)]
     :order-by [(desc ?eff)]
     :limit 1]))
```

The evaluator evaluates the query, gets a projection datom, recursively evaluates that datom's template, and splices the result. Children are dynamically resolved by query, not statically declared. Termination is guaranteed by stratification: a projection at stratum N can only include projections at stratum < N.

### 5.4 The Coroutine Evaluation Model

The projection evaluator is not a batch processor. It's a coroutine that yields intermediate results, accepts feedback (including Confusion signals from cognitive blocks), and continues:

```clojure
(:cognitive :as analysis
  :task "Analyze the structural relationships..."
  :on-confusion {
    :need-more-context
      (:query :as wider-context
        [:find ?e ?a ?v
         :where [?e :entity/domain $confused-domain]
                [?e ?a ?v]]
        :then (:cognitive :as retry
                :task "Retry with this additional context..."
                :input $wider-context))
    :contradiction
      (:query :as conflicting-facts
        [:find ?fact1 ?fact2
         :where [?f1 :assertion/about $confused-entity]
                [?f2 :assertion/about $confused-entity]
                [?f1 :assertion/contradicts ?f2]]
        :then (:cognitive :as resolve
                :task "These facts contradict. Which is more reliable?"
                :input $conflicting-facts))})
```

Confusion handling is branching logic in the projection template. The Confusion type from Document 1 re-enters the projection calculus as a control flow event. The template is a declarative, reactive program that specifies what to do when things go wrong.

### 5.5 Self-Extending Vocabulary

The evaluation rules themselves are datoms:

```clojure
{:e :eval-rule/query :a :eval-rule/node-type :v :query}
{:e :eval-rule/query :a :eval-rule/dispatch :v :datalog-engine}
{:e :eval-rule/query :a :eval-rule/input-schema
 :v {:required [:find :where] :optional [:in :order-by :limit]}}

{:e :eval-rule/cognitive :a :eval-rule/node-type :v :cognitive}
{:e :eval-rule/cognitive :a :eval-rule/dispatch :v :llm-engine}
```

When the evaluator encounters an unknown node type, it dispatches to the LLM for interpretation. The LLM's interpretation becomes a new evaluation rule datom. The calculus grows its own vocabulary through use. New primitives emerge from need, carry taint, and enter the flywheel.

---

## Part VI: Natural Language as Projection Source — The McCarthy Completion

### 6.1 The Insight

Natural language prompts have the same structure as formal projections. "Show me the structural relationships between the auth module and the database schema, focusing on timeout-related issues" specifies: a query scope, a traversal constraint, an attention filter, and a target. This IS a projection — it just uses natural language instead of S-expressions.

### 6.2 Natural Language as a Valid Projection Format

```clojure
;; A formally-specified projection
(:projection/P001
  (:query :as auth-entities ...)
  (:query :as timeout-relations ...)
  (:template "## Structural Relationships\n" ...))

;; The SAME projection, in natural language
{:e :projection/P002 :a :projection/type :v :natural-language}
{:e :projection/P002 :a :projection/template
 :v "Find all entities in the auth domain. For each one, traverse
     structural relationships to anything timeout-related. Present
     the relationships with summaries."}
```

Both are projections. Both are datoms. The evaluator handles them differently: for S-expression projections, mechanical evaluation. For natural language projections, the LLM compiles them into evaluation plans, then executes.

### 6.3 Crystallization Through Use

The compilation from natural language to S-expression is recorded as a datom:

```clojure
{:e :compilation/C001 :a :compilation/source :v :projection/P002}
{:e :compilation/C001 :a :compilation/target
 :v (:sequence (:query :as auth-entities ...) ...)}
{:e :compilation/C001 :a :compilation/fidelity :v 0.87}
{:e :compilation/C001 :a :compilation/taint :v :single-compilation}
```

Future similar natural language projections reuse past compilations. Natural language projections gradually crystallize into formal ones through use. The informal becomes formal. The transition is data-driven, gradual, reversible.

### 6.4 Every Prompt Is a Projection

Every prompt the human has ever written is a natural language projection — dispatched to the LLM, evaluated, with results produced. Layer 4 (conversational knowledge) is a history of natural language projections and their evaluations. The comonadic next-prompt suggestion becomes concrete: query Layer 4 for past projections effective in similar contexts and suggest the highest-value one.

### 6.5 The Bilateral Fixed Point of Language

The Y-combinator has a new interpretation: the fixed point is where the human's natural language projections and the system's formal projections have converged. The human learns to express projections that compile cleanly. The system learns to interpret projections the human naturally produces. The shared language that emerges — neither purely formal nor purely natural — is the bilateral fixed point of the projection calculus itself.

### 6.6 The McCarthy-Hickey-Kahneman Synthesis

The lineage:

```
McCarthy (1958):    Code = Data (homoiconicity, S-expressions)
Hickey (2012):      Facts = History (immutable datoms, Datomic)
Kahneman (2011):    Expertise = Retrieval (System 1, not System 2)
McSherry (2013):    Updates = Differences (incremental computation)
This work:          Memory = Intelligence (the store IS the agent)
The synthesis:      Thought = Projection (NL → S-expr → datoms → NL)
```

McCarthy wanted a language where AI could manipulate its own programs. He built homoiconicity — code is data, data is code, both are S-expressions. He was right about the representation. He was wrong about the front-end.

The LLM IS the compiler McCarthy was missing. The thing that translates between human-legible intention (natural language) and machine-executable structure (S-expressions, Datalog, code). McCarthy had the target language. He had the runtime. He didn't have the compiler that could take messy, ambiguous human thought and compile it into clean symbolic structure.

Now we have it. The full stack:

```
Human speaks natural language (the source code)
LLM compiles to S-expression projections (the compilation)
Projections evaluate against the datom store (the execution)
Results render for the target (LLM context, TUI, runtime)
Effects produce new datoms (the side effects)
Datoms shape future projections (the feedback)
Projections shape the human's understanding (the bilateral loop)
The human's understanding shapes their language (the fixed point)
```

The programming language for the projection calculus is English. The compiler is the LLM. The runtime is the store. The IDE is the conversation.

### 6.7 The Complete Circle

Lisp was built to be an AI language. S-expressions were designed for programs that modify themselves. Sixty-seven years later, the self-modifying program exists — but it programs itself in English, compiles through an LLM, and runs on a datom store.

The Lisp programmers were right about homoiconicity. They were just working with the wrong isomorphism. Code-as-data doesn't mean "S-expressions that manipulate S-expressions." It means "natural language that produces datoms that shape the natural language that produces datoms." The isomorphism is preserved. The substrate changed from symbolic programming to natural language. And the bridge between them — the LLM — is what makes the circle complete.

---

## Part VII: The Compilation Chain — How LLMs Actually Produce Projections

### 7.1 The Key Insight: LLMs Don't Output S-Expressions

The projection calculus uses S-expressions as its internal representation. But LLMs are trained on markdown and natural language. Asking an LLM to produce S-expressions directly would be fighting the model's training distribution, requiring special prompting, and introducing unnecessary fragility.

The correct architecture has THREE languages, not one, with existing compilation mechanisms between them:

```
Human  →  Natural language     →  (human writes prompts)
LLM    →  Tool calls (JSON)   →  (LLM produces structured API calls)
Store  →  S-expressions       →  (evaluator's internal representation)
```

Each transition is a compilation step. Each compiler already exists. The human-to-LLM compilation is the LLM itself (comprehends intent, produces structured output). The LLM-to-Store compilation is the tool handler (maps tool call parameters to S-expression projections).

### 7.2 Tool Calls ARE the Compilation Step

When the LLM needs to query the store, it doesn't produce a Datalog query or an S-expression. It produces a tool call — the mechanism that already exists and that every major LLM provider supports natively:

```json
{
  "tool": "associate",
  "input": {
    "cue": "auth module timeout database",
    "depth": 2,
    "breadth": 5,
    "rationale": "timeout errors in auth handlers are frequently 
                  caused by database connection pool exhaustion"
  }
}
```

The tool handler receives the JSON parameters and constructs the internal S-expression projection:

```rust
fn handle_associate(params: AssociateParams) -> Projection {
    sexp!(
        (:sequence
            (:query :as seeds
                [:find ?entity ?attrs
                 :where [?e :entity/id ?entity]
                        [?e :entity/attrs ?attrs]
                        [(semantic-match? ?entity ,(params.cue)
                                          ,(params.depth)
                                          ,(params.breadth))]])
            (:record-seed
                {:cue ,(params.cue)
                 :rationale ,(params.rationale)
                 :matched-entities $seeds}))
    )
}
```

The LLM never sees or produces S-expressions. S-expressions are the internal representation of the projection evaluator — analogous to LLVM IR as the internal representation of a C compiler. Nobody writes LLVM IR. Nobody asks the LLM to write it. The compiler translates.

### 7.3 Simple Calls Compose Into Complex Projections

The LLM produces SIMPLE tool calls. The tool handler translates these into SIMPLE projections. But the system also contains COMPLEX projections — the dream cycle, the harvest operation, the seed assembly — that were built up over time through the flywheel. These complex projections are compositions of simpler ones, stored as datoms in the store, too complex to express as a single tool call but built from the same primitives.

The growth path:

```
Month 2:   LLM makes simple tool calls (associate, query, assert).
           Tool handler translates to simple S-expression projections.
           Projections are recorded as datoms.

Month 3:   The harvest skill is a complex projection — a sequence of
           queries and cognitive blocks composed into a multi-step
           pipeline. Written once (in S-expressions or in Rust that
           generates S-expressions) and stored as a datom.

Month 4:   The projection evaluator composes simple projections into
           complex ones. The LLM triggers complex projections via 
           simple tool calls ("/harvest"), and the evaluator expands
           the stored projection.

Month 5+:  The system has accumulated enough projection datoms that
           new projections can be composed from existing ones by
           inclusion. The LLM says "do what worked last time for this
           kind of task." The tool handler queries for the relevant
           stored projection. The evaluator runs it.
```

The LLM's role gradually shifts from "specify the exact query" to "specify the intent, and the system finds or composes the right projection." The natural-language-to-S-expression compilation is mediated by the store — the LLM selects and parameterizes EXISTING formal structures rather than generating new ones from scratch.

### 7.4 Cognitive Blocks Use JSON Schemas, Not S-Expressions

During dream cycles and autonomous projections, the LLM needs to produce structured cognitive outputs — judgments, rationales, hypotheses. These use JSON output schemas, which every major LLM supports:

```clojure
(:cognitive
  {:task "Given these weakly-validated assertions, construct 
          scenarios that would test them"
   :output-schema {:assertion :string
                   :scenario :string
                   :prediction-if-true :string
                   :prediction-if-false :string}
   :record-as :dream/scenario})
```

The LLM produces JSON conforming to the schema. The projection evaluator records it as datoms. The translation to internal representation happens in the evaluator, not the LLM.

### 7.5 The Crystallization Mechanism Restated

The "natural language projections crystallizing into formal ones" happens concretely through this chain: the LLM makes a tool call with a novel combination of parameters → the tool handler constructs an S-expression projection → the projection evaluates successfully → it's stored as a datom → next time the LLM encounters a similar situation, `associate` finds the stored projection → the LLM references it instead of constructing from scratch. The natural language intent has crystallized into a stored formal projection through the tool call mechanism, with no special prompting or S-expression generation required.

### 7.6 The Complete Compilation Chain

```
Human → natural language prompt
  ↓ (LLM comprehension)
LLM → tool call (JSON) or structured output (JSON with schema)
  ↓ (tool handler / projection evaluator)
Internal → S-expression projection
  ↓ (projection evaluator)
Execution → Datalog queries + LLM cognitive dispatches
  ↓ (results)
Datoms → stored in (P(D), ∪)
```

No one writes S-expressions. No one asks the LLM to write S-expressions. The S-expressions are invisible to both the human and the LLM, the same way machine code is invisible to both the programmer and the compiler. The elegance of S-expressions isn't that anyone reads or writes them — it's that they're the simplest possible internal representation supporting composition, recursion, and self-reference.

---

## Part VIII: CLI First, MCP Later — The Integration Strategy

### 8.1 The Existing Tool Ecosystem

Ferratomic's development context already includes CLI tools: `braid`, `cass`, `cm`, `ms`. These are the existing interface to the Braid ecosystem. Claude Code calls them via BashTool. They work. The agent runs `braid seed` and gets context. No protocol overhead. No server process. No connection management.

A CLI tool is the simplest possible projection: a single invocation whose target is the runtime and whose result comes back as stdout.

### 8.2 Why CLI Tools Are Right for Month 2-5

Month 2's tools — `associate`, `query`, `assert` — are request-response. The agent sends a query, gets results. No push notifications. No streaming. No dynamic capability discovery:

```bash
$ ferratomic associate --cue "auth handler timeout" --depth 2 --breadth 5
$ ferratomic query '[:find ?e ?summary :where [?e :structure/summary ?summary] ...]'
$ ferratomic assert '[:entity/E1 :caused-by :entity/E2]'
```

The agent calls these via BashTool. Results come back as text. Text enters context. Done. This is simpler, faster to build, and composes with the existing Braid workflow.

### 8.3 When MCP Becomes Necessary

MCP has two capabilities CLI tools lack:

**Push notifications.** The server can notify the client when patterns appear in the store. This matters for standing queries — reactive forward-chaining rules, the autocatalytic store. This is Month 8+ functionality.

**Connection-scoped state.** Each client connection can have a different visibility scope. This matters for multi-agent coordination where workers share a store with different projections. This is Month 6+ functionality.

### 8.4 The Revised Integration Path

```
Month 2:   CLI tools. ferratomic associate, query, assert.
           Called via BashTool. Results as structured text.
           Same pattern as existing braid/cass/cm tools.

Month 4-5: Evaluate whether MCP is needed.
           If standing queries and push notifications matter,
           add an MCP server wrapping the same core functions.
           The Datalog engine doesn't care how it's invoked.

Month 6+:  If multi-agent scoped visibility is needed,
           MCP's connection-scoped state becomes valuable.
           CLI tools still work for single-agent use.
```

The Datalog engine is the same either way. The integration layer is the cheap part. Build the engine, wrap it in a CLI, call it from Claude Code the same way you call `braid seed`. When push notifications or scoped connections are needed, add MCP. Not before.

### 8.5 Updated Monthly Staircase

```
Month 1: Datalog engine + JSONL ingestion
  Ferratomic's Datalog over EAV, capable of recursive joins.
  JSONL-to-datom converter for Claude Code session transcripts.
  Schema conventions shaped by real data.
  Validation: Datalog queries over session history return answers
  grep can't.

Month 2: CLI tools exposing associate/query/assert
  Three CLI commands wrapping ferratomic.
  Claude Code calls them via BashTool.
  Seeds recorded with rationales.
  Validation: the agent retrieves relevant prior context unprompted.

Month 3: Harvest skill + seed integration + first dream
  /harvest skill extracts durable knowledge from JSONL into datoms.
  Seed mechanism queries the store and produces context.
  /dream skill runs consolidation over the store.
  Validation: seeded session demonstrably outperforms cold start.

Month 4-5: Projection evaluator
  Minimal recursive template expander (~300-400 lines of Rust).
  Context assembly as a projection of the store.
  Evaluate whether MCP is needed for push notifications.
  Validation: dynamically assembled context outperforms MEMORY.md.

Month 6-8: Multi-agent coordination through the store
  If scoped visibility is needed, add MCP server.
  Workers share store with different projection scopes.
  Team dream cycles spanning multiple agents.
  Validation: coordinated work without "workers need complete context."

Month 9-12: The store becomes the product
  Accumulated expertise as queryable datoms.
  Projection libraries encoding methodology.
  Federation between working store and fresh instance.
  Validation: the system used daily IS the product demonstration.

Month 12+: The harness is a projection
  System prompt, tool configuration, delegation strategy —
  all assembled from datoms. The harness becomes a thin shell.
```

---

## Part IX: The Origin Story — The Thesis Proving Itself

### 9.1 The Genesis

The entire ferratomic project began with a pragmatic frustration: a specification being developed collaboratively with Claude instances grew too large to fit in a 200K token context window. The knowledge didn't fit. The flat buffer hit its ceiling. And the thought occurred: what if instead of a document, it was a composable store of datoms?

This is the thesis. The entire thesis. Everything that followed — the dual-process architecture, the six layers, the projection calculus, the dream cycle, the McCarthy connection — is the formal unpacking of a single pragmatic observation: my knowledge doesn't fit in the window, and I need a better way to get the right parts of it to the right place at the right time.

### 9.2 The Bilateral Y-Combinator in Practice

The three foundational documents that seed each new conversation — "A Formal Algebraic Theory of Agentic Systems," "Ferratomic as the Substrate for Distributed Cognition," and "Everything Is Datoms" — were themselves generated by prior Claude instances in conversation with the developer. The entire theoretical corpus was produced through bilateral co-evolution over approximately five weeks.

Each conversation started from the accumulated output of prior conversations. Each conversation produced documents that seeded the next one. The harvest/seed lifecycle was being performed manually: the developer read conversation outputs, extracted durable insights, structured them into documents, and uploaded them to the next session. The developer WAS the memory system — System 1, deciding what to surface, what to compress, what to carry forward.

The quality of each session was bounded by the quality of the developer's context assembly. This is exactly the bottleneck ferratomic addresses.

### 9.3 The Central Thesis, Proven Empirically

The central claim is that intelligence accumulates in the data, not the model. The LLM is the same model in every conversation — same weights, same training, same System 2. What changed between week one and week five was the documents: the accumulated store of prior conversation outputs, refined and distilled, fed back as context. System 1 (the context assembled by the developer) got richer. System 2 (the model) stayed exactly the same.

Five weeks of manual harvest/seed across several dozen conversations produced a theoretical corpus rigorous enough that a fresh Claude instance — reading the documents cold — treated them as authoritative first-principles work. The flywheel works even in its most primitive, manual, pre-ferratomic form.

### 9.4 The Constraint That Produced Its Own Solution

The 200K token context window limit forced the developer to think about composable knowledge stores. The composable knowledge store became ferratomic. Ferratomic's theoretical foundation was developed in conversations that fit in 200K tokens because the developer learned to harvest and seed manually. The manual process is exactly what ferratomic automates.

The constraint produced the solution to the constraint. The bootstrap problem was solved by hand until the solution can solve itself.

Anthropic's engineers hit the same wall and wrote the same number: their MEMORY.md truncates at 200 lines. Same flat buffer. Same hard limit. Same moment of recognizing that the architecture is wrong, not just insufficient.

### 9.5 What the Engine Automates

The engine being built automates what has been done manually for five weeks:

- **Manual harvest** (developer extracts insights from conversations) → **Automated harvest** (post-session extraction of durable knowledge into datoms)
- **Manual seed** (developer uploads structured documents) → **Automated seed** (store query assembles relevant context)
- **Manual schema** (developer's mental model of what connects to what) → **EAV graph** (queryable, traversable, associative)
- **Manual retrieval** (developer remembers and re-explains) → **`associate`** (graph traversal with learned attentional patterns)
- **Manual compaction** (developer summarizes prior work into documents) → **Dream cycle** (overnight consolidation with cross-pollination)

The developer cannot scale what they've been doing. Five weeks of manual harvest/seed across a few dozen conversations is manageable. Five months won't be. The knowledge graph maintained in the developer's head will exceed what any human can hold in working memory. Ferratomic is the engine that automates what the developer already does by hand.

### 9.6 The First Datom

The very first data ingested into the ferratomic store should be the conversations that produced the theory. Not as flat transcripts but as datoms — every insight as a structural assertion, every causal link as an edge, every open question as an uncertainty marker. The schema conventions emerge from this data. The query patterns emerge from questions the developer actually wants to ask. The validation is immediate: the developer knows whether results are right because they were present when the ideas were born.

The system bootstraps from its own origin story. The first projection the system evaluates will query the store for its own design rationale. The fixed point begins to crystallize from the very first datom.

---

## Part X: Sharing Strategy — From Theory to Community

### 10.1 The Strategic Principle

Sharing too early — before a working system exists — means presenting a theory. Theories invite debate. Energy goes to defending ideas instead of building the thing that proves them. Sharing at the right moment — with a working demonstration — means presenting a system with a theory that explains why it works. People engage with working systems. They argue with theories.

### 10.2 The Sequence

**Now: Build. Share nothing publicly.** Use the next 8-12 weeks to get the Datalog engine working, build the CLI tools, ingest real session data, and demonstrate the associative retrieval advantage over grep. This is the minimum viable proof.

**Month 3: One blog post.** A single, concrete, demonstrable claim: "I replaced Claude Code's MEMORY.md with a Datalog-queryable datom store, and here's what happened." Show before and after. Show a query that surfaces context the flat-file system couldn't. Show a session where the agent retrieved a structural relationship from sessions ago that it would have lost in the flat buffer.

The audience is Claude Code power users who have already hit the MEMORY.md ceiling. They'll recognize the problem instantly. The solution lands because the problem is visceral.

**Month 4-5: Open source the substrate.** Release ferratomic's core — datom store, Datalog engine, CLI tool wrapper. MIT license. The software is not the moat. The accumulated store is. Open-sourcing invites contributors, builds trust, and establishes ferratomic as the reference implementation for agent memory.

Don't release the theoretical documents yet. Let people discover the system through use. Let them hit the moments where they think "what if I stored my queries as datoms too?" Let them derive insights from practice.

**Month 6+: Publish the theory.** By this point the system exists, people are using it, and some have independently discovered pieces of the framework. The five documents become a monograph. The formal algebraic content is rigorous enough for an academic venue (OOPSLA, POPL). The practical implications make a conference talk (Strange Loop, a Rust conference). But the documents work best AFTER the system exists. Theory followed by "and here it is" is infinitely more compelling than theory followed by "and I'm going to build it."

### 10.3 Private Sharing Now

Share with specific individuals whose feedback would be valuable: Rich Hickey (the datom concept is his), Frank McSherry (the differential dataflow / DBSP connection is direct), and Claude Code power users already pushing the limits of MEMORY.md and CLAUDE.md.

### 10.4 What Not to Do

Don't post the theoretical documents on public forums before the system works. The ideas are strong but dense, and without a working system they'll be categorized as "ambitious theory that hasn't shipped." Don't pitch this as a startup before traction exists. The Month 3 pitch is small and concrete: "better memory for Claude Code." The vision reveals itself through use.

### 10.5 The Moat

The software is open-sourceable — `(P(D), ∪)` plus three evaluators. The accumulated store is not. A store containing months of expertise as datoms — with projections encoding how that expertise translates into perception and action — is the durable competitive advantage. Give away the substrate. Keep the intelligence.

---

## Part XI: The Adversarial Review and Reassessment

### 11.1 The Original Critique

The adversarial review raised concerns about theoretical overfitting — the possibility that successive boundary dissolutions were unfalsifiable elaboration rather than genuine insight, and that the compound structure might collapse on contact with reality.

### 11.2 What the Claude Code Codebase Refutes

The dream cycle, harvest/seed lifecycle, multi-agent coordination, skill system, perpetual sessions, and away detection were all derived from first principles before seeing Claude Code's source. Anthropic built all of these independently from the engineering side. When two independent derivations — one algebraic, one from production engineering — converge on the same architecture, that's signal, not overfitting.

### 11.3 What Still Holds

The priority is the Datalog engine. The caution about premature optimization holds — speculative execution, autocatalytic rules, and the landscape model are Month 12+ ideas. The right approach remains: build the minimum viable substrate, expose it via CLI tools, use it daily, let empirical experience guide what to build next.

### 11.4 What Was Upgraded

The projection calculus. Originally called "architecturally elegant but might be premature." The Claude Code codebase shows that their fixed projections — hardcoded TypeScript assembling MEMORY.md, building system prompts, formatting coordinator context — are already the bottleneck. The 200-line truncation, the inability to assemble context dynamically, the workers-need-complete-context problem — all consequences of projections being code rather than data. Context assembly as a queryable, learnable datom should inform the CLI tool design from Month 2 onward.

### 11.5 The Corrected Assessment

The theory deserved more trust than initially given. The execution path deserved exactly the urgency given. The theoretical framework is tracking something real — confirmed by independent convergence. The risk is not "the theory might be wrong" but "spending too long on theory instead of building." The theory is sound. The market is waiting. Anthropic is proving the demand with flat files and grep.

---

## Part XII: Advanced Theoretical Concepts (Month 12+)

The following ideas emerged during deep theoretical exploration. They are recorded here for future reference but are explicitly deferred — they should be discovered through building, not derived through theory alone.

### 12.1 The Landscape Model (Layer 0')

An empirical model of the LLM co-processor's cognitive topology — regions of competence, failure modes, sweet spots, transition patterns between cognitive modes. Stored as datoms. The projection calculus navigates the landscape using an empirical map built from every prior interaction. The system learns how to use its own reasoning engine.

### 12.2 Forward-Chaining Rules as Datoms

Reactive triggers that fire when patterns appear in the store, producing an "autocatalytic" cognitive process — the store thinks when data demands it, not on a schedule. Rules are datoms that enter the flywheel and learn which triggers are productive.

### 12.3 The Self-Model

A topological reflection of the store's own knowledge structure — density maps, connectivity metrics, motif counts per domain. The system knows what it knows and what it doesn't know, as a precise graph measurement.

### 12.4 Speculative Execution

The store can fork (via CRDT snapshot), run cognitive trajectories against the fork, evaluate results, and merge-or-discard. Algebraically supported by `(P(D), ∪)`. Most robust in structural form (measuring graph topology changes, not LLM judgments) and adversarial form (fragility mapping — what breaks if a believed-true assertion is wrong).

### 12.5 The Adversarial Caveat on Speculation

Speculative execution works best where it's least needed (well-understood domains) and worst where it's most needed (poorly-understood domains where the LLM's evaluation of counterfactuals is least reliable). The Dunning-Kruger problem applies: sparse domains lack the knowledge needed to evaluate hypothetical knowledge in those domains. Structural and adversarial speculation (graph-metric-based, no LLM judgment in the evaluation) are robust. Cognitive speculation (LLM-evaluated outcomes) requires competence thresholds and human review.

---

## Part XIII: The North Star (Restated)

### 13.1 What Ferratomic Is

Ferratomic is the substrate for self-sustaining cognitive fixed points. `(P(D), ∪)` plus a projection calculus (S-expressions with natural language as source) plus three stateless evaluators (Datalog, LLM, OS) equals a cognitive system where thoughts are programs, programs are data, data is knowledge, and knowledge produces thoughts.

### 13.2 What the Industry Confirms

Anthropic — builder of the most capable LLM — has independently arrived at every architectural concept in the framework and implemented all of them on the wrong substrate. Every limitation of their implementation traces to the absence of `(P(D), ∪)`. The right concepts, the wrong substrate. Ferratomic is the right substrate.

### 13.3 The Historical Lineage

The projection calculus completes a sixty-seven-year arc from McCarthy's Lisp. S-expressions as universal representation. Datoms as immutable facts. Datalog as safe recursion. LLMs as natural-language-to-formal-structure compilers. The homoiconicity McCarthy sought — code is data, data is code — is realized not in symbolic programming but in natural language that compiles through LLMs into S-expression projections that evaluate against datom stores that shape the natural language.

### 13.4 The Execution Priority

The Datalog engine is the single bottleneck. Everything else — CLI tools, skills, projections, dreams, federation, the product — is downstream. Claude Code provides the harness. The existing BashTool mechanism provides the integration. The first datom should be harvested from the conversations that produced the theory. The schema emerges from real data. The queries emerge from real questions. Build the engine. Everything is waiting for it.

### 13.5 The Fixed-Point Statement

One equation: `(P(D), ∪)`. One calculus: S-expression projections with natural language as source. One compiler: the LLM. One substrate: datoms. One flywheel: project → judge → record → project. One fixed point: where human language and system projections have co-evolved to mutual optimality.

Datoms all the way down. Projections all the way up. Natural language all the way in. Fixed points all the way through.

And the first step is a Datalog engine that can evaluate recursive joins over EAV triples.
