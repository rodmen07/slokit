//! Drift guard: the dashboard's burn panels must mirror the generator's alerts.
//!
//! `src/dashboard/burn.rs` opens by promising that "the panel set mirrors alert
//! generation (`crate::generate`): a severity whose alert is disabled
//! (`alerting.page_alert.disable` / `alerting.ticket_alert.disable`) gets no
//! panels". Nothing enforced that: the two modules each read
//! `alerting.*_alert.disable` from their own `match`, so a change to one side's
//! rule leaves the other silently disagreeing and a user gets burn-rate panels
//! for an alert that will never fire (or an alert with no panel to explain it).
//!
//! This test reads BOTH sources for every combination of the two flags and
//! asserts they agree, rather than pinning either side's answer on its own.

#![cfg(all(feature = "spec", feature = "dashboard"))]

use std::collections::BTreeSet;

use serde_json::Value;
use slokit::generate::generate_rules;
use slokit::spec::Spec;

/// One SLO on the default burn-rate table (two page conditions, two ticket
/// conditions), with the two `disable` flags supplied per case.
fn spec_with(page_disabled: bool, ticket_disabled: bool) -> Spec {
    let yaml = format!(
        r#"
service: mirrorsvc
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(http_requests_total{{code=~"5.."}}[{{{{.window}}}}]))
        total_query: sum(rate(http_requests_total[{{{{.window}}}}]))
    alerting:
      page_alert:
        disable: {page_disabled}
      ticket_alert:
        disable: {ticket_disabled}
"#
    );
    Spec::from_yaml(&yaml).expect("fixture spec must parse")
}

/// Source A: the severities that actually get an alert rule, read off the
/// serialized generator output (`Rule`'s fields are private, and the serialized
/// form is what a consumer sees).
fn severities_with_alerts(spec: &Spec) -> BTreeSet<String> {
    let rule_set = generate_rules(spec).expect("fixture spec must generate");
    let value = serde_json::to_value(&rule_set).expect("rule sets serialize");
    let mut out = BTreeSet::new();
    for group in value["groups"].as_array().expect("groups is an array") {
        let name = group["name"].as_str().unwrap_or_default();
        if !name.starts_with("slokit-slo-alerts-") {
            continue;
        }
        for rule in group["rules"].as_array().expect("rules is an array") {
            let severity = rule["labels"]["sloth_severity"]
                .as_str()
                .expect("every generated alert carries sloth_severity");
            out.insert(severity.to_string());
        }
    }
    out
}

/// Source B: the severities that actually get burn panels, read off the
/// dashboard JSON by panel title (`"<severity> burn rate (<long>/<short>, ...)"`,
/// built in `dashboard::burn::burn_panel`).
fn severities_with_burn_panels(spec: &Spec) -> BTreeSet<String> {
    let value: Value = slokit::dashboard::dashboard_value(spec);
    let mut out = BTreeSet::new();
    for panel in value["panels"].as_array().expect("panels is an array") {
        // Only burn panels carry this infix; rows, stat tiles and the SLI
        // timeseries are titled differently.
        let title = panel["title"].as_str().unwrap_or_default();
        if let Some((severity, _)) = title.split_once(" burn rate (") {
            out.insert(severity.to_string());
        }
    }
    out
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn burn_panels_cover_exactly_the_severities_that_generate_alerts() {
    let cases = [
        (false, false, set(&["page", "ticket"])),
        (true, false, set(&["ticket"])),
        (false, true, set(&["page"])),
        (true, true, set(&[])),
    ];

    for (page_disabled, ticket_disabled, expected) in cases {
        let spec = spec_with(page_disabled, ticket_disabled);
        let alerts = severities_with_alerts(&spec);
        let panels = severities_with_burn_panels(&spec);

        assert_eq!(
            alerts, panels,
            "generator and dashboard disagree for page_disabled={page_disabled} \
             ticket_disabled={ticket_disabled}: alerts={alerts:?} panels={panels:?}"
        );
        // Pinning the agreed value too: without this, an implementation that
        // emitted nothing on either side would satisfy the equality above.
        assert_eq!(
            alerts, expected,
            "unexpected severity set for page_disabled={page_disabled} \
             ticket_disabled={ticket_disabled}"
        );
    }
}

/// Vacuity guard for the two extractors above. If either one stopped finding
/// anything -- a renamed rule group, a reworded panel title -- the comparison in
/// the test above would degrade to `{} == {}` and pass for all four cases while
/// checking nothing. This fails loudly instead.
#[test]
fn both_extractors_find_something_in_the_fully_enabled_case() {
    let spec = spec_with(false, false);
    assert_eq!(
        severities_with_alerts(&spec).len(),
        2,
        "the alert extractor found no severities; the rule-group name or the \
         sloth_severity label changed"
    );
    assert_eq!(
        severities_with_burn_panels(&spec).len(),
        2,
        "the panel extractor found no severities; the burn-panel title format \
         changed"
    );
}
