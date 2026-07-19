//! End-to-end SLI plugin tests: the golden snapshot for the worked example,
//! byte-identity between a plugin spec and its hand-written twin, and the
//! embedder path with a custom registry.

#![cfg(feature = "spec")]

use std::collections::BTreeMap;
use std::sync::Arc;

use slokit::generate::{generate_rules, generate_rules_with, GenerateOptions};
use slokit::spec::plugin::{OptionKind, OptionSpec, SliPlugin, SliPluginRegistry};
use slokit::spec::Spec;
use slokit::{Result, Sli};

/// The worked example from the design doc: availability via the built-in
/// `slokit/availability/http-requests-total` plugin.
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

/// The hand-written `events` twin of [`PLUGIN_SPEC`]: identical except the SLI
/// queries are written out by hand, exactly as the plugin expands them.
const TWIN_SPEC: &str = r#"
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

/// Replace the crate version so snapshots survive version bumps.
fn redact_version(yaml: String) -> String {
    yaml.replace(env!("CARGO_PKG_VERSION"), "[VERSION]")
}

#[test]
fn plugin_rules_snapshot() {
    let spec = Spec::from_yaml(PLUGIN_SPEC).expect("plugin spec parses");
    let rules = generate_rules(&spec).unwrap();
    let yaml = redact_version(rules.to_prometheus_yaml().unwrap());
    insta::assert_snapshot!("plugin_rules", yaml);
}

#[test]
fn plugin_spec_matches_its_hand_written_twin_byte_for_byte() {
    let plugin_spec = Spec::from_yaml(PLUGIN_SPEC).unwrap();
    let twin_spec = Spec::from_yaml(TWIN_SPEC).unwrap();
    let plugin_rules = generate_rules(&plugin_spec).unwrap();
    let twin_rules = generate_rules(&twin_spec).unwrap();
    assert_eq!(plugin_rules, twin_rules, "RuleSets must be identical");
    assert_eq!(
        plugin_rules.to_prometheus_yaml().unwrap(),
        twin_rules.to_prometheus_yaml().unwrap(),
        "rendered YAML must be byte-identical"
    );
}

/// A minimal embedder plugin: a pre-recorded error-ratio metric.
struct StaticRatio;

const STATIC_RATIO_OPTIONS: &[OptionSpec] = &[OptionSpec::new(
    "metric",
    OptionKind::String,
    "name of the recorded error-ratio metric",
)
.required()];

impl SliPlugin for StaticRatio {
    fn id(&self) -> &str {
        "acme/static-ratio"
    }
    fn description(&self) -> &str {
        "error ratio from a pre-recorded ratio metric"
    }
    fn options(&self) -> &[OptionSpec] {
        STATIC_RATIO_OPTIONS
    }
    fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli> {
        Ok(Sli::Raw {
            error_ratio_query: format!("avg_over_time({}[{{{{.window}}}}])", options["metric"]),
        })
    }
}

/// A deliberately broken embedder plugin: its query has no window token, so
/// post-expansion validation must reject it like a hand-written spec.
struct NoWindow;

impl SliPlugin for NoWindow {
    fn id(&self) -> &str {
        "acme/no-window"
    }
    fn description(&self) -> &str {
        "buggy plugin that forgets the window token"
    }
    fn options(&self) -> &[OptionSpec] {
        &[]
    }
    fn expand(&self, _: &BTreeMap<String, String>) -> Result<Sli> {
        Ok(Sli::Raw {
            error_ratio_query: "app:error_ratio[5m]".to_string(),
        })
    }
}

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

fn embedder_registry() -> SliPluginRegistry {
    let mut registry = SliPluginRegistry::empty();
    registry.register(Box::new(StaticRatio)).unwrap();
    registry.register(Box::new(NoWindow)).unwrap();
    registry
}

#[test]
fn embedder_plugin_flows_through_to_sli_validate_and_generate() {
    let spec = Spec::from_yaml(EMBEDDER_SPEC).unwrap();

    // Without the embedder registry there is genuinely no way to generate
    // output, so plain validation fails with the unknown id.
    let msg = spec.validate().unwrap_err().to_string();
    assert!(
        msg.contains("unknown SLI plugin 'acme/static-ratio'"),
        "{msg}"
    );

    let registry = embedder_registry();
    let sli = spec.slos[0].to_sli_with(&registry).unwrap();
    assert_eq!(
        sli,
        Sli::Raw {
            error_ratio_query: "avg_over_time(app:error_ratio[{{.window}}])".to_string()
        }
    );
    spec.validate_with(&registry)
        .expect("validates with the embedder registry");

    let mut opts = GenerateOptions::default();
    opts.plugins = Arc::new(registry);
    let yaml = generate_rules_with(&spec, &opts)
        .unwrap()
        .to_prometheus_yaml()
        .unwrap();
    assert!(
        yaml.contains("avg_over_time(app:error_ratio[5m])"),
        "{yaml}"
    );
    assert!(yaml.contains("sloth_service: acmesvc"), "{yaml}");
}

#[test]
fn broken_embedder_plugin_is_caught_by_post_expansion_validation() {
    let yaml = r#"
service: acmesvc
slos:
  - name: broken
    objective: 99.9
    sli:
      plugin:
        id: acme/no-window
"#;
    let spec = Spec::from_yaml(yaml).unwrap();
    let msg = spec
        .validate_with(&embedder_registry())
        .unwrap_err()
        .to_string();
    assert!(
        msg.contains("missing the {{.window}} template token"),
        "{msg}"
    );
}
