//! Generate Prometheus rules from a [`Spec`].
//!
//! For each SLO, `slokit` emits three rule groups, mirroring `sloth`'s metric
//! and label conventions so existing Grafana dashboards keep working:
//!
//! - **SLI recordings**: `slo:sli_error:ratio_rate<window>` at every MWMBR
//!   lookback window plus the SLO period.
//! - **Metadata recordings**: `slo:objective:ratio`, `slo:error_budget:ratio`,
//!   `slo:current_burn_rate:ratio`, and friends.
//! - **Alerts**: multi-window multi-burn-rate page and ticket alerts.

mod alert;
mod metadata;
mod recording;

use std::collections::BTreeMap;

use serde::Serialize;

use crate::burn_rate::MwmbrConfig;
use crate::error::{Result, SlokitError};
use crate::sli::Sli;
use crate::slo::Slo;
use crate::spec::{SloSpec, Spec, DEFAULT_PERIOD};
use crate::window::Window;

/// Options controlling rule generation.
#[derive(Debug, Clone)]
pub struct GenerateOptions {
    /// Period used for SLOs that do not set their own `period`.
    pub default_period: Window,
    /// The burn-rate alert configuration.
    pub mwmbr: MwmbrConfig,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            default_period: DEFAULT_PERIOD,
            mwmbr: MwmbrConfig::sre_default(),
        }
    }
}

/// A single Prometheus rule: either a recording rule (`record`) or an alerting
/// rule (`alert`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rule {
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alert: Option<String>,
    expr: String,
    #[serde(rename = "for", skip_serializing_if = "Option::is_none")]
    for_: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

impl Rule {
    /// Build a recording rule.
    fn record(
        name: impl Into<String>,
        expr: impl Into<String>,
        labels: BTreeMap<String, String>,
    ) -> Self {
        Self {
            record: Some(name.into()),
            alert: None,
            expr: expr.into(),
            for_: None,
            labels,
            annotations: BTreeMap::new(),
        }
    }

    /// Build an alerting rule.
    fn alert(
        name: impl Into<String>,
        expr: impl Into<String>,
        labels: BTreeMap<String, String>,
        annotations: BTreeMap<String, String>,
    ) -> Self {
        Self {
            record: None,
            alert: Some(name.into()),
            expr: expr.into(),
            for_: None,
            labels,
            annotations,
        }
    }
}

/// A named group of rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleGroup {
    /// Group name.
    pub name: String,
    /// Rules in the group.
    pub rules: Vec<Rule>,
}

/// A complete set of rule groups, ready to render as Prometheus rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleSet {
    /// The rule groups.
    pub groups: Vec<RuleGroup>,
}

impl RuleSet {
    /// Render as a plain Prometheus `rules.yaml` document.
    pub fn to_prometheus_yaml(&self) -> Result<String> {
        serde_norway::to_string(self).map_err(|e| SlokitError::Spec(e.to_string()))
    }

    /// Render as a Prometheus Operator `PrometheusRule` custom resource.
    pub fn to_operator_yaml(
        &self,
        name: &str,
        labels: &BTreeMap<String, String>,
    ) -> Result<String> {
        #[derive(Serialize)]
        struct Metadata<'a> {
            name: &'a str,
            #[serde(skip_serializing_if = "BTreeMap::is_empty")]
            labels: &'a BTreeMap<String, String>,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct PrometheusRule<'a> {
            api_version: &'a str,
            kind: &'a str,
            metadata: Metadata<'a>,
            spec: &'a RuleSet,
        }
        let doc = PrometheusRule {
            api_version: "monitoring.coreos.com/v1",
            kind: "PrometheusRule",
            metadata: Metadata { name, labels },
            spec: self,
        };
        serde_norway::to_string(&doc).map_err(|e| SlokitError::Spec(e.to_string()))
    }
}

/// Per-SLO data shared by the three rule builders.
struct SloContext<'a> {
    service: &'a str,
    spec_labels: &'a BTreeMap<String, String>,
    slo_spec: &'a SloSpec,
    slo: Slo,
    sli: Sli,
    id: String,
    mwmbr: &'a MwmbrConfig,
}

impl SloContext<'_> {
    /// `{sloth_id="..", sloth_service="..", sloth_slo=".."}` selector.
    fn selector(&self) -> String {
        format!(
            "{{sloth_id=\"{}\", sloth_service=\"{}\", sloth_slo=\"{}\"}}",
            self.id, self.service, self.slo_spec.name
        )
    }

    /// Labels common to this SLO's rules: custom labels plus the `sloth_*`
    /// identity labels.
    fn base_labels(&self) -> BTreeMap<String, String> {
        let mut labels = self.spec_labels.clone();
        labels.extend(self.slo_spec.labels.clone());
        labels.insert("sloth_id".to_string(), self.id.clone());
        labels.insert("sloth_service".to_string(), self.service.to_string());
        labels.insert("sloth_slo".to_string(), self.slo_spec.name.clone());
        labels
    }
}

/// Generate the full rule set for a spec using default options.
pub fn generate_rules(spec: &Spec) -> Result<RuleSet> {
    generate_rules_with(spec, &GenerateOptions::default())
}

/// Generate one merged rule set covering several specs (their rule groups are
/// concatenated), using explicit options.
pub fn generate_all(specs: &[Spec], opts: &GenerateOptions) -> Result<RuleSet> {
    let mut groups = Vec::new();
    for spec in specs {
        groups.extend(generate_rules_with(spec, opts)?.groups);
    }
    Ok(RuleSet { groups })
}

/// Generate the full rule set for a spec using explicit options.
pub fn generate_rules_with(spec: &Spec, opts: &GenerateOptions) -> Result<RuleSet> {
    spec.validate()?;

    let mut groups = Vec::with_capacity(spec.slos.len() * 3);
    for slo_spec in &spec.slos {
        let ctx = SloContext {
            service: &spec.service,
            spec_labels: &spec.labels,
            slo_spec,
            slo: slo_spec.to_slo(opts.default_period)?,
            sli: slo_spec.to_sli()?,
            id: slo_spec.sloth_id(&spec.service),
            mwmbr: &opts.mwmbr,
        };
        groups.push(recording::rules(&ctx));
        groups.push(metadata::rules(&ctx));
        groups.push(alert::rules(&ctx));
    }

    Ok(RuleSet { groups })
}

/// Format an `f64` for embedding in PromQL, trimming trailing zeros and never
/// using scientific notation (so `0.001` stays `0.001`).
pub(crate) fn fmt_num(x: f64) -> String {
    let s = format!("{x:.10}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
service: myservice
labels:
  owner: team-platform
slos:
  - name: requests-availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(http_requests_total{code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total[{{.window}}]))
    alerting:
      page_alert:
        labels:
          severity: page
      ticket_alert:
        labels:
          severity: ticket
"#;

    #[test]
    fn fmt_num_trims_cleanly() {
        assert_eq!(fmt_num(0.001), "0.001");
        assert_eq!(fmt_num(0.999), "0.999");
        assert_eq!(fmt_num(30.0), "30");
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(14.4), "14.4");
        assert_eq!(fmt_num(0.0005), "0.0005");
    }

    #[test]
    fn generates_three_groups_per_slo() {
        let spec = Spec::from_yaml(SAMPLE).unwrap();
        let rs = generate_rules(&spec).unwrap();
        assert_eq!(rs.groups.len(), 3);
        assert!(rs.groups[0].name.contains("sli-recordings"));
        assert!(rs.groups[1].name.contains("meta-recordings"));
        assert!(rs.groups[2].name.contains("alerts"));
    }

    #[test]
    fn renders_prometheus_and_operator_yaml() {
        let spec = Spec::from_yaml(SAMPLE).unwrap();
        let rs = generate_rules(&spec).unwrap();
        let prom = rs.to_prometheus_yaml().unwrap();
        assert!(prom.contains("groups:"));
        assert!(prom.contains("slo:sli_error:ratio_rate5m"));
        let op = rs.to_operator_yaml("myservice", &spec.labels).unwrap();
        assert!(op.contains("kind: PrometheusRule"));
        assert!(op.contains("apiVersion: monitoring.coreos.com/v1"));
    }

    #[test]
    fn invalid_spec_fails_generation() {
        let yaml = r#"
service: s
slos:
  - name: bad
    objective: 150
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(generate_rules(&spec).is_err());
    }
}
