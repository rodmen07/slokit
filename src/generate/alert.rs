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

#[cfg(test)]
mod tests {
    use crate::generate::{generate_rules, Rule};
    use crate::spec::Spec;

    /// Everything above the per-test `alerting:` body: one events-SLI SLO on the
    /// default burn-rate table, which supplies two page conditions and two
    /// ticket conditions, so an *enabled* severity always has something to emit
    /// and an empty result can only mean the severity was skipped.
    const SPEC_HEAD: &str = r#"
service: alertsvc
labels:
  owner: team-platform
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(http_requests_total{code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total[{{.window}}]))
    alerting:
"#;

    /// Both severities present and enabled: the baseline the `disable` tests are
    /// a difference *from*. Without it, "no page alert" would be
    /// indistinguishable from "this fixture never generates alerts at all".
    const BOTH_ENABLED: &str = r#"      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

    fn spec_with_alerting(alerting: &str) -> Spec {
        Spec::from_yaml(&(String::from(SPEC_HEAD) + alerting)).expect("fixture spec must parse")
    }

    /// The alerts group for the fixture's single SLO, looked up by name rather
    /// than by index so the assertion "an alerts group is emitted at all"
    /// survives a change to how many groups precede it.
    fn alert_rules(alerting: &str) -> Vec<Rule> {
        let spec = spec_with_alerting(alerting);
        let rule_set = generate_rules(&spec).expect("fixture spec must generate");
        let group = rule_set
            .groups
            .iter()
            .find(|g| g.name.starts_with("slokit-slo-alerts-"))
            .expect("an alerts group is emitted for every SLO");
        group.rules.clone()
    }

    /// The `sloth_severity` label of each emitted alert, in emission order.
    fn severities(rules: &[Rule]) -> Vec<&str> {
        rules
            .iter()
            .map(|r| {
                r.labels
                    .get("sloth_severity")
                    .expect("every generated alert carries sloth_severity")
                    .as_str()
            })
            .collect()
    }

    #[test]
    fn both_severities_emit_one_alert_when_nothing_is_disabled() {
        let rules = alert_rules(BOTH_ENABLED);
        assert_eq!(severities(&rules), vec!["page", "ticket"]);
    }

    #[test]
    fn disabling_page_removes_the_page_alert_and_keeps_the_ticket_alert() {
        let rules = alert_rules(
            r#"      page_alert:
        disable: true
      ticket_alert:
        labels:
          severity: ticket
"#,
        );
        assert_eq!(severities(&rules), vec!["ticket"]);
    }

    #[test]
    fn disabling_ticket_removes_the_ticket_alert_and_keeps_the_page_alert() {
        let rules = alert_rules(
            r#"      page_alert:
        labels:
          severity: page
      ticket_alert:
        disable: true
"#,
        );
        assert_eq!(severities(&rules), vec!["page"]);
    }

    /// Disabling every severity still emits the (empty) alerts group rather than
    /// dropping it: `generate_rules_with` pushes three groups per SLO
    /// unconditionally. `tests/promtool.rs` proves Prometheus accepts the
    /// resulting `rules: []` group, so this is a shape contract, not a bug.
    #[test]
    fn disabling_both_severities_leaves_the_alerts_group_present_but_empty() {
        let rules = alert_rules(
            r#"      page_alert:
        disable: true
      ticket_alert:
        disable: true
"#,
        );
        assert!(
            rules.is_empty(),
            "expected no alert rules, got {:?}",
            severities(&rules)
        );
    }

    /// The second, independent way a severity produces nothing: it is *enabled*
    /// but the custom window table declares no condition for it. This is the
    /// discriminator that keeps the tests above from passing for the wrong
    /// reason -- an implementation that skipped alerts wholesale would satisfy
    /// them too.
    #[test]
    fn an_enabled_severity_with_no_matching_custom_window_emits_no_rule() {
        let rules = alert_rules(
            r#"      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 14.4
"#,
        );
        assert_eq!(severities(&rules), vec!["page"]);
    }

    #[test]
    fn per_severity_labels_win_over_shared_labels_and_sloth_severity_wins_over_both() {
        let rules = alert_rules(
            r#"      labels:
        team: shared-team
        tier: "1"
      page_alert:
        labels:
          team: page-team
          sloth_severity: attacker-supplied
      ticket_alert:
        disable: true
"#,
        );
        let page = &rules[0];
        assert_eq!(page.labels.get("tier").map(String::as_str), Some("1"));
        assert_eq!(
            page.labels.get("team").map(String::as_str),
            Some("page-team"),
            "the per-severity label must override the shared one"
        );
        assert_eq!(
            page.labels.get("sloth_severity").map(String::as_str),
            Some("page"),
            "sloth_severity is generator-owned and must not be spoofable from the spec"
        );
    }

    #[test]
    fn the_default_summary_is_only_supplied_when_the_spec_omits_it() {
        let generated = alert_rules(
            r#"      page_alert:
        labels:
          severity: page
      ticket_alert:
        disable: true
"#,
        );
        let summary = generated[0]
            .annotations
            .get("summary")
            .expect("a summary annotation is always present");
        assert!(
            summary.starts_with("page burn-rate alert: SLO 'availability' on service 'alertsvc'"),
            "unexpected default summary: {summary}"
        );

        let overridden = alert_rules(
            r#"      annotations:
        runbook: https://runbooks.example.com/availability
      page_alert:
        annotations:
          summary: check the checkout dashboard first
      ticket_alert:
        disable: true
"#,
        );
        assert_eq!(
            overridden[0].annotations.get("summary").map(String::as_str),
            Some("check the checkout dashboard first"),
            "an explicit summary must survive the default"
        );
        assert_eq!(
            overridden[0].annotations.get("runbook").map(String::as_str),
            Some("https://runbooks.example.com/availability"),
            "shared annotations must reach the per-severity alert"
        );
    }
}
