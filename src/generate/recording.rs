//! SLI recording rules: the error ratio at each lookback window, plus the SLO
//! period computed as an average over the recorded 5m metric.

use crate::window::Window;

use super::{Rule, RuleGroup, SloContext};

/// The short window the period aggregation averages over, matching `sloth`.
const BASE_WINDOW: Window = Window::minutes(5);

pub(super) fn rules(ctx: &SloContext<'_>) -> RuleGroup {
    let mut rules = Vec::new();

    // One SLI recording per MWMBR lookback window.
    for window in ctx.mwmbr.lookback_windows() {
        rules.push(sli_rule(ctx, window, ctx.sli.error_ratio_expr(window)));
    }

    // The period (e.g. 30d) error ratio, averaged over the recorded 5m metric
    // so Prometheus never has to scan the full period of raw data.
    let period = ctx.slo.period;
    let base = BASE_WINDOW.prometheus();
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
