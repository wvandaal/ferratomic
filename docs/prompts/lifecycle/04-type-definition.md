# 04 — Type Definitions (Curry-Howard)

> **Purpose**: Implement `ferratom` crate types that encode invariants at the type level.
> Types ARE propositions. Invalid states must be unrepresentable.
>
> **DoF**: Low. Types are precisely defined in spec Level 2 contracts.

---

## Phase 0: Load Context

```bash
ms load rust-formal-engineering -m --full  # Type-level encoding skill
bv --robot-next                            # Top-priority pick
br update <id> --status in_progress        # Claim it
```

---

## Principle

Every type in `ferratom` makes a proposition about valid states.
The compiler proves the proposition at every use site.
A runtime check means a type failed to encode its invariant.

---

## Demonstration: EntityId

### 1. Read the spec

From `spec/01-core-invariants.md`, INV-FERR-012 Level 2:

> EntityId = BLAKE3(content). Two entities with identical content
> have identical EntityIds. Content-addressed identity eliminates
> allocation coordination across replicas.

### 2. Implement

```rust
// ferratom/src/datom.rs

use blake3::Hash;
use std::fmt;

/// Content-addressed entity identifier.
///
/// EntityId = BLAKE3(canonical serialization of entity content).
/// Two entities with identical content produce identical EntityIds,
/// regardless of which replica created them (INV-FERR-012).
///
/// This is a newtype over `[u8; 32]`, not a raw byte array.
/// Construction is only through `EntityId::from_content()` (which hashes)
/// or `EntityId::from_bytes()` (for deserialization of known-good data).
/// There is no `EntityId::new(arbitrary_bytes)` — you cannot forge an ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId([u8; 32]);

impl EntityId {
    /// Create an EntityId by hashing content (INV-FERR-012).
    /// This is the primary constructor. All other paths are deserialization.
    pub fn from_content(content: &[u8]) -> Self {
        Self(*blake3::hash(content).as_bytes())
    }

    /// Reconstruct from serialized bytes. Used for deserialization only.
    /// Does NOT re-hash — caller asserts these bytes are a valid BLAKE3 output.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Raw bytes for serialization.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityId({})", hex::encode(&self.0[..8]))
    }
}
```

### 3. Why this encodes the invariant

| Design choice | Invariant it encodes |
|---------------|---------------------|
| `[u8; 32]` not `Vec<u8>` | Exactly 32 bytes. Cannot be wrong length. |
| No `pub` on inner field | Cannot construct with arbitrary bytes from outside the module. |
| `from_content` hashes | ID is always derived from content. Cannot be forged. |
| `from_bytes` for deserialization | Escape hatch clearly marked. Does not bypass BLAKE3 on new data. |
| `#[derive(Eq, Ord, Hash)]` | Comparison is structural on all 32 bytes. No partial equality. |

---

## Type Catalog

Implement these types in `ferratom/src/`. Each doc comment cites
the INV-FERR it encodes:

| Type | File | INV-FERR | Key property |
|------|------|----------|-------------|
| `EntityId` | `datom.rs` | 012 | Content-addressed, `[u8; 32]` |
| `Attribute` | `datom.rs` | 026 | Interned string, O(1) comparison |
| `Value` | `datom.rs` | 018 | Sum type, exact cardinality per variant |
| `Op` | `datom.rs` | 018 | `Assert` or `Retract`, nothing else |
| `TxId` | `clock.rs` | 015, 016 | HLC timestamp, total order |
| `AgentId` | `clock.rs` | 016 | Node identifier for HLC |
| `HybridClock` | `clock.rs` | 015 | Monotonic tick, NTP regression safe |
| `Frontier` | `clock.rs` | 016 | Causal cut (set of latest TxIds per agent) |
| `Datom` | `datom.rs` | 012 | 5-tuple `[e, a, v, tx, op]`, immutable |
| `Schema` | `schema.rs` | 009 | Attribute definitions, validation |
| `AttributeDef` | `schema.rs` | 009 | Value type + cardinality |
| `ValueType` | `schema.rs` | 009 | Enum of valid value types |
| `Cardinality` | `schema.rs` | 009 | `One` or `Many` |
| `FerraError` | `error.rs` | 019 | Typed errors, never panics |

---

## Design Rules

Encode exactly the valid states. Minimal cardinality: if `Op` has two
variants, use an enum with two variants (not `bool`, not `u8`).
Parse at boundaries, return typed values. Use typestate for lifecycles.
No `_ =>` wildcards on extensible enums.

```rust
/// The operation performed by a datom (INV-FERR-018).
/// Assert adds a fact. Retract removes it. No other operation exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Op {
    Assert,
    Retract,
}
```

---

## Error Type

```rust
// ferratom/src/error.rs

/// Typed error categories for Ferratomic (INV-FERR-019, NEG-FERR-001).
///
/// Callers pattern-match on the variant, not the message string.
/// - `Io`: retryable, external failure
/// - `Validation`: caller bug (bad input)
/// - `SchemaViolation`: caller bug (data doesn't match schema)
/// - `InvariantViolation`: OUR bug (an INV-FERR was violated)
#[derive(Debug)]
pub enum FerraError {
    /// External I/O failure (disk, network). Retryable.
    Io(std::io::Error),
    /// Input failed validation at system boundary. Caller bug.
    Validation(String),
    /// Data violates schema constraints. Caller bug.
    SchemaViolation(String),
    /// An INV-FERR invariant was violated. OUR bug. Include the INV ID.
    InvariantViolation(String),
}
```

---

## Checklist Per Type

1. Read the spec Level 2 contract for the type
2. Write the type with `#[derive]` for Eq, Ord, Hash, Clone as needed
3. Write a doc comment citing INV-FERR-NNN
4. Verify minimal cardinality: can any invalid state be represented?
5. Verify no `pub` fields that allow construction of invalid values
6. Run: `CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace`
7. Run: `CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings`
8. Close task: `br close <id> --reason "Type defined, encodes INV-FERR-NNN"`
