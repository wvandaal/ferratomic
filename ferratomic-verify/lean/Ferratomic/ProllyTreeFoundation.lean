/-
  Ferratomic Prolly Tree Foundation — shared types and axioms for prolly tree proofs.

  This module defines the common abstractions used across all prolly tree
  invariant files: Hash, blake3, ChunkStore, ProllyTree, etc.

  Spec: spec/06-prolly-tree.md §23.9
  Foundation: spec/00-preamble.md §23.0.4
-/

import Ferratomic.Store
import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Lattice.Basic

/-! ### Hash and BLAKE3 Axioms (§23.0.4) -/

/-- Abstract 32-byte hash type (BLAKE3 output). -/
axiom Hash : Type
axiom Hash.instDecidableEq : DecidableEq Hash
noncomputable instance : DecidableEq Hash := Hash.instDecidableEq

/-- Abstract BLAKE3 hash function on byte sequences. -/
axiom blake3 : List UInt8 → Hash

/-- **Axiom**: BLAKE3 collision resistance — different inputs produce different
    outputs. This is the standard cryptographic assumption from §23.0.4. -/
axiom blake3_injective : ∀ {a b : List UInt8}, blake3 a = blake3 b → a = b

/-- Forward direction: same content → same hash (congruence). -/
theorem blake3_deterministic (a b : List UInt8) (h : a = b) :
    blake3 a = blake3 b :=
  congrArg blake3 h

/-! ### ByteVec — fixed-length byte vectors -/

/-- A fixed-length byte vector of length n. -/
def ByteVec (n : Nat) : Type := { v : List UInt8 // v.length = n }

/-- Zero vector of length n. -/
def ByteVec.zero (n : Nat) : ByteVec n :=
  ⟨List.replicate n 0, List.length_replicate ..⟩

/-! ### DatomKey -/

/-- A datom key is a byte sequence used for sorting and boundary detection. -/
abbrev DatomKey := List UInt8

/-! ### Chunk and ChunkStore Abstractions -/

/-- Abstract chunk type (serialized node content). -/
axiom Chunk : Type
axiom Chunk.instDecidableEq : DecidableEq Chunk
noncomputable instance : DecidableEq Chunk := Chunk.instDecidableEq

/-- Extract the content bytes of a chunk. -/
axiom chunk_content : Chunk → List UInt8

/-- Content-addressing: chunk address is BLAKE3 of content. -/
noncomputable def chunk_addr (c : Chunk) : Hash := blake3 (chunk_content c)

/-- Abstract chunk store: a finite set of (address, chunk) pairs. -/
abbrev ChunkStoreSet := Finset (Hash × Chunk)

/-! ### ProllyTree Model -/

/-- Abstract prolly tree type for use in diff/snapshot/transfer theorems. -/
axiom ProllyTree : Type

/-- Key-value pair type used in prolly tree leaves. -/
abbrev Key := List UInt8
abbrev Value := List UInt8

/-- Extract the key-value set from a prolly tree. -/
noncomputable axiom key_values : ProllyTree → Finset (Key × Value)

/-- Alias for kv_set used in diff theorems. -/
noncomputable abbrev kv_set := key_values

/-- Root hash of a prolly tree (content-addressed). -/
axiom tree_root_hash : ProllyTree → Hash

/-- All chunks belonging to a prolly tree. -/
axiom chunks_of : ProllyTree → Finset (Hash × Chunk)

/-- All chunks in a chunk store. -/
abbrev chunks_in (store : ChunkStoreSet) : Finset (Hash × Chunk) := store

/-- Resolve a tree from a chunk store given its root hash. -/
axiom tree_resolve : ChunkStoreSet → Hash → ProllyTree

/-- **Axiom**: Resolving a tree's root hash from a store containing all its
    chunks recovers the original tree. -/
axiom chunk_retrieve_correct (store : ChunkStoreSet) (root : Hash)
    (h : ∃ t : ProllyTree, tree_root_hash t = root ∧ chunks_of t ⊆ store) :
    ∃ t, tree_root_hash t = root ∧ tree_resolve store root = t

/-- Retrieve chunk content from store by address. -/
axiom chunk_retrieve : ChunkStoreSet → Hash → List UInt8

/-- **Axiom**: chunk_retrieve returns the content of the chunk at the given address. -/
axiom chunk_retrieve_eq_content (store : ChunkStoreSet) (addr : Hash)
    (content : List UInt8)
    (h : (addr, content) ∈ store.image (fun ⟨a, c⟩ => (a, chunk_content c))) :
    chunk_retrieve store addr = content

/-- Reachable chunks from a root hash in a store. -/
axiom reachable_from : Hash → Finset (Hash × Chunk) → Finset Chunk

/-- History independence: same key-value set → same root hash. -/
axiom history_independence_root :
    ∀ {t1 t2 : ProllyTree}, key_values t1 = key_values t2 →
    tree_root_hash t1 = tree_root_hash t2

/-- Children's chunks are a subset of parent's chunks. -/
axiom chunks_subset_of_children {t : ProllyTree} {store : ChunkStoreSet} :
    chunks_of t ⊆ store → ∀ child_hash, child_hash ∈ chunks_of t →
    child_hash ∈ store

/-! ### Rolling Hash (for history independence) -/

/-- Abstract rolling hash function for boundary detection. -/
axiom rolling_hash : List UInt8 → Nat

/-- Build prolly tree root from sorted entries and pattern width.
    This is the abstract builder used in history independence theorems. -/
axiom prolly_root_sorted : List (List UInt8 × List UInt8) → Nat → Hash
