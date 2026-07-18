//! Burn rates and the multi-window multi-burn-rate (MWMBR) alert model from
//! the Google SRE Workbook.

use crate::slo::Slo;
use crate::window::Window;

/// How fast an error budget is being consumed.
///
/// A burn rate of `1.0` consumes the entire budget exactly over the SLO period;
/// `14.4` consumes a 30-day budget in roughly 50 hours.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct BurnRate(f64);

impl BurnRate {
    /// Wrap a raw burn-rate factor.
    pub fn new(value: f64) -> Self {
        Self(value)
    }

    /// Derive the burn rate from an observed error ratio against an SLO:
    /// `observed_error_ratio / (1 - objective)`.
    pub fn from_error_ratio(observed_error_ratio: f64, slo: &Slo) -> Self {
        let budget = slo.error_budget_ratio();
        if budget <= 0.0 {
            return Self(f64::INFINITY);
        }
        Self(observed_error_ratio / budget)
    }

    /// The raw factor.
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Fraction of the total budget consumed if this burn rate is sustained for
    /// `window` of a `period`-long budget: `factor * (window / period)`.
    pub fn budget_consumed_over(&self, window: Window, period: Window) -> f64 {
        self.0 * (window.as_secs_f64() / period.as_secs_f64())
    }

    /// Time to exhaust the remaining budget fraction at this sustained rate.
    ///
    /// Returns `None` for non-positive burn rates.
    pub fn time_to_exhaustion(
        &self,
        remaining_budget_ratio: f64,
        period: Window,
    ) -> Option<std::time::Duration> {
        if self.0 <= 0.0 || !self.0.is_finite() {
            return None;
        }
        let secs = (period.as_secs_f64() * remaining_budget_ratio / self.0).max(0.0);
        Some(std::time::Duration::from_secs_f64(secs))
    }
}

/// Alert severity in the MWMBR scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fast burn: wake someone up.
    Page,
    /// Slow burn: file a ticket.
    Ticket,
}

impl Severity {
    /// The `sloth_severity` label value used in generated rules.
    pub fn label(&self) -> &'static str {
        match self {
            Severity::Page => "page",
            Severity::Ticket => "ticket",
        }
    }
}

/// One burn-rate condition: a long and short window that must both be burning
/// faster than `factor` times the budget for the alert to fire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlertWindow {
    /// Whether this condition pages or tickets.
    pub severity: Severity,
    /// The long lookback window (e.g. `1h`).
    pub long: Window,
    /// The short lookback window that confirms the burn is still happening
    /// (e.g. `5m`).
    pub short: Window,
    /// The burn-rate multiplier that triggers this condition.
    pub factor: f64,
}

impl AlertWindow {
    /// The error-ratio threshold for this condition against `slo`:
    /// `factor * (1 - objective)`.
    pub fn threshold(&self, slo: &Slo) -> f64 {
        self.factor * slo.error_budget_ratio()
    }
}

/// A multi-window multi-burn-rate alert configuration: the set of burn-rate
/// conditions that, OR-ed together per severity, form the page and ticket
/// alerts.
#[derive(Debug, Clone, PartialEq)]
pub struct MwmbrConfig {
    /// The burn-rate conditions, ordered page-first.
    pub windows: Vec<AlertWindow>,
}

impl MwmbrConfig {
    /// The canonical SRE Workbook configuration for a 30-day SLO period:
    ///
    /// | Severity | Long | Short | Factor |
    /// |----------|------|-------|--------|
    /// | Page     | 1h   | 5m    | 14.4   |
    /// | Page     | 6h   | 30m   | 6      |
    /// | Ticket   | 1d   | 2h    | 3      |
    /// | Ticket   | 3d   | 6h    | 1      |
    pub fn sre_default() -> Self {
        Self {
            windows: vec![
                AlertWindow {
                    severity: Severity::Page,
                    long: Window::hours(1),
                    short: Window::minutes(5),
                    factor: 14.4,
                },
                AlertWindow {
                    severity: Severity::Page,
                    long: Window::hours(6),
                    short: Window::minutes(30),
                    factor: 6.0,
                },
                AlertWindow {
                    severity: Severity::Ticket,
                    long: Window::days(1),
                    short: Window::hours(2),
                    factor: 3.0,
                },
                AlertWindow {
                    severity: Severity::Ticket,
                    long: Window::days(3),
                    short: Window::hours(6),
                    factor: 1.0,
                },
            ],
        }
    }

    /// The canonical SRE Workbook configuration scaled to `period`.
    ///
    /// Equivalent to `sre_default().scaled(Window::days(30), period)`: the same
    /// burn-rate factors, with every lookback window scaled proportionally so
    /// each condition still consumes the same fraction of the error budget
    /// before it fires.
    pub fn sre_default_for_period(period: Window) -> Self {
        Self::sre_default().scaled(Window::days(30), period)
    }

    /// Scale every lookback window by `to / from`, keeping the burn-rate
    /// factors.
    ///
    /// Because a condition's budget consumption is `factor * (window / period)`,
    /// scaling the windows with the period preserves how much budget each
    /// condition allows before firing. Scaled windows are rounded to the
    /// nearest whole minute and never drop below one minute. When `from == to`
    /// the configuration is returned unchanged.
    pub fn scaled(&self, from: Window, to: Window) -> Self {
        if from == to {
            return self.clone();
        }
        let ratio = to.as_secs_f64() / from.as_secs_f64();
        let scale = |w: Window| {
            let minutes = (w.as_secs_f64() * ratio / 60.0).round().max(1.0);
            Window::minutes(minutes as u64)
        };
        Self {
            windows: self
                .windows
                .iter()
                .map(|w| AlertWindow {
                    severity: w.severity,
                    long: scale(w.long),
                    short: scale(w.short),
                    factor: w.factor,
                })
                .collect(),
        }
    }

    /// The conditions for a given severity, in order.
    pub fn for_severity(&self, severity: Severity) -> impl Iterator<Item = &AlertWindow> {
        self.windows.iter().filter(move |w| w.severity == severity)
    }

    /// Every distinct lookback window referenced by any condition, which is the
    /// set of windows the SLI recording rules must cover. Sorted shortest-first.
    pub fn lookback_windows(&self) -> Vec<Window> {
        let mut windows: Vec<Window> = self
            .windows
            .iter()
            .flat_map(|w| [w.short, w.long])
            .collect();
        windows.sort();
        windows.dedup();
        windows
    }
}

impl Default for MwmbrConfig {
    fn default() -> Self {
        Self::sre_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slo::Objective;

    fn slo_999() -> Slo {
        Slo::new(Objective::percent(99.9).unwrap(), Window::days(30))
    }

    #[test]
    fn budget_consumed_matches_workbook_constants() {
        // The defining identity of the SRE table: factor * (window / period)
        // equals the documented "budget consumed" column.
        let period = Window::days(30);
        let page_fast = BurnRate::new(14.4);
        assert!((page_fast.budget_consumed_over(Window::hours(1), period) - 0.02).abs() < 1e-9);
        let page_slow = BurnRate::new(6.0);
        assert!((page_slow.budget_consumed_over(Window::hours(6), period) - 0.05).abs() < 1e-9);
        let ticket_fast = BurnRate::new(3.0);
        assert!((ticket_fast.budget_consumed_over(Window::days(1), period) - 0.10).abs() < 1e-9);
        let ticket_slow = BurnRate::new(1.0);
        assert!((ticket_slow.budget_consumed_over(Window::days(3), period) - 0.10).abs() < 1e-9);
    }

    #[test]
    fn from_error_ratio_inverts_budget() {
        let slo = slo_999();
        // Observing exactly the budget ratio as the error rate is burn rate 1.0.
        let br = BurnRate::from_error_ratio(0.001, &slo);
        assert!((br.value() - 1.0).abs() < 1e-9);
        // Ten times the budget ratio is burn rate 10.
        let br = BurnRate::from_error_ratio(0.01, &slo);
        assert!((br.value() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn thresholds_are_factor_times_budget() {
        let slo = slo_999();
        let cfg = MwmbrConfig::sre_default();
        let first = cfg.windows[0];
        assert_eq!(first.factor, 14.4);
        assert!((first.threshold(&slo) - 0.0144).abs() < 1e-9);
    }

    #[test]
    fn default_lookback_windows_are_the_six_sloth_windows() {
        let cfg = MwmbrConfig::sre_default();
        assert_eq!(
            cfg.lookback_windows(),
            vec![
                Window::minutes(5),
                Window::minutes(30),
                Window::hours(1),
                Window::hours(2),
                Window::hours(6),
                Window::days(1),
                Window::days(3),
            ]
        );
    }

    #[test]
    fn severity_filter_splits_page_and_ticket() {
        let cfg = MwmbrConfig::sre_default();
        assert_eq!(cfg.for_severity(Severity::Page).count(), 2);
        assert_eq!(cfg.for_severity(Severity::Ticket).count(), 2);
    }

    #[test]
    fn scaling_to_the_same_period_is_identity() {
        let cfg = MwmbrConfig::sre_default();
        assert_eq!(cfg.scaled(Window::days(30), Window::days(30)), cfg);
        assert_eq!(MwmbrConfig::sre_default_for_period(Window::days(30)), cfg);
    }

    #[test]
    fn scaling_to_90d_triples_every_window() {
        let cfg = MwmbrConfig::sre_default_for_period(Window::days(90));
        let expected = [
            (Window::hours(3), Window::minutes(15), 14.4),
            (Window::hours(18), Window::minutes(90), 6.0),
            (Window::days(3), Window::hours(6), 3.0),
            (Window::days(9), Window::hours(18), 1.0),
        ];
        for (w, (long, short, factor)) in cfg.windows.iter().zip(expected) {
            assert_eq!(w.long, long);
            assert_eq!(w.short, short);
            assert_eq!(w.factor, factor);
        }
    }

    #[test]
    fn scaling_preserves_budget_consumed_per_condition() {
        // factor * (window / period) must stay (approximately) constant when
        // period and windows scale together; rounding to minutes may nudge it.
        let period = Window::days(7);
        let cfg = MwmbrConfig::sre_default_for_period(period);
        let base = MwmbrConfig::sre_default();
        for (scaled, orig) in cfg.windows.iter().zip(&base.windows) {
            let consumed = BurnRate::new(scaled.factor).budget_consumed_over(scaled.long, period);
            let orig_consumed =
                BurnRate::new(orig.factor).budget_consumed_over(orig.long, Window::days(30));
            assert!(
                (consumed - orig_consumed).abs() / orig_consumed < 0.05,
                "budget consumed drifted: {consumed} vs {orig_consumed}"
            );
        }
    }

    #[test]
    fn scaling_rounds_to_minutes_and_never_hits_zero() {
        // 1d period: ratio 1/30. The 5m short window would be 10s; it must
        // clamp to the 1m floor.
        let cfg = MwmbrConfig::sre_default_for_period(Window::days(1));
        assert!(cfg.windows.iter().all(|w| w.short >= Window::minutes(1)));
        assert!(cfg
            .windows
            .iter()
            .flat_map(|w| [w.long, w.short])
            .all(|w| w.as_secs() % 60 == 0));
    }
}
