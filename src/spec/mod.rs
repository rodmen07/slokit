//! The `slokit` SLO spec: a [`sloth`](https://sloth.dev)-compatible YAML model
//! plus small native extensions.
//!
//! The model deserializes existing `sloth` `prometheus/v1` specs unchanged
//! (unknown fields are ignored, so newer `sloth` keys do not break parsing) and
//! adds slokit extensions: an optional per-SLO [`SloSpec::period`] override
//! (which `sloth` only exposes as a global CLI flag), a [`LatencySli`] SLI
//! shape that generates the histogram bucket query for latency SLOs, per-SLO
//! custom burn-rate conditions via [`Alerting::windows`], and the
//! sloth-compatible [`PluginSli`] shape resolved against an SLI [`plugin`]
//! registry.

mod lint;
mod parse;
pub mod plugin;
mod validate;

pub use lint::{lint, lint_with, Lint, LintLevel};
pub use validate::{validate, validate_all, validate_all_with, validate_with};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SlokitError};
use crate::sli::Sli;
use crate::slo::{Objective, Slo};
use crate::window::Window;

/// The default SLO period when neither the spec nor the caller overrides it.
pub const DEFAULT_PERIOD: Window = Window::days(30);

fn default_version() -> String {
    "prometheus/v1".to_string()
}

/// A full SLO spec for one service.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Spec {
    /// Spec version, e.g. `prometheus/v1`.
    #[serde(default = "default_version")]
    pub version: String,
    /// The service these SLOs describe.
    pub service: String,
    /// Labels propagated onto every generated rule for this service.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// The SLOs for this service.
    pub slos: Vec<SloSpec>,
}

/// One SLO within a [`Spec`].
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SloSpec {
    /// SLO name, unique within the service.
    pub name: String,
    /// Objective as a percentage, e.g. `99.9`.
    pub objective: f64,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Labels added to this SLO's rules.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// How the error ratio is measured.
    pub sli: SliSpec,
    /// Alerting metadata for the page and ticket alerts.
    #[serde(default)]
    pub alerting: Alerting,
    /// slokit extension: per-SLO period override (e.g. `28d`). Falls back to
    /// the generation default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
}

/// The SLI definition: exactly one of `events`, `raw`, `latency`, or `plugin`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct SliSpec {
    /// Events-based SLI (bad events over total events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<EventsSli>,
    /// Raw error-ratio SLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawSli>,
    /// Latency SLI generated from a Prometheus histogram (slokit extension).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<LatencySli>,
    /// Plugin-provided SLI (sloth-compatible spec surface).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginSli>,
}

/// An events-based SLI.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct EventsSli {
    /// Query counting failing events over `{{.window}}`.
    pub error_query: String,
    /// Query counting all events over `{{.window}}`.
    pub total_query: String,
}

/// A raw error-ratio SLI.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct RawSli {
    /// Query yielding an error ratio over `{{.window}}`.
    pub error_ratio_query: String,
}

/// A latency SLI generated from a Prometheus histogram.
///
/// Produces the error ratio "fraction of requests slower than `threshold`",
/// i.e. `1 - (bucket(le=threshold) / count)`, so callers do not hand-write it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LatencySli {
    /// Histogram base metric, without the `_bucket`/`_count` suffix.
    pub histogram_metric: String,
    /// The `le` bucket boundary, as a string (e.g. `"0.3"`).
    pub threshold: String,
    /// Optional label matchers, written without braces (e.g. `job="api"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

/// A plugin-provided SLI: a plugin registry `id` plus option values, expanded
/// into one of the core SLI shapes during resolution (see
/// [`SloSpec::to_sli_with`] and the [`plugin`] module).
///
/// This is the sloth-compatible `sli.plugin` spec surface. Only the shape is
/// compatible: slokit resolves ids against its own registry and never loads
/// sloth's Go plugin files, so `sloth-common/...` ids fail with an
/// unknown-plugin-id validation error.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PluginSli {
    /// The registry key, e.g. `slokit/availability/http-requests-total`.
    pub id: String,
    /// Option values passed to the plugin. Scalar YAML values (strings,
    /// numbers, bools) are coerced to strings, so `threshold: 0.5` and
    /// `threshold: "0.5"` are equivalent; non-scalar values are a parse error.
    #[serde(
        default,
        deserialize_with = "deserialize_scalar_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub options: BTreeMap<String, String>,
}

/// Deserialize `sli.plugin.options` as a string map, coercing scalar YAML
/// values (numbers, bools) to their string form to stay a superset of sloth's
/// `map[string]string`. Non-scalar values (maps, lists) are a parse error.
fn deserialize_scalar_map<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ScalarString(String);

    impl<'de> Deserialize<'de> for ScalarString {
        fn deserialize<D2>(deserializer: D2) -> std::result::Result<Self, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            struct Visitor;

            impl serde::de::Visitor<'_> for Visitor {
                type Value = ScalarString;

                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a scalar plugin option value (string, number, or bool)")
                }

                fn visit_str<E: serde::de::Error>(
                    self,
                    v: &str,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(ScalarString(v.to_string()))
                }

                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(ScalarString(v.to_string()))
                }

                fn visit_i64<E: serde::de::Error>(
                    self,
                    v: i64,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(ScalarString(v.to_string()))
                }

                fn visit_u64<E: serde::de::Error>(
                    self,
                    v: u64,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(ScalarString(v.to_string()))
                }

                fn visit_f64<E: serde::de::Error>(
                    self,
                    v: f64,
                ) -> std::result::Result<Self::Value, E> {
                    Ok(ScalarString(v.to_string()))
                }
            }

            deserializer.deserialize_any(Visitor)
        }
    }

    let map = BTreeMap::<String, ScalarString>::deserialize(deserializer)?;
    Ok(map.into_iter().map(|(k, v)| (k, v.0)).collect())
}

/// Alerting metadata shared and per-severity.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct Alerting {
    /// Alert rule name. Defaults to the SLO name when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Labels applied to both severities.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Annotations applied to both severities.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
    /// Page-alert overrides.
    #[serde(default)]
    pub page_alert: AlertMeta,
    /// Ticket-alert overrides.
    #[serde(default)]
    pub ticket_alert: AlertMeta,
    /// slokit extension: custom burn-rate conditions replacing the default
    /// MWMBR window table for this SLO. When empty, the defaults apply (scaled
    /// to the SLO period unless period scaling is disabled).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<AlertWindowSpec>,
}

/// One custom burn-rate condition (slokit extension): the alert for `severity`
/// fires when the error ratio exceeds `factor` times the budget over both the
/// `long` and `short` windows.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AlertWindowSpec {
    /// Which alert this condition belongs to: `page` or `ticket`.
    pub severity: String,
    /// The long lookback window, e.g. `1h`.
    pub long: String,
    /// The short confirmation window, e.g. `5m`.
    pub short: String,
    /// The burn-rate multiplier that triggers this condition.
    pub factor: f64,
}

impl AlertWindowSpec {
    /// Convert to a core [`AlertWindow`], failing on an unknown severity or an
    /// unparseable duration.
    pub fn to_alert_window(&self) -> Result<crate::burn_rate::AlertWindow> {
        let severity = match self.severity.as_str() {
            "page" => crate::burn_rate::Severity::Page,
            "ticket" => crate::burn_rate::Severity::Ticket,
            other => {
                return Err(SlokitError::Spec(format!(
                    "unknown alert window severity '{other}' (expected `page` or `ticket`)"
                )))
            }
        };
        Ok(crate::burn_rate::AlertWindow {
            severity,
            long: Window::parse(&self.long)?,
            short: Window::parse(&self.short)?,
            factor: self.factor,
        })
    }
}

/// Per-severity alert overrides.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct AlertMeta {
    /// Skip generating this severity's alert.
    #[serde(default)]
    pub disable: bool,
    /// Labels for this severity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Annotations for this severity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl Spec {
    /// Parse a spec from a YAML string. See also [`Spec::from_path`].
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        parse::from_yaml(yaml)
    }

    /// Read and parse a spec from a YAML file.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self> {
        parse::from_path(path.as_ref())
    }

    /// Read and parse every `*.yaml`/`*.yml` spec in a directory, sorted by path.
    pub fn from_dir(dir: impl AsRef<std::path::Path>) -> Result<Vec<Self>> {
        parse::from_dir(dir.as_ref())
    }

    /// Load one or many specs from a path: a file yields one spec, a directory
    /// yields every `*.yaml`/`*.yml` spec it contains.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Vec<Self>> {
        parse::load(path.as_ref())
    }

    /// Validate the spec, returning [`SlokitError::Validation`] on any problem.
    /// `plugin` SLIs are resolved against slokit's built-in plugin registry.
    ///
    /// When several specs will be merged into one rules file, prefer
    /// [`validate_all`], which adds cross-spec checks (duplicate service/SLO
    /// identities) on top of this per-spec validation.
    pub fn validate(&self) -> Result<()> {
        validate(self)
    }

    /// Like [`Spec::validate`], but resolving `plugin` SLIs against an
    /// explicit [`SliPluginRegistry`](plugin::SliPluginRegistry) (for
    /// embedders with custom plugins).
    pub fn validate_with(&self, plugins: &plugin::SliPluginRegistry) -> Result<()> {
        validate_with(self, plugins)
    }

    /// Run advisory [`lint`] checks and return every finding (never fails).
    /// Plugin option names are checked against slokit's built-in registry.
    ///
    /// Unlike [`Spec::validate`], this reports legal-but-questionable
    /// configurations rather than hard errors. An empty vec means no advisories.
    pub fn lint(&self) -> Vec<Lint> {
        lint(self)
    }

    /// Like [`Spec::lint`], but checking plugin option names against an
    /// explicit [`SliPluginRegistry`](plugin::SliPluginRegistry), so
    /// `PLUGIN_UNKNOWN_OPTION` compares against the right declarations.
    pub fn lint_with(&self, plugins: &plugin::SliPluginRegistry) -> Vec<Lint> {
        lint_with(self, plugins)
    }
}

impl SloSpec {
    /// The `sloth_id` for this SLO: `"<service>-<name>"`.
    pub fn sloth_id(&self, service: &str) -> String {
        format!("{service}-{}", self.name)
    }

    /// Resolve the SLO period, using `default` when no override is set.
    pub fn resolve_period(&self, default: Window) -> Result<Window> {
        match &self.period {
            Some(p) => Window::parse(p),
            None => Ok(default),
        }
    }

    /// Build a core [`Slo`] from this spec's objective and resolved period.
    pub fn to_slo(&self, default_period: Window) -> Result<Slo> {
        let objective = Objective::percent(self.objective)?;
        let period = self.resolve_period(default_period)?;
        Ok(Slo::new(objective, period))
    }

    /// Build a core [`Sli`] from this spec's SLI definition, resolving a
    /// `plugin` SLI against slokit's built-in plugin registry. Exactly one of
    /// `events`, `raw`, `latency`, or `plugin` must be set.
    ///
    /// Use [`SloSpec::to_sli_with`] to resolve against a custom registry.
    pub fn to_sli(&self) -> Result<Sli> {
        self.to_sli_with(&plugin::SliPluginRegistry::with_builtins())
    }

    /// Like [`SloSpec::to_sli`], but resolving `plugin` SLIs against an
    /// explicit [`SliPluginRegistry`](plugin::SliPluginRegistry) (for
    /// embedders with custom plugins).
    pub fn to_sli_with(&self, plugins: &plugin::SliPluginRegistry) -> Result<Sli> {
        let set_count = [
            self.sli.events.is_some(),
            self.sli.raw.is_some(),
            self.sli.latency.is_some(),
            self.sli.plugin.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if set_count > 1 {
            return Err(SlokitError::Spec(format!(
                "SLO '{}' sets multiple SLIs; pick one of `events`, `raw`, `latency`, or `plugin`",
                self.name
            )));
        }
        if let Some(events) = &self.sli.events {
            Ok(Sli::Events {
                error_query: events.error_query.clone(),
                total_query: events.total_query.clone(),
            })
        } else if let Some(raw) = &self.sli.raw {
            Ok(Sli::Raw {
                error_ratio_query: raw.error_ratio_query.clone(),
            })
        } else if let Some(latency) = &self.sli.latency {
            Ok(Sli::Latency {
                histogram_metric: latency.histogram_metric.clone(),
                threshold: latency.threshold.clone(),
                selector: latency.selector.clone(),
            })
        } else if let Some(plugin_sli) = &self.sli.plugin {
            if plugin_sli.id.trim().is_empty() {
                return Err(SlokitError::Spec(format!(
                    "SLO '{}': `sli.plugin.id` must not be empty",
                    self.name
                )));
            }
            plugins.resolve(&plugin_sli.id, &plugin_sli.options)
        } else {
            Err(SlokitError::Spec(format!(
                "SLO '{}' has no `events`, `raw`, `latency`, or `plugin` SLI",
                self.name
            )))
        }
    }

    /// Build the custom MWMBR configuration from `alerting.windows`, if any.
    ///
    /// Returns `Ok(None)` when the SLO defines no custom windows, so callers
    /// fall back to the (period-scaled) default table.
    pub fn custom_mwmbr(&self) -> Result<Option<crate::burn_rate::MwmbrConfig>> {
        if self.alerting.windows.is_empty() {
            return Ok(None);
        }
        let windows = self
            .alerting
            .windows
            .iter()
            .map(|w| w.to_alert_window())
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(crate::burn_rate::MwmbrConfig { windows }))
    }

    /// The effective alert rule name (the configured name, or the SLO name).
    pub fn alert_name(&self) -> &str {
        if self.alerting.name.is_empty() {
            &self.name
        } else {
            &self.alerting.name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
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
        error_query: sum(rate(http_requests_total{code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total[{{.window}}]))
    alerting:
      name: HighErrorRate
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

    #[test]
    fn parses_sample_spec() {
        let spec = Spec::from_yaml(SAMPLE).unwrap();
        assert_eq!(spec.service, "myservice");
        assert_eq!(spec.slos.len(), 1);
        let slo = &spec.slos[0];
        assert_eq!(slo.objective, 99.9);
        assert_eq!(slo.sloth_id("myservice"), "myservice-requests-availability");
        assert_eq!(slo.alert_name(), "HighErrorRate");
    }

    #[test]
    fn converts_to_core_types() {
        let spec = Spec::from_yaml(SAMPLE).unwrap();
        let slo = spec.slos[0].to_slo(DEFAULT_PERIOD).unwrap();
        assert!((slo.objective.as_percent() - 99.9).abs() < 1e-9);
        assert_eq!(slo.period, DEFAULT_PERIOD);
        let sli = spec.slos[0].to_sli().unwrap();
        assert!(matches!(sli, Sli::Events { .. }));
    }

    #[test]
    fn per_slo_period_override_is_respected() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    period: 7d
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let slo = spec.slos[0].to_slo(DEFAULT_PERIOD).unwrap();
        assert_eq!(slo.period, Window::days(7));
    }

    #[test]
    fn ignores_unknown_sloth_fields() {
        let yaml = r#"
service: s
some_future_sloth_key: true
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#;
        assert!(Spec::from_yaml(yaml).is_ok());
    }

    #[test]
    fn latency_sli_converts_to_core() {
        let yaml = r#"
service: s
slos:
  - name: latency
    objective: 99.0
    sli:
      latency:
        histogram_metric: http_request_duration_seconds
        threshold: "0.3"
        selector: job="api"
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let sli = spec.slos[0].to_sli().unwrap();
        assert!(matches!(sli, Sli::Latency { .. }));
        assert!(sli
            .error_ratio_expr(Window::minutes(5))
            .contains("le=\"0.3\""));
    }

    #[test]
    fn custom_alert_windows_convert_to_mwmbr() {
        let yaml = r#"
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
          factor: 10
        - severity: ticket
          long: 6h
          short: 30m
          factor: 2
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let cfg = spec.slos[0].custom_mwmbr().unwrap().expect("custom config");
        assert_eq!(cfg.windows.len(), 2);
        assert_eq!(cfg.windows[0].long, Window::minutes(30));
        assert_eq!(cfg.windows[0].factor, 10.0);
        assert_eq!(cfg.windows[1].severity, crate::burn_rate::Severity::Ticket);
    }

    #[test]
    fn no_custom_windows_means_none() {
        let spec = Spec::from_yaml(SAMPLE).unwrap();
        assert!(spec.slos[0].custom_mwmbr().unwrap().is_none());
    }

    #[test]
    fn bad_custom_window_severity_is_an_error() {
        let yaml = r#"
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
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let err = spec.slos[0].custom_mwmbr().unwrap_err();
        assert!(err.to_string().contains("unknown alert window severity"));
    }

    #[test]
    fn multiple_slis_is_an_error() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
      latency:
        histogram_metric: m
        threshold: "1"
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let err = spec.slos[0].to_sli().unwrap_err();
        assert!(err.to_string().contains("multiple SLIs"));
    }

    #[test]
    fn plugin_sli_resolves_against_the_builtin_registry() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let sli = spec.slos[0].to_sli().unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query:
                    "sum(rate(http_requests_total{job=\"api\", code=~\"5..\"}[{{.window}}]))"
                        .to_string(),
                total_query: "sum(rate(http_requests_total{job=\"api\"}[{{.window}}]))".to_string(),
            }
        );
    }

    #[test]
    fn plugin_options_coerce_scalars_to_strings() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: some/plugin
        options:
          threshold: 0.5
          count: 5
          enabled: true
          name: plain
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let plugin = spec.slos[0].sli.plugin.as_ref().unwrap();
        assert_eq!(plugin.options["threshold"], "0.5");
        assert_eq!(plugin.options["count"], "5");
        assert_eq!(plugin.options["enabled"], "true");
        assert_eq!(plugin.options["name"], "plain");
    }

    #[test]
    fn non_scalar_plugin_option_values_are_parse_errors() {
        for value in ["[1, 2]", "{a: b}"] {
            let yaml = format!(
                r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: some/plugin
        options:
          bad: {value}
"#
            );
            let err = Spec::from_yaml(&yaml).unwrap_err();
            assert!(
                err.to_string().contains("scalar plugin option value"),
                "value {value}: {err}"
            );
        }
    }

    #[test]
    fn plugin_options_default_to_empty() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: slokit/availability/http-requests-total
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let plugin = spec.slos[0].sli.plugin.as_ref().unwrap();
        assert!(plugin.options.is_empty());
        assert!(spec.slos[0].to_sli().is_ok());
    }

    #[test]
    fn empty_plugin_id_is_an_error() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: "  "
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let err = spec.slos[0].to_sli().unwrap_err();
        assert!(err
            .to_string()
            .contains("`sli.plugin.id` must not be empty"));
    }

    #[test]
    fn unknown_plugin_id_is_an_error() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: sloth-common/kubernetes/apiserver/availability
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let err = spec.slos[0].to_sli().unwrap_err();
        assert!(err
            .to_string()
            .contains("unknown SLI plugin 'sloth-common/kubernetes/apiserver/availability'"));
    }

    #[test]
    fn every_sli_pair_including_plugin_is_mutually_exclusive() {
        let shapes = [
            (
                "events",
                "events:\n        error_query: e[{{.window}}]\n        total_query: t[{{.window}}]",
            ),
            ("raw", "raw:\n        error_ratio_query: r[{{.window}}]"),
            (
                "latency",
                "latency:\n        histogram_metric: m\n        threshold: \"1\"",
            ),
            (
                "plugin",
                "plugin:\n        id: slokit/availability/http-requests-total",
            ),
        ];
        for (i, (name_a, a)) in shapes.iter().enumerate() {
            for (name_b, b) in shapes.iter().skip(i + 1) {
                let yaml = format!(
                    "service: s\nslos:\n  - name: a\n    objective: 99.0\n    sli:\n      {a}\n      {b}\n"
                );
                let spec = Spec::from_yaml(&yaml).unwrap();
                let err = spec.slos[0].to_sli().unwrap_err();
                assert!(
                    err.to_string().contains("multiple SLIs"),
                    "{name_a}+{name_b}: {err}"
                );
            }
        }
    }

    #[test]
    fn to_sli_with_uses_the_given_registry() {
        use std::collections::BTreeMap as Map;

        use plugin::{OptionKind, OptionSpec, SliPlugin, SliPluginRegistry};

        struct Toy;
        impl SliPlugin for Toy {
            fn id(&self) -> &str {
                "acme/toy"
            }
            fn description(&self) -> &str {
                "toy"
            }
            fn options(&self) -> &[OptionSpec] {
                const OPTIONS: &[OptionSpec] = &[OptionSpec {
                    name: "metric",
                    kind: OptionKind::String,
                    required: true,
                    default: None,
                    help: "ratio metric",
                }];
                OPTIONS
            }
            fn expand(&self, options: &Map<String, String>) -> Result<Sli> {
                Ok(Sli::Raw {
                    error_ratio_query: format!(
                        "{}[{}]",
                        options["metric"],
                        crate::sli::WINDOW_TOKEN
                    ),
                })
            }
        }

        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      plugin:
        id: acme/toy
        options:
          metric: app:ratio
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        // Not in the built-in registry.
        assert!(spec.slos[0].to_sli().is_err());
        let mut registry = SliPluginRegistry::empty();
        registry.register(Box::new(Toy)).unwrap();
        let sli = spec.slos[0].to_sli_with(&registry).unwrap();
        assert_eq!(
            sli,
            Sli::Raw {
                error_ratio_query: "app:ratio[{{.window}}]".to_string()
            }
        );
    }
}
