//! SLI recording rules: the error ratio at each lookback window, plus the SLO
//! period computed as an average over the shortest recorded window's metric.

use crate::window::Window;

use super::{Rule, RuleGroup, SloContext};

/// Fallback base window when the MWMBR config has no windows at all.
const BASE_WINDOW: Window = Window::minutes(5);

/// The shortest recorded lookback window: the period aggregation must average
/// over a metric that actually exists, which for the default config is the
/// sloth-compatible 5m recording.
pub(super) fn base_window(ctx: &SloContext<'_>) -> Window {
    ctx.mwmbr
        .lookback_windows()
        .first()
        .copied()
        .unwrap_or(BASE_WINDOW)
}

pub(super) fn rules(ctx: &SloContext<'_>) -> RuleGroup {
    let mut rules = Vec::new();

    // One SLI recording per MWMBR lookback window.
    for window in ctx.mwmbr.lookback_windows() {
        rules.push(sli_rule(ctx, window, ctx.sli.error_ratio_expr(window)));
    }

    // The period (e.g. 30d) error ratio, averaged over the shortest recorded
    // metric so Prometheus never has to scan the full period of raw data.
    let period = ctx.slo.period;
    let base = base_window(ctx).prometheus();
    let sel = ctx.selector();
    let period_expr = format!(
        "sum_over_time(slo:sli_error:ratio_rate{base}{sel}[{p}])\n/\ncount_over_time(slo:sli_error:ratio_rate{base}{sel}[{p}])",
        p = period.prometheus()
    );
    rules.push(sli_rule(ctx, period, period_expr));

    RuleGroup {
        name: format!(
            "slokit-slo-sli-recordings-{}-{}",
            ctx.service, ctx.slo_spec.name
        ),
        rules,
    }
}

fn sli_rule(ctx: &SloContext<'_>, window: Window, expr: String) -> Rule {
    let mut labels = ctx.base_labels();
    labels.insert("sloth_window".to_string(), window.prometheus());
    Rule::record(
        format!("slo:sli_error:ratio_rate{}", window.prometheus()),
        expr,
        labels,
    )
}
