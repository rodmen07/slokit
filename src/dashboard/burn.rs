//! Per-severity burn-rate panels: one timeseries panel per enabled alert
//! condition, plotting the long- and short-window burn rates that condition
//! compares, with a threshold line at its burn-rate factor.
//!
//! The panel set mirrors alert generation ([`crate::generate`]): a severity
//! whose alert is disabled (`alerting.page_alert.disable` /
//! `alerting.ticket_alert.disable`) gets no panels, and each panel divides the
//! recorded `slo:sli_error:ratio_rate<window>` series by
//! `slo:error_budget:ratio` using the same `on(...) group_left` idiom the
//! generator's `slo:current_burn_rate:ratio` rule uses, so every expression
//! references only series the generator records. Values are burn-rate
//! multiples: the threshold line is the plain SRE-table factor.

use serde_json::{json, Value};

use crate::burn_rate::{AlertWindow, MwmbrConfig, Severity};
use crate::generate::fmt_num;
use crate::spec::SloSpec;

/// The label-matching idiom for dividing an SLI series by the budget series,
/// kept textually identical to the generator's metadata rules.
const GROUPING: &str = "on(sloth_id, sloth_slo, sloth_service) group_left";

/// Build the burn-rate panels for one SLO: one half-width timeseries panel per
/// enabled alert condition, page conditions first, two panels per row.
///
/// `id` and `y` are the dashboard's running panel id and vertical offset; they
/// advance exactly as far as the emitted panels occupy.
pub(super) fn panels(
    slo: &SloSpec,
    mwmbr: &MwmbrConfig,
    sel: &str,
    id: &mut i64,
    y: &mut i64,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut x = 0;
    for severity in [Severity::Page, Severity::Ticket] {
        if severity_disabled(slo, severity) {
            continue;
        }
        for window in mwmbr.for_severity(severity) {
            out.push(burn_panel(*id, severity, window, sel, x, *y));
            *id += 1;
            if x == 0 {
                x = 12;
            } else {
                x = 0;
                *y += 8;
            }
        }
    }
    if x == 12 {
        // An odd panel count leaves a half-open row: close it.
        *y += 8;
    }
    out
}

/// Whether this severity's alert is disabled in the spec, mirroring the
/// generator's skip in `alert::alert_rule`.
fn severity_disabled(slo: &SloSpec, severity: Severity) -> bool {
    match severity {
        Severity::Page => slo.alerting.page_alert.disable,
        Severity::Ticket => slo.alerting.ticket_alert.disable,
    }
}

/// The burn-rate expression for one lookback window: the recorded SLI error
/// ratio divided by the recorded error budget.
fn burn_expr(window_suffix: &str, sel: &str) -> String {
    format!(
        "slo:sli_error:ratio_rate{window_suffix}{sel}\n/ {GROUPING}\nslo:error_budget:ratio{sel}"
    )
}

fn burn_panel(
    id: i64,
    severity: Severity,
    window: &AlertWindow,
    sel: &str,
    x: i64,
    y: i64,
) -> Value {
    let long = window.long.prometheus();
    let short = window.short.prometheus();
    let title = format!(
        "{} burn rate ({long}/{short}, threshold {}x)",
        severity.label(),
        fmt_num(window.factor),
    );
    json!({
        "id": id,
        "type": "timeseries",
        "title": title,
        "datasource": super::datasource(),
        "gridPos": { "h": 8, "w": 12, "x": x, "y": y },
        "fieldConfig": {
            "defaults": {
                "unit": "none",
                "custom": { "thresholdsStyle": { "mode": "line" } },
                "thresholds": {
                    "mode": "absolute",
                    "steps": [
                        { "color": "green", "value": null },
                        { "color": "red", "value": window.factor },
                    ],
                },
            },
            "overrides": [],
        },
        "targets": [
            target("A", burn_expr(&long, sel), format!("long ({long})")),
            target("B", burn_expr(&short, sel), format!("short ({short})")),
        ],
    })
}

/// A panel query target with an explicit `refId` and legend, so the long and
/// short series are distinguishable in one panel.
fn target(ref_id: &str, expr: String, legend: String) -> Value {
    json!({
        "refId": ref_id,
        "expr": expr,
        "legendFormat": legend,
        "datasource": super::datasource(),
    })
}

#[cfg(test)]
mod tests {
    use crate::dashboard::dashboard_value;
    use crate::spec::Spec;
    use serde_json::Value;

    /// A spec with one custom page window and one custom ticket window, so the
    /// expected panel set is small and every value is hand-checkable.
    fn custom_windows_spec(extra_alerting: &str) -> Spec {
        Spec::from_yaml(&format!(
            r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{{{.window}}}}]
    alerting:
      windows:
        - severity: page
          long: 1h
          short: 10m
          factor: 10
        - severity: ticket
          long: 1d
          short: 2h
          factor: 2
{extra_alerting}"#,
        ))
        .unwrap()
    }

    fn burn_panels(spec: &Spec) -> Vec<Value> {
        dashboard_value(spec)["panels"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| {
                p["title"]
                    .as_str()
                    .is_some_and(|t| t.contains("burn rate ("))
            })
            .cloned()
            .collect()
    }

    fn threshold_of(panel: &Value) -> f64 {
        panel["fieldConfig"]["defaults"]["thresholds"]["steps"][1]["value"]
            .as_f64()
            .unwrap()
    }

    #[test]
    fn one_panel_per_severity_with_threshold_equal_to_factor() {
        let spec = custom_windows_spec("");
        let panels = burn_panels(&spec);
        assert_eq!(panels.len(), 2, "one page + one ticket condition");
        let page = &panels[0];
        let ticket = &panels[1];
        assert_eq!(
            page["title"], "page burn rate (1h/10m, threshold 10x)",
            "page panels come first and are titled by severity"
        );
        assert_eq!(ticket["title"], "ticket burn rate (1d/2h, threshold 2x)");
        assert_eq!(threshold_of(page), 10.0);
        assert_eq!(threshold_of(ticket), 2.0);
        assert_eq!(
            page["fieldConfig"]["defaults"]["custom"]["thresholdsStyle"]["mode"], "line",
            "the threshold renders as a line, not just a color ramp"
        );
    }

    #[test]
    fn panel_targets_query_long_and_short_burn_rates() {
        let spec = custom_windows_spec("");
        let page = &burn_panels(&spec)[0];
        let targets = page["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 2);
        let long_expr = targets[0]["expr"].as_str().unwrap();
        let short_expr = targets[1]["expr"].as_str().unwrap();
        assert!(
            long_expr.contains("slo:sli_error:ratio_rate1h{sloth_id=\"myservice-a\"}"),
            "long target queries the recorded long-window series: {long_expr}"
        );
        assert!(
            short_expr.contains("slo:sli_error:ratio_rate10m{sloth_id=\"myservice-a\"}"),
            "short target queries the recorded short-window series: {short_expr}"
        );
        for expr in [long_expr, short_expr] {
            assert!(
                expr.contains("/ on(sloth_id, sloth_slo, sloth_service) group_left"),
                "burn rate divides by budget with the generator's grouping idiom: {expr}"
            );
            assert!(
                expr.contains("slo:error_budget:ratio{sloth_id=\"myservice-a\"}"),
                "denominator is the recorded error budget: {expr}"
            );
        }
    }

    #[test]
    fn disabled_ticket_alert_emits_no_ticket_panel() {
        let spec = custom_windows_spec(
            r#"      ticket_alert:
        disable: true
"#,
        );
        let panels = burn_panels(&spec);
        assert_eq!(panels.len(), 1, "the page panel must survive");
        assert!(panels[0]["title"].as_str().unwrap().starts_with("page"));
    }

    #[test]
    fn disabling_both_severities_emits_no_burn_panels() {
        let spec = custom_windows_spec(
            r#"      page_alert:
        disable: true
      ticket_alert:
        disable: true
"#,
        );
        assert!(burn_panels(&spec).is_empty());
    }

    #[test]
    fn default_table_emits_four_panels_with_sre_factors() {
        let spec = Spec::from_yaml(
            r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
        )
        .unwrap();
        let panels = burn_panels(&spec);
        let thresholds: Vec<f64> = panels.iter().map(threshold_of).collect();
        assert_eq!(thresholds, vec![14.4, 6.0, 3.0, 1.0]);
    }

    #[test]
    fn panel_ids_stay_unique_across_the_dashboard() {
        let spec = custom_windows_spec("");
        let ids: Vec<i64> = dashboard_value(&spec)["panels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_i64().unwrap())
            .collect();
        let mut deduped = ids.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "duplicate panel id: {ids:?}");
    }
}
