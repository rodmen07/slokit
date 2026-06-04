//! Error-budget math: how much failure an SLO permits, and how much is left.

use crate::burn_rate::BurnRate;
use crate::window::Window;

/// A concrete error budget: the allowable failures for a known event volume
/// over an SLO period.
///
/// Create one with [`crate::Slo::error_budget`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErrorBudget {
    total_events: f64,
    budget_ratio: f64,
}

impl ErrorBudget {
    /// Build a budget directly from a total event count and the budget ratio
    /// (`1 - objective`).
    pub fn new(total_events: f64, budget_ratio: f64) -> Self {
        Self {
            total_events,
            budget_ratio,
        }
    }

    /// Total events observed (or projected) over the period.
    pub fn total_events(&self) -> f64 {
        self.total_events
    }

    /// The budget ratio, i.e. `1 - objective`.
    pub fn budget_ratio(&self) -> f64 {
        self.budget_ratio
    }

    /// The number of events that may fail before the budget is exhausted.
    pub fn allowed_bad_events(&self) -> f64 {
        self.total_events * self.budget_ratio
    }

    /// Events of budget remaining after `observed_bad` failures. May be
    /// negative when the budget is overspent.
    pub fn remaining_events(&self, observed_bad: f64) -> f64 {
        self.allowed_bad_events() - observed_bad
    }

    /// Fraction of the budget already consumed by `observed_bad` failures
    /// (`0.0` = untouched, `1.0` = exactly exhausted, `> 1.0` = overspent).
    pub fn consumed_ratio(&self, observed_bad: f64) -> f64 {
        let allowed = self.allowed_bad_events();
        if allowed <= 0.0 {
            return f64::INFINITY;
        }
        observed_bad / allowed
    }

    /// Fraction of the budget still available (`1.0` = untouched, `0.0` =
    /// exhausted). Clamped at zero on the low end.
    pub fn remaining_ratio(&self, observed_bad: f64) -> f64 {
        (1.0 - self.consumed_ratio(observed_bad)).max(0.0)
    }

    /// Time until the budget is exhausted, given a sustained burn rate and the
    /// SLO period.
    ///
    /// Returns `None` when the burn rate is zero or negative (the budget is
    /// never exhausted at that rate).
    pub fn time_to_exhaustion(
        &self,
        observed_bad: f64,
        burn_rate: BurnRate,
        period: Window,
    ) -> Option<std::time::Duration> {
        burn_rate.time_to_exhaustion(self.remaining_ratio(observed_bad), period)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget_999() -> ErrorBudget {
        // 99.9% over a period with 1,000,000 events: 1,000 allowed failures.
        ErrorBudget::new(1_000_000.0, 0.001)
    }

    #[test]
    fn allowed_bad_events_is_total_times_budget() {
        assert!((budget_999().allowed_bad_events() - 1_000.0).abs() < 1e-6);
    }

    #[test]
    fn consumed_and_remaining_are_complementary() {
        let b = budget_999();
        assert!((b.consumed_ratio(250.0) - 0.25).abs() < 1e-12);
        assert!((b.remaining_ratio(250.0) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn overspend_clamps_remaining_to_zero() {
        let b = budget_999();
        assert_eq!(b.remaining_ratio(5_000.0), 0.0);
        assert!(b.remaining_events(5_000.0) < 0.0);
    }

    #[test]
    fn time_to_exhaustion_scales_with_burn_rate() {
        let b = budget_999();
        let period = Window::days(30);
        // Fresh budget, burn rate 1.0: exhausts in exactly one period.
        let ttx = b
            .time_to_exhaustion(0.0, BurnRate::new(1.0), period)
            .unwrap();
        assert_eq!(ttx.as_secs(), period.as_secs());
        // Burn rate 10x: exhausts in a tenth of the period.
        let fast = b
            .time_to_exhaustion(0.0, BurnRate::new(10.0), period)
            .unwrap();
        assert_eq!(fast.as_secs(), period.as_secs() / 10);
        // No burn: never.
        assert!(b
            .time_to_exhaustion(0.0, BurnRate::new(0.0), period)
            .is_none());
    }
}
