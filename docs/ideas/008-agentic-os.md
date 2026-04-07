# The Agentic Operating System: From Event-Driven Substrate to Knowledge Workspace

## Preamble

This document is the sixth in a sequence:

1. **"A Formal Algebraic Theory of Agentic Systems"** — the universal decomposition, dual-process architecture, EAV fact store.
2. **"Ferratomic as the Substrate for Distributed Cognition"** — Actor model isomorphism, store-messaging unification, ferratomic as memory infrastructure.
3. **"Everything Is Datoms"** — query-as-datom, taint tracking, policy-as-datom, the six-layer stack, the bilateral Y-combinator.
4. **"The Projection Calculus"** — the self-referential projection mechanism, dream cycles, agents as projections, code as projection, self-sustaining cognitive fixed points.
5. **"From Projections to Practice"** — differential dataflow validation, Claude Code codebase analysis, S-expression syntax, the three-language compilation chain, CLI-first integration, the McCarthy completion, the origin story.
6. **This document** — discovers that ferratomic is not a tool or a framework but an operating system for knowledge work. Establishes the event-driven reactive architecture, the formal type theory of System 1 (state monad returning comonad), the concrete implementation of confusion detection, the human interface layer (situations replacing conversations), the pi_agent_rust integration path, and the validation from Karpathy's LLM Wiki pattern.

Documents 1-5 established what the system is, how it works, and how to build it. This document establishes what it BECOMES when fully realized: an operating system through which knowledge work operates, where conversations are replaced by situations, the LLM is a co-processor rather than a conversation partner, and the human navigates an intelligent workspace rather than talking to a chatbot.

---

## Part I: The Event-Driven Architecture

### 1.1 From Polling to Reactive

The conventional agent loop is polling-shaped:

```
loop {
    context = assemble_context(conversation_history);
    response = llm.complete(context);
    for tool_call in response.tool_calls {
        result = execute_tool(tool_call);
        conversation_history.push(tool_call, result);
    }
}
```

The LLM is the center. Everything revolves around the LLM's turn. Tools are passive. The store is passive. The human waits.

The event-driven architecture inverts this. The store is the center. Everything is a reaction to datom commits:

```rust
fn on_event(event: Event, store: &mut Store) {
    store.assert(event.to_datoms());
    for rule in store.matching_rules(&event) {
        match rule.action {
            Action::Query(q) => {
                let result = store.query(q);
                emit(Event::QueryExecuted(q, result));
            }
            Action::CognitiveProjection(proj) => {
                let context = evaluate_assembly(proj, store);
                let response = llm.complete(context);
                emit(Event::LLMResponse(response));
            }
            Action::NotifyHuman(message) => {
                tui.display(message);
            }
        }
    }
}
```

The LLM is no longer the loop driver. It's an event handler — triggered when a rule determines cognitive evaluation is needed. The agent loop dissolves into event handlers.

### 1.2 Everything Writes Datoms

The critical insight: the LLM is just one event source among many. Anything can write datoms:

```
Event Sources:
  Human (TUI input, corrections, confirmations)
  LLM (responses, judgments, assertions)
  Git (commits, branch operations, PR events)
  Filesystem (file changes, new files, deletions)
  CRM (HubSpot leads, Zoho updates, pipeline changes)
  Slack (messages, reactions, thread updates)
  Email (incoming messages, attachments)
  Calendar (events, schedule changes)
  CI/CD (build results, deploy events)
  Timers (cron triggers, idle detection)
  Webhooks (any external system)
```

Every event becomes datoms. Every datom commit potentially triggers rules. Rules trigger projections. Projections may invoke the LLM, update the TUI, execute code, or write more datoms. The LLM is the highest-quality event processor but not the most prolific — the filesystem changes thousands of times daily, git produces hundreds of commits, while the LLM produces maybe a few hundred tool calls.

### 1.3 The Adapter Pattern

Each external system needs a thin adapter — a small program (50-100 lines of Rust) that translates the system's native events into datoms:

```
adapter-git:      watches .git → writes :world/file-change datoms
adapter-fs:       watches filesystem → writes :world/file datoms
adapter-hubspot:  receives webhooks → writes :world/lead datoms
adapter-zoho:     polls/webhooks → writes :world/crm datoms
adapter-slack:    receives events → writes :world/message datoms
adapter-calendar: polls/events → writes :world/event datoms
adapter-email:    watches inbox → writes :world/email datoms
adapter-ci:       receives webhook → writes :world/build datoms
```

The store doesn't know or care where datoms came from. A datom from git and a datom from the LLM are identical in structure. The adapter configuration is itself Layer 0 substrate knowledge — which directories to watch, which pipelines to subscribe to — and it learns: if a particular source never produces datoms the LLM queries, the adapter can reduce its polling frequency.

### 1.4 The LLM as Co-Processor

The system is always on — not because there's a persistent LLM session (that would be expensive), but because adapters are always writing datoms, rules are always matching, mechanical projections are always evaluating, and the TUI is always displaying the current epistemic state.

The LLM wakes up when needed — when a rule determines cognitive evaluation is required, when the human asks a question, when the dream cycle runs. Between those moments, the store is still growing, still being queried, still triggering mechanical reactions. The system thinks mechanically (Datalog, rules, projections) continuously and cognitively (LLM) on demand.

Mechanical thinking is cheap and continuous. Cognitive thinking is expensive and on-demand. The LLM is the most powerful tool in the toolbox. It's not the toolbox.

### 1.5 Consequences of the Event Model

**The store can interrupt the LLM.** If a datom commit triggers a rule that invalidates what the LLM is working on — the human corrected a fact the LLM is reasoning about — the system can cancel the LLM call and restart with updated context. Structured cancellation makes this clean.

**The TUI updates continuously.** Every datom commit can trigger a TUI refresh. The epistemic state display updates as the store changes, not just when the LLM produces output.

**Multiple event sources drive cognition simultaneously.** The human types a prompt; while the LLM processes it, the store detects a contradiction from a prior dream cycle. Both events are handled. The system is responsive to multiple inputs, not locked into one request-response cycle.

**The dream cycle is just another event handler.** "When idle time exceeds threshold, emit DreamTrigger." The handler runs consolidation projections. No separate cron job needed.

**Harvest is automatic.** SessionEnd is an event. The handler runs harvest projections over the session transcript. No explicit `/harvest` command needed.

**Human corrections propagate immediately.** The human confirms a contradiction resolution in the TUI. That's an event. The event handler asserts datoms. Retractions trigger re-evaluation of dependent standing queries. The TUI updates. The next LLM call gets corrected context.

### 1.6 The Complete Event Architecture

```
Event Sources           The Store              Event Sinks
                       ┌──────────┐
  Human (TUI) ────────►│          │──────────► TUI (display)
  LLM (responses) ────►│          │──────────► LLM (context)
  Git (watcher) ──────►│ (P(D),∪) │──────────► Runtime (code)
  HubSpot (hooks) ────►│          │──────────► Slack (messages)
  Zoho (hooks) ───────►│  + Rules  │──────────► Email (sends)
  Slack (events) ─────►│          │──────────► Webhooks (calls)
  Email (IMAP) ───────►│ + Projs.  │──────────► Filesystem
  Calendar ───────────►│          │
  Filesystem ─────────►│          │
  CI/CD ──────────────►│          │
  Timers ─────────────►│          │
                       └──────────┘
```

---

## Part II: System 1 in Practice — The State Monad Returning Comonad

### 2.1 The Formal Type of System 1

System 1 (the retrieval policy / context assembly) has a precise type:

```
System1 : Context → State Store (Comonad QueryResult)
```

**The state monad part:** Every System 1 operation threads the store through. It reads from the store AND writes to it (seed datoms, query traces, coverage observations):

```
associate : Cue × Depth × Breadth → State Store SchemaNeighborhood
query     : Datalog               → State Store ResultSet
assert    : [Datom]               → State Store ()
```

Each operation takes the store, produces a value, and returns an updated store. The state mutation is the recording — seeds, query traces, coverage metrics — the Layer 3 cognitive datoms that make the system learn from its own retrieval.

**The comonad part:** The return value isn't flat results. It's results IN CONTEXT — the surrounding graph topology, what's reachable, what's sparse, what past queries from similar positions led to:

```
W a = (a, Context)

where Context = {
  graph_neighborhood : SchemaNeighborhood,
  coverage           : CoverageMetrics,
  past_queries       : [QueryDatom],
  suggested_next     : [PossibleQuery],
  sparse_regions     : [Namespace],
  confusion_history  : [ConfusionDatom]
}

extract : W a → a                    -- just the query results
extend  : (W a → b) → W a → W b     -- what would happen at each
                                        reachable position
```

The `extract` is trivial — the LLM reads query results. The `extend` is the "ALSO RELEVANT" computation — from the current focus, what further queries are available at each reachable position.

### 2.2 The Composition

```
System1 : Context → State Store (Comonad QueryResult)
        ≅ Context → Store → (Comonad QueryResult, Store')
        ≅ "take current context and knowledge, produce a
           result-in-context and enriched knowledge state"
```

The state monad is the flywheel mechanism (each operation enriches the store). The comonad is the retrieval structure (each result comes with its context of possibilities). The composition: each retrieval enriches the store which enriches the context of future retrievals.

### 2.3 The Agent Loop as Comonadic Navigation

```
step : Comonad QueryResult → State Store (Comonad QueryResult)
step w =
  let focus = extract w
      next  = system2_decides(w)     -- LLM picks from comonadic context
  in case next of
       Done        → return w
       MoreQuery q → query q         -- another monadic step
       Widen cue   → associate cue   -- widen the aperture
       Assert d    → assert d >> return w
```

Each step produces a new comonad. The LLM navigates the comonadic structure by choosing which `extend` to follow. The state monad records the navigation as datoms.

### 2.4 The Conversation as Comonad of Comonads

The conversation-level comonad composes with the query-level comonad:

```
ConversationState = W_conversation (W_query ResultSet)

-- The outer extend computes across conversation turns
-- The inner extend computes across graph positions
-- Composition: "given the full conversation trajectory
--   AND the current graph position, what's the best next move?"
```

### 2.5 System 1 Operates at Three Timescales

**Session start (the seed).** Before the first human prompt, the system assembles initial context. This is the most literal System 1 — it runs before System 2 has any input:

```bash
$ ferratomic seed --task-context "franchise development" --agent willem
```

Queries the store for relevant Layer 6 policy, Layer 2 structural knowledge, Layer 3 cognitive patterns, and composes them into a context block. The seed IS System 1 — it determines what the LLM knows at session start.

**Per-turn enrichment (auto-associate).** The `associate` call can be EMBEDDED in the `query` tool — the CLI does it automatically:

```bash
$ ferratomic query --auto-associate '[:find ?e ?summary :where ...]'
```

Under the hood: parse the query to extract referenced entities, run `associate` from those entities, if the neighborhood reveals likely-relevant attributes the query didn't reference (based on past patterns), augment the result with a "you might also want" section. The LLM doesn't invoke System 1 explicitly. System 1 is embedded in the tool.

**Between sessions (the dream).** The dream cycle is System 1 running without System 2. Consolidation, cross-pollination, gap mapping — the associative reorganization that restructures the graph so that FUTURE System 1 operations produce better results.

### 2.6 Practical CLI Tool Output

The comonadic structure renders as JSON metadata alongside query results:

```json
{
  "results": [...],
  "neighborhood": {
    "reachable_entities": [...],
    "available_attributes": [...],
    "edge_types": [...]
  },
  "suggested_next": [
    {
      "query": "[:find ...]",
      "rationale": "db-timeout-config is 3 hops away via :depends-on",
      "expected_value": 0.7
    }
  ],
  "coverage": {
    "sparse_regions": ["database/*"],
    "dead_ends": 3,
    "similar_past_confusions": [...]
  }
}
```

The `results` field is `extract`. The `suggested_next` field is `extend`. The `coverage` is structural metadata. The state mutation (recording seeds, queries, coverage as datoms) happens as a side effect of the CLI call.

---

## Part III: Confusion Detection in Practice

### 3.1 Three Kinds of Detection

**Kind 1 — Store-detected (mechanical, reliable).** Pure Datalog computation, no LLM involved:

- **Sparse neighborhood:** `associate` traversal finds few reachable entities relative to graph average density.
- **Dead-end traversals:** entities with edges pointing to targets that have no attributes of their own — dangling references.
- **Zero results:** well-formed query, no matching datoms.
- **Contradictions:** same entity and attribute, different values, from different sessions.
- **Prior confusion match:** past confusion episodes with similar cues found in Layer 3 history.

All computable by Datalog, returned as metadata alongside results, recorded as datoms automatically.

**Kind 2 — Behaviorally detected (retrospective, from datom patterns):**

- **Re-querying:** more than three queries with the same cue in one session — the LLM is circling.
- **Trajectory curvature:** the sequence of seed cues jerks between domains (auth → database → auth) rather than flowing smoothly.
- **Explicit hedging:** during harvest, the LLM reviews its session transcript and flags moments of expressed uncertainty.

Detectable from the datom chain after the fact — the patterns ARE the datoms.

**Kind 3 — Harvest-detected (retrospective, cognitive, most reliable):**

```clojure
(:cognitive
  {:task "Compare initial task description with final resolution.
          Identify every point where the trajectory could have been
          shorter if better context had been available. For each,
          what specific datom would have prevented the detour?"
   :input {:task $task :trajectory $seed-sequence :resolution $outcome}
   :output-schema {:detour-point :missing-datom :time-wasted :severity}
   :record-as :confusion/retrospective-trajectory})
```

The missing datom IS the assertion that should be written. The confusion episode becomes the trigger for the self-authoring flywheel.

### 3.2 Composition Across Timescales

```
During session (real-time, mechanical):
  Store computes coverage metadata on every query.
  Returned alongside results as comonadic context.
  Recorded as Layer 3 datoms automatically.

During session (real-time, behavioral):
  Multi-turn query patterns recorded as linked seed datoms.
  High-frequency re-querying and trajectory curvature
  detectable from the datom chain.

During harvest (post-session, cognitive):
  LLM reviews full trajectory, identifies detours,
  specifies missing datoms. Those datoms are asserted.

During dream (between sessions, consolidation):
  Cross-session confusion patterns analyzed.
  Seed patterns adjusted based on confusion trends.
  System 1 learns from System 1's failures.
```

---

## Part IV: The Human Interface — Situations Replace Conversations

### 4.1 Why Conversations Are Compelling but Wrong

Conversations map onto deep social cognition — turn-taking dialogue, theory of mind, narrative sense-making. They provide linear narrative: beginning (problem), middle (working through), end (resolution). This is cognitively manageable.

But conversations create incorrect assumptions: continuity of personality, basin trapping (the conversation stays in whatever region it started in), the illusion that the AI "remembers" you, the anthropomorphic framing that obscures the actual mechanics.

The event-driven store has no linearity. Datoms arrive asynchronously from everywhere. There's no beginning, middle, or end. No "turn." Showing the human that the system is a reactive knowledge substrate rather than a conversational partner is truthful but requires a different mental model.

### 4.2 The Human Navigates Situations, Not Conversations

A situation is: the current state of a region of the knowledge graph, plus active events, plus available actions. It's not a dialogue. It's a PLACE.

The mental model is a physical workspace. You sit down. You see what's there. Papers from yesterday. A note from a colleague. A notification on your monitor. You assess. You decide what to engage with. You act. The workspace updates.

The TUI isn't a chat interface. It's a DESK. Things are on it. Some arrived overnight (dream results). Some just appeared (adapter events). Some have been there for days (ongoing projects). The human scans, prioritizes, engages.

### 4.3 Three Modes of Engagement

**Mode 1 — Ambient awareness (most of the time).** The human glances at the TUI between other activities. The landscape shows current state. The epistemic sidebar shows what changed. No interaction required. Like having a well-organized desk with a good filing system.

**Mode 2 — Directed inquiry (when something needs attention).** The human notices something — a confusion signal, a new connection, a pipeline update. They ask a question. One question. One answer. Maybe two follow-ups. The mental model: "asking my desk a question." Brief. Contextual. No context-setting needed because the store already has it.

**Mode 3 — Deep collaboration (rare, high-value).** Extended cognitive engagement for architecture design, complex analysis, strategy development. This IS a conversation, but explicitly BOUNDED — a purpose, a scope, an expected endpoint. It starts from the situation (the store's current state) and ends by harvesting results back. The mental model: hiring a consultant for two hours. Brief, synthesize, capture, done.

### 4.4 The Situation Board

```
┌─────────────────────────────────────────────────┐
│  SITUATIONS                              ⚡ live │
├─────────────────────────────────────────────────┤
│                                                 │
│  🔴 Boardroom: Justin Boltz follow-up           │
│     FDD signed 3 weeks ago, no MTD scheduled    │
│     Pipeline velocity: stalled                  │
│     → "Schedule MTD for Justin"                 │
│                                                 │
│  🟡 Showhomes: FDD amendment received           │
│     New document from legal, unreviewed          │
│     3 territory datoms may be affected           │
│     → "Review FDD amendment impact"             │
│                                                 │
│  🟢 Ferratomic: Datalog engine                  │
│     Last session: recursive joins working        │
│     6 invariants passing, 2 pending              │
│     Dream found: DDlog operator pattern          │
│     → "Continue Datalog implementation"          │
│                                                 │
│  💤 Dream insights (last night):                 │
│     Found parallel: Boardroom broker comp        │
│     ↔ Showhomes territory design                │
│     → "Explore broker/territory parallel"        │
│                                                 │
├─────────────────────────────────────────────────┤
│  ▸ Quick ask: _                                 │
└─────────────────────────────────────────────────┘
```

Each situation is a region of the knowledge graph with active events. Color indicates urgency (computed from datoms — time since last action, pipeline velocity, deadlines). The "→" suggestion is the comonadic `extend`. Situations emerge from datoms — computed by projections that cluster related datoms and surface those with active events. The clustering is learnable.

### 4.5 Interaction Within a Situation

When the human enters a situation, they see the landscape for that region. They can:

- **Act** — tap a suggested action (mechanical or cognitive)
- **Ask** — type a question (store-answered or LLM-answered)
- **Explore** — navigate the landscape, follow edges to connected situations
- **Correct** — fix datoms, resolve contradictions, dismiss false positives
- **Focus** — enter deep collaboration mode (bounded conversation)

### 4.6 The Key Cognitive Shift

From: "I need to talk to my AI assistant about this."
To: "I need to check on this situation and decide what it needs."

The AI isn't a partner you talk to. The AI is the INTELLIGENCE OF THE SITUATION. The situation knows things, suggests actions, answers questions, learns from engagement. The AI is woven into the situation, not sitting across from you.

---

## Part V: Epistemic State Visibility — The Human's System 1

### 5.1 What the Human Should See

The comonadic query structure provides rich structural metadata that the human currently has no access to. Surfacing it transforms the harness from "a chat window" to "an intelligent workspace."

### 5.2 Current Basin of Attraction

The agent's recent queries and seeds cluster in a region of the graph. The cluster has a shape — entities in focus, edges between them, boundary beyond which the agent hasn't looked.

Computable from seed and query datoms:
```
[:find ?entity (count ?query)
 :where [?q :query/session $current-session]
        [?q :query/touched-entity ?entity]
 :order-by [(desc (count ?query))]]
```

The human sees where the agent has been operating. If the problem involves a domain the agent hasn't visited, the human can redirect.

### 5.3 Basin Stability

Is the basin deepening (exploitation) or drifting (exploration)? Compute the centroid of queried entities per turn and measure movement. A basin shift mid-session might be productive (following a lead) or confused (lost the thread). The human sees "basin shifted at turn 7" and can intervene if the shift was wrong.

### 5.4 Gravitational Wells

Some entities are hubs that `associate` keeps finding regardless of the cue — fixation points. Computable: entities that appear in query results across multiple distinct cues. The human sees: "⚠ Gravitational well: auth-handler (appeared in 7/8 queries)" and can explicitly redirect.

### 5.5 Knowledge Horizon

The boundary of what the store knows, rendered as density per region:

```
franchise-development: ██████████  (2,847 datoms, rich)
auth-systems:          ████░░░░░░  (423 datoms, moderate)
database-config:       ██░░░░░░░░  (89 datoms, sparse)
deployment:            ░░░░░░░░░░  (12 datoms, nearly empty)
monitoring:            (no datoms)
```

The human sees at a glance where the store is rich and where it's thin. If the task involves a sparse region, expectations are calibrated.

### 5.6 Cross-Session Trajectory

```
Growing:   auth/* (+147 datoms this week)
Stable:    infrastructure/* (±3 datoms)
Decaying:  deployment/* (no queries in 5 days)

Flywheel:  23 assertions from confusion resolution
           12 assertions from explicit assert
           8 assertions from dream

Confusion trend: ▼ declining (4.2 → 2.1/session this week)
```

The human sees the system learning. The confusion rate declining. The flywheel producing assertions from multiple sources. This is the meta-view of cognitive development over time.

### 5.7 Active Seed

What version of the agent showed up:

```
Seeded from: last 5 sessions
Policies: 3 active (eff avg: 0.88)
Structural datoms loaded: 47
Cognitive patterns: 12 seed patterns active
Last dream: 14h ago (8 new assertions)
```

Different seeds produce different agents. The human should know which one they're working with.

### 5.8 Human Interventions as High-Value Datoms

Every human interaction with the epistemic display produces datoms:

- "Confirm" on a contradiction: human verified, record with high trust
- "Dismiss" on a sparse region warning: false positive, calibrate future detection
- "Ignore" on a suggestion: not relevant now, don't resurface this session

These are the highest-trust datoms in the store — human verdicts with provenance. They calibrate future confusion detection, suggestion quality, and display relevance.

### 5.9 Progressive Interface Levels

```
Month 2:   stderr annotations on CLI output.
           Coverage metadata, one-line basin indicator,
           one suggested query. 20 lines of Rust formatting.

Month 4-5: Companion TUI display (tmux split or dedicated pane).
           Persistent epistemic state: knowledge horizon,
           basin visualization, active seed, trajectory.

Month 6+:  Interactive epistemic dashboard.
           Suggested prompts, confirm/dismiss buttons,
           gravitational well warnings, theory-of-mind model.

Eventually: The knowledge landscape — a visual, navigable,
           interactive terrain where valleys are expertise
           and the conversation is a path through it.
```

### 5.10 The Knowledge Landscape (North Star for Interface)

The store's knowledge graph projected into 2D. Dense regions are valleys (high knowledge). Sparse regions are peaks (low knowledge). The conversation is a path through the terrain. Past sessions are faded paths. Other team members' paths in different colors. Dream cycle results appear as new terrain features overnight.

The landscape is interactive — the human can point at unexplored regions and the system generates prompts to steer toward them. Suggested next prompts render as directional arrows. The comonadic `extend` made visual and spatial.

The conversation as physics: position (current basin), momentum (query direction), potential energy (information density), kinetic energy (DoF reduction rate), temperature (exploration vs exploitation).

The human navigates the landscape, not the conversation. Federation as shared cartography. The landscape IS the product.

---

## Part VI: The Harness — pi_agent_rust as the First Shell

### 6.1 Why pi_agent_rust

Pi (27.3k stars on the original mono repo by Mario Zechner / badlogic) is a well-established, actively-maintained agent CLI toolkit. The Rust port (pi_agent_rust by Dicklesworthstone) provides:

- **Same language as ferratomic:** Rust throughout. No crossing from Rust to TypeScript. Ferratomic tools are library calls, not process spawns. Tool call latency drops from 50ms (process spawn) to 50µs (function call).
- **Open and modifiable:** Every architectural decision is accessible. System prompt assembly, session management, compaction strategy, TUI rendering — all modifiable.
- **Session branching in JSONL:** Maps to the comonadic conversation structure. The tree IS the space of explored and unexplored paths.
- **Skills system:** Drop SKILL.md files. Direct integration point for `/harvest`, `/dream`, `/seed`.
- **Extension system:** Capability-gated QuickJS runtime for sandboxed execution.
- **Built on asupersync:** Structured concurrency async runtime — event-loop-shaped, with structured cancellation. The infrastructure for the event-driven model already exists.
- **Sub-100ms startup:** Critical for CLI tools invoked frequently, for dream cycle cron jobs, for quick lookups.

### 6.2 Why Not Claude Code

Claude Code is a closed binary. You can plug ferratomic into it via BashTool, but you can never make the harness itself a projection. The system prompt assembly, MEMORY.md loading, coordinator mode — compiled TypeScript you can read but can't modify.

The endgame — harness becomes a projection of the store — is impossible with Claude Code. With pi_agent_rust, it's a series of incremental modifications.

### 6.3 The Progressive Migration

```
Month 1:   Use Claude Code. Stability while building the Datalog engine.

Month 2:   Fork pi_agent_rust. Make ferratomic a native Rust library
           dependency. Replace session JSONL with datom writes.
           ferratomic tools are library calls, not process spawns.

Month 3:   Replace context assembly with ferratomic seed query.
           System prompt comes from the store.
           Add /harvest and /dream as skills.
           Migrate daily workflow to the fork.

Month 4-5: Projection evaluator lives inside the harness.
           Context assembly IS projection evaluation.
           TUI gets epistemic state sidebar.

Month 6+:  Tool dispatch is projection-driven. Compaction strategy
           queries the store. Session branching maps to comonadic
           conversation model.

Month 12:  The harness IS a thin shell around ferratomic.
           Session management, context assembly, tool dispatch,
           TUI rendering — all projections.
```

### 6.4 The Event-Driven Refactoring

Pi_agent_rust's asupersync runtime maps onto the event-driven architecture:

```
Step 1: Keep existing agent loop, add event bus alongside it.
        Every tool result, LLM response, human prompt emits
        an event. The bus does nothing yet.

Step 2: Add ferratomic as event consumer. Every event writes datoms.

Step 3: Add rules as datoms. Rules match events and trigger handlers.
        Start simple: "on SessionEnd, run harvest."

Step 4: Move context assembly to a reactive projection.
        "On HumanPrompt, evaluate seed projection, assemble context,
        call LLM." The agent loop is now an event handler.

Step 5: Everything is an event handler. The agent loop is gone.
        There's only the event loop.
```

### 6.5 The Harness Becomes the Event Loop

The harness doesn't become a projection of the store. The harness becomes the EVENT LOOP of the store. The thinnest possible shell: receive events, dispatch to handlers, handlers are projections, projections are datoms. One loop. One store. One flywheel.

---

## Part VII: CLI First, MCP Later

### 7.1 CLI for Month 2-5

The existing Braid CLI tools (`braid`, `cass`, `cm`, `ms`) establish the pattern. `ferratomic associate/query/assert` as CLI commands called via BashTool (while using Claude Code) or as library calls (once migrated to pi_agent_rust fork).

CLI tools are request-response. No protocol overhead, no server process, no connection management. The simplest possible integration:

```bash
$ ferratomic associate --cue "auth handler timeout" --depth 2 --breadth 5
$ ferratomic query '[:find ?e ?summary :where ...]'
$ ferratomic assert '[:entity/E1 :caused-by :entity/E2]'
```

### 7.2 When MCP Becomes Necessary

**Push notifications:** The server can notify the client when patterns appear in the store. Needed for standing queries and reactive forward-chaining rules. Month 8+ functionality.

**Connection-scoped state:** Each client connection has a different visibility scope. Needed for multi-agent coordination where workers share a store with different projections. Month 6+ functionality.

### 7.3 The Integration Path

```
Month 2:   CLI tools via BashTool or library calls.
Month 4-5: Evaluate MCP need based on actual usage patterns.
Month 6+:  Add MCP server if push notifications or scoped
           visibility are needed. CLI tools continue working
           for single-agent use.
```

The Datalog engine doesn't care how it's invoked. The integration layer is the cheap part.

---

## Part VIII: The Agentic Operating System

### 8.1 The Reframing

The assembled components form an operating system:

```
Unix OS                    Ferratomic OS
────────────────────────────────────────────────
Kernel                     Event loop + store
Filesystem                 (P(D), ∪)
Device drivers             Adapters (git, CRM, email, Slack...)
System calls               Tool calls (CLI, projections)
Process scheduler          Rule engine (forward-chaining datoms)
Window manager             Projection renderer (TUI, landscape)
Shell                      Natural language → LLM → tool calls
Pipes                      Datom flows between projections
Users                      Agents (projection bundles)
Permissions                Namespace visibility + taint
Cron                       Dream cycle
Filesystem events          Datom commit observers
/proc                      Layer 0 (substrate self-model)
Man pages                  Layer 6 (policy datoms)
```

The mapping is structural, not metaphorical. Every component emerged from first principles over the course of five weeks.

### 8.2 "Everything is a file" → "Everything is a datom"

Unix's genius: the universal interface. Everything is a file descriptor. Any tool that can read/write files can interact with any device, network connection, or process.

Ferratomic's universal interface: everything is a datom. LLM outputs, git commits, CRM updates, calendar events, human corrections, policy instructions — all stored as datoms, all queryable by Datalog, all triggering the same rules, all renderable by the same projections.

### 8.3 From Vertical Product to Horizontal Platform

A memory system has a ceiling. An operating system has no ceiling. It's the foundation on which everything else is built. Applications you haven't imagined yet will run on it.

Before Unix, every application managed its own I/O. Unix eliminated the need for every application to solve the same infrastructure problems. Before ferratomic, every knowledge tool manages its own memory, context, integrations, search. Ferratomic eliminates the duplication: there's a store, everything is a datom, queries connect facts, the event loop handles reactions.

### 8.4 The Monthly Staircase as OS Development

```
Month 1: The kernel (datom store + Datalog engine)
Month 2: First device drivers (adapters) + first system calls (CLI)
Month 3: First shell (pi_agent_rust fork) + first cron (dream)
Month 4-5: Window manager (projection evaluator + TUI)
           + first pipes (projection chains) + scheduler (rules)
Month 6+: More drivers, multi-user (federation), the OS grows
```

### 8.5 The Moat

The software — kernel, shell, adapters — is open-sourceable. Just like Linux. The value is the ecosystem: accumulated stores, projection libraries, adapter catalogs, rule collections, methodology datoms, trained attentional patterns. Open-source the kernel, build a business on the ecosystem. Root + Rise doesn't sell ferratomic. It sells the franchise development methodology that runs on it.

---

## Part IX: Karpathy's LLM Wiki — Industry Validation

### 9.1 The Pattern

On April 4, 2026 — two days before this writing — Andrej Karpathy published an "LLM Wiki" gist that reached 5,000+ stars. His core insight is identical: "the LLM is rediscovering knowledge from scratch on every question. There's no accumulation."

His three-layer architecture maps directly:

```
Karpathy's layers        Ferratomic's layers
─────────────────────────────────────────────
Raw sources (immutable)   Layer 1 (append-only datoms)
Wiki (compiled artifact)  Layers 2-6 (structural, cognitive, policy...)
Schema (conventions)      Projection datoms + namespace conventions
```

His operations map too: "Ingest" = harvest. "Query" = associate + query. "Lint" = dream cycle. He cites Vannevar Bush's Memex (1945). He identifies that LLMs solve the maintenance problem Bush couldn't.

### 9.2 Where the LLM Wiki Hits Walls

**Cross-reference maintenance is O(n²).** Each new source touches 10-15 wiki pages. At 1000 sources, the LLM must read the index, identify relevant pages, read and update them. Datalog does this mechanically in milliseconds.

**Contradiction detection requires full re-read.** "Lint" asks the LLM to read every page and notice conflicts. At 500 pages, infeasible. Datalog queries over typed assertions with taint tracking do this mechanically.

**No associative retrieval.** Query reads an index (flat list of titles with summaries). Keyword matching on summaries misses structural connections described with different terminology. `associate` does graph traversal.

**No learning from retrieval.** Which pages were accessed, whether retrieval was productive, what the LLM wished it had found — none recorded. No Layer 3. The system doesn't learn how to search itself.

**No ambient integration.** Sources are manually dropped into a directory. No adapters.

**No autonomous consolidation.** "Lint" is manual. No dream cycle.

### 9.3 The Positioning

Karpathy gave the ferratomic thesis the simplest possible framing: knowledge should compile, not re-derive. His implementation is markdown files with grep — the most sophisticated version of the flat-file architecture. It will hit the walls the theory predicts.

The Month 3 blog post framing: "Karpathy's LLM Wiki pattern is exactly right. Here's the implementation that makes it scale. Same pattern. Better substrate."

The audience that starred his gist IS the ferratomic audience. They understand the problem. They've accepted the pattern. They'll hit the walls. Ferratomic has the solution.

---

## Part X: Implications for Human-AI Collaboration and Work

### 10.1 The Unit of Collaboration Is No Longer the Conversation

If anything writes datoms — git, Slack, CRM, email, calendar, filesystem, LLM — then collaboration isn't a conversation. It's the store. Collaboration is continuous, ambient, and asynchronous. It happens when the human is coding, in meetings, and asleep (dream cycle).

The human and AI don't take turns. They cohabitate a shared knowledge space.

### 10.2 A Tuesday Morning

The human arrives. The TUI shows the current state. Overnight, the dream cycle found a structural parallel between engagements. The git adapter noticed a merged PR. The CRM adapter recorded new leads. The email adapter flagged an FDD amendment.

The human hasn't typed a word. The system shows a curated view of what changed, what it learned, what needs attention. They see the parallel, click "explore," read the connection, type one prompt: "Tell me more about this parallel." The LLM wakes up, elaborates with full context from the store, suggests actions. The human picks one. The LLM executes. The human goes to their meeting.

Total conversation: one prompt, one response. Total collaboration: continuous.

### 10.3 Work Stops Being Divided

No context switch between "AI-assisted" and "regular work." The store is always there. Events always flow. The AI's involvement is a spectrum from fully mechanical (TUI updates, rule triggers, taint propagation) through lightweight cognitive (suggested actions from past patterns) to full cognitive (LLM analysis or execution).

Most value comes from the lightweight end. The human needs the STORE — ambient awareness of changes, relevance, connections — more than they need the LLM. The LLM is heavy machinery for specific jobs. The store is the workshop floor you walk on all day.

### 10.4 Teams

A team sharing a store doesn't need traditional "communication." Patti closes a deal → CRM adapter writes datoms → Willem's TUI shows "New deal closed — territory X allocated." No Slack message needed. No standup needed. The store tells each person what changed in their projection of the shared knowledge.

When deeper coordination is needed, the store already has the context. "How does Patti's new deal affect Willem's territory analysis?" is a Datalog query, not a meeting.

### 10.5 Cognitive Load

Knowledge workers spend enormous energy on context management — scanning email, checking Slack, reviewing PRs, updating task boards. This is manual System 1 work.

The store does this. Adapters write datoms from every system. Rules and projections compute relevance. The TUI surfaces it. The human's cognitive load drops from "maintain a mental model of everything" to "glance at the TUI and decide what to engage with."

### 10.6 Organizational Knowledge

When every system writes datoms, the dream cycle cross-pollinates, and structural assertions accumulate, the organization develops expertise through accumulated experience, not training programs. A new team member gets a projection scoped to their role, containing the methodology datoms from months of everyone's work. Their first day is like everyone else's hundredth day.

### 10.7 What "AI Collaboration" Becomes

It stops meaning "I talk to an AI." It starts meaning "I work in a knowledge-rich environment where everything I do accumulates as queryable structure, and cognitive processing is available on demand."

The AI isn't a colleague. The AI is the intelligence of the environment. The walls remember. The tools learn. The workspace adapts. The conversations happen when needed — and they're short, because the context is already there.

---

## Part XI: The North Star (Final)

### 11.1 What Ferratomic Is

An operating system for knowledge work. `(P(D), ∪)` as the kernel. Adapters as device drivers. The event loop as the scheduler. Projections as the window manager. Natural language as the shell. The LLM as a co-processor. Datoms as the universal interface. Everything is a datom. Everything writes datoms. Everything reads datoms. The LLM is called when cognition is needed and idle when it's not. The system is always alive.

### 11.2 What the Industry Confirms

Anthropic (Claude Code's KAIROS/dream/MEMORY.md), Karpathy (LLM Wiki with 5000+ stars), and the differential dataflow ecosystem (DDlog/DBSP/Materialize) have independently validated every major architectural concept. All three hit the same walls the theory predicts. All three are solved by the same substrate: `(P(D), ∪)`.

### 11.3 The Interface

Not a chatbot. An intelligent workspace. Situations replace conversations. Ambient awareness replaces context-setting. The knowledge landscape replaces chat history. The human navigates situations, not dialogues. The AI is the intelligence of the environment, not a partner across the table.

### 11.4 The Formal Foundation

System 1 is `State Store (Comonad QueryResult)`. The state monad is the flywheel. The comonad is the retrieval structure. The composition: each retrieval enriches the store which enriches future retrievals. One type signature. One flywheel. One fixed point.

### 11.5 The Execution Priority

The Datalog engine is the kernel. The kernel is the bottleneck. Everything else — adapters, event loop, projections, rules, TUI, landscape, federation, the OS — is downstream. Build the kernel. Everything follows.

### 11.6 The Final Statement

One equation: `(P(D), ∪)`. One architecture: event-driven reactive substrate. One interface: situations, not conversations. One flywheel: events → datoms → rules → projections → events. One fixed point: where human cognitive habits and system projections have co-evolved to mutual optimality. One operating system: for knowledge work.

Datoms all the way down. Events all the way in. Projections all the way up. Situations all the way out. Fixed points all the way through.

And the first step is still a Datalog engine that can evaluate recursive joins over EAV triples.
