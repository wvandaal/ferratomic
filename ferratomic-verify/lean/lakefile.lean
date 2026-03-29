import Lake
open Lake DSL

package ferratomic where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

require mathlib from git
  "https://github.com/leanprover-community/mathlib4" @ "main"

@[default_target]
lean_lib Ferratomic where
  srcDir := "."
