//! OpenSLO v1 export: serialize a slokit [`Spec`] as `kind: SLO` documents.
//!
//! This is the inverse of the import in the parent module. Together they make
//! adoption reversible: a team can migrate into slokit from OpenSLO tooling and
//! hand specs back to any OpenSLO consumer afterwards.
//!
//! One slokit SLO becomes one `apiVersion: openslo/v1`, `kind: SLO` document
//! with exactly one objective, so a multi-SLO [`Spec`] serializes as a
//! multi-document YAML stream. Emitting one objective per document (rather than
//! one document per service with N objectives) is what keeps SLO names stable:
//! the import suffixes objective names only when a document carries more than
//! one.
//!
//! # Mapping (the inverse of the import's table)
//!
//! | slokit model | OpenSLO construct |
//! |--------------|-------------------|
//! | [`Spec::service`] | `spec.service` on every emitted document |
//! | [`Spec::labels`] | merged into each document's `metadata.labels` (see "Reported as notes") |
//! | [`SloSpec::name`] | `metadata.name` |
//! | [`SloSpec::labels`] | `metadata.labels` (wins over a same-named [`Spec::labels`] entry) |
//! | [`SloSpec::description`] | `spec.description` (omitted when empty) |
//! | [`SloSpec::period`] | `spec.timeWindow[0]` as `{duration, isRolling: true}` (omitted when unset) |
//! | [`SloSpec::objective`] (a percent) | `spec.objectives[0].target` (a unit fraction; see "Fidelity") |
//! | the slokit model itself (event-ratio budgets) | `spec.budgetingMethod: Occurrences` |
//! | [`EventsSli`] | `ratioMetric.bad` = `error_query`, `ratioMetric.total` = `total_query` |
//! | [`RawSli`] | `ratioMetric.raw` = `error_ratio_query` with `rawType: failure` |
//! | [`LatencySli`] | `thresholdMetric` query `metric{selector}`, objective `op: lte` and `value` = `threshold` |
//!
//! Every query is emitted as an inline Prometheus metric source
//! (`metricSource.type: Prometheus`, `metricSource.spec.query`), because that is
//! the only source the import resolves.
//!
//! # Fidelity
//!
//! The contract is a SEMANTIC round trip, not byte identity: the two models are
//! not isomorphic and field order differs.
//! [`from_yaml`](super::from_yaml)`(`[`to_yaml`]`(spec))` yields the same spec
//! with the documented transformations below applied, and nothing else changed.
//!
//! The objective is the one value that is not carried as-is: a percent becomes
//! a unit fraction on the way out and is multiplied by 100 on the way back in.
//! A percent and its hundredth are rarely both exactly representable as f64, so
//! the mapping promises f64 rounding rather than bit identity for an arbitrary
//! percent. In practice it is bit-exact for every realistic objective (`99`,
//! `99.5`, `99.9`, `99.95`, `99.99`, `99.999`, `99.9999`) and for every spec in
//! this repo, because [`objective_target`] picks between the shortest decimal
//! (`99.9` -> `0.999`) and the plain division on exactly that criterion.
//!
//! # Reported as notes (transformed or dropped, never silently)
//!
//! Notes are the export-side twin of [`ImportNote`](super::ImportNote): the
//! spec WAS representable, but something did not survive one-to-one.
//!
//! - [`SloSpec::alerting`]: slokit generates multi-window multi-burn-rate
//!   alerts from the objective, and OpenSLO expresses alerting as separate
//!   `kind: AlertPolicy` documents with a different model (conditions and
//!   notification targets, not burn-rate windows). Non-default alerting
//!   metadata is dropped with a note naming what was dropped. The import is the
//!   mirror image: it drops `spec.alertPolicies` with a note.
//! - [`Spec::labels`]: OpenSLO has no service-level label bag, so service
//!   labels are merged into each document's `metadata.labels`. This is
//!   meaning-preserving for what slokit emits (generation merges spec labels
//!   and SLO labels into the same rule labels, SLO labels winning) but it
//!   RELOCATES them: re-importing puts them on every SLO instead of on the
//!   spec.
//! - [`Spec::version`]: the sloth dialect tag has no OpenSLO counterpart
//!   (`apiVersion: openslo/v1` replaces it). Only noted when it is not the
//!   default, since re-import restores the default.
//! - The `{{.window}}` template token is kept verbatim in exported queries,
//!   which is what makes the round trip exact; a non-slokit OpenSLO consumer
//!   must substitute its own lookback. Noted once per export when any query
//!   carries the token.
//!
//! # Errors (unrepresentable specs, fail closed)
//!
//! A construct with no OpenSLO representation is a hard error naming the field,
//! never a silent drop and never best-effort YAML an OpenSLO consumer would
//! reject downstream:
//!
//! - [`PluginSli`](super::super::PluginSli): a plugin SLI has no query until the
//!   registered plugin expands it at generation time, and OpenSLO has no plugin
//!   concept. Expanding it here would export a different (and unrecoverable)
//!   SLI shape than the spec declares.
//! - An SLI with no variant set, or with more than one set at once.
//! - An empty `service`, an empty SLO `name`, or a spec with no SLOs.
//! - An objective that is not a finite percent in `(0, 100]`.
//! - A latency `threshold` that is not a finite positive number (OpenSLO's
//!   objective `value` is numeric), or a `histogram_metric` that is not a bare
//!   Prometheus metric name.
//!
//! Note that these are export-time representability checks, not a substitute
//! for [`Spec::validate`]: run that first, exactly as the import's docs say to
//! run it on imported specs.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::error::{Result, SlokitError};
use crate::sli::WINDOW_TOKEN;
use crate::spec::validate::is_metric_name;

use super::super::{EventsSli, LatencySli, RawSli, SliSpec, SloSpec, Spec};
use super::API_VERSION;

/// The `kind` of every document this module emits.
const SLO_KIND: &str = "SLO";

/// The only metric source type the import resolves, so the only one worth
/// emitting.
const PROMETHEUS: &str = "Prometheus";

/// The only budgeting method the slokit model represents.
const OCCURRENCES: &str = "Occurrences";

/// The result of exporting a [`Spec`]: the OpenSLO YAML plus lint-style notes
/// about what did not survive the conversion one-to-one.
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume, and
/// future report fields must not be breaking changes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Export {
    /// The OpenSLO v1 YAML: one `kind: SLO` document per slokit SLO, in spec
    /// order, joined as a multi-document stream.
    pub yaml: String,
    /// Lint-style notes: slokit constructs that were relocated or dropped
    /// because OpenSLO has no equivalent. An empty vec means a lossless export.
    pub notes: Vec<ExportNote>,
}

/// One advisory note produced during export (not an error: the spec was
/// representable, but something was dropped or moved on the way out).
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume, and
/// future fields (for example a machine-readable code) must not be breaking
/// changes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExportNote {
    /// Where the note applies, e.g. `slo 'requests-availability'`.
    pub location: String,
    /// What was dropped or moved, and why.
    pub message: String,
}

impl std::fmt::Display for ExportNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Serialize a slokit [`Spec`] as OpenSLO v1 YAML.
///
/// The sibling of [`from_yaml`](super::from_yaml). See the module docs for the
/// mapping, what is reported as a note, and which constructs are hard errors.
/// Use [`to_yaml_reported`] instead when the notes matter (a CLI prints them).
///
/// ```
/// use slokit::spec::{openslo, EventsSli, SliSpec, SloSpec, Spec};
///
/// let sli = SliSpec::events(EventsSli::new(
///     "sum(rate(errs_total[{{.window}}]))",
///     "sum(rate(reqs_total[{{.window}}]))",
/// ));
/// let spec = Spec::new("api", vec![SloSpec::new("availability", 99.9, sli)]);
///
/// let yaml = openslo::to_yaml(&spec).unwrap();
/// assert!(yaml.contains("apiVersion: openslo/v1"));
/// assert!(yaml.contains("target: 0.999"));
///
/// // The round trip lands back on the same spec.
/// let back = openslo::from_yaml(&yaml).unwrap();
/// assert_eq!(back.specs[0].slos[0].name, "availability");
/// ```
pub fn to_yaml(spec: &Spec) -> Result<String> {
    to_yaml_reported(spec).map(|e| e.yaml)
}

/// Serialize a slokit [`Spec`] as OpenSLO v1 YAML, keeping the advisory notes.
///
/// Identical to [`to_yaml`] except that the dropped and relocated constructs
/// are reported rather than only documented. See the module docs.
pub fn to_yaml_reported(spec: &Spec) -> Result<Export> {
    let service = spec.service.trim();
    if service.is_empty() {
        return Err(err("spec", "service must not be empty"));
    }
    if spec.slos.is_empty() {
        return Err(err(
            &format!("service '{service}'"),
            "spec has no SLOs to export",
        ));
    }

    let mut notes: Vec<ExportNote> = Vec::new();
    let sloc = format!("service '{service}'");

    if spec.version != super::super::default_version() {
        note(
            &mut notes,
            &sloc,
            format!(
                "version '{}' is a slokit dialect tag with no OpenSLO counterpart (apiVersion: {API_VERSION} replaces it); re-importing restores the default",
                spec.version
            ),
        );
    }
    if !spec.labels.is_empty() {
        note(
            &mut notes,
            &sloc,
            format!(
                "{} service-level label(s) were merged into every SLO's metadata.labels (OpenSLO has no service-level label bag); re-importing leaves them on the SLOs, not on the spec",
                spec.labels.len()
            ),
        );
    }

    let mut docs: Vec<SloDoc> = Vec::with_capacity(spec.slos.len());
    let mut window_token_used = false;
    for slo in &spec.slos {
        let doc = export_slo(service, &spec.labels, slo, &mut notes)?;
        window_token_used |= doc.spec.queries().iter().any(|q| q.contains(WINDOW_TOKEN));
        docs.push(doc);
    }

    if window_token_used {
        note(
            &mut notes,
            &sloc,
            format!(
                "exported queries keep slokit's {WINDOW_TOKEN} template token verbatim; slokit re-imports it unchanged, but any other OpenSLO consumer must substitute its own lookback"
            ),
        );
    }

    let mut yaml = String::new();
    for (i, doc) in docs.iter().enumerate() {
        if i > 0 {
            yaml.push_str("---\n");
        }
        let rendered = serde_norway::to_string(doc)
            .map_err(|e| err(&sloc, format!("serializing OpenSLO YAML: {e}")))?;
        yaml.push_str(&rendered);
        if !yaml.ends_with('\n') {
            yaml.push('\n');
        }
    }

    Ok(Export { yaml, notes })
}

/// Build the export error for `location`, mirroring the import's message shape.
fn err(location: &str, message: impl AsRef<str>) -> SlokitError {
    SlokitError::Spec(format!("openslo export {location}: {}", message.as_ref()))
}

fn note(notes: &mut Vec<ExportNote>, location: &str, message: impl Into<String>) {
    notes.push(ExportNote {
        location: location.to_string(),
        message: message.into(),
    });
}

fn export_slo(
    service: &str,
    spec_labels: &BTreeMap<String, String>,
    slo: &SloSpec,
    notes: &mut Vec<ExportNote>,
) -> Result<SloDoc> {
    let name = slo.name.trim();
    if name.is_empty() {
        return Err(err(
            &format!("service '{service}'"),
            "an SLO has an empty name; OpenSLO requires metadata.name",
        ));
    }
    let loc = format!("slo '{name}'");

    if !slo.objective.is_finite() || slo.objective <= 0.0 || slo.objective > 100.0 {
        return Err(err(
            &loc,
            format!(
                "objective {} is not a finite percent in (0, 100]; OpenSLO's objectives[0].target is a unit fraction",
                slo.objective
            ),
        ));
    }

    if slo.alerting != super::super::Alerting::default() {
        note(
            notes,
            &loc,
            format!(
                "alerting ({}) was dropped: slokit derives multi-window multi-burn-rate alerts from the objective, while OpenSLO models alerting as separate kind: AlertPolicy documents with conditions and notification targets",
                describe_alerting(slo)
            ),
        );
    }

    // Service labels first so an SLO label of the same name wins, which is the
    // precedence `generate::base_labels` already applies to rule labels.
    let mut labels = spec_labels.clone();
    labels.extend(slo.labels.clone());

    let time_window = match slo.period.as_deref().map(str::trim) {
        Some(period) if !period.is_empty() => vec![TimeWindowOut {
            duration: period.to_string(),
            is_rolling: true,
        }],
        _ => Vec::new(),
    };

    let (sli, objective_op, objective_value) = export_sli(&slo.sli, &loc)?;

    Ok(SloDoc {
        api_version: API_VERSION,
        kind: SLO_KIND,
        metadata: MetadataOut {
            name: name.to_string(),
            labels,
        },
        spec: SloDocSpecOut {
            description: slo.description.clone(),
            service: service.to_string(),
            indicator: IndicatorOut {
                metadata: MetadataOut {
                    name: name.to_string(),
                    labels: BTreeMap::new(),
                },
                spec: sli,
            },
            time_window,
            budgeting_method: OCCURRENCES,
            objectives: vec![ObjectiveOut {
                op: objective_op,
                value: objective_value,
                target: objective_target(slo.objective),
            }],
        },
    })
}

/// Convert a slokit objective percent to OpenSLO's unit-fraction `target`.
///
/// Two candidates exist and neither is exact for every percent, because a
/// percent and its hundredth are rarely both representable as f64. The decimal
/// shift (`99.9` -> `0.999`) is preferred because it is what a human writes and
/// because it multiplies back to the source percent bit-for-bit for essentially
/// every real objective, including `99.9`, `99.95`, `99.99` and `99.9999`; the
/// plain division would emit `0.9990000000000001` for the first of those. The
/// division is the fallback where the shift does not round-trip (`99.999`), and
/// where neither does, the division's value ships and the documented f64
/// rounding applies.
fn objective_target(objective: f64) -> f64 {
    if let Ok(shifted) = format!("{objective}e-2").parse::<f64>() {
        if shifted * 100.0 == objective {
            return shifted;
        }
    }
    objective / 100.0
}

/// One-line summary of the alerting metadata a note says was dropped, so the
/// note names what was lost rather than only that something was.
fn describe_alerting(slo: &SloSpec) -> String {
    let a = &slo.alerting;
    let mut parts: Vec<String> = Vec::new();
    if !a.name.is_empty() {
        parts.push(format!("name '{}'", a.name));
    }
    if !a.labels.is_empty() {
        parts.push(format!("{} label(s)", a.labels.len()));
    }
    if !a.annotations.is_empty() {
        parts.push(format!("{} annotation(s)", a.annotations.len()));
    }
    if a.page_alert != super::super::AlertMeta::default() {
        parts.push("page alert metadata".to_string());
    }
    if a.ticket_alert != super::super::AlertMeta::default() {
        parts.push("ticket alert metadata".to_string());
    }
    if !a.windows.is_empty() {
        parts.push(format!("{} custom burn-rate window(s)", a.windows.len()));
    }
    if parts.is_empty() {
        "non-default alerting metadata".to_string()
    } else {
        parts.join(", ")
    }
}

/// Convert one SLI, returning the OpenSLO SLI spec plus the objective `op` and
/// `value` a threshold SLI needs (both `None` for ratio SLIs).
fn export_sli(sli: &SliSpec, loc: &str) -> Result<(SliSpecOut, Option<&'static str>, Option<f64>)> {
    let set: Vec<&str> = [
        sli.events.is_some().then_some("events"),
        sli.raw.is_some().then_some("raw"),
        sli.latency.is_some().then_some("latency"),
        sli.plugin.is_some().then_some("plugin"),
    ]
    .into_iter()
    .flatten()
    .collect();

    match set.as_slice() {
        [] => Err(err(
            loc,
            "sli sets none of events/raw/latency/plugin; there is nothing to export",
        )),
        [_, _, ..] => Err(err(
            loc,
            format!(
                "sli sets {} at once ({}); OpenSLO's indicator holds exactly one",
                set.len(),
                set.join(" + ")
            ),
        )),
        ["plugin"] => Err(err(
            loc,
            format!(
                "sli.plugin '{}' is not representable: a plugin SLI has no query until the registered plugin expands it at generation time, and OpenSLO has no plugin concept",
                sli.plugin.as_ref().map(|p| p.id.as_str()).unwrap_or("")
            ),
        )),
        ["events"] => {
            let events: &EventsSli = sli.events.as_ref().expect("events is set");
            Ok((
                SliSpecOut {
                    ratio_metric: Some(RatioMetricOut {
                        bad: Some(MetricOut::prometheus(&events.error_query)),
                        total: Some(MetricOut::prometheus(&events.total_query)),
                        raw: None,
                        raw_type: None,
                    }),
                    threshold_metric: None,
                },
                None,
                None,
            ))
        }
        ["raw"] => {
            let raw: &RawSli = sli.raw.as_ref().expect("raw is set");
            Ok((
                SliSpecOut {
                    ratio_metric: Some(RatioMetricOut {
                        bad: None,
                        total: None,
                        raw: Some(MetricOut::prometheus(&raw.error_ratio_query)),
                        // slokit's raw SLI IS the error ratio, which is
                        // OpenSLO's `failure` sense.
                        raw_type: Some("failure"),
                    }),
                    threshold_metric: None,
                },
                None,
                None,
            ))
        }
        ["latency"] => {
            let latency: &LatencySli = sli.latency.as_ref().expect("latency is set");
            let metric = latency.histogram_metric.trim();
            if !is_metric_name(metric) {
                return Err(err(
                    loc,
                    format!(
                        "sli.latency.histogram_metric '{metric}' is not a bare Prometheus metric name, so it cannot be written as an OpenSLO thresholdMetric query"
                    ),
                ));
            }
            let threshold: f64 = latency.threshold.trim().parse().map_err(|_| {
                err(
                    loc,
                    format!(
                        "sli.latency.threshold '{}' is not a number; OpenSLO's objectives[0].value is numeric",
                        latency.threshold
                    ),
                )
            })?;
            if !threshold.is_finite() || threshold <= 0.0 {
                return Err(err(
                    loc,
                    format!(
                        "sli.latency.threshold '{}' must be a positive finite number",
                        latency.threshold
                    ),
                ));
            }
            let query = match latency.selector.as_deref().map(str::trim) {
                Some(selector) if !selector.is_empty() => format!("{metric}{{{selector}}}"),
                _ => metric.to_string(),
            };
            Ok((
                SliSpecOut {
                    ratio_metric: None,
                    threshold_metric: Some(MetricOut::prometheus(&query)),
                },
                // The import treats `lte` as "good means at or below the
                // threshold", which is exactly slokit's latency model.
                Some("lte"),
                Some(threshold),
            ))
        }
        [other] => Err(err(loc, format!("unhandled sli variant '{other}'"))),
    }
}

// ---------------------------------------------------------------------------
// OpenSLO document shapes (serialization only). Field order here IS the output
// order, and every map is a BTreeMap, so the YAML is deterministic.
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SloDoc {
    api_version: &'static str,
    kind: &'static str,
    metadata: MetadataOut,
    spec: SloDocSpecOut,
}

#[derive(Debug, Serialize)]
struct MetadataOut {
    name: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SloDocSpecOut {
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    service: String,
    indicator: IndicatorOut,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    time_window: Vec<TimeWindowOut>,
    budgeting_method: &'static str,
    objectives: Vec<ObjectiveOut>,
}

impl SloDocSpecOut {
    /// Every Prometheus query this document carries, for the window-token note.
    fn queries(&self) -> Vec<&str> {
        let sli = &self.indicator.spec;
        let mut out = Vec::new();
        if let Some(ratio) = &sli.ratio_metric {
            for metric in [ratio.bad.as_ref(), ratio.total.as_ref(), ratio.raw.as_ref()]
                .into_iter()
                .flatten()
            {
                out.push(metric.metric_source.spec.query.as_str());
            }
        }
        if let Some(metric) = &sli.threshold_metric {
            out.push(metric.metric_source.spec.query.as_str());
        }
        out
    }
}

#[derive(Debug, Serialize)]
struct IndicatorOut {
    metadata: MetadataOut,
    spec: SliSpecOut,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SliSpecOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    ratio_metric: Option<RatioMetricOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold_metric: Option<MetricOut>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RatioMetricOut {
    // `counter` is deliberately not emitted: slokit does not model the
    // counter/gauge distinction, so any value here would be a guess, and the
    // import ignores the field for exactly that reason.
    #[serde(skip_serializing_if = "Option::is_none")]
    bad: Option<MetricOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<MetricOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw: Option<MetricOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_type: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricOut {
    metric_source: MetricSourceOut,
}

impl MetricOut {
    fn prometheus(query: &str) -> Self {
        MetricOut {
            metric_source: MetricSourceOut {
                type_: PROMETHEUS,
                spec: QuerySpecOut {
                    query: query.to_string(),
                },
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct MetricSourceOut {
    #[serde(rename = "type")]
    type_: &'static str,
    spec: QuerySpecOut,
}

#[derive(Debug, Serialize)]
struct QuerySpecOut {
    query: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimeWindowOut {
    duration: String,
    is_rolling: bool,
}

#[derive(Debug, Serialize)]
struct ObjectiveOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    op: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    target: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Alerting, PluginSli};

    fn events_slo(name: &str) -> SloSpec {
        SloSpec::new(
            name,
            99.9,
            SliSpec::events(EventsSli::new(
                "sum(rate(errs_total[{{.window}}]))",
                "sum(rate(reqs_total[{{.window}}]))",
            )),
        )
    }

    #[test]
    fn an_empty_spec_is_an_error_not_an_empty_document() {
        let spec = Spec::new("api", vec![]);
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("no SLOs to export"), "{message}");
    }

    #[test]
    fn a_blank_service_is_an_error() {
        let spec = Spec::new("   ", vec![events_slo("availability")]);
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("service must not be empty"), "{message}");
    }

    #[test]
    fn a_plugin_sli_fails_closed_naming_the_field() {
        let spec = Spec::new(
            "api",
            vec![SloSpec::new(
                "availability",
                99.9,
                SliSpec::plugin(PluginSli::new("my/plugin")),
            )],
        );
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("sli.plugin 'my/plugin'"), "{message}");
        assert!(message.contains("not representable"), "{message}");
    }

    #[test]
    fn two_sli_variants_at_once_are_an_error() {
        let mut slo = events_slo("availability");
        slo.sli.raw = Some(RawSli::new("r[{{.window}}]"));
        let spec = Spec::new("api", vec![slo]);
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("sets 2 at once"), "{message}");
        assert!(message.contains("events + raw"), "{message}");
    }

    #[test]
    fn an_out_of_range_objective_is_an_error() {
        let mut slo = events_slo("availability");
        slo.objective = 120.0;
        let spec = Spec::new("api", vec![slo]);
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("(0, 100]"), "{message}");
    }

    #[test]
    fn a_non_numeric_latency_threshold_is_an_error() {
        let spec = Spec::new(
            "api",
            vec![SloSpec::new(
                "latency",
                99.5,
                SliSpec::latency(LatencySli::new("http_request_duration_seconds", "fast")),
            )],
        );
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(message.contains("is not a number"), "{message}");
    }

    #[test]
    fn a_non_metric_histogram_name_is_an_error() {
        let spec = Spec::new(
            "api",
            vec![SloSpec::new(
                "latency",
                99.5,
                SliSpec::latency(LatencySli::new("sum(rate(x[5m]))", "0.3")),
            )],
        );
        let message = to_yaml_reported(&spec).unwrap_err().to_string();
        assert!(
            message.contains("not a bare Prometheus metric name"),
            "{message}"
        );
    }

    #[test]
    fn dropped_alerting_is_reported_and_names_what_was_lost() {
        let mut slo = events_slo("availability");
        let mut alerting = Alerting {
            name: "HighErrorRate".to_string(),
            ..Alerting::default()
        };
        alerting
            .annotations
            .insert("runbook".to_string(), "https://example.com".to_string());
        alerting
            .page_alert
            .labels
            .insert("severity".to_string(), "page".to_string());
        slo.alerting = alerting;

        let report = to_yaml_reported(&Spec::new("api", vec![slo])).unwrap();
        let note = report
            .notes
            .iter()
            .find(|n| n.message.contains("alerting"))
            .expect("alerting drop is reported");
        assert_eq!(note.location, "slo 'availability'");
        assert!(note.message.contains("name 'HighErrorRate'"), "{note}");
        assert!(note.message.contains("1 annotation(s)"), "{note}");
        assert!(note.message.contains("page alert metadata"), "{note}");
        assert!(!report.yaml.contains("HighErrorRate"), "{}", report.yaml);
    }

    #[test]
    fn default_alerting_produces_no_note() {
        let report = to_yaml_reported(&Spec::new("api", vec![events_slo("availability")])).unwrap();
        assert!(
            !report.notes.iter().any(|n| n.message.contains("alerting")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn service_labels_are_merged_with_slo_labels_winning() {
        let mut slo = events_slo("availability");
        slo.labels
            .insert("tier".to_string(), "critical".to_string());
        let mut spec = Spec::new("api", vec![slo]);
        spec.labels.insert("owner".to_string(), "team".to_string());
        spec.labels
            .insert("tier".to_string(), "default".to_string());

        let report = to_yaml_reported(&spec).unwrap();
        assert!(report.yaml.contains("owner: team"), "{}", report.yaml);
        assert!(report.yaml.contains("tier: critical"), "{}", report.yaml);
        assert!(!report.yaml.contains("tier: default"), "{}", report.yaml);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.message.contains("service-level label")),
            "{:?}",
            report.notes
        );
    }

    #[test]
    fn the_window_token_note_fires_only_when_a_query_carries_the_token() {
        let with_token =
            to_yaml_reported(&Spec::new("api", vec![events_slo("availability")])).unwrap();
        assert!(
            with_token
                .notes
                .iter()
                .any(|n| n.message.contains("template token")),
            "{:?}",
            with_token.notes
        );

        // A latency SLI exports a bare histogram metric, no window token.
        let latency = Spec::new(
            "api",
            vec![SloSpec::new(
                "latency",
                99.5,
                SliSpec::latency(LatencySli::new("http_request_duration_seconds", "0.3")),
            )],
        );
        let without = to_yaml_reported(&latency).unwrap();
        assert!(
            !without
                .notes
                .iter()
                .any(|n| n.message.contains("template token")),
            "{:?}",
            without.notes
        );
    }

    #[test]
    fn a_missing_period_omits_the_time_window_entirely() {
        let report = to_yaml_reported(&Spec::new("api", vec![events_slo("availability")])).unwrap();
        assert!(!report.yaml.contains("timeWindow"), "{}", report.yaml);
    }

    #[test]
    fn output_is_byte_deterministic_across_runs() {
        let mut slo = events_slo("availability");
        slo.labels.insert("b".to_string(), "2".to_string());
        slo.labels.insert("a".to_string(), "1".to_string());
        slo.period = Some("30d".to_string());
        let spec = Spec::new("api", vec![slo]);

        let first = to_yaml(&spec).unwrap();
        let second = to_yaml(&spec).unwrap();
        let third = to_yaml(&spec).unwrap();
        assert_eq!(first, second);
        assert_eq!(second, third);
    }

    #[test]
    fn the_objective_target_is_the_readable_decimal_and_still_multiplies_back() {
        // The readable candidate wins where it round-trips, which is where the
        // plain division would have emitted 0.9990000000000001.
        assert_eq!(objective_target(99.9), 0.999);
        assert_eq!(objective_target(99.99), 0.9999);
        assert_eq!(objective_target(99.5), 0.995);
        assert_eq!(objective_target(100.0), 1.0);
        // 99.999 is the case where the readable candidate does NOT multiply
        // back, so the division ships instead: less pretty, still exact.
        assert_ne!(objective_target(99.999), 0.99999);
        for percent in [90.0, 95.0, 99.0, 99.5, 99.9, 99.95, 99.99, 99.999, 99.9999] {
            assert_eq!(
                objective_target(percent) * 100.0,
                percent,
                "objective {percent} must survive the unit-fraction conversion"
            );
        }
    }

    #[test]
    fn multiple_slos_become_a_multi_document_stream() {
        let spec = Spec::new(
            "api",
            vec![events_slo("availability"), events_slo("freshness")],
        );
        let yaml = to_yaml(&spec).unwrap();
        assert_eq!(yaml.matches("kind: SLO").count(), 2, "{yaml}");
        assert_eq!(yaml.matches("\n---\n").count(), 1, "{yaml}");
    }
}
