import Lake
open Lake DSL

package ferratomic where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

require mathlib from git
  "https://github.com/leanprover-community/mathlib4" @ "00fca21215c51e01d2d90adc3b3d273de909050b"

@[default_target]
lean_lib Ferratomic where
  srcDir := "."
