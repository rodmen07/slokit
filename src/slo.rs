//! Service Level Objectives: a target reliability over a rolling period.

use crate::budget::ErrorBudget;
use crate::error::{Result, SlokitError};
use crate::window::Window;

/// A reliability target, stored as a ratio in the open interval `(0, 1)`.
///
/// `99.9%` availability is `Objective::percent(99.9)` and reads back as a ratio
/// of `0.999`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Objective(f64);

impl Objective {
    /// Build an objective from a percentage in the open interval `(0, 100)`.
    pub fn percent(p: f64) -> Result<Self> {
        if !p.is_finite() || p <= 0.0 || p >= 100.0 {
            return Err(SlokitError::InvalidObjective(format!(
                "{p} is not a percentage in the open interval (0, 100)"
            )));
        }
        Ok(Self(p / 100.0))
    }

    /// Build an objective from a ratio in the open interval `(0, 1)`.
    pub fn ratio(r: f64) -> Result<Self> {
        if !r.is_finite() || r <= 0.0 || r >= 1.0 {
            return Err(SlokitError::InvalidObjective(format!(
                "{r} is not a ratio in the open interval (0, 1)"
            )));
        }
        Ok(Self(r))
    }

    /// The objective as a ratio, e.g. `0.999`.
    pub fn as_ratio(&self) -> f64 {
        self.0
    }

    /// The objective as a percentage, e.g. `99.9`.
    pub fn as_percent(&self) -> f64 {
        self.0 * 100.0
    }

    /// The error-budget ratio, i.e. `1 - objective`.
    pub fn error_budget_ratio(&self) -> f64 {
        1.0 - self.0
    }
}

/// A Service Level Objective: an [`Objective`] measured over a rolling
/// [`Window`] (the SLO period, typically 30 days).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slo {
    /// The reliability target.
    pub objective: Objective,
    /// The rolling period the objective is measured over.
    pub period: Window,
}

impl Slo {
    /// Create an SLO from an objective and a period.
    pub fn new(objective: Objective, period: Window) -> Self {
        Self { objective, period }
    }

    /// The fraction of events allowed to fail, i.e. `1 - objective`.
    pub fn error_budget_ratio(&self) -> f64 {
        self.objective.error_budget_ratio()
    }

    /// Build a concrete [`ErrorBudget`] for a known total event count over the
    /// period.
    pub fn error_budget(&self, total_events: f64) -> ErrorBudget {
        ErrorBudget::new(total_events, self.error_budget_ratio())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_round_trips() {
        let o = Objective::percent(99.9).unwrap();
        assert!((o.as_ratio() - 0.999).abs() < 1e-12);
        assert!((o.as_percent() - 99.9).abs() < 1e-9);
        assert!((o.error_budget_ratio() - 0.001).abs() < 1e-12);
    }

    #[test]
    fn rejects_out_of_range_objectives() {
        assert!(Objective::percent(0.0).is_err());
        assert!(Objective::percent(100.0).is_err());
        assert!(Objective::percent(150.0).is_err());
        assert!(Objective::ratio(0.0).is_err());
        assert!(Objective::ratio(1.0).is_err());
        assert!(Objective::ratio(f64::NAN).is_err());
    }

    #[test]
    fn error_budget_ratio_matches_workbook() {
        let slo = Slo::new(Objective::percent(99.95).unwrap(), Window::days(30));
        assert!((slo.error_budget_ratio() - 0.0005).abs() < 1e-12);
    }
}
