//! Multi-window multi-burn-rate alert rules (page and ticket).

use std::collections::BTreeMap;

use crate::burn_rate::Severity;
use crate::spec::AlertMeta;

use super::{fmt_num, Rule, RuleGroup, SloContext};

pub(super) fn rules(ctx: &SloContext<'_>) -> RuleGroup {
    let mut rules = Vec::new();
    for severity in [Severity::Page, Severity::Ticket] {
        if let Some(rule) = alert_rule(ctx, severity) {
            rules.push(rule);
        }
    }
    RuleGroup {
        name: format!("slokit-slo-alerts-{}-{}", ctx.service, ctx.slo_spec.name),
        rules,
    }
}

fn alert_rule(ctx: &SloContext<'_>, severity: Severity) -> Option<Rule> {
    let meta = severity_meta(ctx, severity);
    if meta.disable {
        return None;
    }

    let budget = ctx.slo.error_budget_ratio();
    let sel = ctx.selector();

    let conditions: Vec<String> = ctx
        .mwmbr
        .for_severity(severity)
        .map(|w| {
            let threshold = format!("({} * {})", fmt_num(w.factor), fmt_num(budget));
            format!(
                "(\n  max(slo:sli_error:ratio_rate{long}{sel} > {threshold}) without (sloth_window)\n  and\n  max(slo:sli_error:ratio_rate{short}{sel} > {threshold}) without (sloth_window)\n)",
                long = w.long.prometheus(),
                short = w.short.prometheus(),
            )
        })
        .collect();

    if conditions.is_empty() {
        return None;
    }

    let expr = conditions.join("\nor\n");
    let labels = severity_labels(ctx, severity, meta);
    let annotations = severity_annotations(ctx, severity, meta);

    Some(Rule::alert(
        ctx.slo_spec.alert_name(),
        expr,
        labels,
        annotations,
    ))
}

fn severity_meta<'a>(ctx: &'a SloContext<'_>, severity: Severity) -> &'a AlertMeta {
    match severity {
        Severity::Page => &ctx.slo_spec.alerting.page_alert,
        Severity::Ticket => &ctx.slo_spec.alerting.ticket_alert,
    }
}

fn severity_labels(
    ctx: &SloContext<'_>,
    severity: Severity,
    meta: &AlertMeta,
) -> BTreeMap<String, String> {
    let mut labels = ctx.slo_spec.alerting.labels.clone();
    labels.extend(meta.labels.clone());
    labels.insert("sloth_severity".to_string(), severity.label().to_string());
    labels
}

fn severity_annotations(
    ctx: &SloContext<'_>,
    severity: Severity,
    meta: &AlertMeta,
) -> BTreeMap<String, String> {
    let mut annotations = ctx.slo_spec.alerting.annotations.clone();
    annotations.extend(meta.annotations.clone());
    annotations.entry("summary".to_string()).or_insert_with(|| {
        format!(
            "{} burn-rate alert: SLO '{}' on service '{}' is consuming its error budget too fast",
            severity.label(),
            ctx.slo_spec.name,
            ctx.service,
        )
    });
    annotations
}
