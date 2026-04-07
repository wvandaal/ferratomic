# Architectural Influences

> Extracted from `FERRATOMIC_ARCHITECTURE.md` section 17.
>
> Design genealogy -- what each reference system contributed to Ferratomic's
> architecture, and where Ferratomic deliberately departs.

---

## From PostgreSQL: MVCC, WAL, Group Commit, Buffer Pool

**MVCC** (Multi-Version Concurrency Control): PostgreSQL's insight that readers and writers
should not block each other. Ferratomic takes this further -- not only do readers not block
writers, readers do not even contend with each other (ArcSwap vs. PostgreSQL's shared lock
on tuple headers).

**WAL** (Write-Ahead Log): PostgreSQL's WAL ensures that committed transactions survive
crashes. Ferratomic's WAL follows the same principle (INV-FERR-008: WAL fsync BEFORE epoch
advance) but with a simpler frame format (no page-level granularity needed because the data
model is datoms, not pages).

**Group commit**: PostgreSQL batches multiple commits into a single fsync. Ferratomic's
Phase 4a path already enforces WAL-before-visible ordering on a serialized writer, and the
planned Phase 4b writer actor is where explicit queue draining and group commit become the
steady-state implementation.

**Buffer pool**: PostgreSQL's shared buffer pool mediates between disk and memory. Ferratomic
replaces this with im::OrdMap persistent data structures, which provide the same benefit
(in-memory access to frequently-used data) without the complexity of a page replacement
algorithm (LRU, clock sweep).

## From Aurora: "Log Is the Database", Storage-Compute Separation

**"The log is the database"**: Aurora's insight that the WAL is the authoritative state and
all other representations are derived. Ferratomic follows this principle: the WAL (and by
extension, the EDN transaction files) is the source of truth. Checkpoints, in-memory
indexes, and snapshots are derived state that can be reconstructed from the log.

**Storage-compute separation**: Aurora separates storage nodes from compute nodes. Ferratomic
achieves a weaker form of this via the `Transport` trait: the storage mechanism
(local filesystem or network) is independent of the computation (transact, query, merge).

## From Redis: Single-Writer, AOF/RDB, Pub/Sub

**Single-writer**: Redis processes all writes in a single thread, eliminating write
contention. Ferratomic's serialized write path follows the same principle while allowing
concurrent reads (Redis blocks reads during writes; Ferratomic does not). Phase 4b moves
that serialized path into an explicit writer actor.

**AOF/RDB duality**: Redis offers two persistence modes: AOF (append-only file, similar to
WAL) and RDB (point-in-time snapshot, similar to checkpoint). Ferratomic uses both: WAL for
durability, checkpoint for fast recovery. Redis forces a choice; Ferratomic uses both
complementarily.

**Pub/sub**: Redis pub/sub notifies subscribers of new data. Ferratomic's DatomObserver
trait serves the same purpose for datom consumers, with stronger delivery guarantees
(at-least-once with epoch-based dedup vs. Redis's at-most-once pub/sub).

## From Kafka: Log-Structured, Consumer Groups, Zero-Copy, Batch I/O

**Log-structured**: Kafka's append-only log model where consumers track their offset.
Ferratomic's WAL is a log; observers track their last-seen epoch (analogous to Kafka's
consumer offset).

**Consumer groups**: Kafka allows multiple consumers to independently track their position
in the log. Ferratomic's observers independently track their epochs, enabling different
consumers (MaterializedViews, CLI, MCP server) to process datoms at different rates.

**Batch I/O**: Kafka batches multiple messages into single I/O operations. Ferratomic's
group commit batches multiple transactions into a single fsync.

## From Erlang/OTP: Supervision Trees, Per-Process Heaps, "Let It Crash"

**Supervision trees**: Erlang's hierarchical process management ensures that crashed
processes are restarted with clean state. Ferratomic's three-level recovery (section 5)
follows the same philosophy: if the in-memory state is corrupted, reconstruct from WAL;
if WAL is corrupted, reconstruct from EDN files; if EDN files are corrupted, start from
genesis.

**Per-process heaps**: Erlang processes have isolated heaps, eliminating garbage collection
pauses from cross-process references. Ferratomic's snapshot isolation achieves a similar
effect: each reader holds an independent snapshot with no shared mutable state.

**"Let it crash"**: Rather than defending against every possible failure with error
handling, Erlang processes crash and restart from a known-good state. Ferratomic's
crash-recovery model follows this philosophy: if anything goes wrong during a transaction,
the WAL is the recovery point. No partial state needs to be repaired.

## From Actor Model: Actors as Concurrency Unit, Mailbox, Location Transparency

**Actors as concurrency unit**: This is the planned Phase 4b refinement. The writer actor
will own the mutable store state and communicate via messages, preserving the same
single-writer invariant that Phase 4a currently enforces with a mutex.

**Mailbox**: In the actorized design, the mpsc channel is the writer's mailbox. Messages
(transactions) are processed in order and published as a batch.

**Location transparency**: The `Transport` trait provides location transparency -- the writer
actor does not know whether its storage is local or remote. The same actor logic works in
both embedded and distributed modes.

## From Asupersync: Structured Concurrency, DPOR, Cancel-Awareness, Obligation Tracking

**Structured concurrency**: Child tasks cannot outlive their parent scope. Applied to
Ferratomic: observer notifications cannot outlive the transaction that triggered them.
If the writer crashes, all pending observer notifications are cancelled (and will be
re-delivered via catch-up after recovery).

**DPOR** (Dynamic Partial Order Reduction): A technique for reducing the state space in
model checking by exploiting commutativity of independent operations. Applied to
Ferratomic: Stateright model checking of the CRDT protocol uses DPOR to avoid exploring
redundant orderings of independent writes.

**Cancel-awareness**: Tasks respond to cancellation requests. Applied to Ferratomic: long
checkpoint writes can be cancelled if the store is shutting down, with the partial
checkpoint discarded (no partial state persists).

**Obligation tracking**: Structured concurrency tracks which tasks are still running. Applied
to Ferratomic: the observer broadcast tracks which observers have acknowledged which epochs,
enabling the catch-up protocol for slow or crashed observers.

## From FrankenSQLite: MVCC, Lock-Free Reads, Birthday Paradox Model, ARC Buffer Pool

**MVCC with lock-free reads**: FrankenSQLite's key contribution is MVCC that provides truly
lock-free reads (no reader-writer contention). Ferratomic achieves this via ArcSwap, which
is simpler than FrankenSQLite's page-level approach because Ferratomic operates on datoms
(immutable values) rather than pages (mutable containers).

**Birthday paradox model**: FrankenSQLite uses a birthday-paradox argument to bound the
probability of hash collisions in its snapshot identification scheme. Ferratomic uses the
same argument for BLAKE3 content-addressing: with 256-bit hashes, the birthday bound gives
collision probability < 2^{-128} for 2^64 datoms.

**Group commit with two-fsync barrier**: FrankenSQLite's group commit protocol (WAL fsync,
then checkpoint fsync) is adopted directly by Ferratomic. The two-fsync barrier ensures
both durability and fast recovery.

**ARC buffer pool**: FrankenSQLite uses an Adaptive Replacement Cache (ARC) for its buffer
pool, balancing recency and frequency of access. Ferratomic does not need a buffer pool
(im::OrdMap keeps all data in memory) but would adopt ARC-like policies for the tiered
storage feature in Phase 4b, where cold datoms are evicted to disk.
