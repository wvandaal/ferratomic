# Migration Path from braid-kernel to Ferratomic

> Extracted from `FERRATOMIC_ARCHITECTURE.md` section 13.
> Documents the migration path from braid-kernel to ferratomic.

**Disclaimer:** This document describes design intent for the braid-kernel migration.
Implementation is Phase 5. Code examples are illustrative, not compiled.

---

## Format Compatibility (Reads Existing .edn Files)

Ferratomic reads the existing `.braid/txns/` EDN transaction files produced by the current
braid-kernel `DiskLayout`. This is the Level 3 recovery path (section 5) applied as a
migration strategy:

```
Current format:
  .braid/
    txns/
      ab/
        ab1234...5678.edn  -- per-transaction EDN files
      cd/
        cd9abc...def0.edn
    store.bin               -- binary cache

Ferratomic reads:
  1. store.bin (if present and valid) as the checkpoint
  2. txns/ directory as the EDN transaction files
  3. Generates WAL and checkpoint in Ferratomic format
  4. Continues operation in Ferratomic format
```

No manual migration step is required. The first `braid` command after upgrading to
Ferratomic performs the migration automatically.

## API Adapter Pattern

The current braid-kernel `Store` interface is preserved via an adapter:

```rust
/// Adapter that wraps a Ferratomic Engine to provide the braid-kernel Store API.
///
/// This allows all existing braid-kernel code (harvest, seed, guidance, bilateral,
/// methodology, topology, etc.) to work unchanged with Ferratomic underneath.
pub struct StoreAdapter {
    engine: Engine<LocalTransport>,
}

impl StoreAdapter {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, StoreError> {
        // Translate braid-kernel Transaction to Ferratomic Transaction
        // Submit via Engine
        // Return braid-kernel TxReceipt
    }

    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        // Read from current Ferratomic snapshot
    }

    pub fn len(&self) -> usize {
        self.engine.snapshot().datom_count()
    }

    // ... all other Store methods ...
}
```

## Observer Bridge for MaterializedViews

The current `MaterializedViews` is updated in-line during `Store::apply_datoms()`. With
Ferratomic, it registers as a `DatomObserver` (section 6):

```rust
// Before (braid-kernel Store, synchronous in-line update):
fn apply_datoms(&mut self, datoms: &[Datom]) {
    for d in datoms {
        self.views.apply_datom(d);  // synchronous, in the write path
    }
}

// After (Ferratomic, observer-based):
let views_observer = MaterializedViewsObserver::new(views.clone());
engine.register_observer(Box::new(views_observer));
// Views are updated asynchronously after each commit
```

## store.bin Migration

The current `store.bin` (bincode-serialized `Store`) is readable as a Ferratomic checkpoint
at migration time. The field layout is compatible because Ferratomic uses the same `Datom`
type (from the `ferratom` leaf crate, which is extracted from the current
`braid-kernel::datom` module).

After the first successful checkpoint in Ferratomic format, the old `store.bin` is no longer
needed (but is not deleted -- C1 applies to all artifacts).

## INV-STORE to INV-FERR Mapping

Every historical INV-STORE invariant has a corresponding INV-FERR invariant in the canonical
modular Ferratomic spec that refines it:

| INV-STORE | INV-FERR | Relationship |
|-----------|----------|-------------|
| INV-STORE-001 (Append-only) | INV-FERR-004 (Monotonicity) | FERR adds WAL durability |
| INV-STORE-002 (Growth) | INV-FERR-004 (Monotonicity) | Same property, crash-safe |
| INV-STORE-003 (Content-addr) | INV-FERR-012 (Content-addr) | Same property, BLAKE3 spec |
| INV-STORE-004 (Commutativity) | INV-FERR-001 (Commutativity) | Same property, crash-safe |
| INV-STORE-005 (Associativity) | INV-FERR-002 (Associativity) | Same property, crash-safe |
| INV-STORE-006 (Idempotency) | INV-FERR-003 (Idempotency) | Same property, crash-safe |
| INV-STORE-009 (Durability) | INV-FERR-008 (WAL Fsync) | FERR specifies the mechanism |
| INV-STORE-010 (Causal order) | INV-FERR-007 (Write Linear.) | FERR specifies epoch ordering |
| INV-STORE-011 (HLC Mono.) | INV-FERR-011 (Observer Mono.) | FERR extends to observers |
| INV-STORE-012 (LIVE) | INV-FERR-005 (Index Bijection) | FERR specifies full bijection |
| INV-STORE-013 (Snapshot) | INV-FERR-006 (Snapshot Iso.) | FERR specifies epoch mechanism |
