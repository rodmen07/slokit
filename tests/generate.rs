//! End-to-end generation tests: golden snapshots plus targeted assertions on
//! the burn-rate alert thresholds.

use slokit::generate::{generate_rules, generate_rules_with, GenerateOptions};
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

#[test]
fn custom_alert_windows_replace_the_default_table() {
    let spec = Spec::from_yaml(
        r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 30m
          short: 10m
          factor: 10
        - severity: ticket
          long: 12h
          short: 1h
          factor: 2
"#,
    )
    .unwrap();
    let yaml = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();
    // Alert conditions use the custom windows and factors.
    assert!(yaml.contains("slo:sli_error:ratio_rate30m"));
    assert!(yaml.contains("(10 * 0.001)"));
    assert!(yaml.contains("(2 * 0.001)"));
    assert!(!yaml.contains("(14.4 * 0.001)"));
    // Recordings cover exactly the custom lookbacks; the default 5m and 3d
    // recordings are gone, and the period aggregation uses the shortest custom
    // window as its base.
    assert!(!yaml.contains("ratio_rate5m"));
    assert!(!yaml.contains("ratio_rate3d"));
    assert!(yaml.contains("sum_over_time(slo:sli_error:ratio_rate10m"));
    assert!(yaml.contains("slo:sli_error:ratio_rate10m{sloth_id=\"s-a\""));
}

#[test]
fn non_default_period_scales_the_default_windows() {
    let spec = Spec::from_yaml(
        r#"
service: s
slos:
  - name: a
    objective: 99.9
    period: 90d
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      labels: { severity: page }
"#,
    )
    .unwrap();
    let yaml = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();
    // 90d = 3x the 30d calibration: 1h -> 3h, 5m -> 15m, 3d -> 9d.
    assert!(yaml.contains("slo:sli_error:ratio_rate3h"));
    assert!(yaml.contains("slo:sli_error:ratio_rate15m"));
    assert!(yaml.contains("slo:sli_error:ratio_rate9d"));
    assert!(yaml.contains("slo:sli_error:ratio_rate90d"));
    assert!(!yaml.contains("ratio_rate5m"));
    // The current-burn-rate metadata rule follows the shortest window.
    assert!(yaml.contains("slo:sli_error:ratio_rate15m{sloth_id=\"s-a\""));
}

#[test]
fn period_scaling_can_be_disabled() {
    let spec = Spec::from_yaml(
        r#"
service: s
slos:
  - name: a
    objective: 99.9
    period: 90d
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      labels: { severity: page }
"#,
    )
    .unwrap();
    let mut opts = GenerateOptions::default();
    opts.period_aware = false;
    let yaml = generate_rules_with(&spec, &opts)
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    // Windows stay at the 30d calibration even though the period is 90d.
    assert!(yaml.contains("slo:sli_error:ratio_rate5m"));
    assert!(yaml.contains("slo:sli_error:ratio_rate3d"));
    assert!(yaml.contains("slo:sli_error:ratio_rate90d"));
    assert!(!yaml.contains("ratio_rate15m"));
}

#[test]
fn default_period_output_is_unchanged_by_scaling() {
    // 30d scales to itself: period-aware generation must be byte-identical to
    // the pre-scaling output for default-period specs.
    let aware = generate_rules(&sample_spec())
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    let mut opts = GenerateOptions::default();
    opts.period_aware = false;
    let unaware = generate_rules_with(&sample_spec(), &opts)
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    assert_eq!(aware, unaware);
}
