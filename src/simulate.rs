//! Forward-looking "what if" error-budget simulation.
//!
//! Where [`ErrorBudget`](crate::ErrorBudget) answers "given the bad events I
//! have ALREADY observed, how much budget is left", this module answers the
//! planning question: "if my service SUSTAINS a given error ratio from here,
//! how fast do I burn, when do I run out, and which multi-window multi-burn-rate
//! (MWMBR) alert conditions would page or ticket?"
//!
//! It is pure and dependency-free, built entirely on the existing core
//! ([`BurnRate`], [`MwmbrConfig`], [`Slo`]); the CLI `simulate` subcommand is a
//! thin shell over [`simulate`].
//!
//! ## Steady-state model
//!
//! A real MWMBR condition fires only when the burn rate is above its factor over
//! BOTH its long and its short lookback window. This simulation assumes a
//! CONSTANT error ratio, so the burn rate is identical over every window and a
//! condition fires exactly when that single burn rate reaches its factor,
//! equivalently when the error ratio reaches the condition's
//! [`threshold`](AlertWindow::threshold). This is the right model for capacity
//! planning ("what rate can I sustain before paging?"); it is deliberately NOT a
//! replay of a time-varying incident, where the long and short windows diverge.
//!
//! ```
//! use slokit::{Objective, Slo, Window, MwmbrConfig};
//! use slokit::simulate::simulate;
//!
//! let slo = Slo::new(Objective::percent(99.9).unwrap(), Window::days(30));
//! // Sustaining a 0.5% error ratio against a 99.9% objective is a 5x burn.
//! let sim = simulate(&slo, 0.005, 1.0, &MwmbrConfig::sre_default_for_period(slo.period));
//! assert!((sim.burn_rate.value() - 5.0).abs() < 1e-9);
//! // At 5x, a full 30-day budget is gone in ~6 days.
//! assert!(sim.time_to_exhaustion.unwrap().as_secs_f64() > 5.0 * 86_400.0);
//! ```

use std::time::Duration;

use crate::burn_rate::{AlertWindow, MwmbrConfig, Severity};
use crate::slo::Slo;
use crate::BurnRate;

/// What one MWMBR alert condition does under a simulated steady error ratio.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowOutcome {
    /// The alert condition being evaluated.
    pub window: AlertWindow,
    /// Whether it fires: the simulated burn rate is at or above its factor
    /// (equivalently, the error ratio is at or above its threshold).
    pub fires: bool,
    /// The error-ratio threshold this condition fires at, for context.
    pub threshold: f64,
    /// Fraction of the TOTAL budget this condition's long window accounts for at
    /// the simulated rate: `burn_rate * (long / period)`. A value >= 1.0 means a
    /// single long window would spend the whole budget.
    pub budget_consumed_over_long: f64,
}

/// The result of a steady-state budget simulation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// The sustained error ratio that was simulated.
    pub error_ratio: f64,
    /// The remaining budget fraction the simulation started from (1.0 = full).
    pub remaining_budget_ratio: f64,
    /// The burn rate the error ratio produces against the SLO.
    pub burn_rate: BurnRate,
    /// Time to exhaust the remaining budget at this sustained rate, or `None`
    /// when the budget never burns (a zero or negative burn rate) or the rate
    /// is not finite.
    pub time_to_exhaustion: Option<Duration>,
    /// Per-condition outcomes, in the configuration's order (page-first for the
    /// SRE default).
    pub windows: Vec<WindowOutcome>,
}

impl Simulation {
    /// Whether any condition of the given severity fires.
    pub fn fires_any(&self, severity: Severity) -> bool {
        self.windows
            .iter()
            .any(|w| w.fires && w.window.severity == severity)
    }

    /// Whether any page condition fires.
    pub fn pages(&self) -> bool {
        self.fires_any(Severity::Page)
    }

    /// Whether any ticket condition fires.
    pub fn tickets(&self) -> bool {
        self.fires_any(Severity::Ticket)
    }
}

/// Simulate sustaining `error_ratio` against `slo`, starting from
/// `remaining_budget_ratio` of the budget (1.0 for a fresh period), and report
/// the burn rate, projected exhaustion, and which of `config`'s conditions fire.
///
/// `error_ratio` is a ratio in `[0, 1]` (0.005 = 0.5% of requests failing).
/// `remaining_budget_ratio` is clamped into `[0, 1]`.
pub fn simulate(
    slo: &Slo,
    error_ratio: f64,
    remaining_budget_ratio: f64,
    config: &MwmbrConfig,
) -> Simulation {
    let remaining = remaining_budget_ratio.clamp(0.0, 1.0);
    let burn_rate = BurnRate::from_error_ratio(error_ratio, slo);
    let time_to_exhaustion = burn_rate.time_to_exhaustion(remaining, slo.period);

    let windows = config
        .windows
        .iter()
        .map(|w| {
            let threshold = w.threshold(slo);
            WindowOutcome {
                window: *w,
                // >= so a rate landing exactly on the factor fires, matching the
                // Prometheus `>=` comparison in the generated alert expressions.
                fires: burn_rate.value() >= w.factor,
                threshold,
                budget_consumed_over_long: burn_rate.budget_consumed_over(w.long, slo.period),
            }
        })
        .collect();

    Simulation {
        error_ratio,
        remaining_budget_ratio: remaining,
        burn_rate,
        time_to_exhaustion,
        windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slo::Objective;
    use crate::window::Window;

    fn slo_999() -> Slo {
        Slo::new(Objective::percent(99.9).unwrap(), Window::days(30))
    }

    fn config() -> MwmbrConfig {
        MwmbrConfig::sre_default_for_period(Window::days(30))
    }

    #[test]
    fn burn_rate_inverts_the_budget() {
        // Error ratio equal to the budget ratio (0.1%) is exactly 1x.
        let sim = simulate(&slo_999(), 0.001, 1.0, &config());
        assert!((sim.burn_rate.value() - 1.0).abs() < 1e-9);
        // Five times the budget ratio is 5x.
        let sim = simulate(&slo_999(), 0.005, 1.0, &config());
        assert!((sim.burn_rate.value() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_condition_fires_exactly_at_its_threshold() {
        let slo = slo_999();
        let cfg = config();
        // The slowest ticket condition has factor 1.0, threshold = budget ratio.
        let ticket_slow = cfg.windows.iter().find(|w| w.factor == 1.0).unwrap();
        let thr = ticket_slow.threshold(&slo);
        assert!((thr - 0.001).abs() < 1e-9);

        // Just below the threshold: it does not fire.
        let below = simulate(&slo, thr - 1e-6, 1.0, &cfg);
        assert!(
            !below
                .windows
                .iter()
                .find(|w| w.window.factor == 1.0)
                .unwrap()
                .fires
        );
        // Exactly at the threshold: it fires (matches Prometheus `>=`).
        let at = simulate(&slo, thr, 1.0, &cfg);
        assert!(
            at.windows
                .iter()
                .find(|w| w.window.factor == 1.0)
                .unwrap()
                .fires
        );
    }

    #[test]
    fn faster_burn_lights_up_more_severe_pages() {
        let slo = slo_999();
        let cfg = config();

        // 3x burn: both tickets (factors 1, 3) fire, neither page (6, 14.4).
        let three_x = simulate(&slo, 0.003, 1.0, &cfg);
        assert!(three_x.tickets());
        assert!(!three_x.pages());

        // 15x burn: everything fires, including the 14.4 fast page.
        let fifteen_x = simulate(&slo, 0.015, 1.0, &cfg);
        assert!(fifteen_x.pages());
        assert!(fifteen_x.tickets());
        assert!(fifteen_x.windows.iter().all(|w| w.fires));
    }

    #[test]
    fn exhaustion_scales_inversely_with_burn_rate() {
        let slo = slo_999();
        let cfg = config();
        // 1x burn empties a full 30-day budget in ~30 days.
        let one_x = simulate(&slo, 0.001, 1.0, &cfg);
        let d1 = one_x.time_to_exhaustion.unwrap().as_secs_f64();
        assert!((d1 - 30.0 * 86_400.0).abs() < 1.0);
        // 10x burn empties it in ~3 days.
        let ten_x = simulate(&slo, 0.01, 1.0, &cfg);
        let d10 = ten_x.time_to_exhaustion.unwrap().as_secs_f64();
        assert!((d10 - 3.0 * 86_400.0).abs() < 1.0);
    }

    #[test]
    fn zero_error_never_exhausts_and_fires_nothing() {
        let sim = simulate(&slo_999(), 0.0, 1.0, &config());
        assert_eq!(sim.burn_rate.value(), 0.0);
        assert!(sim.time_to_exhaustion.is_none());
        assert!(!sim.pages());
        assert!(!sim.tickets());
        assert!(sim.windows.iter().all(|w| !w.fires));
    }

    #[test]
    fn a_partly_spent_budget_exhausts_sooner() {
        let slo = slo_999();
        let cfg = config();
        let full = simulate(&slo, 0.005, 1.0, &cfg).time_to_exhaustion.unwrap();
        let half = simulate(&slo, 0.005, 0.5, &cfg).time_to_exhaustion.unwrap();
        assert!((full.as_secs_f64() - 2.0 * half.as_secs_f64()).abs() < 1.0);
    }

    #[test]
    fn remaining_budget_is_clamped() {
        // A remaining fraction above 1.0 is treated as a full budget, not more.
        let slo = slo_999();
        let cfg = config();
        let over = simulate(&slo, 0.005, 1.5, &cfg);
        let full = simulate(&slo, 0.005, 1.0, &cfg);
        assert_eq!(over.remaining_budget_ratio, 1.0);
        assert_eq!(over.time_to_exhaustion, full.time_to_exhaustion);
    }

    #[test]
    fn budget_consumed_over_long_matches_the_workbook() {
        // At the exact threshold of the fast page (14.4x), its 1h long window
        // accounts for 2% of the 30-day budget: the canonical table value.
        let slo = slo_999();
        let cfg = config();
        let sim = simulate(&slo, 0.0144, 1.0, &cfg);
        let fast_page = sim
            .windows
            .iter()
            .find(|w| w.window.factor == 14.4)
            .unwrap();
        assert!((fast_page.budget_consumed_over_long - 0.02).abs() < 1e-9);
    }
}
