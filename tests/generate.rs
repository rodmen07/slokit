//! End-to-end generation tests: golden snapshots plus targeted assertions on
//! the burn-rate alert thresholds.

use slokit::generate::generate_rules;
use slokit::spec::Spec;

const SAMPLE: &str = include_str!("fixtures/sample.yaml");

fn sample_spec() -> Spec {
    Spec::from_yaml(SAMPLE).expect("fixture parses")
}

/// Replace the crate version so snapshots survive version bumps.
fn redact_version(yaml: String) -> String {
    yaml.replace(env!("CARGO_PKG_VERSION"), "[VERSION]")
}

#[test]
fn prometheus_rules_snapshot() {
    let rules = generate_rules(&sample_spec()).unwrap();
    let yaml = redact_version(rules.to_prometheus_yaml().unwrap());
    insta::assert_snapshot!("prometheus_rules", yaml);
}

#[test]
fn operator_rules_snapshot() {
    let spec = sample_spec();
    let rules = generate_rules(&spec).unwrap();
    let yaml = redact_version(rules.to_operator_yaml(&spec.service, &spec.labels).unwrap());
    insta::assert_snapshot!("operator_rules", yaml);
}

#[test]
fn page_thresholds_are_factor_times_budget() {
    // 99.9% objective => budget ratio 0.001. Fast page window factor 14.4 must
    // appear as the threshold (14.4 * 0.001).
    let yaml = generate_rules(&sample_spec())
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    assert!(yaml.contains("(14.4 * 0.001)"));
    assert!(yaml.contains("(6 * 0.001)"));
    assert!(yaml.contains("(3 * 0.001)"));
    assert!(yaml.contains("(1 * 0.001)"));
}

#[test]
fn emits_expected_group_count() {
    // Two SLOs, three groups each.
    let rules = generate_rules(&sample_spec()).unwrap();
    assert_eq!(rules.groups.len(), 6);
}

#[test]
fn raw_sli_period_rule_is_present() {
    // The latency SLO is a raw SLI; it still gets a 30d period recording.
    let yaml = generate_rules(&sample_spec())
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    assert!(yaml.contains("slo:sli_error:ratio_rate30d"));
    assert!(yaml.contains("sloth_slo: requests-latency"));
}
