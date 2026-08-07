//! OpenSLO **v1alpha** import tests (v1.5.0 PR 1).
//!
//! Every assertion here fails against the pre-PR importer, which rejected the
//! whole schema with `unsupported apiVersion 'openslo/v1alpha'`. That is the
//! behavior difference this suite exists to prove.
//!
//! The two fixtures are committed **verbatim** from the sloth reference
//! examples (`slok/sloth@main`, `examples/openslo-getting-started.yml` and
//! `examples/openslo-kubernetes-apiserver.yml`), because the point of the
//! milestone is reading the corpus the ecosystem actually publishes rather
//! than a synthetic document written to fit the mapper. Do not "tidy" them.
//!
//! Synthetic documents appear only where the fixtures cannot reach a path:
//! the fail-closed error set and the threshold SLI (neither sloth example uses
//! `spec.indicator.thresholdMetric`).

#![cfg(feature = "spec")]

use slokit::generate::generate_rules;
use slokit::spec::{openslo, validate_all, Spec};

const GETTING_STARTED: &str = include_str!("fixtures/openslo/v1alpha/getting-started.yaml");
const GETTING_STARTED_TWIN: &str =
    include_str!("fixtures/openslo/v1alpha/getting-started-twin.yaml");
const K8S_APISERVER: &str = include_str!("fixtures/openslo/v1alpha/kubernetes-apiserver.yaml");

/// A minimal valid v1alpha document that the error tests mutate. Keeping one
/// base and mutating it means every negative test differs from a *working*
/// document by exactly the construct under test.
const BASE: &str = r#"apiVersion: openslo/v1alpha
kind: SLO
metadata:
  name: base
spec:
  service: svc
  description: "base"
  budgetingMethod: Occurrences
  objectives:
    - ratioMetrics:
        good:
          source: prometheus
          queryType: promql
          query: sum(rate(ok_total[5m]))
        total:
          source: prometheus
          queryType: promql
          query: sum(rate(all_total[5m]))
      target: 0.999
  timeWindows:
    - count: 30
      unit: Day
"#;

fn import_err(yaml: &str) -> String {
    openslo::from_yaml(yaml).unwrap_err().to_string()
}

/// The base document itself must import, or the mutations below prove nothing.
#[test]
fn the_error_test_base_document_imports_cleanly() {
    let import = openslo::from_yaml(BASE).unwrap();
    assert_eq!(import.specs.len(), 1);
    assert_eq!(import.specs[0].slos.len(), 1);
}

#[test]
fn the_sloth_getting_started_document_maps_to_an_events_spec() {
    let import = openslo::from_yaml(GETTING_STARTED).unwrap();
    assert_eq!(import.specs.len(), 1);
    let spec = &import.specs[0];
    assert_eq!(spec.service, "my-service");
    assert_eq!(spec.version, "prometheus/v1");
    assert_eq!(spec.slos.len(), 1);

    let slo = &spec.slos[0];
    assert_eq!(slo.name, "sloth-slo-my-service");
    assert!((slo.objective - 99.9).abs() < 1e-9, "{}", slo.objective);
    assert_eq!(
        slo.description,
        "Common SLO based on availability for HTTP request responses."
    );
    // `count: 30` + `unit: Day` is the v1alpha spelling of v1's `duration: 30d`.
    assert_eq!(slo.period.as_deref(), Some("30d"));
    assert!(slo.labels.is_empty(), "v1alpha has no metadata.labels");

    let events = slo.sli.events.as_ref().expect("events SLI");
    assert_eq!(
        events.total_query,
        "sum(rate(http_request_duration_seconds_count{job=\"myservice\"}[{{.window}}]))"
    );
    // OpenSLO counts good events; slokit models the error side.
    assert_eq!(
        events.error_query,
        "(sum(rate(http_request_duration_seconds_count{job=\"myservice\"}[{{.window}}]))) \
         - (sum(rate(http_request_duration_seconds_count{job=\"myservice\",code!~\"(5..|429)\"}[{{.window}}])))"
    );

    // metadata.displayName is dropped, and says so rather than vanishing.
    let notes: Vec<String> = import.notes.iter().map(ToString::to_string).collect();
    assert!(
        notes.iter().any(|n| n.contains("metadata.displayName")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("total minus good")),
        "{notes:?}"
    );
}

#[test]
fn the_getting_started_import_generates_the_same_rules_as_its_committed_twin() {
    // The roadmap's done-when clause 2: parsing is not mapping. A mapper that
    // reads the document but derives the wrong query, period or objective
    // still passes `validate`, and fails right here.
    let import = openslo::from_yaml(GETTING_STARTED).unwrap();
    let twin = Spec::from_yaml(GETTING_STARTED_TWIN).unwrap();

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
fn the_sloth_apiserver_stream_merges_two_documents_and_splits_objectives() {
    let import = openslo::from_yaml(K8S_APISERVER).unwrap();
    // Two SLO documents, one shared spec.service, so one spec.
    assert_eq!(import.specs.len(), 1);
    let spec = &import.specs[0];
    assert_eq!(spec.service, "k8s-apiserver");

    // The availability document has one objective (no suffix); the latency
    // document has two, so it becomes two SLOs with 1-based index suffixes
    // (neither objective sets displayName).
    let names: Vec<&str> = spec.slos.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "requests-availability-openslo",
            "requests-latency-openslo-1",
            "requests-latency-openslo-2",
        ]
    );

    let objectives: Vec<f64> = spec.slos.iter().map(|s| s.objective).collect();
    assert!((objectives[0] - 99.9).abs() < 1e-9, "{objectives:?}");
    assert!((objectives[1] - 99.0).abs() < 1e-9, "{objectives:?}");
    assert!((objectives[2] - 99.9).abs() < 1e-9, "{objectives:?}");

    // The two latency objectives share a total and differ only in `le`, which
    // is exactly the per-objective-metric shape v1 cannot express.
    let first = spec.slos[1].sli.events.as_ref().expect("events SLI");
    let second = spec.slos[2].sli.events.as_ref().expect("events SLI");
    assert_eq!(first.total_query, second.total_query);
    assert!(first.error_query.contains("le=\"0.4\""), "{first:?}");
    assert!(second.error_query.contains("le=\"5\""), "{second:?}");

    for slo in &spec.slos {
        assert_eq!(slo.period.as_deref(), Some("30d"));
    }
}

#[test]
fn both_sloth_fixtures_pass_validate_and_are_detected_as_openslo() {
    for fixture in [GETTING_STARTED, K8S_APISERVER] {
        assert!(openslo::is_openslo(fixture));
        let import = openslo::from_yaml(fixture).unwrap();
        validate_all(&import.specs).expect("imported v1alpha specs validate");
    }
}

#[test]
fn a_v1alpha_threshold_indicator_becomes_a_latency_sli() {
    // Neither sloth fixture uses spec.indicator, so this path needs its own
    // document. It also pins the v1alpha spelling: the metric source is flat
    // (`source`/`queryType`/`query`), not v1's nested `metricSource`.
    let yaml = r#"apiVersion: openslo/v1alpha
kind: SLO
metadata:
  name: latency
spec:
  service: svc
  budgetingMethod: Occurrences
  indicator:
    thresholdMetric:
      source: prometheus
      queryType: promql
      query: http_request_duration_seconds{job="api"}
  objectives:
    - target: 0.99
      op: lte
      value: 0.3
  timeWindows:
    - count: 4
      unit: Week
"#;
    let import = openslo::from_yaml(yaml).unwrap();
    let slo = &import.specs[0].slos[0];
    assert_eq!(slo.period.as_deref(), Some("4w"));
    let latency = slo.sli.latency.as_ref().expect("latency SLI");
    assert_eq!(latency.histogram_metric, "http_request_duration_seconds");
    assert_eq!(latency.threshold, "0.3");
    assert_eq!(latency.selector.as_deref(), Some("job=\"api\""));
}

#[test]
fn unrepresentable_v1alpha_constructs_error_naming_the_field() {
    // Every case is BASE with one construct changed, so the message under test
    // is the only difference from a document that imports.
    let cases: Vec<(String, &str)> = vec![
        (
            BASE.replace("unit: Day", "unit: Month"),
            "spec.timeWindows[0].unit 'Month'",
        ),
        (
            BASE.replace("unit: Day", "unit: Decade"),
            "spec.timeWindows[0].unit 'Decade'",
        ),
        (
            BASE.replace("count: 30", "count: 0"),
            "spec.timeWindows[0].count must be greater than zero",
        ),
        (
            BASE.replace("      unit: Day\n", "      unit: Day\n      isRolling: false\n"),
            "spec.timeWindows[0].isRolling",
        ),
        (
            BASE.replace(
                "      unit: Day\n",
                "      unit: Day\n      calendar:\n        startTime: \"2020-01-21 12:30:00\"\n        timeZone: America/New_York\n",
            ),
            "spec.timeWindows[0].calendar",
        ),
        (
            BASE.replace("budgetingMethod: Occurrences", "budgetingMethod: Timeslices"),
            "spec.budgetingMethod 'Timeslices'",
        ),
        (
            BASE.replacen("source: prometheus", "source: datadog", 1),
            "'datadog' is not supported",
        ),
        (
            BASE.replacen("queryType: promql", "queryType: flux", 1),
            "queryType 'flux'",
        ),
        (
            BASE.replace("      target: 0.999", "      target: 99.9"),
            "must be a unit fraction",
        ),
        (
            BASE.replace("      target: 0.999\n", ""),
            "target is required",
        ),
        (
            BASE.replace("  service: svc\n", ""),
            "spec.service must not be empty",
        ),
        (
            BASE.replace("  name: base", "  name: \"\""),
            "metadata.name must not be empty",
        ),
        (
            // No metric at all: neither ratioMetrics nor an indicator.
            BASE.replace("    - ratioMetrics:", "    - notRatioMetrics:"),
            "needs ratioMetrics",
        ),
        (
            BASE.replace("apiVersion: openslo/v1alpha", "apiVersion: openslo/v2"),
            "unsupported apiVersion 'openslo/v2'",
        ),
    ];

    for (yaml, expected) in cases {
        let msg = import_err(&yaml);
        assert!(msg.contains(expected), "expected {expected:?} in {msg}");
    }
}

#[test]
fn ratio_metrics_and_a_document_indicator_together_are_an_error() {
    let yaml = BASE.replace(
        "  objectives:",
        "  indicator:\n    thresholdMetric:\n      source: prometheus\n      queryType: promql\n      query: http_seconds\n  objectives:",
    );
    let msg = import_err(&yaml);
    assert!(
        msg.contains("alongside a document-level spec.indicator.thresholdMetric"),
        "{msg}"
    );
}

#[test]
fn two_time_windows_are_an_error() {
    let yaml = BASE.replace(
        "    - count: 30\n      unit: Day\n",
        "    - count: 30\n      unit: Day\n    - count: 7\n      unit: Day\n",
    );
    let msg = import_err(&yaml);
    assert!(msg.contains("spec.timeWindows has 2 entries"), "{msg}");
}

#[test]
fn a_windowless_ratio_query_is_still_an_error_in_v1alpha() {
    // The shared window convention applies to both versions: a query with
    // neither the token nor a rewritable range selector cannot be evaluated
    // per burn-rate window.
    let yaml = BASE.replace("sum(rate(all_total[5m]))", "sum(all_total_ratio)");
    let msg = import_err(&yaml);
    assert!(msg.contains("no fixed range selector"), "{msg}");
}

#[test]
fn fixed_ranges_in_v1alpha_queries_are_rewritten_to_the_window_token() {
    // BASE uses literal `[5m]` lookbacks (the sloth fixtures already carry the
    // token, so this path needs its own document).
    let import = openslo::from_yaml(BASE).unwrap();
    let events = import.specs[0].slos[0]
        .sli
        .events
        .as_ref()
        .expect("events SLI");
    assert_eq!(events.total_query, "sum(rate(all_total[{{.window}}]))");
    assert_eq!(
        events.error_query,
        "(sum(rate(all_total[{{.window}}]))) - (sum(rate(ok_total[{{.window}}])))"
    );
    let notes: Vec<String> = import.notes.iter().map(ToString::to_string).collect();
    assert!(notes.iter().any(|n| n.contains("[5m]")), "{notes:?}");
}

#[test]
fn a_v1alpha_kind_that_is_not_slo_is_noted_not_imported() {
    let yaml = format!(
        "{BASE}---\napiVersion: openslo/v1alpha\nkind: Service\nmetadata:\n  name: svc\nspec:\n  description: \"a service\"\n"
    );
    let import = openslo::from_yaml(&yaml).unwrap();
    assert_eq!(import.specs.len(), 1);
    let notes: Vec<String> = import.notes.iter().map(ToString::to_string).collect();
    assert!(
        notes.iter().any(|n| n.contains("kind 'Service'")),
        "{notes:?}"
    );
}

#[test]
fn labels_on_a_v1alpha_document_are_reported_rather_than_silently_honored() {
    // metadata.labels is v1-only. Honoring it here would invent schema; the
    // note is what stops the drop from being silent.
    let yaml = BASE.replace(
        "  name: base\n",
        "  name: base\n  labels:\n    owner: team-platform\n",
    );
    let import = openslo::from_yaml(&yaml).unwrap();
    assert!(import.specs[0].slos[0].labels.is_empty());
    let notes: Vec<String> = import.notes.iter().map(ToString::to_string).collect();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("metadata.labels are not part of openslo/v1alpha")),
        "{notes:?}"
    );
}

#[test]
fn a_stream_may_mix_v1_and_v1alpha_documents() {
    // apiVersion is dispatched per document, not per input, so a directory
    // concatenation or an `export` round trip cannot strand one half.
    let v1 = r#"apiVersion: openslo/v1
kind: SLO
metadata:
  name: v1-slo
spec:
  service: mixed
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
              query: sum(rate(errs_total[{{.window}}]))
        total:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(reqs_total[{{.window}}]))
  objectives:
    - target: 0.99
"#;
    let yaml = format!(
        "{}---\n{v1}",
        BASE.replace("service: svc", "service: mixed")
    );
    let import = openslo::from_yaml(&yaml).unwrap();
    assert_eq!(import.specs.len(), 1, "both documents share spec.service");
    let names: Vec<&str> = import.specs[0]
        .slos
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["base", "v1-slo"]);
}
