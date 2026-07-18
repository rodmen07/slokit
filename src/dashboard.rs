//! Generate a Grafana dashboard from a [`Spec`].
//!
//! The dashboard renders one block per SLO, querying the same `slo:...` metrics
//! the rule [generator](crate::generate) emits: error budget remaining, current
//! burn rate, objective, and the SLI error ratio over time. It declares a
//! `datasource` template variable so it imports cleanly into any Grafana with a
//! Prometheus data source.

use serde_json::{json, Value};

use crate::burn_rate::MwmbrConfig;
use crate::error::{Result, SlokitError};
use crate::spec::{SloSpec, Spec, DEFAULT_PERIOD};
use crate::window::Window;

/// The shortest lookback window this SLO's rules record, which is the metric
/// the SLI timeseries panel must query. Mirrors the generator's resolution:
/// custom `alerting.windows` win, else the default table scaled to the period.
fn sli_panel_window(slo: &SloSpec) -> Window {
    let mwmbr = slo.custom_mwmbr().ok().flatten().unwrap_or_else(|| {
        let period = slo.resolve_period(DEFAULT_PERIOD).unwrap_or(DEFAULT_PERIOD);
        MwmbrConfig::sre_default_for_period(period)
    });
    mwmbr
        .lookback_windows()
        .first()
        .copied()
        .unwrap_or(Window::minutes(5))
}

/// Build the Grafana dashboard as a [`serde_json::Value`].
pub fn dashboard_value(spec: &Spec) -> Value {
    let mut panels = Vec::new();
    let mut id: i64 = 1;
    let mut y: i64 = 0;

    for slo in &spec.slos {
        let sloth_id = slo.sloth_id(&spec.service);
        let sel = format!("{{sloth_id=\"{sloth_id}\"}}");
        let sli_window = sli_panel_window(slo).prometheus();

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
    }

    json!({
        "uid": dashboard_uid(&spec.service),
        "title": format!("slokit: {}", spec.service),
        "tags": ["slokit", "slo"],
        "schemaVersion": 39,
        "editable": true,
        "timezone": "",
        "refresh": "1m",
        "time": { "from": "now-30d", "to": "now" },
        "templating": { "list": [datasource_var()] },
        "panels": panels,
    })
}

/// Build the Grafana dashboard as pretty-printed JSON.
pub fn dashboard_json(spec: &Spec) -> Result<String> {
    serde_json::to_string_pretty(&dashboard_value(spec))
        .map_err(|e| SlokitError::Spec(e.to_string()))
}

/// Build dashboards for one or many specs: a single spec renders one dashboard
/// object, multiple specs render a JSON array of dashboards.
pub fn dashboards_json(specs: &[Spec]) -> Result<String> {
    let out = match specs {
        [one] => dashboard_value(one),
        many => Value::Array(many.iter().map(dashboard_value).collect()),
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
        // One SLO => 1 row + 3 stats + 1 timeseries.
        assert_eq!(panels.len(), 5);
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
