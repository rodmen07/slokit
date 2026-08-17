//! Generate a Grafana dashboard from a [`Spec`].
//!
//! The dashboard renders one block per SLO, querying the same `slo:...` metrics
//! the rule [generator](crate::generate) emits: error budget remaining, current
//! burn rate, objective, the SLI error ratio over time, and one burn-rate
//! panel per enabled alert condition with a threshold line at that condition's
//! burn-rate factor (see [`burn`]). It declares a `datasource` template
//! variable so it imports cleanly into any Grafana with a Prometheus data
//! source, and its default time range follows the longest period any SLO in
//! the spec resolves to (see [`default_time_range`]), so the dashboard opens
//! showing every SLO's whole period.

mod burn;

use serde_json::{json, Value};

use crate::burn_rate::MwmbrConfig;
use crate::error::{Result, SlokitError};
use crate::generate::GenerateOptions;
use crate::spec::{SloSpec, Spec};
use crate::window::Window;

/// The effective burn-rate configuration for one SLO under the same options
/// the rule generator ran with, resolved through the generator's own seam
/// ([`crate::generate::resolve_mwmbr`]) rather than re-derived here.
///
/// Re-deriving it is exactly what used to go wrong: this function hardcoded
/// the 30d default period and always scaled, so `slokit generate --period 7d`
/// or `--no-period-scaling` produced rules whose window-scoped series names no
/// panel here referenced, and every burn panel rendered "No data".
fn resolved_mwmbr(slo: &SloSpec, opts: &GenerateOptions) -> MwmbrConfig {
    let period = slo
        .resolve_period(opts.default_period)
        .unwrap_or(opts.default_period);
    crate::generate::resolve_mwmbr(slo.custom_mwmbr().ok().flatten(), period, opts)
}

/// The shortest lookback window this SLO's rules record, which is the metric
/// the SLI timeseries panel must query.
fn sli_panel_window(mwmbr: &MwmbrConfig) -> Window {
    mwmbr
        .lookback_windows()
        .first()
        .copied()
        .unwrap_or(Window::minutes(5))
}

/// The dashboard's default time range: the longest period any SLO in the spec
/// resolves to under `opts`, so the dashboard opens showing every SLO's whole
/// period.
///
/// "The whole period" is not a new opinion but the semantic the dashboard
/// already had at the default period: the range was the literal `now-30d`,
/// which equals the period for every default-period SLO and only diverged for
/// SLOs declaring their own (`period: 7d` opened on four times its period,
/// `period: 90d` on a third of its own error budget's span). The error-budget
/// stat is a period-scoped number and the burn thresholds gate the period's
/// budget, so the period is the span those panels are about. Viewers change
/// the range in Grafana at will; this is only where the dashboard opens.
fn default_time_range(spec: &Spec, opts: &GenerateOptions) -> Window {
    spec.slos
        .iter()
        .map(|slo| {
            slo.resolve_period(opts.default_period)
                .unwrap_or(opts.default_period)
        })
        .max()
        .unwrap_or(opts.default_period)
}

/// Build the Grafana dashboard as a [`serde_json::Value`], using default
/// generation options.
///
/// Equivalent to [`dashboard_value_with`] with
/// [`GenerateOptions::default`]. Use the `_with` form whenever the rules were
/// generated with non-default options (`slokit generate --period`,
/// `--no-period-scaling`), or the panels will query series those rules do not
/// record.
pub fn dashboard_value(spec: &Spec) -> Value {
    dashboard_value_with(spec, &GenerateOptions::default())
}

/// Build the Grafana dashboard as a [`serde_json::Value`] for rules generated
/// with `opts`.
///
/// The dashboard's panels query the `slo:` series
/// [`generate_rules_with`](crate::generate::generate_rules_with) records, and
/// the window-scoped ones (`slo:sli_error:ratio_rate<window>`) carry the
/// resolved burn-rate window in their NAME. Passing the same options both
/// commands ran with is therefore what keeps the dashboard pointed at series
/// that exist.
pub fn dashboard_value_with(spec: &Spec, opts: &GenerateOptions) -> Value {
    let mut panels = Vec::new();
    let mut id: i64 = 1;
    let mut y: i64 = 0;
    let time_range = default_time_range(spec, opts);

    for slo in &spec.slos {
        let sloth_id = slo.sloth_id(&spec.service);
        let sel = format!("{{sloth_id=\"{sloth_id}\"}}");
        let mwmbr = resolved_mwmbr(slo, opts);
        let sli_window = sli_panel_window(&mwmbr).prometheus();

        panels.push(row_panel(id, &slo.name, y));
        id += 1;
        y += 1;

        panels.push(stat_panel(
            id,
            "Error budget remaining",
            format!("slo:period_error_budget_remaining:ratio{sel}"),
            "percentunit",
            0,
            y,
        ));
        id += 1;
        panels.push(stat_panel(
            id,
            "Current burn rate",
            format!("slo:current_burn_rate:ratio{sel}"),
            "none",
            8,
            y,
        ));
        id += 1;
        panels.push(stat_panel(
            id,
            "Objective",
            format!("slo:objective:ratio{sel}"),
            "percentunit",
            16,
            y,
        ));
        id += 1;
        y += 6;

        panels.push(timeseries_panel(
            id,
            &format!("SLI error ratio ({sli_window})"),
            format!("slo:sli_error:ratio_rate{sli_window}{sel}"),
            "percentunit",
            y,
        ));
        id += 1;
        y += 8;

        panels.extend(burn::panels(slo, &mwmbr, &sel, &mut id, &mut y));
    }

    json!({
        "uid": dashboard_uid(&spec.service),
        "title": format!("slokit: {}", spec.service),
        "tags": ["slokit", "slo"],
        "schemaVersion": 39,
        "editable": true,
        "timezone": "",
        "refresh": "1m",
        "time": { "from": format!("now-{}", time_range.prometheus()), "to": "now" },
        "templating": { "list": [datasource_var()] },
        "panels": panels,
    })
}

/// Build the Grafana dashboard as pretty-printed JSON, using default
/// generation options.
pub fn dashboard_json(spec: &Spec) -> Result<String> {
    dashboard_json_with(spec, &GenerateOptions::default())
}

/// Build the Grafana dashboard as pretty-printed JSON for rules generated with
/// `opts`. See [`dashboard_value_with`].
pub fn dashboard_json_with(spec: &Spec, opts: &GenerateOptions) -> Result<String> {
    serde_json::to_string_pretty(&dashboard_value_with(spec, opts))
        .map_err(|e| SlokitError::Spec(e.to_string()))
}

/// Build dashboards for one or many specs: a single spec renders one dashboard
/// object, multiple specs render a JSON array of dashboards. Uses default
/// generation options.
pub fn dashboards_json(specs: &[Spec]) -> Result<String> {
    dashboards_json_with(specs, &GenerateOptions::default())
}

/// Build dashboards for one or many specs for rules generated with `opts`.
/// See [`dashboard_value_with`].
pub fn dashboards_json_with(specs: &[Spec], opts: &GenerateOptions) -> Result<String> {
    let out = match specs {
        [one] => dashboard_value_with(one, opts),
        many => Value::Array(
            many.iter()
                .map(|spec| dashboard_value_with(spec, opts))
                .collect(),
        ),
    };
    serde_json::to_string_pretty(&out).map_err(|e| SlokitError::Spec(e.to_string()))
}

fn datasource() -> Value {
    json!({ "type": "prometheus", "uid": "${datasource}" })
}

fn datasource_var() -> Value {
    json!({
        "name": "datasource",
        "label": "Data source",
        "type": "datasource",
        "query": "prometheus",
        "hide": 0,
        "refresh": 1,
        "regex": "",
        "current": {},
    })
}

fn row_panel(id: i64, title: &str, y: i64) -> Value {
    json!({
        "id": id,
        "type": "row",
        "title": title,
        "collapsed": false,
        "gridPos": { "h": 1, "w": 24, "x": 0, "y": y },
        "panels": [],
    })
}

fn stat_panel(id: i64, title: &str, expr: String, unit: &str, x: i64, y: i64) -> Value {
    json!({
        "id": id,
        "type": "stat",
        "title": title,
        "datasource": datasource(),
        "gridPos": { "h": 6, "w": 8, "x": x, "y": y },
        "fieldConfig": { "defaults": { "unit": unit }, "overrides": [] },
        "targets": [target(expr)],
    })
}

fn timeseries_panel(id: i64, title: &str, expr: String, unit: &str, y: i64) -> Value {
    json!({
        "id": id,
        "type": "timeseries",
        "title": title,
        "datasource": datasource(),
        "gridPos": { "h": 8, "w": 24, "x": 0, "y": y },
        "fieldConfig": { "defaults": { "unit": unit }, "overrides": [] },
        "targets": [target(expr)],
    })
}

fn target(expr: String) -> Value {
    json!({ "refId": "A", "expr": expr, "datasource": datasource() })
}

/// A deterministic, Grafana-safe dashboard uid derived from the service name.
fn dashboard_uid(service: &str) -> String {
    let cleaned: String = service
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("slokit-{cleaned}").chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        Spec::from_yaml(
            r#"
service: myservice
slos:
  - name: requests-availability
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
        )
        .unwrap()
    }

    #[test]
    fn builds_panels_per_slo() {
        let v = dashboard_value(&spec());
        assert_eq!(v["uid"], "slokit-myservice");
        let panels = v["panels"].as_array().unwrap();
        // One SLO => 1 row + 3 stats + 1 SLI timeseries + 4 default-table
        // burn-rate panels.
        assert_eq!(panels.len(), 9);
        assert_eq!(panels[0]["type"], "row");
    }

    #[test]
    fn references_generated_metrics() {
        // Quotes inside the JSON string are escaped, so match the metric prefix
        // (the `sloth_id` value follows it).
        let json = dashboard_json(&spec()).unwrap();
        assert!(json.contains("slo:period_error_budget_remaining:ratio{sloth_id="));
        assert!(json.contains("slo:current_burn_rate:ratio{sloth_id="));
        assert!(json.contains("slo:sli_error:ratio_rate5m{sloth_id="));
    }

    #[test]
    fn uid_is_sanitized() {
        assert_eq!(dashboard_uid("my service/app"), "slokit-my-service-app");
    }

    #[test]
    fn sli_panel_follows_custom_windows() {
        let spec = Spec::from_yaml(
            r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      windows:
        - severity: page
          long: 1h
          short: 10m
          factor: 10
"#,
        )
        .unwrap();
        let json = dashboard_json(&spec).unwrap();
        assert!(json.contains("slo:sli_error:ratio_rate10m{sloth_id="));
        assert!(json.contains("SLI error ratio (10m)"));
    }

    #[test]
    fn sli_panel_follows_scaled_period() {
        // 90d period scales the default 5m short window to 15m.
        let spec = Spec::from_yaml(
            r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    period: 90d
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
        )
        .unwrap();
        let json = dashboard_json(&spec).unwrap();
        assert!(json.contains("slo:sli_error:ratio_rate15m{sloth_id="));
    }
}
