# RaptorQ Federation Transport Repair

RaptorQ could be meaningfully accretive for Ferratomic's distribution layer (Phase 4c federation), but the
alignment is more nuanced than a direct port from FrankenSQLite.

Where it fits:

Ferratomic's federation protocol (INV-FERR-037..044, spec/05-federation.md) requires:
1. Replica bootstrap — a new node loads a checkpoint from an existing replica
2. Anti-entropy sync (INV-FERR-022) — after partition heals, nodes exchange deltas via Merkle-tree diff
3. Selective merge — federated query fan-out returns partial result sets that get merged

RaptorQ (RFC 6330 fountain codes) would help specifically with #1 and #2:

- Checkpoint transfer over lossy networks: A checkpoint is ~20GB at 100M datoms (bincode). With RaptorQ, the sender
generates a rateless stream of repair symbols. The receiver can reconstruct the full checkpoint from any sufficient
subset of symbols — no retransmission negotiation needed. This is bandwidth-optimal for lossy links (satellite,
cross-datacenter WAN, mesh networks between edge nodes).
- Anti-entropy delta repair: After a partition heals, the prolly tree diff (INV-FERR-047: O(d) diff) identifies which
chunks differ. RaptorQ could encode those delta chunks as repair symbols, making the sync tolerant to packet loss
without application-level retry logic.

Where it does NOT fit:

- WAL durability — Ferratomic's WAL uses CRC32 for frame integrity and BLAKE3 for checkpoint integrity. FrankenSQLite
uses RaptorQ repair symbols within WAL frames to self-heal torn writes. But Ferratomic's approach is different: torn
frames are simply truncated and discarded (the CRDT model means the lost transaction can be re-merged from another
replica). Self-healing WAL frames solve a problem Ferratomic doesn't have — the G-Set CRDT makes data loss a merge
problem, not a repair problem.
- Local storage — No benefit. RaptorQ adds overhead for single-node operation where the filesystem is the only
transport.

The deeper question:

The CRDT semilattice (Store = (P(D), ∪)) already gives you something RaptorQ provides in FrankenSQLite: tolerance to
incomplete data. If a replica receives 90% of a merge payload, it can apply what it has (set union is incremental) and
get the remaining 10% on the next anti-entropy round. FrankenSQLite needs RaptorQ because SQLite pages are atomic — you
 either have a complete page or you don't. Ferratomic's datoms are individually addressable, so partial receipt is
inherently safe.

Bottom line: RaptorQ is worth investigating for Phase 4c transport optimization (checkpoint transfer + delta sync over
lossy networks), but it's a performance optimization, not a correctness requirement. The algebraic structure already
provides the fault tolerance that RaptorQ provides at the coding-theory level. I'd rate it 5/10 for accretive value —
useful but not load-bearing. Worth a research bead if you're targeting edge/mesh deployment scenarios.
