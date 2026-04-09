/-
  Ferratomic Prolly Tree Codec — INV-FERR-045c: Leaf Chunk Codec Conformance.

  Invariants proven:
    INV-FERR-045c  Leaf Chunk Codec Conformance (T1–T5 trait-level theorems)

  The LeafChunkCodec trait is the dock for all leaf chunk codecs. Each
  concrete codec (e.g., DatomPair from INV-FERR-045a) discharges the
  round-trip obligation (T1). T2–T5 follow structurally from T1.

  Spec: spec/06-prolly-tree.md §23.9 (INV-FERR-045c)
-/

import Ferratomic.Store
import Ferratomic.ProllyTreeFoundation

/-! ## INV-FERR-045c: Leaf Chunk Codec Conformance

  A codec is a structure (encode, decode, boundary_key, codec_tag) where
  encode : Finset Datom → List UInt8 and decode : List UInt8 → Option (Finset Datom).
  The five conformance theorems formalize what it means for a codec to be
  well-behaved; any concrete codec must discharge them. -/

/-- The LeafChunkCodec trait: encode, decode, tag, boundary_key. -/
structure LeafChunkCodec where
  encode      : Finset Datom → List UInt8
  decode      : List UInt8 → Option (Finset Datom)
  codecTag    : UInt8
  boundaryKey : List UInt8 → Option DatomKey

/-- T1: Round-trip — decode is the structural inverse of encode on every
    finite datom set in the codec's domain. This is the ONLY independent
    per-codec discharge obligation. -/
def isRoundTrip (C : LeafChunkCodec) : Prop :=
  ∀ d : Finset Datom, C.decode (C.encode d) = some d

/-- T2: Determinism — encode is a pure function. In Lean's pure-functional
    model this is automatic (Lean has no notion of state or impure effects),
    so the theorem is `rfl`. -/
theorem encode_deterministic (C : LeafChunkCodec) (d : Finset Datom) :
    C.encode d = C.encode d := rfl

/-- T3: Injectivity — different inputs produce different outputs. -/
def isInjective (C : LeafChunkCodec) : Prop :=
  ∀ d₁ d₂ : Finset Datom, d₁ ≠ d₂ → C.encode d₁ ≠ C.encode d₂

/-- The structural theorem: round-trip implies injectivity. This is the
    proof referenced in the Level 0 algebraic law for T3. -/
theorem roundtrip_implies_injective (C : LeafChunkCodec)
    (h : isRoundTrip C) : isInjective C := by
  intro d₁ d₂ h_neq h_eq
  have r₁ : C.decode (C.encode d₁) = some d₁ := h d₁
  have r₂ : C.decode (C.encode d₂) = some d₂ := h d₂
  rw [h_eq] at r₁
  -- r₁ : C.decode (C.encode d₂) = some d₁
  -- r₂ : C.decode (C.encode d₂) = some d₂
  -- Functional equality of decode forces the somethings to agree.
  have h_some : some d₁ = some d₂ := by rw [← r₁, r₂]
  exact h_neq (Option.some.inj h_some)

/-! ### T4: Framework Fingerprint Invariance

  The framework fingerprint is computed at the datom level via INV-FERR-086's
  canonical_bytes XORed per INV-FERR-074. The codec is transparent to the
  fingerprint — a round-trip codec preserves it. -/

/-- Canonical byte representation of a datom (INV-FERR-086). -/
axiom canonicalDatomBytes : Datom → List UInt8

/-- BLAKE3 on byte sequences producing a 32-byte vector. -/
axiom blake3Bytes : List UInt8 → ByteVec 32

/-- XOR two 32-byte vectors. -/
axiom xorByteVecs : ByteVec 32 → ByteVec 32 → ByteVec 32

/-- Framework fingerprint: XOR of per-datom BLAKE3 hashes.
    Uses an axiomatized fold over Finset to avoid needing a Comm instance
    on the XOR operation at this abstraction level. -/
axiom frameworkFingerprint : Finset Datom → ByteVec 32

/-- T4: For a round-trip codec, the framework fingerprint computed
    directly from D equals the framework fingerprint computed from ANY
    `d'` returned by `decode(encode(D))`. -/
theorem fingerprint_codec_invariant
    (C : LeafChunkCodec) (h : isRoundTrip C) (d : Finset Datom) :
    ∀ d' : Finset Datom,
      C.decode (C.encode d) = some d' →
      frameworkFingerprint d = frameworkFingerprint d' := by
  intro d' h_dec
  have r : some d = some d' := by rw [← h d]; exact h_dec
  have h_eq : d = d' := Option.some.inj r
  rw [h_eq]

/-- T5: Order independence — encode is a function on Finset, not on List.
    By Lean's type system, Finset has no notion of order, so encode applied
    to two equal Finsets returns equal results by definitional equality. -/
theorem encode_order_independent (C : LeafChunkCodec)
    (l₁ l₂ : List Datom) (h : l₁.toFinset = l₂.toFinset) :
    C.encode l₁.toFinset = C.encode l₂.toFinset := by
  rw [h]

/-! ### Conformance Bundle -/

/-- A codec is conforming iff it satisfies T1 (the only propositional
    obligation requiring per-codec proof). -/
def isConforming (C : LeafChunkCodec) : Prop :=
  isRoundTrip C

/-- Conformance implies all five theorem statements. The structure of the
    proof makes the dependence visible: T1 is the only independent
    obligation; everything else is derived. -/
theorem conforming_implies_all_five
    (C : LeafChunkCodec) (h : isConforming C) :
    isRoundTrip C ∧
    (∀ d, C.encode d = C.encode d) ∧                              -- T2
    isInjective C ∧                                               -- T3
    (∀ d d',                                                      -- T4
        C.decode (C.encode d) = some d' →
        frameworkFingerprint d = frameworkFingerprint d') ∧
    (∀ l₁ l₂ : List Datom, l₁.toFinset = l₂.toFinset →            -- T5
              C.encode l₁.toFinset = C.encode l₂.toFinset) := by
  refine ⟨h, ?_, ?_, ?_, ?_⟩
  · intro d; rfl
  · exact roundtrip_implies_injective C h
  · intro d d' h_dec; exact fingerprint_codec_invariant C h d d' h_dec
  · intro l₁ l₂ h_eq; rw [h_eq]
