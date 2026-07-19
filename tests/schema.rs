//! Spec JSON Schema tests: the in-repo schema accepts every native fixture
//! spec (including the slokit-native twins of the OpenSLO goldens and the
//! plugin specs), rejects structurally broken specs that slokit itself also
//! rejects, and stays in sync with both the embedded [`SCHEMA_JSON`] constant
//! and the `slokit schema` subcommand.
//!
//! The `jsonschema` crate is a dev-dependency only: the schema ships as a
//! plain JSON file (embedded as a string), so the runtime dependency set and
//! the lean core are unchanged.

#![cfg(feature = "spec")]

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use slokit::spec::{Spec, SCHEMA_JSON};

/// The schema file, read at compile time straight from the repository.
const SCHEMA_FILE: &str = include_str!("../schema/slokit-spec.schema.json");

/// The smallest valid spec: one events SLO, nothing optional.
const MINIMAL: &str = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(err[{{.window}}]))
        total_query: sum(rate(tot[{{.window}}]))
"#;

/// The spaced window-token form `{{ .window }}`: the second of exactly two
/// token spellings generation substitutes and `slokit validate` accepts.
const SPACED_WINDOW_TOKEN: &str = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: sum(rate(err[{{ .window }}])) / sum(rate(tot[{{ .window }}]))
"#;

/// The slokit-native twin of `tests/fixtures/openslo/ratio.yaml`, kept in
/// lockstep with the byte-identical-rules equivalence test in
/// `tests/openslo.rs`.
const OPENSLO_RATIO_TWIN: &str = r#"
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
"#;

/// The worked plugin example from `tests/plugin.rs` (and the design doc).
const PLUGIN_SPEC: &str = r#"
version: "prometheus/v1"
service: myservice
labels:
  owner: team-platform
slos:
  - name: requests-availability
    objective: 99.9
    description: "99.9% of requests succeed"
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
    alerting:
      page_alert:
        labels: { severity: page }
      ticket_alert:
        labels: { severity: ticket }
"#;

/// The hand-written `events` twin of [`PLUGIN_SPEC`] from `tests/plugin.rs`.
const PLUGIN_TWIN_SPEC: &str = r#"
version: "prometheus/v1"
service: myservice
labels:
  owner: team-platform
slos:
  - name: requests-availability
    objective: 99.9
    description: "99.9% of requests succeed"
    sli:
      events:
        error_query: sum(rate(http_requests_total{job="api", code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total{job="api"}[{{.window}}]))
    alerting:
      page_alert:
        labels: { severity: page }
      ticket_alert:
        labels: { severity: ticket }
"#;

/// The 0.7.0 alerting-extension spec promtool validates in
/// `tests/promtool.rs`: custom `alerting.windows` plus a period-scaled 90d
/// SLO.
const CUSTOM_ALERTING: &str = r#"
service: promsvc
labels:
  owner: team-platform
slos:
  - name: custom-windows
    objective: 99.9
    description: "custom burn-rate window table"
    sli:
      raw:
        error_ratio_query: sum(rate(app_errors_total[{{.window}}])) / sum(rate(app_requests_total[{{.window}}]))
    alerting:
      name: PromsvcCustomBurnRate
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
      windows:
        - severity: page
          long: 30m
          short: 10m
          factor: 10
        - severity: ticket
          long: 12h
          short: 1h
          factor: 2
  - name: scaled-windows
    objective: 99.5
    period: 90d
    description: "90d period scales the default MWMBR table"
    sli:
      raw:
        error_ratio_query: sum(rate(app_errors_total[{{.window}}])) / sum(rate(app_requests_total[{{.window}}]))
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

/// Every schema surface at once: spec/SLO/alerting label maps, annotations, a
/// latency SLI with a selector, a compound-duration custom window, a disabled
/// severity, and a per-SLO period.
const KITCHEN_SINK: &str = r#"
version: "prometheus/v1"
service: kitchen-sink
labels:
  owner: team-platform
  tier: "1"
slos:
  - name: latency-with-everything
    objective: 99.95
    description: "exercises every field the schema knows"
    period: 45d
    labels:
      journey: checkout
    sli:
      latency:
        histogram_metric: http_request_duration_seconds
        threshold: "0.3"
        selector: job="api", code!~"5.."
    alerting:
      name: KitchenSinkLatency
      labels:
        team: platform
      annotations:
        runbook: https://runbooks.example.com/latency
      page_alert:
        labels:
          severity: page
        annotations:
          summary: "latency budget burning fast"
      ticket_alert:
        disable: true
      windows:
        - severity: page
          long: 1h30m
          short: 10m
          factor: 12.5
        - severity: ticket
          long: 12h
          short: 1h
          factor: 2
  - name: raw-ratio
    objective: 99.0
    sli:
      raw:
        error_ratio_query: sum(rate(app_errors_total[{{.window}}])) / sum(rate(app_requests_total[{{.window}}]))
"#;

/// The embedder spec from `tests/plugin.rs`: schema-valid, but its plugin id
/// only exists in an embedder registry (see
/// [`schema_does_not_encode_plugin_registry_membership`]).
const EMBEDDER_SPEC: &str = r#"
service: acmesvc
slos:
  - name: static-ratio
    objective: 99.9
    description: "embedder plugin SLI"
    sli:
      plugin:
        id: acme/static-ratio
        options:
          metric: app:error_ratio
    alerting:
      page_alert:
        labels: { severity: page }
      ticket_alert:
        labels: { severity: ticket }
"#;

/// Structurally broken specs. Each must be rejected by the schema and by
/// slokit itself (parse or validate), pinning that on these shapes the schema
/// only rejects what the tool rejects.
const NEGATIVE_SPECS: &[(&str, &str)] = &[
    (
        "bad_window_severity",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      windows:
        - severity: critical
          long: 30m
          short: 5m
          factor: 10
"#,
    ),
    (
        "events_and_plugin_both_set",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      events:
        error_query: e[{{.window}}]
        total_query: t[{{.window}}]
      plugin:
        id: slokit/availability/http-requests-total
"#,
    ),
    (
        "malformed_period_duration",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    period: monthly
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "malformed_window_duration",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      windows:
        - severity: page
          long: nonsense
          short: 5m
          factor: 10
"#,
    ),
    (
        "window_factor_zero",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      windows:
        - severity: page
          long: 30m
          short: 5m
          factor: 0
"#,
    ),
    (
        "objective_zero",
        r#"
service: s
slos:
  - name: a
    objective: 0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "objective_over_100",
        r#"
service: s
slos:
  - name: a
    objective: 150
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "objective_as_string",
        r#"
service: s
slos:
  - name: a
    objective: "99.9"
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "missing_service",
        r#"
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "no_slos",
        r#"
service: s
slos: []
"#,
    ),
    (
        "missing_sli",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
"#,
    ),
    (
        "empty_sli",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli: {}
"#,
    ),
    (
        "empty_label_name",
        r#"
service: s
labels:
  "": spec-level
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#,
    ),
    (
        "query_without_window_token",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: sum(rate(errs[5m]))
"#,
    ),
    (
        // Only `{{.window}}` and `{{ .window }}` are substituted; an
        // asymmetrically spaced token is left in the query verbatim, so both
        // the schema pattern and `slokit validate` reject it.
        "asymmetric_window_token",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: sum(rate(errs[{{ .window}}]))
"#,
    ),
    (
        "non_scalar_plugin_option",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          bad: [1, 2]
"#,
    ),
    (
        "whitespace_only_alerting_name",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      name: "   "
"#,
    ),
    (
        "bad_histogram_metric_name",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: "http duration seconds"
        threshold: "0.3"
"#,
    ),
    (
        "padded_latency_threshold",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: m
        threshold: " 0.3 "
"#,
    ),
    (
        "latency_selector_with_braces",
        r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: m
        threshold: "0.3"
        selector: '{job="x"}'
"#,
    ),
];

fn schema_value() -> Value {
    serde_json::from_str(SCHEMA_FILE).expect("schema/slokit-spec.schema.json is valid JSON")
}

fn compiled_schema() -> jsonschema::Validator {
    jsonschema::validator_for(&schema_value()).expect("the schema compiles")
}

fn yaml_to_json(yaml: &str) -> Value {
    serde_norway::from_str(yaml).expect("spec parses as YAML")
}

fn schema_errors(validator: &jsonschema::Validator, instance: &Value) -> Vec<String> {
    validator
        .iter_errors(instance)
        .map(|e| e.to_string())
        .collect()
}

/// Every native fixture file on disk. The `openslo/` subdirectory is
/// excluded: those documents are OpenSLO input for the importer, not slokit
/// specs, and a separate test pins that this schema rejects them.
fn native_fixture_files() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("fixtures dir is readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no native fixtures found in {}",
        dir.display()
    );
    files
}

/// Every valid spec sample: the on-disk fixtures plus the inline twins.
fn positive_specs() -> Vec<(String, String)> {
    let mut cases: Vec<(String, String)> = native_fixture_files()
        .into_iter()
        .map(|path| {
            let yaml = fs::read_to_string(&path).expect("fixture is readable");
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, yaml)
        })
        .collect();
    for (name, yaml) in [
        ("minimal", MINIMAL),
        ("spaced_window_token", SPACED_WINDOW_TOKEN),
        ("openslo_ratio_twin", OPENSLO_RATIO_TWIN),
        ("plugin_spec", PLUGIN_SPEC),
        ("plugin_twin_spec", PLUGIN_TWIN_SPEC),
        ("custom_alerting", CUSTOM_ALERTING),
        ("kitchen_sink", KITCHEN_SINK),
    ] {
        cases.push((name.to_string(), yaml.to_string()));
    }
    cases
}

#[test]
fn schema_file_is_draft_2020_12_and_compiles() {
    let schema = schema_value();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert!(
        schema["$id"]
            .as_str()
            .expect("$id is a string")
            .ends_with("schema/slokit-spec.schema.json"),
        "$id should point at the in-repo schema path"
    );
    let _ = compiled_schema();
}

#[test]
fn embedded_schema_matches_the_repo_file() {
    // `slokit::spec::SCHEMA_JSON` is the single string the CLI prints; it
    // must stay byte-identical to the file this suite validates against.
    let on_disk = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/slokit-spec.schema.json"),
    )
    .expect("schema file is readable");
    assert_eq!(SCHEMA_JSON, on_disk);
    assert_eq!(SCHEMA_JSON, SCHEMA_FILE);
    let parsed: Value = serde_json::from_str(SCHEMA_JSON).expect("embedded schema parses as JSON");
    assert!(parsed.is_object());
}

#[test]
fn every_native_spec_validates_against_the_schema() {
    let validator = compiled_schema();
    for (name, yaml) in positive_specs() {
        let errors = schema_errors(&validator, &yaml_to_json(&yaml));
        assert!(
            errors.is_empty(),
            "{name} should validate against the schema, got: {errors:#?}"
        );
    }
}

#[test]
fn schema_positives_also_pass_the_rust_validator() {
    // The samples above are not just schema-valid: everything the schema test
    // accepts also parses and passes `slokit validate`, so the two layers
    // agree on what a sound spec looks like.
    for (name, yaml) in positive_specs() {
        let spec = Spec::from_yaml(&yaml).unwrap_or_else(|e| panic!("{name} should parse: {e}"));
        spec.validate()
            .unwrap_or_else(|e| panic!("{name} should pass validate: {e}"));
    }
}

#[test]
fn schema_does_not_encode_plugin_registry_membership() {
    // The embedder spec is structurally sound, so the schema accepts it, but
    // its plugin id only exists in an embedder registry, so `slokit validate`
    // (built-in registry) rejects it. Registry resolution is validator-owned
    // semantics, deliberately not encoded in the schema.
    let validator = compiled_schema();
    let errors = schema_errors(&validator, &yaml_to_json(EMBEDDER_SPEC));
    assert!(
        errors.is_empty(),
        "embedder spec should be schema-valid: {errors:#?}"
    );
    let msg = Spec::from_yaml(EMBEDDER_SPEC)
        .unwrap()
        .validate()
        .unwrap_err()
        .to_string();
    assert!(msg.contains("unknown SLI plugin"), "{msg}");
}

#[test]
fn broken_specs_fail_the_schema_and_the_tool() {
    let validator = compiled_schema();
    for (name, yaml) in NEGATIVE_SPECS {
        let instance = yaml_to_json(yaml);
        assert!(
            !validator.is_valid(&instance),
            "{name} should fail schema validation"
        );
        let tool_rejects = match Spec::from_yaml(yaml) {
            Err(_) => true,
            Ok(spec) => spec.validate().is_err(),
        };
        assert!(
            tool_rejects,
            "{name} should also be rejected by slokit parse/validate"
        );
    }
}

#[test]
fn openslo_documents_are_not_native_specs() {
    // OpenSLO fixtures are importer input (`slokit --input-format openslo`);
    // this schema describes only the native format, so editors wired to it
    // must flag an OpenSLO document rather than accept it.
    let validator = compiled_schema();
    for fixture in ["ratio.yaml", "latency.yaml", "multi-objective.yaml"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/openslo")
            .join(fixture);
        let yaml = fs::read_to_string(&path).expect("openslo fixture is readable");
        assert!(
            !validator.is_valid(&yaml_to_json(&yaml)),
            "openslo/{fixture} must not validate against the native spec schema"
        );
    }
}

#[cfg(feature = "cli")]
mod cli {
    use super::SCHEMA_FILE;

    #[test]
    fn schema_subcommand_prints_the_schema_file_verbatim() {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_slokit"))
            .arg("schema")
            .output()
            .expect("slokit schema runs");
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            SCHEMA_FILE,
            "`slokit schema` output must equal schema/slokit-spec.schema.json byte for byte"
        );
    }
}
