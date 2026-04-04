#![forbid(unsafe_code)]

//! Stateright model for INV-FERR-015 (HLC Monotonicity).
//!
//! Models a single agent's Hybrid Logical Clock under adversarial wall-clock
//! behavior: advance, stall, and NTP backward regression. The HLC algorithm
//! from spec §02-concurrency guarantees that every tick produces a
//! `(physical, logical)` pair strictly greater than the previous in
//! lexicographic order, regardless of wall-clock dynamics.
//!
//! Properties verified:
//! - **Safety**: `inv_ferr_015_hlc_monotonicity` — the sequence of tick
//!   outputs is strictly increasing in lexicographic `(physical, logical)`
//!   order. This directly encodes Level 0: `∀ consecutive ticks t₁, t₂:
//!   tick(t₂) > tick(t₁)` in the total order.
//! - **Liveness**: `inv_ferr_015_regression_monotonicity_reachable` — a state
//!   where `wall_clock < prev_physical` and `tick_count >= 2` is reachable,
//!   confirming the model explores the NTP backward-correction scenario.
//! - **Liveness**: `inv_ferr_015_tick_after_advance_reachable` — a state with
//!   at least 3 ticks is reachable, confirming the model explores multi-tick
//!   histories.

use stateright::{Model, Property};

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Full state of the HLC monotonicity model for a single agent.
///
/// The `tick_outputs` vector records every `(physical, logical)` pair produced
/// by the HLC algorithm. INV-FERR-015 requires this sequence to be strictly
/// increasing in lexicographic order.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HlcState {
    /// Current wall-clock value. Can advance, stall, or regress.
    pub wall_clock: u8,
    /// HLC physical component from the last tick.
    pub prev_physical: u8,
    /// HLC logical component from the last tick.
    pub prev_logical: u8,
    /// Number of ticks issued (bounds the model).
    pub tick_count: u8,
    /// Sequence of `(physical, logical)` outputs from all ticks.
    /// INV-FERR-015 requires this to be strictly increasing in
    /// lexicographic order.
    pub tick_outputs: Vec<(u8, u8)>,
}

/// Actions available to the Stateright checker.
///
/// Wall-clock mutations are decoupled from ticks so the checker can explore
/// all interleavings: advance-then-tick, regress-then-tick, stall-then-tick,
/// multiple advances before a tick, etc.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum HlcAction {
    /// Wall clock advances by 1.
    WallClockAdvance,
    /// Wall clock stays the same (stall, e.g., fast successive calls).
    WallClockStall,
    /// Wall clock decreases by 1 (NTP backward correction).
    WallClockRegress,
    /// Issue an HLC tick — applies the spec algorithm from §02-concurrency.
    Tick,
}

// ---------------------------------------------------------------------------
// Model configuration
// ---------------------------------------------------------------------------

/// Bounded Stateright model for INV-FERR-015 HLC monotonicity.
///
/// The model explores all interleavings of wall-clock mutations and HLC ticks
/// within a finite domain. The defaults (`max_wall_clock: 3`, `max_ticks: 5`,
/// `max_logical: 6`) produce a state space that is small enough for BFS
/// exhaustion yet large enough to cover all interesting scenarios: advance,
/// stall, regression, multi-tick sequences, and logical counter buildup.
#[derive(Clone, Debug)]
pub struct HlcModel {
    /// Maximum wall-clock value (inclusive upper bound).
    pub max_wall_clock: u8,
    /// Maximum number of ticks before bounding (inclusive upper bound).
    pub max_ticks: u8,
    /// Maximum logical counter value (inclusive upper bound; prevents `u8`
    /// overflow in the bounded model).
    pub max_logical: u8,
}

impl HlcModel {
    /// Constructs a bounded HLC model.
    pub const fn new(max_wall_clock: u8, max_ticks: u8, max_logical: u8) -> Self {
        Self {
            max_wall_clock,
            max_ticks,
            max_logical,
        }
    }
}

impl Default for HlcModel {
    fn default() -> Self {
        Self::new(3, 5, 6)
    }
}

// ---------------------------------------------------------------------------
// Model implementation
// ---------------------------------------------------------------------------

impl Model for HlcModel {
    type State = HlcState;
    type Action = HlcAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![HlcState {
            wall_clock: 0,
            prev_physical: 0,
            prev_logical: 0,
            tick_count: 0,
            tick_outputs: Vec::new(),
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        // Wall-clock mutations are always available (within bounds).
        if state.wall_clock < self.max_wall_clock {
            actions.push(HlcAction::WallClockAdvance);
        }
        // Stall is always available — it is a no-op on wall_clock.
        actions.push(HlcAction::WallClockStall);
        if state.wall_clock > 0 {
            actions.push(HlcAction::WallClockRegress);
        }
        // Tick is available if we haven't exhausted the tick budget.
        if state.tick_count < self.max_ticks {
            actions.push(HlcAction::Tick);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            HlcAction::WallClockAdvance => {
                if state.wall_clock >= self.max_wall_clock {
                    return None;
                }
                next.wall_clock = state.wall_clock.checked_add(1)?;
            }
            HlcAction::WallClockStall => {
                // No change to wall_clock — explicitly a no-op.
            }
            HlcAction::WallClockRegress => {
                if state.wall_clock == 0 {
                    return None;
                }
                next.wall_clock = state.wall_clock.checked_sub(1)?;
            }
            HlcAction::Tick => {
                if state.tick_count >= self.max_ticks {
                    return None;
                }

                // HLC algorithm from spec §02-concurrency Level 0:
                //   pt = max(prev.physical, wall_clock)
                //   if pt == prev.physical:
                //     logical = prev.logical + 1
                //   else:
                //     logical = 0
                let pt = state.wall_clock.max(state.prev_physical);
                let log = if pt == state.prev_physical {
                    // Wall clock stalled or regressed — increment logical.
                    let candidate = state.prev_logical.checked_add(1)?;
                    // Guard against exceeding the bounded logical domain.
                    if candidate > self.max_logical {
                        return None;
                    }
                    candidate
                } else {
                    // Wall clock advanced past prev_physical — reset logical.
                    0
                };

                next.prev_physical = pt;
                next.prev_logical = log;
                next.tick_count = state.tick_count.checked_add(1)?;
                next.tick_outputs.push((pt, log));
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.tick_count <= self.max_ticks
            && state.wall_clock <= self.max_wall_clock
            && state.prev_logical <= self.max_logical
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // INV-FERR-015 Safety: The sequence of tick outputs is strictly
            // increasing in lexicographic (physical, logical) order.
            //
            // This directly encodes Level 0:
            //   ∀ consecutive ticks t₁, t₂: tick(t₂) > tick(t₁)
            //
            // The (u8, u8) tuple comparison in Rust is lexicographic by
            // default (Ord for tuples), so `w[0] < w[1]` is the correct
            // strict lexicographic comparison.
            Property::always(
                "inv_ferr_015_hlc_monotonicity",
                |_: &HlcModel, state: &HlcState| state.tick_outputs.windows(2).all(|w| w[0] < w[1]),
            ),
            // Liveness: A state where the wall clock has regressed below
            // prev_physical AND at least 2 ticks have occurred is reachable.
            // This confirms the model exercises the NTP backward-correction
            // scenario — the most interesting case for HLC monotonicity.
            Property::sometimes(
                "inv_ferr_015_regression_monotonicity_reachable",
                |_: &HlcModel, state: &HlcState| {
                    state.wall_clock < state.prev_physical && state.tick_count >= 2
                },
            ),
            // Liveness: A state with at least 3 ticks is reachable.
            // Confirms the model explores multi-tick histories, not just
            // trivial 0-or-1-tick paths.
            Property::sometimes(
                "inv_ferr_015_tick_after_advance_reachable",
                |_: &HlcModel, state: &HlcState| state.tick_count >= 3,
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use stateright::{Checker, Model};

    use super::{HlcAction, HlcModel, HlcState};

    /// Helper: apply a sequence of actions to the model from its initial state.
    fn apply_sequence(model: &HlcModel, actions: &[HlcAction]) -> HlcState {
        let mut state = model
            .init_states()
            .into_iter()
            .next()
            .expect("INV-FERR-015: model must have an initial state");
        for action in actions {
            state = model.next_state(&state, action.clone()).unwrap_or_else(|| {
                panic!(
                    "INV-FERR-015: action {:?} must succeed from state {:?}",
                    action, state
                )
            });
        }
        state
    }

    // -- Unit tests for individual transitions --

    #[test]
    fn inv_ferr_015_wall_clock_advance() {
        let model = HlcModel::default();
        let state = apply_sequence(&model, &[HlcAction::WallClockAdvance]);

        assert_eq!(
            state.wall_clock, 1,
            "INV-FERR-015: WallClockAdvance must increment wall_clock by 1"
        );
        assert!(
            state.tick_outputs.is_empty(),
            "INV-FERR-015: wall-clock mutation must not produce tick output"
        );
    }

    #[test]
    fn inv_ferr_015_wall_clock_stall() {
        let model = HlcModel::default();
        let state = apply_sequence(&model, &[HlcAction::WallClockStall]);

        assert_eq!(
            state.wall_clock, 0,
            "INV-FERR-015: WallClockStall must not change wall_clock"
        );
    }

    #[test]
    fn inv_ferr_015_wall_clock_regress() {
        let model = HlcModel::default();
        let state = apply_sequence(
            &model,
            &[HlcAction::WallClockAdvance, HlcAction::WallClockRegress],
        );

        assert_eq!(
            state.wall_clock, 0,
            "INV-FERR-015: WallClockRegress must decrement wall_clock by 1"
        );
    }

    #[test]
    fn inv_ferr_015_wall_clock_regress_at_zero_is_invalid() {
        let model = HlcModel::default();
        let init = model
            .init_states()
            .into_iter()
            .next()
            .expect("INV-FERR-015: model must have an initial state");

        assert!(
            model
                .next_state(&init, HlcAction::WallClockRegress)
                .is_none(),
            "INV-FERR-015: WallClockRegress at wall_clock=0 must return None"
        );
    }

    #[test]
    fn inv_ferr_015_tick_from_initial_state() {
        let model = HlcModel::default();
        let state = apply_sequence(&model, &[HlcAction::Tick]);

        assert_eq!(
            state.prev_physical, 0,
            "INV-FERR-015: first tick with wall_clock=0 must set physical=0"
        );
        assert_eq!(
            state.prev_logical, 1,
            "INV-FERR-015: first tick with wall_clock=0 must increment logical (0 == prev_physical)"
        );
        assert_eq!(
            state.tick_count, 1,
            "INV-FERR-015: tick_count must increment"
        );
        assert_eq!(
            state.tick_outputs,
            vec![(0, 1)],
            "INV-FERR-015: tick_outputs must record (0, 1)"
        );
    }

    #[test]
    fn inv_ferr_015_tick_after_advance() {
        let model = HlcModel::default();
        let state = apply_sequence(&model, &[HlcAction::WallClockAdvance, HlcAction::Tick]);

        assert_eq!(
            state.prev_physical, 1,
            "INV-FERR-015: tick after advance must set physical to wall_clock=1"
        );
        assert_eq!(
            state.prev_logical, 0,
            "INV-FERR-015: tick after advance must reset logical to 0 \
             (wall_clock > prev_physical)"
        );
        assert_eq!(
            state.tick_outputs,
            vec![(1, 0)],
            "INV-FERR-015: tick_outputs must record (1, 0)"
        );
    }

    #[test]
    fn inv_ferr_015_tick_after_stall() {
        let model = HlcModel::default();
        // Advance to 1, tick (sets prev_physical=1), stall, tick again.
        let state = apply_sequence(
            &model,
            &[
                HlcAction::WallClockAdvance,
                HlcAction::Tick,
                HlcAction::WallClockStall,
                HlcAction::Tick,
            ],
        );

        assert_eq!(
            state.prev_physical, 1,
            "INV-FERR-015: tick after stall must keep physical at prev_physical"
        );
        assert_eq!(
            state.prev_logical, 1,
            "INV-FERR-015: tick after stall must increment logical"
        );
        assert_eq!(
            state.tick_outputs,
            vec![(1, 0), (1, 1)],
            "INV-FERR-015: consecutive stall ticks must produce strictly increasing outputs"
        );
        assert!(
            state.tick_outputs[0] < state.tick_outputs[1],
            "INV-FERR-015: (1,0) < (1,1) in lexicographic order"
        );
    }

    /// The critical NTP regression scenario from the bead specification:
    /// advance wall clock to 2, tick (output (2,0)), regress wall clock to 0,
    /// tick again. The HLC must produce (2,1) — physical holds at 2 because
    /// `max(0, 2) = 2`, logical increments because `pt == prev_physical`.
    #[test]
    fn inv_ferr_015_ntp_regression_scenario() {
        let model = HlcModel::default();
        let state = apply_sequence(
            &model,
            &[
                HlcAction::WallClockAdvance, // wall_clock=1
                HlcAction::WallClockAdvance, // wall_clock=2
                HlcAction::Tick,             // output (2,0), prev_physical=2
                HlcAction::WallClockRegress, // wall_clock=1
                HlcAction::WallClockRegress, // wall_clock=0
                HlcAction::Tick,             // output must be (2,1)
            ],
        );

        assert_eq!(
            state.tick_outputs,
            vec![(2, 0), (2, 1)],
            "INV-FERR-015: after NTP regression, physical holds at max(0,2)=2, \
             logical increments to 1"
        );
        assert!(
            state.tick_outputs[0] < state.tick_outputs[1],
            "INV-FERR-015: tick output after regression must be strictly greater"
        );
        assert!(
            state.wall_clock < state.prev_physical,
            "INV-FERR-015: wall_clock=0 < prev_physical=2 confirms regression scenario"
        );
    }

    /// Multiple consecutive ticks without wall-clock change: logical counter
    /// must increment each time, maintaining strict monotonicity.
    #[test]
    fn inv_ferr_015_consecutive_stall_ticks() {
        let model = HlcModel::default();
        let state = apply_sequence(
            &model,
            &[
                HlcAction::Tick, // (0, 1)  — initial: prev=(0,0), pt=max(0,0)=0, log=0+1=1
                HlcAction::Tick, // (0, 2)
                HlcAction::Tick, // (0, 3)
            ],
        );

        assert_eq!(
            state.tick_outputs,
            vec![(0, 1), (0, 2), (0, 3)],
            "INV-FERR-015: consecutive stall ticks must produce incrementing logical values"
        );
        assert!(
            state.tick_outputs.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-015: all consecutive pairs must be strictly increasing"
        );
    }

    /// After wall-clock regression and recovery: advance past the previous
    /// physical, verify logical resets to 0.
    #[test]
    fn inv_ferr_015_advance_past_regression_resets_logical() {
        let model = HlcModel::default();
        let state = apply_sequence(
            &model,
            &[
                HlcAction::WallClockAdvance, // wall_clock=1
                HlcAction::Tick,             // (1, 0)
                HlcAction::WallClockRegress, // wall_clock=0
                HlcAction::Tick,             // (1, 1) — physical holds
                HlcAction::WallClockAdvance, // wall_clock=1
                HlcAction::WallClockAdvance, // wall_clock=2
                HlcAction::Tick,             // (2, 0) — wall_clock > prev_physical, reset
            ],
        );

        assert_eq!(
            state.tick_outputs,
            vec![(1, 0), (1, 1), (2, 0)],
            "INV-FERR-015: advancing past prev_physical must reset logical to 0"
        );
        assert!(
            state.tick_outputs.windows(2).all(|w| w[0] < w[1]),
            "INV-FERR-015: strict monotonicity must hold across regression recovery"
        );
    }

    // -- Model checker tests --

    #[test]
    fn inv_ferr_015_model_checker_all_properties() {
        let checker = HlcModel::new(3, 5, 6)
            .checker()
            .target_max_depth(12)
            .spawn_bfs()
            .join();

        // Safety: monotonicity must hold in ALL reachable states.
        checker.assert_no_discovery("inv_ferr_015_hlc_monotonicity");

        // Liveness: these states must be reachable.
        checker.assert_any_discovery("inv_ferr_015_regression_monotonicity_reachable");
        checker.assert_any_discovery("inv_ferr_015_tick_after_advance_reachable");
    }

    #[test]
    fn inv_ferr_015_model_checker_larger_domain() {
        // Slightly larger domain to explore more interleavings.
        let checker = HlcModel::new(4, 6, 8)
            .checker()
            .target_max_depth(16)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_015_hlc_monotonicity");
        checker.assert_any_discovery("inv_ferr_015_regression_monotonicity_reachable");
        checker.assert_any_discovery("inv_ferr_015_tick_after_advance_reachable");
    }

    /// Minimal model: verify properties hold even in the smallest non-trivial
    /// configuration.
    #[test]
    fn inv_ferr_015_model_checker_minimal() {
        let checker = HlcModel::new(1, 3, 3)
            .checker()
            .target_max_depth(8)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_015_hlc_monotonicity");
        checker.assert_any_discovery("inv_ferr_015_regression_monotonicity_reachable");
        checker.assert_any_discovery("inv_ferr_015_tick_after_advance_reachable");
    }
}
