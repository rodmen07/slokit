//! OpenSLO v1 export tests: the round-trip property over every committed spec
//! in this repo, the fail-closed error set, the advisory notes, byte
//! determinism, and a golden snapshot of the emitted YAML.
//!
//! The round-trip property is the whole point of the milestone, so it is
//! asserted as "export then import yields the source spec with EXACTLY the
//! documented transformations applied, and nothing else changed" rather than as
//! a handful of field spot-checks. A silent drop anywhere in the mapping fails
//! [`assert_round_trip`].

#![cfg(feature = "spec")]

use std::collections::BTreeMap;

use slokit::generate::generate_rules;
use slokit::spec::{
    openslo, Alerting, EventsSli, LatencySli, SliSpec, SloSpec, SourceDialect, Spec,
};

const SLOKIT_SAMPLE: &str = include_str!("fixtures/sample.yaml");

/// Every committed slokit spec in the repo, so the round trip is proven over
/// real specs and not only over hand-built ones. `examples/infraportal/slos/`
/// is the dogfooding set (8 services, 16 SLOs) that `tests/examples_infraportal.rs`
/// already pins against the generator.
const INFRAPORTAL: &[(&str, &str)] = &[
    (
        "accounts-service",
        include_str!("../examples/infraportal/slos/accounts-service.yaml"),
    ),
    (
        "activities-service",
        include_str!("../examples/infraportal/slos/activities-service.yaml"),
    ),
    (
        "automation-service",
        include_str!("../examples/infraportal/slos/automation-service.yaml"),
    ),
    (
        "contacts-service",
        include_str!("../examples/infraportal/slos/contacts-service.yaml"),
    ),
    (
        "integrations-service",
        include_str!("../examples/infraportal/slos/integrations-service.yaml"),
    ),
    (
        "opportunities-service",
        include_str!("../examples/infraportal/slos/opportunities-service.yaml"),
    ),
    (
        "reporting-service",
        include_str!("../examples/infraportal/slos/reporting-service.yaml"),
    ),
    (
        "search-service",
        include_str!("../examples/infraportal/slos/search-service.yaml"),
    ),
];

/// The default `Spec::version`, read from the constructor rather than hardcoded
/// so a change to the dialect tag does not silently pass this suite.
fn default_version() -> String {
    Spec::new("probe", Vec::new()).version
}

/// The documented round-trip transformations, and ONLY those:
///
/// - service-level labels move onto every SLO (SLO labels win on a key clash),
/// - alerting metadata is dropped (OpenSLO models alerting separately),
/// - the slokit dialect tag returns to its default,
/// - **provenance becomes OpenSLO's**, because that is literally true of the
///   returned spec: it was produced by the `openslo/v1` importer reading the
///   document `to_yaml` wrote, so [`Spec::dialect`] is
///   [`SourceDialect::OpenSloV1`] and [`Spec::api_version`] is that document's
///   `apiVersion`. Provenance describes where a spec came from, and a
///   round-tripped spec came from an OpenSLO document.
///
/// Everything else must survive untouched, which is what makes
/// [`assert_round_trip`] a real property rather than a spot check.
///
/// The `apiVersion` is read back off the exported YAML rather than written as
/// a literal here, so this suite cannot agree with a wrong export: if
/// `to_yaml` ever emitted a different version, the expectation would follow it
/// and the assertion below would still be comparing the importer's answer to
/// the document the exporter actually wrote.
fn expected_after_round_trip(spec: &Spec, exported: &str) -> Spec {
    let mut out = spec.clone();
    out.version = default_version();
    out.dialect = SourceDialect::OpenSloV1;
    out.api_version = Some(
        exported
            .lines()
            .find_map(|l| l.strip_prefix("apiVersion: "))
            .expect("the exported OpenSLO document declares an apiVersion")
            .trim()
            .to_string(),
    );
    for slo in &mut out.slos {
        let mut labels = spec.labels.clone();
        labels.extend(slo.labels.clone());
        slo.labels = labels;
        slo.alerting = Alerting::default();
    }
    out.labels = BTreeMap::new();
    out
}

/// Export `spec` to OpenSLO, import it back, and assert the result equals the
/// source with exactly the documented transformations applied.
fn assert_round_trip(label: &str, spec: &Spec) {
    let yaml = openslo::to_yaml(spec).unwrap_or_else(|e| panic!("{label}: export failed: {e}"));
    let import = openslo::from_yaml(&yaml)
        .unwrap_or_else(|e| panic!("{label}: re-import of exported YAML failed: {e}\n{yaml}"));

    assert_eq!(
        import.specs.len(),
        1,
        "{label}: one service in, one spec out"
    );
    let got = &import.specs[0];
    let want = expected_after_round_trip(spec, &yaml);

    assert_eq!(
        got.slos.len(),
        want.slos.len(),
        "{label}: SLO count changed across the round trip"
    );
    for (g, w) in got.slos.iter().zip(want.slos.iter()) {
        assert_eq!(
            g.objective, w.objective,
            "{label}: objective for '{}' did not survive percent/unit-fraction conversion \
             bit-for-bit (the mapping promises only f64 rounding, but every objective in \
             this repo is exact; investigate before loosening this)",
            w.name
        );
    }
    assert_eq!(
        got, &want,
        "{label}: the round trip changed something the mapping does not document"
    );
}

#[test]
fn the_sample_spec_round_trips_through_openslo() {
    let spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    assert_round_trip("fixtures/sample.yaml", &spec);
}

#[test]
fn every_committed_infraportal_spec_round_trips_through_openslo() {
    for (name, yaml) in INFRAPORTAL {
        let spec = Spec::from_yaml(yaml).unwrap();
        assert_round_trip(name, &spec);
    }
}

#[test]
fn every_sli_shape_round_trips() {
    let events = SloSpec::new(
        "availability",
        99.9,
        SliSpec::events(EventsSli::new(
            "sum(rate(errs_total{job=\"api\"}[{{.window}}]))",
            "sum(rate(reqs_total{job=\"api\"}[{{.window}}]))",
        )),
    );
    let raw = SloSpec::new(
        "error-ratio",
        99.0,
        SliSpec::raw(slokit::spec::RawSli::new(
            "sum(rate(errs_total[{{.window}}])) / sum(rate(reqs_total[{{.window}}]))",
        )),
    );
    let mut latency_sli = LatencySli::new("http_request_duration_seconds", "0.3");
    latency_sli.selector = Some("job=\"api\"".to_string());
    let latency = SloSpec::new("latency", 99.5, SliSpec::latency(latency_sli));

    for slo in [events, raw, latency] {
        let name = slo.name.clone();
        let mut spec = Spec::new("api", vec![slo]);
        spec.slos[0].period = Some("30d".to_string());
        spec.slos[0].description = "round trip me".to_string();
        assert_round_trip(&name, &spec);
    }
}

#[test]
fn a_latency_slo_without_a_selector_round_trips() {
    let spec = Spec::new(
        "api",
        vec![SloSpec::new(
            "latency",
            99.5,
            SliSpec::latency(LatencySli::new("http_request_duration_seconds", "1")),
        )],
    );
    assert_round_trip("bare latency", &spec);
}

#[test]
fn service_and_slo_labels_survive_the_documented_relocation() {
    let mut spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    spec.labels.insert("tier".to_string(), "gold".to_string());
    spec.slos[0]
        .labels
        .insert("tier".to_string(), "platinum".to_string());

    let import = openslo::from_yaml(&openslo::to_yaml(&spec).unwrap()).unwrap();
    let round_tripped = &import.specs[0];

    // The service-level bag is gone, but nothing was lost: it now sits on every
    // SLO, with the SLO's own value winning the clash exactly as rule-label
    // generation resolves it.
    assert!(round_tripped.labels.is_empty());
    assert_eq!(round_tripped.slos[0].labels["owner"], "team-platform");
    assert_eq!(round_tripped.slos[0].labels["tier"], "platinum");
    assert_eq!(round_tripped.slos[1].labels["tier"], "gold");

    assert_round_trip("sample + clashing labels", &spec);
}

#[test]
fn exported_yaml_re_imports_into_a_spec_that_validates_and_generates() {
    let spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    let import = openslo::from_yaml(&openslo::to_yaml(&spec).unwrap()).unwrap();
    let round_tripped = &import.specs[0];

    round_tripped
        .validate()
        .expect("an exported-then-imported spec is still a valid slokit spec");
    let rules = generate_rules(round_tripped).expect("and it still generates rules");
    assert!(!rules.to_prometheus_yaml().unwrap().is_empty());
}

/// The alerting drop is the one lossy step that changes generated output, so it
/// is asserted directly rather than left implicit in the round-trip helper: a
/// spec whose alerting is already default generates BYTE-IDENTICAL rules before
/// and after a round trip.
#[test]
fn a_spec_without_alerting_metadata_generates_identical_rules_after_a_round_trip() {
    let mut spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    for slo in &mut spec.slos {
        slo.alerting = Alerting::default();
    }
    let before = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();

    let import = openslo::from_yaml(&openslo::to_yaml(&spec).unwrap()).unwrap();
    let after = generate_rules(&import.specs[0])
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();

    assert_eq!(before, after);
}

#[test]
fn the_notes_report_every_lossy_step_on_a_real_spec() {
    let spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    let report = openslo::to_yaml_reported(&spec).unwrap();
    let messages: Vec<&str> = report.notes.iter().map(|n| n.message.as_str()).collect();

    assert!(
        messages.iter().any(|m| m.contains("service-level label")),
        "{messages:?}"
    );
    assert!(
        messages.iter().filter(|m| m.contains("alerting")).count() == 2,
        "both SLOs in the sample carry alerting metadata: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("template token")),
        "{messages:?}"
    );
    // Nothing about the alerting metadata leaks into the YAML itself.
    assert!(
        !report.yaml.contains("MyServiceHighErrorRate"),
        "{}",
        report.yaml
    );
    assert!(!report.yaml.contains("runbook"), "{}", report.yaml);
}

#[test]
fn a_plugin_sli_is_a_hard_error_naming_the_field() {
    let spec = Spec::new(
        "api",
        vec![SloSpec::new(
            "availability",
            99.9,
            SliSpec::plugin(slokit::spec::PluginSli::new("slokit/http-availability")),
        )],
    );
    let message = openslo::to_yaml(&spec).unwrap_err().to_string();
    assert!(
        message.contains("sli.plugin 'slokit/http-availability'"),
        "{message}"
    );
    assert!(message.contains("not representable"), "{message}");
}

#[test]
fn the_export_is_byte_deterministic() {
    let spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    let runs: Vec<String> = (0..3).map(|_| openslo::to_yaml(&spec).unwrap()).collect();
    assert_eq!(runs[0], runs[1]);
    assert_eq!(runs[1], runs[2]);
}

#[test]
fn openslo_export_snapshot() {
    let spec = Spec::from_yaml(SLOKIT_SAMPLE).unwrap();
    let yaml = openslo::to_yaml(&spec).unwrap();
    insta::assert_snapshot!("openslo_export", yaml);
}
