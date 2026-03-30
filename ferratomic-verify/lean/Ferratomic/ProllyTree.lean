/-
  Ferratomic Prolly Tree — content-addressing, history independence, snapshot proofs.

  Invariants proven:
    INV-FERR-045  Chunk content addressing (same content → same address)
    INV-FERR-046  Prolly tree history independence (same set → same tree)
    INV-FERR-049  Snapshot = root hash (snapshot creation is O(1))

  Spec: spec/06-prolly-tree.md §23.9

  NOTE: Hash functions are axiomatized. Collision resistance is a
  cryptographic assumption, not a mathematical certainty (same as VKN.lean).
-/

import Ferratomic.Store

/-! ### Content-Addressing Axioms -/

/-- Abstract content hash type (BLAKE3 output). -/
axiom ContentHash : Type
axiom ContentHash.instDecidableEq : DecidableEq ContentHash
noncomputable instance : DecidableEq ContentHash := ContentHash.instDecidableEq

/-- Content-addressing: deterministic hash from chunk data to address. -/
axiom content_hash : List Nat → ContentHash

/-- **Axiom**: Determinism — same input always produces same hash. -/
axiom content_hash_deterministic : ∀ (a b : List Nat), a = b → content_hash a = content_hash b

/-- **Axiom**: Collision resistance — different inputs produce different hashes.
    This is the standard cryptographic assumption for BLAKE3. -/
axiom content_hash_injective : ∀ (a b : List Nat), a ≠ b → content_hash a ≠ content_hash b

/-! ## INV-FERR-045: Chunk Content Addressing

  Identical chunk content produces identical hash addresses.
  Different content produces different addresses (collision resistance).
  Deduplication is structural — storing the same content twice is a no-op. -/

/-- INV-FERR-045 forward: same content → same address (determinism). -/
theorem chunk_content_identity (d1 d2 : List Nat) (h : d1 = d2) :
    content_hash d1 = content_hash d2 :=
  content_hash_deterministic d1 d2 h

/-- INV-FERR-045 backward: different content → different address (collision resistance). -/
theorem chunk_content_distinct (d1 d2 : List Nat) (h : d1 ≠ d2) :
    content_hash d1 ≠ content_hash d2 :=
  content_hash_injective d1 d2 h

/-- INV-FERR-045 biconditional: content equality ↔ hash equality. -/
theorem chunk_content_iff (d1 d2 : List Nat) :
    d1 = d2 ↔ content_hash d1 = content_hash d2 := by
  constructor
  · exact content_hash_deterministic d1 d2
  · intro h; by_contra hne; exact content_hash_injective d1 d2 hne h

/-- Deduplication in a chunk store: inserting the same entry twice is a no-op. -/
theorem chunk_store_idempotent (s : Finset (ContentHash × List Nat)) (data : List Nat) :
    let entry := (content_hash data, data)
    s ∪ {entry} ∪ {entry} = s ∪ {entry} := by
  simp

/-! ## INV-FERR-046: Prolly Tree History Independence

  The tree structure is a function of the final key-value set,
  not the insertion history. Modeled via an axiomatized prolly_root function
  whose signature takes a Finset (inherently order-independent). -/

/-- Key-value store modeled as Finset of pairs. -/
abbrev KVStore := Finset (Nat × Nat)

/-- **Axiom**: Prolly tree root hash is a deterministic function of the key-value set. -/
axiom prolly_root : KVStore → ContentHash

/-- **Axiom**: Same key-value set produces same root (determinism). -/
axiom prolly_root_deterministic : ∀ (s1 s2 : KVStore), s1 = s2 → prolly_root s1 = prolly_root s2

/-- **Axiom**: Different key-value sets produce different roots (faithfulness). -/
axiom prolly_root_injective : ∀ (s1 s2 : KVStore), prolly_root s1 = prolly_root s2 → s1 = s2

/-- INV-FERR-046: History independence — merge order doesn't affect the root hash.
    This follows from Finset being order-independent + prolly_root determinism. -/
theorem history_independence (a b : KVStore) :
    prolly_root (a ∪ b) = prolly_root (b ∪ a) :=
  prolly_root_deterministic _ _ (Finset.union_comm a b)

/-- Merge commutativity extends to prolly tree roots. -/
theorem prolly_merge_comm (a b : KVStore) :
    prolly_root (a ∪ b) = prolly_root (b ∪ a) :=
  history_independence a b

/-- Merge associativity extends to prolly tree roots. -/
theorem prolly_merge_assoc (a b c : KVStore) :
    prolly_root ((a ∪ b) ∪ c) = prolly_root (a ∪ (b ∪ c)) :=
  prolly_root_deterministic _ _ (Finset.union_assoc a b c)

/-! ## INV-FERR-049: Snapshot = Root Hash

  A snapshot is uniquely identified by the prolly tree root hash.
  Same store → same snapshot. Different store → different snapshot. -/

/-- Snapshot is the prolly tree root hash. -/
noncomputable def snapshot_hash (s : KVStore) : ContentHash := prolly_root s

/-- INV-FERR-049: Same content produces same snapshot. -/
theorem snapshot_deterministic (s1 s2 : KVStore) (h : s1 = s2) :
    snapshot_hash s1 = snapshot_hash s2 :=
  prolly_root_deterministic s1 s2 h

/-- INV-FERR-049: Different content produces different snapshot (faithfulness). -/
theorem snapshot_faithful (s1 s2 : KVStore) (h : snapshot_hash s1 = snapshot_hash s2) :
    s1 = s2 :=
  prolly_root_injective s1 s2 h

/-- Snapshot identity is biconditional: s1 = s2 ↔ snapshot(s1) = snapshot(s2). -/
theorem snapshot_iff (s1 s2 : KVStore) :
    s1 = s2 ↔ snapshot_hash s1 = snapshot_hash s2 :=
  ⟨snapshot_deterministic s1 s2, snapshot_faithful s1 s2⟩
