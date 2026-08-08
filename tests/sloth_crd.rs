//! sloth **Kubernetes CRD** import tests (v1.6.0 PR 1).
//!
//! Every assertion here fails against the pre-PR loader, which had no notion
//! of the dialect at all: a `kind: PrometheusServiceLevel` document fell
//! through to the native parser and died with
//! `spec error: document 1: missing field 'service'`, a message naming neither
//! the dialect nor the problem.
//! [`the_pre_pr_behavior_the_mapper_changes`] pins exactly that, so the
//! behavior difference this suite proves is stated in the suite rather than
//! only in a PR body.
//!
//! The three CRD fixtures are committed **verbatim** from the sloth reference
//! examples (`slok/sloth@main`, `examples/k8s-getting-started.yml`,
//! `examples/k8s-home-wifi.yml`, `examples/k8s-multifile.yml`), and so are the
//! two native twins (`examples/getting-started.yml`, `examples/home-wifi.yml`).
//! sloth declares the pairings itself: each `k8s-*.yml` opens with "the same
//! example as `<native>.yml` but using Sloth Kubernetes CRD". Do not "tidy"
//! any of the five — the byte-identity assertions below are only worth
//! something because both sides are upstream's own documents.
//!
//! sloth's other two CRD examples are deliberately absent.
//! `slo-plugin-k8s-getting-started.yml` uses `spec.sloPlugins` and
//! `slos[].plugins`, which fail closed (see the error tests, which reproduce
//! its exact shape), and `plugin-k8s-getting-started.yml` is internally
//! inconsistent — a `kind: PrometheusServiceLevel` document that writes the
//! *native* `page_alert:`/`ticket_alert:` where the CRD's own Go tags say
//! `pageAlert`/`ticketAlert`. That one is the reason `spec::sloth_crd` refuses
//! those two spellings instead of ignoring them.

#![cfg(feature = "spec")]

use std::collections::BTreeMap;

use slokit::generate::{generate_rules_with, GenerateOptions};
use slokit::spec::{sloth_crd, validate_all, Spec};
use slokit::Window;

const K8S_GETTING_STARTED: &str = include_str!("fixtures/sloth_crd/k8s-getting-started.yaml");
const GETTING_STARTED_TWIN: &str = include_str!("fixtures/sloth_crd/getting-started-twin.yaml");
const K8S_HOME_WIFI: &str = include_str!("fixtures/sloth_crd/k8s-home-wifi.yaml");
const HOME_WIFI_TWIN: &str = include_str!("fixtures/sloth_crd/home-wifi-twin.yaml");
const K8S_MULTIFILE: &str = include_str!("fixtures/sloth_crd/k8s-multifile.yaml");

/// A minimal valid CRD document the error tests mutate, so every negative test
/// differs from a *working* document by exactly the construct under test.
const BASE: &str = r#"apiVersion: sloth.slok.dev/v1
kind: PrometheusServiceLevel
metadata:
  name: base
spec:
  service: svc
  slos:
    - name: availability
      objective: 99.9
      sli:
        events:
          errorQuery: sum(rate(err_total[{{.window}}]))
          totalQuery: sum(rate(all_total[{{.window}}]))
"#;

fn import_err(yaml: &str) -> String {
    sloth_crd::from_yaml(yaml).unwrap_err().to_string()
}

fn notes(yaml: &str) -> Vec<String> {
    sloth_crd::from_yaml(yaml)
        .unwrap()
        .notes
        .iter()
        .map(ToString::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// The behavior difference
// ---------------------------------------------------------------------------

/// What the loader did before this PR, pinned so the suite states its own
/// premise. `Spec::from_yaml_stream` is still the native path; the point is
/// that a CRD document is NOT a native spec, so before the mapper existed
/// there was nothing else for it to reach.
#[test]
fn the_pre_pr_behavior_the_mapper_changes() {
    let native = Spec::from_yaml_stream(K8S_GETTING_STARTED)
        .unwrap_err()
        .to_string();
    assert!(
        native.contains("missing field") && native.contains("service"),
        "the native parser must still reject a CRD document (that is the gap \
         this dialect closes), got: {native}"
    );
    // ...and the new importer accepts exactly that document.
    assert_eq!(
        sloth_crd::from_yaml(K8S_GETTING_STARTED)
            .unwrap()
            .specs
            .len(),
        1
    );
}

#[test]
fn detection_routes_on_the_api_group_and_never_panics_on_junk() {
    assert!(sloth_crd::is_sloth_crd(K8S_GETTING_STARTED));
    assert!(sloth_crd::is_sloth_crd(K8S_MULTIFILE));
    // A native spec, an OpenSLO document and unparseable input all stay out.
    assert!(!sloth_crd::is_sloth_crd(GETTING_STARTED_TWIN));
    assert!(!sloth_crd::is_sloth_crd(
        "apiVersion: openslo/v1\nkind: SLO\n"
    ));
    assert!(!sloth_crd::is_sloth_crd("\t- [unbalanced"));
    // Leading empty documents are skipped, not treated as "not a CRD".
    assert!(sloth_crd::is_sloth_crd(&format!("---\n{BASE}")));
}

// ---------------------------------------------------------------------------
// The mapping, asserted absolutely (not only as agreement with the twin)
// ---------------------------------------------------------------------------

#[test]
fn the_k8s_getting_started_document_maps_field_by_field() {
    let import = sloth_crd::from_yaml(K8S_GETTING_STARTED).unwrap();
    assert_eq!(import.specs.len(), 1);
    let spec = &import.specs[0];

    assert_eq!(spec.version, "prometheus/v1");
    assert_eq!(spec.service, "myservice");
    // `spec.labels` are the rule labels and DO map; `metadata.labels` do not.
    assert_eq!(
        spec.labels,
        BTreeMap::from([
            ("owner".to_string(), "myteam".to_string()),
            ("repo".to_string(), "myorg/myservice".to_string()),
            ("tier".to_string(), "2".to_string()),
        ])
    );
    assert_eq!(spec.slos.len(), 1);

    let slo = &spec.slos[0];
    assert_eq!(slo.name, "requests-availability");
    assert!((slo.objective - 99.9).abs() < 1e-9, "{}", slo.objective);
    assert_eq!(
        slo.description,
        "Common SLO based on availability for HTTP request responses."
    );
    // The CRD has no per-SLO period field, so the generation default applies.
    assert_eq!(slo.period, None);

    let events = slo.sli.events.as_ref().expect("events SLI");
    assert_eq!(
        events.error_query,
        "sum(rate(http_request_duration_seconds_count{job=\"myservice\",code=~\"(5..|429)\"}[{{.window}}]))"
    );
    assert_eq!(
        events.total_query,
        "sum(rate(http_request_duration_seconds_count{job=\"myservice\"}[{{.window}}]))"
    );

    let alerting = &slo.alerting;
    assert_eq!(alerting.name, "MyServiceHighErrorRate");
    assert_eq!(
        alerting.labels,
        BTreeMap::from([("category".to_string(), "availability".to_string())])
    );
    assert_eq!(
        alerting.annotations,
        BTreeMap::from([(
            "summary".to_string(),
            "High error rate on 'myservice' requests responses".to_string()
        )])
    );
    // The camelCase rename is the whole risk of this dialect: pageAlert and
    // ticketAlert carry the routing labels, and dropping them is silent.
    assert_eq!(
        alerting.page_alert.labels,
        BTreeMap::from([
            ("severity".to_string(), "pageteam".to_string()),
            ("routing_key".to_string(), "myteam".to_string()),
        ])
    );
    assert_eq!(
        alerting.ticket_alert.labels,
        BTreeMap::from([
            ("severity".to_string(), "slack".to_string()),
            ("slack_channel".to_string(), "#alerts-myteam".to_string()),
        ])
    );
    assert!(!alerting.page_alert.disable);
    assert!(alerting.windows.is_empty(), "the CRD has no custom windows");

    validate_all(&import.specs).expect("imported specs validate");
}

#[test]
fn the_k8s_multifile_stream_yields_one_spec_per_document() {
    let import = sloth_crd::from_yaml(K8S_MULTIFILE).unwrap();
    // One PrometheusServiceLevel document is one Kubernetes object is one
    // spec, matching `Spec::from_yaml_stream` rather than OpenSLO's
    // merge-by-service.
    assert_eq!(import.specs.len(), 2);
    assert_eq!(import.specs[0].service, "myservice");
    assert_eq!(import.specs[1].service, "myservice2");
    assert!((import.specs[1].slos[0].objective - 99.99).abs() < 1e-9);
    validate_all(&import.specs).expect("imported specs validate");
}

#[test]
fn the_sli_plugin_shape_imports_because_only_slo_plugin_chains_are_refused() {
    let yaml = BASE.replace(
        "        events:\n          errorQuery: sum(rate(err_total[{{.window}}]))\n          totalQuery: sum(rate(all_total[{{.window}}]))\n",
        "        plugin:\n          id: \"slokit/availability/http-requests-total\"\n          options:\n            job: myservice\n",
    );
    let import = sloth_crd::from_yaml(&yaml).unwrap();
    let plugin = import.specs[0].slos[0]
        .sli
        .plugin
        .as_ref()
        .expect("plugin SLI");
    assert_eq!(plugin.id, "slokit/availability/http-requests-total");
    assert_eq!(
        plugin.options,
        BTreeMap::from([("job".to_string(), "myservice".to_string())])
    );
}

// ---------------------------------------------------------------------------
// Ignored-with-a-note
// ---------------------------------------------------------------------------

#[test]
fn kubernetes_object_metadata_is_reported_rather_than_silently_dropped() {
    // k8s-home-wifi carries all three: name, namespace, and object labels.
    let notes = notes(K8S_HOME_WIFI);
    assert!(
        notes.iter().any(|n| n.contains("metadata.name")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("metadata.namespace")),
        "{notes:?}"
    );
    let labels_note = notes
        .iter()
        .find(|n| n.contains("metadata.labels"))
        .unwrap_or_else(|| panic!("{notes:?}"));
    // The note names the keys, because honoring them would have added exactly
    // these to every generated rule.
    for key in ["prometheus", "role", "app"] {
        assert!(labels_note.contains(key), "{labels_note}");
    }

    // ...and they really are absent from the imported spec.
    let spec = &sloth_crd::from_yaml(K8S_HOME_WIFI).unwrap().specs[0];
    for key in ["prometheus", "role", "app"] {
        assert!(!spec.labels.contains_key(key), "{:?}", spec.labels);
    }
}

#[test]
fn a_document_of_another_kind_is_ignored_with_a_note_but_an_all_ignored_stream_errors() {
    let other =
        "apiVersion: sloth.slok.dev/v1\nkind: SomethingElse\nspec:\n  service: svc\n  slos: []\n";
    let notes = notes(&format!("{BASE}---\n{other}"));
    assert!(
        notes.iter().any(|n| n.contains("SomethingElse")),
        "{notes:?}"
    );
    let msg = import_err(other);
    assert!(
        msg.contains("no kind: PrometheusServiceLevel documents"),
        "{msg}"
    );
}

// ---------------------------------------------------------------------------
// Fail closed
// ---------------------------------------------------------------------------

/// The base document itself must import, or every mutation below proves
/// nothing.
#[test]
fn the_error_test_base_document_imports_cleanly() {
    let import = sloth_crd::from_yaml(BASE).unwrap();
    assert_eq!(import.specs.len(), 1);
    assert_eq!(import.specs[0].slos.len(), 1);
}

#[test]
fn slo_plugin_chains_error_naming_the_field() {
    // Both shapes, taken from sloth's slo-plugin-k8s-getting-started.yml.
    let doc_level = BASE.replace(
        "  slos:\n",
        "  sloPlugins:\n    chain:\n      - id: \"sloth.dev/core/debug/v1\"\n        config: {msg: \"Plugin 99\"}\n  slos:\n",
    );
    let msg = import_err(&doc_level);
    assert!(msg.contains("spec.sloPlugins"), "{msg}");

    let slo_level = BASE.replace(
        "      objective: 99.9\n",
        "      objective: 99.9\n      plugins:\n        chain:\n          - id: \"sloth.dev/core/debug/v1\"\n",
    );
    let msg = import_err(&slo_level);
    assert!(msg.contains("slos[].plugins"), "{msg}");
}

#[test]
fn the_native_snake_case_alert_keys_error_instead_of_being_ignored() {
    // Exactly sloth's own plugin-k8s-getting-started.yml bug: a CRD document
    // spelling the two per-severity keys the native way. Ignoring them (the
    // default for unknown keys) would drop the routing labels in silence.
    for (native, camel) in [("page_alert", "pageAlert"), ("ticket_alert", "ticketAlert")] {
        let yaml = format!(
            "{BASE}      alerting:\n        name: X\n        {native}:\n          labels:\n            severity: pageteam\n"
        );
        let msg = import_err(&yaml);
        assert!(msg.contains(native), "{msg}");
        assert!(msg.contains(camel), "{msg}");
    }

    // The camelCase spelling of the same document imports and keeps the label,
    // which is what makes the rejection above a spelling rule and not a ban.
    let ok = format!(
        "{BASE}      alerting:\n        name: X\n        pageAlert:\n          labels:\n            severity: pageteam\n"
    );
    let import = sloth_crd::from_yaml(&ok).unwrap();
    assert_eq!(
        import.specs[0].slos[0].alerting.page_alert.labels,
        BTreeMap::from([("severity".to_string(), "pageteam".to_string())])
    );
}

#[test]
fn structural_errors_name_the_offending_field() {
    for (mutation, needle) in [
        (
            BASE.replace("sloth.slok.dev/v1", "sloth.slok.dev/v2"),
            "unsupported apiVersion",
        ),
        (
            BASE.replace("kind: PrometheusServiceLevel\n", ""),
            "`kind` is missing",
        ),
        (
            BASE.replace("  service: svc\n", "  service: \"\"\n"),
            "spec.service must not be empty",
        ),
        (
            BASE.replace("    - name: availability\n", "    - name: \"\"\n"),
            "spec.slos[].name must not be empty",
        ),
    ] {
        let msg = import_err(&mutation);
        assert!(msg.contains(needle), "expected {needle:?} in: {msg}");
    }

    // Exactly one SLI shape, both directions.
    let none = BASE.replace(
        "        events:\n          errorQuery: sum(rate(err_total[{{.window}}]))\n          totalQuery: sum(rate(all_total[{{.window}}]))\n",
        "        {}\n",
    );
    assert!(
        import_err(&none).contains("none were set"),
        "{}",
        import_err(&none)
    );

    let two = BASE.replace(
        "      sli:\n",
        "      sli:\n        raw:\n          errorRatioQuery: sum(rate(err_total[{{.window}}]))\n",
    );
    assert!(
        import_err(&two).contains("more than one was set"),
        "{}",
        import_err(&two)
    );
}

// ---------------------------------------------------------------------------
// The twins: byte identity across the option space (L-045)
// ---------------------------------------------------------------------------

/// Every generate configuration a CLI user can reach that changes the rendered
/// rules, labelled by the invocation that reaches it. `generate_rules(spec)`
/// alone is the first entry only; comparing at that single point would prove
/// agreement under defaults and nothing else.
fn option_matrix() -> Vec<(&'static str, GenerateOptions)> {
    let mut seven_day = GenerateOptions::default();
    seven_day.default_period = Window::days(7);

    let mut unscaled = GenerateOptions::default();
    unscaled.period_aware = false;

    let mut seven_day_unscaled = GenerateOptions::default();
    seven_day_unscaled.default_period = Window::days(7);
    seven_day_unscaled.period_aware = false;

    vec![
        ("slokit generate", GenerateOptions::default()),
        ("slokit generate --period 7d", seven_day),
        ("slokit generate --no-period-scaling", unscaled),
        (
            "slokit generate --period 7d --no-period-scaling",
            seven_day_unscaled,
        ),
    ]
}

/// Import the CRD document and parse the native twin.
fn twin_pair(crd: &str, twin: &str) -> (Spec, Spec) {
    let import = sloth_crd::from_yaml(crd).unwrap();
    assert_eq!(import.specs.len(), 1);
    (import.specs[0].clone(), Spec::from_yaml(twin).unwrap())
}

/// Assert the imported CRD and its native twin render identically in every
/// output format under every option combination.
///
/// Deliberately does NOT start from `imported == twin`: spec equality is its
/// own test below, and if it ran here it would short-circuit the matrix and
/// make the option space decorative. A mis-mapping that only *some* generate
/// options expose — pinning `period: 30d` where the twin leaves it unset is
/// the concrete example — is invisible at the default point and caught here.
fn assert_twins_agree(crd: &str, twin: &str) {
    let (imported_spec, twin_spec) = twin_pair(crd, twin);
    let (imported_spec, twin_spec) = (&imported_spec, &twin_spec);

    for (invocation, opts) in option_matrix() {
        let imported = generate_rules_with(imported_spec, &opts).unwrap();
        let native = generate_rules_with(twin_spec, &opts).unwrap();
        assert_eq!(imported, native, "RuleSets differ under `{invocation}`");
        assert_eq!(
            imported.to_prometheus_yaml().unwrap(),
            native.to_prometheus_yaml().unwrap(),
            "rendered prometheus YAML differs under `{invocation}`"
        );
        let labels = BTreeMap::from([("app".to_string(), "slokit".to_string())]);
        assert_eq!(
            imported.to_operator_yaml("slo", &labels).unwrap(),
            native.to_operator_yaml("slo", &labels).unwrap(),
            "rendered operator YAML differs under `{invocation} --format operator`"
        );
    }
}

#[test]
fn the_getting_started_import_generates_the_same_rules_as_its_native_twin() {
    // The roadmap's done-when clause 2: parsing is not mapping. An importer
    // that reads the document but derives the wrong query, objective or alert
    // labels still passes every `validate`, and fails right here.
    assert_twins_agree(K8S_GETTING_STARTED, GETTING_STARTED_TWIN);
}

#[test]
fn the_home_wifi_import_generates_the_same_rules_as_its_native_twin() {
    // Two SLOs, no shared alerting annotations, and object-level metadata
    // labels that must NOT reach the rules.
    assert_twins_agree(K8S_HOME_WIFI, HOME_WIFI_TWIN);
}

#[test]
fn the_import_reconstructs_each_native_twin_exactly() {
    // Stronger than rendering agreement, and the reason this dialect is a
    // rename-and-unwrap layer rather than a second model: the imported spec is
    // the twin, field for field, including the extensions the CRD cannot
    // express staying at their native defaults.
    for (crd, twin) in [
        (K8S_GETTING_STARTED, GETTING_STARTED_TWIN),
        (K8S_HOME_WIFI, HOME_WIFI_TWIN),
    ] {
        let (imported, native) = twin_pair(crd, twin);
        assert_eq!(imported, native);
    }
}
