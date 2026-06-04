//! The `slokit` SLO spec: a [`sloth`](https://sloth.dev)-compatible YAML model
//! plus small native extensions.
//!
//! The model deserializes existing `sloth` `prometheus/v1` specs unchanged
//! (unknown fields are ignored, so newer `sloth` keys do not break parsing) and
//! adds an optional per-SLO [`SloSpec::period`] override, which `sloth` only
//! exposes as a global CLI flag.

mod parse;
mod validate;

pub use validate::validate;

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

/// The SLI definition: exactly one of `events` or `raw`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
pub struct SliSpec {
    /// Events-based SLI (bad events over total events).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<EventsSli>,
    /// Raw error-ratio SLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawSli>,
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

    /// Validate the spec, returning [`SlokitError::Validation`] on any problem.
    pub fn validate(&self) -> Result<()> {
        validate(self)
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

    /// Build a core [`Sli`] from this spec's SLI definition.
    pub fn to_sli(&self) -> Result<Sli> {
        match (&self.sli.events, &self.sli.raw) {
            (Some(events), None) => Ok(Sli::Events {
                error_query: events.error_query.clone(),
                total_query: events.total_query.clone(),
            }),
            (None, Some(raw)) => Ok(Sli::Raw {
                error_ratio_query: raw.error_ratio_query.clone(),
            }),
            (Some(_), Some(_)) => Err(SlokitError::Spec(format!(
                "SLO '{}' sets both `events` and `raw` SLIs; pick one",
                self.name
            ))),
            (None, None) => Err(SlokitError::Spec(format!(
                "SLO '{}' has no `events` or `raw` SLI",
                self.name
            ))),
        }
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
}
