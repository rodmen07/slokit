//! Golden snapshot of the generated Grafana dashboard JSON, plus the
//! contract for the dashboard's default time range: it follows the longest
//! resolved SLO period rather than a hardcoded `now-30d`, so a 7d SLO's
//! dashboard opens on its own period instead of four times it.

#![cfg(feature = "dashboard")]

use slokit::dashboard::{dashboard_json, dashboard_value, dashboard_value_with};
use slokit::generate::GenerateOptions;
use slokit::spec::Spec;
use slokit::Window;

const SAMPLE: &str = include_str!("fixtures/sample.yaml");

/// A minimal spec whose SLOs declare the given periods (`None` = no `period`
/// key, so the SLO takes the default).
fn spec_with_periods(periods: &[Option<&str>]) -> Spec {
    let mut yaml = String::from("service: myservice\nslos:\n");
    for (i, period) in periods.iter().enumerate() {
        yaml.push_str(&format!(
            "  - name: slo-{i}\n    objective: 99.9\n    sli:\n      raw:\n        error_ratio_query: r[{{{{.window}}}}]\n"
        ));
        if let Some(p) = period {
            yaml.push_str(&format!("    period: {p}\n"));
        }
    }
    Spec::from_yaml(&yaml).unwrap()
}

fn time_from(value: &serde_json::Value) -> &str {
    value["time"]["from"].as_str().unwrap()
}

#[test]
fn dashboard_json_snapshot() {
    let spec = Spec::from_yaml(SAMPLE).unwrap();
    let json = dashboard_json(&spec).unwrap();
    insta::assert_snapshot!("dashboard", json);
}

#[test]
fn dashboard_has_a_block_per_slo() {
    let spec = Spec::from_yaml(SAMPLE).unwrap();
    let value = slokit::dashboard::dashboard_value(&spec);
    // Two SLOs => 2 rows + 2 * (3 stats + 1 SLI timeseries + 4 default-table
    // burn panels) = 18 panels.
    assert_eq!(value["panels"].as_array().unwrap().len(), 18);
}

#[test]
fn time_range_follows_a_declared_slo_period() {
    // A 7d SLO's dashboard opens on its own period, not on a hardcoded 30d
    // window four times as long.
    let value = dashboard_value(&spec_with_periods(&[Some("7d")]));
    assert_eq!(
        time_from(&value),
        "now-7d",
        "the dashboard time range must follow the SLO's declared period"
    );
}

#[test]
fn time_range_stays_30d_for_the_default_period() {
    // The default-period dashboard keeps its shipped range byte-for-byte:
    // following the resolved period IS `now-30d` at the 30d default, so
    // existing default-period output does not move (docs/SEMVER.md).
    let value = dashboard_value(&spec_with_periods(&[None]));
    assert_eq!(
        value["time"],
        serde_json::json!({ "from": "now-30d", "to": "now" })
    );
}

#[test]
fn time_range_follows_the_default_period_option() {
    // The same axis through options rather than the spec: an SLO with no
    // period of its own resolves to `GenerateOptions::default_period`
    // (the CLI's `--period`), and the time range follows that resolution.
    // `GenerateOptions` is `#[non_exhaustive]`, so build-then-assign is the
    // only construction available outside the crate (the dashboard_drift.rs
    // convention).
    let mut opts = GenerateOptions::default();
    opts.default_period = Window::days(7);
    let value = dashboard_value_with(&spec_with_periods(&[None]), &opts);
    assert_eq!(
        time_from(&value),
        "now-7d",
        "the dashboard time range must follow the resolved default period"
    );
}

#[test]
fn time_range_of_a_mixed_period_spec_covers_the_longest() {
    // Three SLOs, longest period neither first nor last, so an implementation
    // taking the first, the last, or the shortest all fail here. The longest
    // period is the only range under which every SLO's whole period is
    // visible when the dashboard opens.
    let value = dashboard_value(&spec_with_periods(&[Some("7d"), Some("90d"), Some("30d")]));
    assert_eq!(
        time_from(&value),
        "now-90d",
        "a mixed-period spec's dashboard must open on the longest resolved period"
    );
}
