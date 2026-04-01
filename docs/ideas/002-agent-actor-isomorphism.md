# Does the Actor Model's Algebraic Structure Align with Ferratomic's?

Ferratomic's governing algebra: Store = (P(D), ∪) — a G-Set CRDT semilattice. The fundamental insight is that the data structure IS the consistency mechanism. Merge is set union. No coordination. No consensus. No message ordering
requirements. Commutativity, associativity, idempotency are structural tautologies.

The Actor Model's governing algebra: Actors are isolated state machines communicating via asynchronous message passing. The key properties are: (1) no shared state, (2) messages are ordered per-sender-receiver pair but not globally, (3)
actors process one message at a time (sequential internal consistency), (4) supervision trees for fault tolerance.

## Where They Align (Genuinely)

1. Isolation as a structural guarantee. Ferratomic already uses this principle — ferratom has zero project deps, Store is behind ArcSwap, writers are serialized. The actor model formalizes this: each actor owns its state exclusively. This aligns with Ferratomic's single-writer pattern (INV-FERR-007). The WriterActor upgrade path in ADR-FERR-003 is literally an actor — an mpsc channel draining to a single writer task.

2. Supervision for crash recovery. Erlang/Akka's "let it crash" philosophy maps well to Ferratomic's three-level recovery cascade (checkpoint + WAL, WAL-only, genesis). A supervisor actor that restarts crashed subsystems is structurally similar to cold_start_with_backend(). Lunatic's Erlang-on-WASM approach brings this to Rust with preemptive scheduling — interesting for federation (Phase 4c).

3. Location transparency for federation. In Phase 4c, Ferratomic needs to send merge messages between nodes. Actors provide natural location transparency — actor.send(MergeRequest) works whether the actor is local or remote. This maps to INV-FERR-037-044 (federation invariants).

## Where They Conflict (Critically)

1. The fundamental tension: message ordering vs. order independence. The actor model's strength is managing ordered message flows between components. But Ferratomic's entire architecture is built on order not mattering. Merge is commutative (INV-FERR-001). Transactions are content-addressed (INV-FERR-012). The CRDT guarantee means you don't need the actor model's ordering guarantees — and paying for them is waste.

This is the deepest point: actors solve a coordination problem that CRDTs eliminate. Adding actor-model infrastructure to a CRDT system is solving an already-solved problem with heavier machinery.

2. Shared-nothing vs. structural sharing. Actors enforce shared-nothing by copying messages between mailboxes. Ferratomic uses im::OrdSet with structural sharing (ADR-FERR-001) — O(1) snapshot clones via shared tree spines. Actor message passing would force serialization/deserialization at every boundary, destroying the O(1) snapshot property that makes ArcSwap + im-rs so elegant. You'd replace a pointer swap with a serialization round-trip.

3. Mailbox backpressure vs. semaphore backpressure. Ferratomic's WriteLimiter (INV-FERR-021) is a lock-free atomic semaphore — ~2 nanoseconds per acquire. Actor mailbox backpressure requires checking queue depth, potentially blocking on bounded channels. The overhead is 100-1000x higher for the same semantic guarantee.

4. Deterministic testing. Ferratomic uses asupersync's LabRuntime with DPOR for deterministic interleaving exploration (ADR-FERR-002). Actor systems are notoriously hard to test deterministically — message delivery order is non-deterministic by design. Akka has TestKit, but it's fundamentally less powerful than DPOR. Lunatic's deterministic testing story is immature compared to asupersync's LabRuntime.

## Lunatic Specifically

Lunatic is fascinating but has a specific problem: it's archived/unmaintained (the GitHub repo shows very low recent activity, and the project appears to have stalled). Building on it would mean depending on infrastructure with uncertain future. More importantly:

- Lunatic's WASM sandbox model adds overhead that Ferratomic doesn't need (we already have #![forbid(unsafe_code)] for memory safety)
- Lunatic's preemptive scheduling is valuable for untrusted code but Ferratomic controls all its own code
- The Erlang-style process model is appealing for fault isolation but redundant with asupersync's structured concurrency (Scope::spawn already guarantees no orphaned tasks)

## The First-Principles Verdict

The actor model is axiologically misaligned with Ferratomic's core algebra. Here's why:

The actor model's value proposition is: "Manage complexity of concurrent, distributed systems by isolating state and ordering communication." Ferratomic's value proposition is: "Eliminate complexity of concurrent, distributed systems by making the data structure commutative, associative, and idempotent."

These are competing solutions to the same problem. You don't need ordered message passing when your merge operation is order-independent. You don't need actor isolation when your data structure is append-only and conflict-free. You don't need supervision trees when your recovery is deterministic from WAL + checkpoint.

### What IS accretive from this space:

1. The WriterActor pattern (already planned in ADR-FERR-003) — a single actor for write serialization with group commit. This is the one place where actor semantics genuinely help, and it's already in the spec.
2. Observer delivery as actor mailboxes — the observer broadcast (INV-FERR-011) could use bounded channels withbackpressure, which is actor-adjacent. Also already designed.
3. Federation transport as actor messaging — Phase 4c anti-entropy protocol (INV-FERR-022) could use an actor-like message passing abstraction for node-to-node communication. But asupersync's channels already provide this without the full actor runtime.

Bottom line: Take the WriterActor pattern (already taken). Skip the actor runtime. The CRDT semilattice IS the coordination mechanism — adding an actor framework on top would be adding a coordination layer to a system whose entire  point is not needing one.

The one scenario where I'd revisit this: if Phase 4c federation discovers that multi-node coordination is more complex than pure CRDT merge (e.g., schema migration coordination, partition detection protocol state), then a lightweight actor abstraction for the federation control plane (not the data plane) could be justified. But that's a Phase 4c discovery, not a Phase 4b decision.
