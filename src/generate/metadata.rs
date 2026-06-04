//! Metadata recording rules: objective, error budget, burn rates, and an info
//! series, all keyed by the `sloth_*` identity labels.

use super::{fmt_num, Rule, RuleGroup, SloContext};

const GROUPING: &str = "on(sloth_id, sloth_slo, sloth_service) group_left";

pub(super) fn rules(ctx: &SloContext<'_>) -> RuleGroup {
    let sel = ctx.selector();
    let base = ctx.base_labels();
    let period = ctx.slo.period.prometheus();

    let mut rules = vec![
        Rule::record(
            "slo:objective:ratio",
            format!("vector({})", fmt_num(ctx.slo.objective.as_ratio())),
            base.clone(),
        ),
        Rule::record(
            "slo:error_budget:ratio",
            format!("vector({})", fmt_num(ctx.slo.error_budget_ratio())),
            base.clone(),
        ),
        Rule::record(
            "slo:time_period:days",
            format!("vector({})", fmt_num(ctx.slo.period.as_days_f64())),
            base.clone(),
        ),
        Rule::record(
            "slo:current_burn_rate:ratio",
            format!("slo:sli_error:ratio_rate5m{sel}\n/ {GROUPING}\nslo:error_budget:ratio{sel}"),
            base.clone(),
        ),
        Rule::record(
            "slo:period_burn_rate:ratio",
            format!(
                "slo:sli_error:ratio_rate{period}{sel}\n/ {GROUPING}\nslo:error_budget:ratio{sel}"
            ),
            base.clone(),
        ),
        Rule::record(
            "slo:period_error_budget_remaining:ratio",
            format!("1 - slo:period_burn_rate:ratio{sel}"),
            base.clone(),
        ),
    ];

    // The info series carries discoverability metadata for dashboards.
    let mut info_labels = base;
    info_labels.insert("sloth_mode".to_string(), "cli-gen-prometheus".to_string());
    info_labels.insert("sloth_spec".to_string(), "prometheus/v1".to_string());
    info_labels.insert(
        "sloth_version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );
    info_labels.insert(
        "sloth_objective".to_string(),
        fmt_num(ctx.slo.objective.as_percent()),
    );
    rules.push(Rule::record("slo:info", "vector(1)", info_labels));

    RuleGroup {
        name: format!(
            "slokit-slo-meta-recordings-{}-{}",
            ctx.service, ctx.slo_spec.name
        ),
        rules,
    }
}
