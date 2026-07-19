//! The `slokit` SLO spec: a [`sloth`](https://sloth.dev)-compatible YAML model
//! plus small native extensions.
//!
//! The model deserializes existing `sloth` `prometheus/v1` specs unchanged
//! (unknown fields are ignored, so newer `sloth` keys do not break parsing) and
//! adds slokit extensions: an optional per-SLO [`SloSpec::period`] override
//! (which `sloth` only exposes as a global CLI flag), a [`LatencySli`] SLI
//! shape that generates the histogram bucket query for latency SLOs, and
//! per-SLO custom burn-rate conditions via [`Alerting::windows`].

mod lint;
mod parse;
mod validate;

pub use lint::{lint, Lint, LintLevel};
pub use validate::{validate, validate_all};

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

/// The SLI definition: exactly one of `events`, `raw`, or `latency`.
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
    ///
    /// When several specs will be merged into one rules file, prefer
    /// [`validate_all`], which adds cross-spec checks (duplicate service/SLO
    /// identities) on top of this per-spec validation.
    pub fn validate(&self) -> Result<()> {
        validate(self)
    }

    /// Run advisory [`lint`] checks and return every finding (never fails).
    ///
    /// Unlike [`Spec::validate`], this reports legal-but-questionable
    /// configurations rather than hard errors. An empty vec means no advisories.
    pub fn lint(&self) -> Vec<Lint> {
        lint(self)
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

    /// Build a core [`Sli`] from this spec's SLI definition. Exactly one of
    /// `events`, `raw`, or `latency` must be set.
    pub fn to_sli(&self) -> Result<Sli> {
        let set_count = [
            self.sli.events.is_some(),
            self.sli.raw.is_some(),
            self.sli.latency.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if set_count > 1 {
            return Err(SlokitError::Spec(format!(
                "SLO '{}' sets multiple SLIs; pick one of `events`, `raw`, or `latency`",
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
        } else {
            Err(SlokitError::Spec(format!(
                "SLO '{}' has no `events`, `raw`, or `latency` SLI",
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
}
