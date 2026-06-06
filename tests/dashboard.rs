//! Golden snapshot of the generated Grafana dashboard JSON.

#![cfg(feature = "dashboard")]

use slokit::dashboard::dashboard_json;
use slokit::spec::Spec;

const SAMPLE: &str = include_str!("fixtures/sample.yaml");

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
    // Two SLOs => 2 rows + 2 * (3 stats + 1 timeseries) = 10 panels.
    assert_eq!(value["panels"].as_array().unwrap().len(), 10);
}
