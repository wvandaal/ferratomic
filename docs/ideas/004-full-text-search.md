That's a fascinating question. Let me think through what this would mean architecturally.

What Quickwit/Tantivy Does

Quickwit is a distributed search engine built on Tantivy (a Rust Lucene equivalent). It provides: full-text search over structured/semi-structured data, distributed indexing, sub-second search over petabyte-scale log data, S3-native storage,
 and Kafka-native ingestion.

What "Federated Tantivy on Ferratomic" Would Mean

The Natural Mapping

Ferratomic's datom model [entity, attribute, value, tx, op] is already a universal fact store. A federated search layer would treat datoms as the atomic indexable unit:

- Entity → document ID
- Attribute → field name
- Value → field value (already typed: String, Keyword, Long, Double, Bytes, etc.)
- TxId → ingestion timestamp (HLC gives you causal ordering for free)
- Op → tombstone semantics (Retract = delete from index)

The CRDT merge property (Store = (P(D), ∪)) means index shards merge by set union — no coordination protocol needed. This is something Quickwit/Elasticsearch/Solr all struggle with. They need complex shard rebalancing, replica sync, and
split-brain resolution. Ferratomic's algebraic foundation eliminates all of that.

What You'd Build

1. Inverted index as a derived view — like LIVE view but for full-text. For every Value::String or Value::Keyword datom, maintain a token→entity posting list. This is a pure function of the datom set, so it's CRDT-mergeable by construction.
The index IS the data, not a separate system.
2. Prolly tree as the segment format — Phase 4b's prolly tree (INV-FERR-045-050) gives you content-addressed, O(d log n) diffable storage. This is analogous to Lucene segments but with structural sharing and chunk-level federation transfer.
Two nodes can sync their search indexes by exchanging only the chunks that differ.
3. CALM-compliant query fan-out — INV-FERR-033/037 already specify this for Datalog. Full-text search queries are monotonic (more datoms → more results), so they can fan out to shards and merge results via set union. Ranking/scoring is
non-monotonic (requires global IDF), but BM25 with local IDF is a well-understood approximation.
4. Temporal search for free — every datom has a TxId. "What did we know about X at time T?" is a native query. This is something no existing search engine does well. Quickwit can search logs by timestamp, but Ferratomic can search any fact
by causal time, including facts that were later retracted.

The Architectural Question: ON vs. IN

ON Ferratomic (separate crate, consumes datoms):
- ferratomic-search crate that subscribes to datom commits via the Observer API
- Maintains its own inverted index (Tantivy or custom)
- Queries hit the search index, then resolve entities back through Ferratomic
- Cleaner separation. Ferratomic stays a database engine. Search is a consumer.

IN Ferratomic (as a native index type):
- Inverted index as another IndexBackend implementation alongside EAVT/AEVT/VAET/AVET
- Every transact automatically updates the search index atomically
- Queries can combine structured (Datalog) and unstructured (full-text) in one evaluation
- More powerful but violates "not an application framework" (GOALS.md anti-goal)

The project's own identity statement says "not an application framework" and "not a retrieval heuristic." That strongly suggests ON, not IN. Ferratomic provides the substrate; search builds on top.
