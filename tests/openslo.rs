//! OpenSLO v1 import tests: fixture mapping, the golden snapshot, equivalence
//! with a hand-written slokit spec, unrepresentable-document errors, and
//! round-trip consistency (imported specs pass validate and lint cleanly
//! except for the documented alerting advisories).

#![cfg(feature = "spec")]

use slokit::generate::generate_rules;
use slokit::spec::{openslo, validate_all, Spec};

const RATIO: &str = include_str!("fixtures/openslo/ratio.yaml");
const MULTI_OBJECTIVE: &str = include_str!("fixtures/openslo/multi-objective.yaml");
const LATENCY: &str = include_str!("fixtures/openslo/latency.yaml");
const UNREPRESENTABLE: &str = include_str!("fixtures/openslo/unrepresentable.yaml");
const STREAM: &str = include_str!("fixtures/openslo/stream.yaml");
const SLOKIT_SAMPLE: &str = include_str!("fixtures/sample.yaml");

/// A minimal valid OpenSLO SLO document that error tests mutate.
const BASE: &str = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: base
spec:
  description: "base"
  service: svc
  budgetingMethod: Occurrences
  timeWindow:
    - duration: 30d
      isRolling: true
  indicator:
    spec:
      ratioMetric:
        bad:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(errs_total[5m]))
        total:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(reqs_total[5m]))
  objectives:
    - target: 0.999
"#;

/// Replace the crate version so snapshots survive version bumps.
fn redact_version(yaml: String) -> String {
    yaml.replace(env!("CARGO_PKG_VERSION"), "[VERSION]")
}

fn import_err(yaml: &str) -> String {
    openslo::from_yaml(yaml).unwrap_err().to_string()
}

#[test]
fn ratio_fixture_maps_to_an_events_slokit_spec() {
    let import = openslo::from_yaml(RATIO).unwrap();
    assert_eq!(import.specs.len(), 1);
    let spec = &import.specs[0];
    assert_eq!(spec.service, "myservice");
    assert_eq!(spec.version, "prometheus/v1");
    assert_eq!(spec.slos.len(), 1);

    let slo = &spec.slos[0];
    assert_eq!(slo.name, "requests-availability");
    assert!((slo.objective - 99.9).abs() < 1e-9);
    assert_eq!(slo.description, "99.9% of HTTP requests succeed");
    assert_eq!(slo.labels["owner"], "team-platform");
    assert_eq!(slo.period.as_deref(), Some("30d"));

    let events = slo.sli.events.as_ref().expect("events SLI");
    assert_eq!(
        events.error_query,
        "sum(rate(http_requests_total{job=\"myservice\",code=~\"5..\"}[{{.window}}]))"
    );
    assert_eq!(
        events.total_query,
        "sum(rate(http_requests_total{job=\"myservice\"}[{{.window}}]))"
    );

    // The fixed [5m] windows were rewritten, and the import says so.
    assert!(
        import
            .notes
            .iter()
            .any(|n| n.location == "slo 'requests-availability'"
                && n.message.contains("rewrote fixed range window(s) [5m]")),
        "missing rewrite note: {:?}",
        import.notes
    );
}

#[test]
fn ratio_fixture_generates_the_same_rules_as_its_slokit_twin() {
    // The roadmap's done-when: an OpenSLO fixture generates the same rules as
    // its equivalent sloth-compatible spec.
    let twin = Spec::from_yaml(
        r#"
version: "prometheus/v1"
service: myservice
slos:
  - name: requests-availability
    objective: 99.9
    description: "99.9% of HTTP requests succeed"
    labels:
      owner: team-platform
    period: 30d
    sli:
      events:
        error_query: sum(rate(http_requests_total{job="myservice",code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total{job="myservice"}[{{.window}}]))
"#,
    )
    .unwrap();

    let import = openslo::from_yaml(RATIO).unwrap();
    let imported = generate_rules(&import.specs[0]).unwrap();
    let hand_written = generate_rules(&twin).unwrap();
    assert_eq!(imported, hand_written, "RuleSets must be identical");
    assert_eq!(
        imported.to_prometheus_yaml().unwrap(),
        hand_written.to_prometheus_yaml().unwrap(),
        "rendered YAML must be byte-identical"
    );
}

#[test]
fn openslo_rules_snapshot() {
    let import = openslo::from_yaml(RATIO).unwrap();
    let rules = generate_rules(&import.specs[0]).unwrap();
    let yaml = redact_version(rules.to_prometheus_yaml().unwrap());
    insta::assert_snapshot!("openslo_rules", yaml);
}

#[test]
fn multi_objective_document_yields_one_slo_per_objective() {
    let import = openslo::from_yaml(MULTI_OBJECTIVE).unwrap();
    assert_eq!(import.specs.len(), 1);
    let spec = &import.specs[0];
    assert_eq!(spec.service, "checkout");
    assert_eq!(spec.slos.len(), 2);

    let fast = &spec.slos[0];
    assert_eq!(fast.name, "api-latency-fast");
    assert!((fast.objective - 99.0).abs() < 1e-9);
    assert_eq!(fast.period.as_deref(), Some("28d"));
    assert_eq!(fast.labels["team"], "checkout");
    let fast_latency = fast.sli.latency.as_ref().expect("latency SLI");
    assert_eq!(
        fast_latency.histogram_metric,
        "http_request_duration_seconds"
    );
    assert_eq!(fast_latency.threshold, "0.3");
    assert_eq!(fast_latency.selector.as_deref(), Some("job=\"checkout\""));

    let tolerable = &spec.slos[1];
    assert_eq!(tolerable.name, "api-latency-tolerable");
    assert!((tolerable.objective - 99.9).abs() < 1e-9);
    assert_eq!(tolerable.sli.latency.as_ref().unwrap().threshold, "1.5");
}

#[test]
fn latency_fixture_maps_to_the_latency_sli() {
    let import = openslo::from_yaml(LATENCY).unwrap();
    let spec = &import.specs[0];
    assert_eq!(spec.service, "search");
    let slo = &spec.slos[0];
    assert_eq!(slo.name, "search-latency");
    let latency = slo.sli.latency.as_ref().expect("latency SLI");
    assert_eq!(latency.histogram_metric, "search_request_duration_seconds");
    assert_eq!(latency.threshold, "0.3");
    assert_eq!(
        latency.selector.as_deref(),
        Some("job=\"search\", handler=\"/search\"")
    );
}

#[test]
fn stream_merges_services_resolves_refs_and_notes_ignored_kinds() {
    let import = openslo::from_yaml(STREAM).unwrap();
    assert_eq!(
        import.specs.len(),
        1,
        "both SLOs share the payments service"
    );
    let spec = &import.specs[0];
    assert_eq!(spec.service, "payments");
    assert_eq!(spec.slos.len(), 2);

    // The availability SLO resolved its indicatorRef against the kind: SLI
    // document, with the error query derived from good/total.
    let avail = &spec.slos[0];
    assert_eq!(avail.name, "payments-availability");
    let events = avail.sli.events.as_ref().expect("events SLI");
    assert_eq!(
        events.error_query,
        "(sum(rate(payments_requests_total[{{.window}}]))) - (sum(rate(payments_requests_total{code!~\"5..\"}[{{.window}}])))"
    );
    assert_eq!(
        events.total_query,
        "sum(rate(payments_requests_total[{{.window}}]))"
    );
    assert!(
        import
            .notes
            .iter()
            .any(|n| n.message.contains("total minus good")),
        "missing good-derivation note: {:?}",
        import.notes
    );

    let latency = &spec.slos[1];
    assert_eq!(latency.name, "payments-latency");
    assert_eq!(latency.sli.latency.as_ref().unwrap().threshold, "1");
    assert!((latency.objective - 95.0).abs() < 1e-9);

    // The Service document does not map, and the import says so.
    assert!(
        import
            .notes
            .iter()
            .any(|n| n.message.contains("kind 'Service'")),
        "missing ignored-kind note: {:?}",
        import.notes
    );
}

#[test]
fn imported_fixtures_pass_validate_and_lint_cleanly() {
    // Round-trip consistency: everything the importer produces must survive
    // slokit's own validation, and lint may only report the documented
    // advisories (imported SLOs carry no alert routing labels).
    for (name, yaml) in [
        ("ratio", RATIO),
        ("multi-objective", MULTI_OBJECTIVE),
        ("latency", LATENCY),
        ("stream", STREAM),
    ] {
        let import = openslo::from_yaml(yaml)
            .unwrap_or_else(|e| panic!("fixture {name} failed to import: {e}"));
        validate_all(&import.specs)
            .unwrap_or_else(|e| panic!("fixture {name} failed validation: {e}"));
        for spec in &import.specs {
            for finding in spec.lint() {
                assert!(
                    matches!(finding.code, "NO_ALERT_LABELS" | "NO_DESCRIPTION"),
                    "fixture {name}: unexpected lint {}: {}",
                    finding.code,
                    finding.message
                );
            }
            // And the imported spec generates rules without complaint.
            generate_rules(spec)
                .unwrap_or_else(|e| panic!("fixture {name} failed generation: {e}"));
        }
    }
}

#[test]
fn unrepresentable_calendar_window_is_a_clear_error() {
    let msg = import_err(UNREPRESENTABLE);
    assert!(msg.contains("slo 'monthly-report-freshness'"), "{msg}");
    assert!(msg.contains("spec.timeWindow[0].calendar"), "{msg}");
    assert!(msg.contains("not representable"), "{msg}");
}

#[test]
fn unsupported_budgeting_method_is_an_error() {
    let msg = import_err(&BASE.replace(
        "budgetingMethod: Occurrences",
        "budgetingMethod: Timeslices",
    ));
    assert!(msg.contains("spec.budgetingMethod 'Timeslices'"), "{msg}");
}

#[test]
fn non_prometheus_metric_source_is_an_error() {
    let msg = import_err(&BASE.replace("type: Prometheus", "type: Datadog"));
    assert!(msg.contains("metricSource.type 'Datadog'"), "{msg}");
    assert!(msg.contains("only Prometheus metric sources map"), "{msg}");
}

#[test]
fn calendar_duration_units_are_errors() {
    let msg = import_err(&BASE.replace("duration: 30d", "duration: 1M"));
    assert!(msg.contains("calendar unit 'M'"), "{msg}");
}

#[test]
fn non_rolling_window_is_an_error() {
    let msg = import_err(&BASE.replace("isRolling: true", "isRolling: false"));
    assert!(msg.contains("spec.timeWindow[0].isRolling"), "{msg}");
}

#[test]
fn target_and_target_percent_together_are_an_error() {
    let msg = import_err(&BASE.replace(
        "- target: 0.999",
        "- target: 0.999\n      targetPercent: 99.9",
    ));
    assert!(msg.contains("set either target or targetPercent"), "{msg}");
}

#[test]
fn windowless_query_is_an_error() {
    let msg = import_err(&BASE.replace("sum(rate(errs_total[5m]))", "sum(errs_ratio)"));
    assert!(msg.contains("no fixed range selector"), "{msg}");
    assert!(msg.contains("ratioMetric.bad"), "{msg}");
}

#[test]
fn unresolvable_indicator_ref_is_an_error() {
    let yaml = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: refslo
spec:
  service: svc
  indicatorRef: nowhere-to-be-found
  objectives:
    - target: 0.99
"#;
    let msg = import_err(yaml);
    assert!(
        msg.contains("spec.indicatorRef 'nowhere-to-be-found'"),
        "{msg}"
    );
    assert!(msg.contains("no kind: SLI document"), "{msg}");
}

#[test]
fn threshold_op_gte_and_non_bare_queries_are_errors() {
    let threshold = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: t
spec:
  service: svc
  timeWindow:
    - duration: 30d
      isRolling: true
  indicator:
    spec:
      thresholdMetric:
        metricSource:
          type: Prometheus
          spec:
            query: http_request_duration_seconds
  objectives:
    - op: lte
      value: 0.5
      target: 0.99
"#;
    // Sanity: the base threshold document imports.
    openslo::from_yaml(threshold).unwrap();

    let msg = import_err(&threshold.replace("op: lte", "op: gte"));
    assert!(msg.contains("op 'gte' is not representable"), "{msg}");

    let msg = import_err(&threshold.replace(
        "query: http_request_duration_seconds",
        "query: histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))",
    ));
    assert!(msg.contains("not representable"), "{msg}");
    assert!(
        msg.contains("bare Prometheus histogram base metric"),
        "{msg}"
    );
}

#[test]
fn threshold_op_lt_imports_with_a_note() {
    let yaml = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: t
spec:
  service: svc
  timeWindow:
    - duration: 30d
      isRolling: true
  indicator:
    spec:
      thresholdMetric:
        metricSource:
          type: Prometheus
          spec:
            query: http_request_duration_seconds
  objectives:
    - op: lt
      value: 0.5
      target: 0.99
"#;
    let import = openslo::from_yaml(yaml).unwrap();
    assert!(import.specs[0].slos[0].sli.latency.is_some());
    assert!(
        import
            .notes
            .iter()
            .any(|n| n.message.contains("'lt' is treated as 'lte'")),
        "{:?}",
        import.notes
    );
}

#[test]
fn raw_ratio_maps_by_raw_type() {
    let raw = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: rawslo
spec:
  service: svc
  timeWindow:
    - duration: 30d
      isRolling: true
  indicator:
    spec:
      ratioMetric:
        rawType: failure
        raw:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(errs[5m])) / sum(rate(reqs[5m]))
  objectives:
    - target: 0.99
"#;
    let import = openslo::from_yaml(raw).unwrap();
    let sli = import.specs[0].slos[0].sli.raw.as_ref().expect("raw SLI");
    assert_eq!(
        sli.error_ratio_query,
        "sum(rate(errs[{{.window}}])) / sum(rate(reqs[{{.window}}]))"
    );

    let import = openslo::from_yaml(&raw.replace("rawType: failure", "rawType: success")).unwrap();
    let sli = import.specs[0].slos[0].sli.raw.as_ref().expect("raw SLI");
    assert_eq!(
        sli.error_ratio_query,
        "1 - (sum(rate(errs[{{.window}}])) / sum(rate(reqs[{{.window}}])))"
    );

    let msg = import_err(&raw.replace("rawType: failure\n        ", ""));
    assert!(msg.contains("rawType is required"), "{msg}");
}

#[test]
fn unsupported_api_version_is_an_error() {
    let msg = import_err(&BASE.replace("openslo/v1", "openslo/v2alpha"));
    assert!(
        msg.contains("unsupported apiVersion 'openslo/v2alpha'"),
        "{msg}"
    );
}

#[test]
fn stream_without_slo_documents_is_an_error() {
    let yaml = r#"apiVersion: openslo/v1
kind: Service
metadata:
  name: lonely
spec:
  description: "no SLOs here"
"#;
    let msg = import_err(yaml);
    assert!(msg.contains("no kind: SLO documents"), "{msg}");
}

#[test]
fn detection_recognizes_openslo_and_rejects_slokit_specs() {
    for yaml in [RATIO, MULTI_OBJECTIVE, LATENCY, UNREPRESENTABLE, STREAM] {
        assert!(openslo::is_openslo(yaml));
    }
    assert!(!openslo::is_openslo(SLOKIT_SAMPLE));
    assert!(!openslo::is_openslo("service: s\nslos: []\n"));
    assert!(!openslo::is_openslo(": not yaml : ["));
}
