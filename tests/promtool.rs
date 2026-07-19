//! Validate generated rule files with `promtool check rules`.
//!
//! Each test generates rules, writes them to a temp file, and runs
//! `promtool check rules` on the result. When promtool is not on PATH the
//! tests print a skip message and pass, so local runs never require a
//! Prometheus install. Setting `SLOKIT_REQUIRE_PROMTOOL=1` (as CI does) turns
//! promtool absence into a hard failure instead.

#![cfg(feature = "spec")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use slokit::generate::{generate_all, generate_rules, GenerateOptions};
use slokit::spec::Spec;

const SAMPLE: &str = include_str!("fixtures/sample.yaml");
const OPENSLO_RATIO: &str = include_str!("fixtures/openslo/ratio.yaml");

/// A spec exercising the 0.7.0 alerting extensions: one SLO with custom
/// `alerting.windows` and one non-30d-period SLO whose default windows get
/// period-scaled, so both code paths reach promtool.
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

/// A spec exercising both built-in SLI plugins, so plugin-expanded queries are
/// externally validated by promtool end to end.
const PLUGIN_AVAILABILITY: &str = r#"
service: pluginsvc
labels:
  owner: team-platform
slos:
  - name: http-availability
    objective: 99.9
    description: "availability via the http built-in plugin"
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
          error_code_regex: "5..|429"
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
  - name: grpc-availability
    objective: 99.5
    description: "availability via the grpc built-in plugin"
    sli:
      plugin:
        id: slokit/availability/grpc-server-handled
        options:
          selector: job="rpc"
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

const SPEC_ALPHA: &str = r#"
service: alpha
slos:
  - name: avail
    objective: 99.9
    sli:
      raw:
        error_ratio_query: sum(rate(alpha_errors_total[{{.window}}])) / sum(rate(alpha_requests_total[{{.window}}]))
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

const SPEC_BETA: &str = r#"
service: beta
slos:
  - name: avail
    objective: 99
    sli:
      events:
        error_query: sum(rate(beta_requests_total{code=~"5.."}[{{.window}}]))
        total_query: sum(rate(beta_requests_total[{{.window}}]))
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-promtool-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Whether promtool absence should fail the test instead of skipping.
///
/// Only the exact value `1` opts in; unset or any other value keeps the
/// graceful local skip. CI sets `SLOKIT_REQUIRE_PROMTOOL=1` so a broken
/// promtool install can never silently skip validation there.
fn promtool_required_from(value: Option<&str>) -> bool {
    value == Some("1")
}

fn promtool_required() -> bool {
    promtool_required_from(std::env::var("SLOKIT_REQUIRE_PROMTOOL").ok().as_deref())
}

/// Run `promtool check rules` on `path`. `None` means promtool is not on
/// PATH; any other spawn failure panics.
fn promtool_check(path: &Path) -> Option<Output> {
    match Command::new("promtool")
        .arg("check")
        .arg("rules")
        .arg(path)
        .output()
    {
        Ok(out) => Some(out),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => panic!("failed to spawn promtool: {e}"),
    }
}

/// Write `rules_yaml` to a temp file and validate it with promtool, skipping
/// (or failing under `SLOKIT_REQUIRE_PROMTOOL=1`) when promtool is absent.
fn check_rules_with_promtool(tag: &str, rules_yaml: &str) {
    let dir = temp_dir(tag);
    let path = dir.join(format!("{tag}.rules.yaml"));
    fs::write(&path, rules_yaml).unwrap();

    match promtool_check(&path) {
        Some(out) => {
            assert!(
                out.status.success(),
                "promtool check rules rejected the generated output for '{tag}':\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
        None if promtool_required() => {
            panic!(
                "SLOKIT_REQUIRE_PROMTOOL=1 is set but promtool was not found on PATH; \
                 install Prometheus (promtool) or unset the variable to skip"
            );
        }
        None => {
            eprintln!(
                "skipping promtool validation for '{tag}': promtool not found on PATH \
                 (set SLOKIT_REQUIRE_PROMTOOL=1 to fail instead of skipping)"
            );
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn require_promtool_env_only_accepts_exactly_1() {
    assert!(promtool_required_from(Some("1")));
    assert!(!promtool_required_from(Some("0")));
    assert!(!promtool_required_from(Some("true")));
    assert!(!promtool_required_from(Some("")));
    assert!(!promtool_required_from(None));
}

#[test]
fn sample_fixture_rules_pass_promtool() {
    let spec = Spec::from_yaml(SAMPLE).expect("fixture parses");
    let yaml = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();
    check_rules_with_promtool("sample", &yaml);
}

#[test]
fn multi_spec_directory_rules_pass_promtool() {
    let dir = temp_dir("specs");
    fs::write(dir.join("alpha.yaml"), SPEC_ALPHA).unwrap();
    fs::write(dir.join("beta.yaml"), SPEC_BETA).unwrap();

    let specs = Spec::from_dir(&dir).unwrap();
    assert_eq!(specs.len(), 2);
    let yaml = generate_all(&specs, &GenerateOptions::default())
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);

    check_rules_with_promtool("multi", &yaml);
}

#[test]
fn plugin_generated_rules_pass_promtool() {
    let spec = Spec::from_yaml(PLUGIN_AVAILABILITY).expect("plugin spec parses");
    let yaml = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();
    // Sanity: both plugin expansions are actually in the output promtool sees.
    assert!(
        yaml.contains("http_requests_total{job=\"api\", code=~\"5..|429\"}"),
        "http plugin expansion missing"
    );
    assert!(
        yaml.contains("grpc_server_handled_total{job=\"rpc\", grpc_code!~\"OK\"}"),
        "grpc plugin expansion missing"
    );
    check_rules_with_promtool("plugin", &yaml);
}

#[test]
fn openslo_imported_rules_pass_promtool() {
    let import = slokit::spec::openslo::from_yaml(OPENSLO_RATIO).expect("openslo fixture imports");
    assert_eq!(import.specs.len(), 1);
    let yaml = generate_rules(&import.specs[0])
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    // Sanity: the rewritten window token reached the rendered rules.
    assert!(
        yaml.contains("sum(rate(http_requests_total{job=\"myservice\",code=~\"5..\"}[5m]))"),
        "imported query missing from the output promtool sees"
    );
    check_rules_with_promtool("openslo", &yaml);
}

#[test]
fn custom_and_scaled_alerting_rules_pass_promtool() {
    let spec = Spec::from_yaml(CUSTOM_ALERTING).expect("custom-alerting spec parses");
    let yaml = generate_rules(&spec).unwrap().to_prometheus_yaml().unwrap();
    // Sanity: both extension paths are actually in the output promtool sees.
    assert!(
        yaml.contains("slo:sli_error:ratio_rate10m"),
        "custom window missing"
    );
    assert!(
        yaml.contains("slo:sli_error:ratio_rate90d"),
        "scaled period missing"
    );
    check_rules_with_promtool("custom-alerting", &yaml);
}
