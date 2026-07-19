//! OpenSLO v1 import: convert `kind: SLO` documents into slokit [`Spec`]s.
//!
//! [OpenSLO](https://openslo.com) is a vendor-neutral SLO specification. This
//! module maps `apiVersion: openslo/v1` documents (single documents or
//! multi-document YAML streams) onto the sloth-compatible slokit model, so
//! every downstream feature (validate, lint, generate, check, dashboard)
//! works on imported specs unchanged.
//!
//! The import is honest about fidelity: constructs slokit cannot represent at
//! all are hard errors naming the OpenSLO path (see "Errors" below), while
//! constructs that are dropped or transformed on the way in are reported as
//! lint-style [`ImportNote`]s on the returned [`Import`].
//!
//! # Mapping
//!
//! | OpenSLO construct | slokit model |
//! |-------------------|--------------|
//! | `metadata.name` | [`SloSpec::name`] (multi-objective documents produce one SLO per objective, suffixed with the objective `displayName` or 1-based index) |
//! | `metadata.labels` | [`SloSpec::labels`] (multi-value labels are joined with commas, with a note) |
//! | `spec.description` | [`SloSpec::description`] |
//! | `spec.service` | [`Spec::service`]; SLO documents in one stream that share a service merge into one [`Spec`] |
//! | `spec.timeWindow[0]` (rolling `duration`) | [`SloSpec::period`] |
//! | `spec.budgetingMethod: Occurrences` (or absent) | the slokit model itself (event-ratio error budgets) |
//! | `spec.objectives[i].target` (unit fraction) or `targetPercent` | [`SloSpec::objective`] (a percent) |
//! | `ratioMetric.bad` + `total` (Prometheus queries) | `events` SLI: `error_query` = bad, `total_query` = total |
//! | `ratioMetric.good` + `total` (Prometheus queries) | `events` SLI: `error_query` = `(total) - (good)`, with a note |
//! | `ratioMetric.raw` with `rawType: failure` | `raw` SLI (query as written) |
//! | `ratioMetric.raw` with `rawType: success` | `raw` SLI: `1 - (query)` |
//! | `thresholdMetric` whose query is a bare histogram base metric, with objective `op: lte`/`lt` and `value` | `latency` SLI: `histogram_metric` from the query, selector from its `{...}` matchers, `threshold` from `value` |
//! | `spec.indicator` (inline SLI) | converted in place |
//! | `spec.indicatorRef` | resolved against `kind: SLI` documents in the same input |
//!
//! # Window convention
//!
//! slokit templates every lookback as `[{{.window}}]` and renders the query
//! once per burn-rate window. Imported Prometheus queries that already contain
//! the token (`{{.window}}` or `{{ .window }}`) are kept as written. Otherwise
//! every fixed range selector whose content is a plain duration (`[5m]`,
//! `[1h]`, `[1h30m]`) is rewritten to `[{{.window}}]`, and an [`ImportNote`]
//! records exactly which literals were rewritten. Subquery ranges (`[1h:5m]`)
//! and brackets inside string literals are left untouched. A query with
//! neither the token nor a rewritable range selector is an error, because
//! slokit has no way to evaluate it per window.
//!
//! # Ignored with a note (lint-style)
//!
//! - `spec.alertPolicies`: slokit generates its own multi-window
//!   multi-burn-rate alerts from the objective; imported SLOs carry default
//!   (empty) alerting metadata, so `slokit lint` reports `NO_ALERT_LABELS`
//!   until routing labels are added.
//! - `metadata.annotations` (they are object metadata, not alert annotations).
//! - `timeSliceTarget` / `timeSliceWindow` (time-slice budgeting fields).
//! - Objective `op`/`value` on ratio SLIs (they only apply to thresholds).
//! - Documents of any kind other than `SLO` and `SLI` (`Service`,
//!   `DataSource`, `AlertPolicy`, ...).
//! - A missing `spec.timeWindow` (the generation-time default period applies).
//! - `ratioMetric.counter` is accepted and ignored silently: queries are used
//!   verbatim, so the counter/gauge distinction changes nothing slokit emits.
//!
//! # Errors (unrepresentable documents)
//!
//! - `apiVersion` other than `openslo/v1`.
//! - Calendar-aligned time windows (`timeWindow[0].calendar` or
//!   `isRolling: false`) and calendar duration units (`M`, `Q`, `Y`): slokit
//!   periods are fixed-length rolling windows.
//! - `budgetingMethod` other than `Occurrences`: slokit has no time-slice
//!   model.
//! - Metric sources that are not Prometheus, and `metricSourceRef` references
//!   (`DataSource` documents are not resolved).
//! - `thresholdMetric` queries that are not a bare histogram base metric
//!   (optionally with a `{...}` selector), and threshold objectives with
//!   `op: gt`/`gte`: slokit's latency SLI models "good means at or below the
//!   threshold" over a Prometheus histogram.
//! - An `indicatorRef` with no matching `kind: SLI` document in the input.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_norway::{Deserializer as YamlDeserializer, Value};

use crate::error::{Result, SlokitError};
use crate::sli::WINDOW_TOKEN;
use crate::window::Window;

use super::validate::is_metric_name;
use super::{Alerting, EventsSli, LatencySli, RawSli, SliSpec, SloSpec, Spec};

/// The only OpenSLO API version this importer understands.
const API_VERSION: &str = "openslo/v1";

/// The result of importing an OpenSLO YAML input: the converted specs plus
/// lint-style notes about constructs that were dropped or transformed.
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume,
/// and future report fields must not be breaking changes.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Import {
    /// The converted specs, one per distinct `spec.service`, in first-seen
    /// document order. Run [`validate_all`](super::validate_all) before
    /// generating rules from them.
    pub specs: Vec<Spec>,
    /// Lint-style notes: OpenSLO constructs that do not map one-to-one and
    /// were ignored or rewritten. An empty vec means a lossless import.
    pub notes: Vec<ImportNote>,
}

/// One advisory note produced during import (not an error: the document was
/// representable, but something was dropped or rewritten on the way in).
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume,
/// and future fields (for example a machine-readable code) must not be
/// breaking changes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportNote {
    /// Where the note applies, e.g. `slo 'requests-availability'`.
    pub location: String,
    /// What was ignored or rewritten, and why.
    pub message: String,
}

impl std::fmt::Display for ImportNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Cheap format detection for auto-detecting CLI input: true when the first
/// non-empty YAML document has a top-level `apiVersion` starting with
/// `openslo/`. Returns false on unparseable YAML (the caller's normal spec
/// parser will surface the real error).
pub fn is_openslo(yaml: &str) -> bool {
    for de in YamlDeserializer::from_str(yaml) {
        let Ok(value) = Value::deserialize(de) else {
            return false;
        };
        if value.is_null() {
            continue;
        }
        return value
            .get("apiVersion")
            .and_then(Value::as_str)
            .is_some_and(|v| v.starts_with("openslo/"));
    }
    false
}

/// Import OpenSLO v1 YAML (a single document or a multi-document stream) into
/// slokit [`Spec`]s. See the module docs for the exact mapping, the window
/// rewrite convention, and which constructs error versus which produce
/// [`ImportNote`]s.
pub fn from_yaml(yaml: &str) -> Result<Import> {
    let mut notes: Vec<ImportNote> = Vec::new();
    let mut docs: Vec<(usize, Envelope)> = Vec::new();

    for (idx, de) in YamlDeserializer::from_str(yaml).enumerate() {
        let n = idx + 1;
        let value = Value::deserialize(de)
            .map_err(|e| SlokitError::Spec(format!("openslo document {n}: {e}")))?;
        if value.is_null() {
            continue;
        }
        let env: Envelope = serde_norway::from_value(value)
            .map_err(|e| SlokitError::Spec(format!("openslo document {n}: {e}")))?;
        if env.api_version != API_VERSION {
            return Err(SlokitError::Spec(format!(
                "openslo document {n}: unsupported apiVersion '{}' (expected {API_VERSION})",
                env.api_version
            )));
        }
        docs.push((n, env));
    }

    if docs.is_empty() {
        return Err(SlokitError::Spec(
            "openslo: input contains no YAML documents".to_string(),
        ));
    }

    // Index kind: SLI documents so SLO documents can resolve indicatorRef.
    let mut slis: BTreeMap<String, SliSpecDoc> = BTreeMap::new();
    for (n, env) in &docs {
        if env.kind != "SLI" {
            continue;
        }
        let name = env.metadata.name.trim();
        if name.is_empty() {
            notes.push(ImportNote {
                location: format!("document {n}"),
                message:
                    "kind: SLI document without metadata.name cannot be referenced and was ignored"
                        .to_string(),
            });
            continue;
        }
        let sli: SliSpecDoc = serde_norway::from_value(env.spec.clone())
            .map_err(|e| err(&format!("sli '{name}'"), format!("spec: {e}")))?;
        if slis.insert(name.to_string(), sli).is_some() {
            return Err(err(
                &format!("document {n}"),
                format!("duplicate kind: SLI document name '{name}'; indicatorRef resolution would be ambiguous"),
            ));
        }
    }

    let mut specs: Vec<Spec> = Vec::new();
    let mut saw_slo = false;
    for (n, env) in &docs {
        match env.kind.as_str() {
            "SLO" => {
                saw_slo = true;
                let (service, slos) = convert_slo(*n, env, &slis, &mut notes)?;
                match specs.iter_mut().find(|s| s.service == service) {
                    Some(spec) => spec.slos.extend(slos),
                    None => specs.push(Spec {
                        version: super::default_version(),
                        service,
                        labels: BTreeMap::new(),
                        slos,
                    }),
                }
            }
            // Already indexed above.
            "SLI" => {}
            "" => {
                return Err(err(&format!("document {n}"), "`kind` is missing"));
            }
            other => notes.push(ImportNote {
                location: format!("document {n}"),
                message: format!(
                    "kind '{other}' does not map to the slokit model and was ignored (only SLO and referenced SLI documents import)"
                ),
            }),
        }
    }

    if !saw_slo {
        return Err(SlokitError::Spec(
            "openslo: no kind: SLO documents in input (nothing to import)".to_string(),
        ));
    }

    Ok(Import { specs, notes })
}

/// Read and import OpenSLO v1 YAML from a file. See [`from_yaml`].
pub fn from_path(path: impl AsRef<Path>) -> Result<Import> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path)
        .map_err(|e| SlokitError::Spec(format!("reading {}: {e}", path.display())))?;
    from_yaml(&contents)
}

/// Build an import error whose message names the OpenSLO location and path.
fn err(location: &str, message: impl AsRef<str>) -> SlokitError {
    SlokitError::Spec(format!("openslo {location}: {}", message.as_ref()))
}

fn note(notes: &mut Vec<ImportNote>, location: &str, message: impl Into<String>) {
    notes.push(ImportNote {
        location: location.to_string(),
        message: message.into(),
    });
}

// ---------------------------------------------------------------------------
// Raw OpenSLO document shapes (deserialization only).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    #[serde(default)]
    api_version: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    spec: Value,
}

#[derive(Debug, Default, Deserialize)]
struct Metadata {
    #[serde(default)]
    name: String,
    #[serde(default)]
    labels: BTreeMap<String, LabelValue>,
    #[serde(default)]
    annotations: BTreeMap<String, Value>,
}

/// OpenSLO label values may be a single string or a list of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LabelValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SloDocSpec {
    #[serde(default)]
    description: String,
    #[serde(default)]
    service: String,
    indicator: Option<IndicatorDoc>,
    indicator_ref: Option<String>,
    #[serde(default)]
    time_window: Vec<TimeWindowDoc>,
    budgeting_method: Option<String>,
    #[serde(default)]
    objectives: Vec<ObjectiveDoc>,
    #[serde(default)]
    alert_policies: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct IndicatorDoc {
    spec: SliSpecDoc,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SliSpecDoc {
    ratio_metric: Option<RatioMetricDoc>,
    threshold_metric: Option<MetricDoc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RatioMetricDoc {
    /// The counter/gauge distinction changes nothing slokit emits (queries
    /// are used verbatim), so this field is accepted and ignored.
    #[serde(default, rename = "counter")]
    _counter: Option<bool>,
    good: Option<MetricDoc>,
    bad: Option<MetricDoc>,
    total: Option<MetricDoc>,
    raw: Option<MetricDoc>,
    raw_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricDoc {
    metric_source: MetricSourceDoc,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricSourceDoc {
    metric_source_ref: Option<String>,
    #[serde(default, rename = "type")]
    type_: String,
    #[serde(default)]
    spec: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeWindowDoc {
    duration: Option<String>,
    is_rolling: Option<bool>,
    calendar: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectiveDoc {
    #[serde(default)]
    display_name: String,
    op: Option<String>,
    value: Option<f64>,
    target: Option<f64>,
    target_percent: Option<f64>,
    time_slice_target: Option<f64>,
    time_slice_window: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversion.
// ---------------------------------------------------------------------------

fn convert_slo(
    doc_no: usize,
    env: &Envelope,
    slis: &BTreeMap<String, SliSpecDoc>,
    notes: &mut Vec<ImportNote>,
) -> Result<(String, Vec<SloSpec>)> {
    let name = env.metadata.name.trim();
    if name.is_empty() {
        return Err(err(
            &format!("document {doc_no}"),
            "metadata.name must not be empty",
        ));
    }
    let loc = format!("slo '{name}'");

    let doc: SloDocSpec =
        serde_norway::from_value(env.spec.clone()).map_err(|e| err(&loc, format!("spec: {e}")))?;

    let service = doc.service.trim();
    if service.is_empty() {
        return Err(err(
            &loc,
            "spec.service must not be empty (slokit groups SLOs by service)",
        ));
    }

    if let Some(method) = doc.budgeting_method.as_deref() {
        if method != "Occurrences" {
            return Err(err(
                &loc,
                format!(
                    "spec.budgetingMethod '{method}' is not representable; slokit models the Occurrences method only"
                ),
            ));
        }
    }

    let period = convert_time_window(&doc.time_window, &loc, notes)?;

    let (sli_doc, sli_path) = match (&doc.indicator, &doc.indicator_ref) {
        (Some(_), Some(_)) => {
            return Err(err(
                &loc,
                "spec sets both indicator and indicatorRef; use exactly one",
            ));
        }
        (Some(ind), None) => (&ind.spec, "spec.indicator.spec".to_string()),
        (None, Some(r)) => match slis.get(r.as_str()) {
            Some(sli) => (sli, format!("sli '{r}' spec")),
            None => {
                return Err(err(
                    &loc,
                    format!(
                        "spec.indicatorRef '{r}': no kind: SLI document named '{r}' in this input (external SLI references cannot be resolved)"
                    ),
                ));
            }
        },
        (None, None) => {
            return Err(err(
                &loc,
                "spec.indicator (an inline SLI) or spec.indicatorRef is required",
            ));
        }
    };

    if doc.objectives.is_empty() {
        return Err(err(
            &loc,
            "spec.objectives must contain at least one objective",
        ));
    }
    if !doc.alert_policies.is_empty() {
        note(
            notes,
            &loc,
            "spec.alertPolicies do not map; slokit generates multi-window multi-burn-rate alerts from the objective instead",
        );
    }
    if !env.metadata.annotations.is_empty() {
        note(
            notes,
            &loc,
            "metadata.annotations do not map and were ignored",
        );
    }
    let labels = convert_labels(&env.metadata, &loc, notes);

    let multi = doc.objectives.len() > 1;
    let mut out = Vec::with_capacity(doc.objectives.len());
    for (i, obj) in doc.objectives.iter().enumerate() {
        let opath = format!("spec.objectives[{i}]");
        let objective = objective_percent(obj, &loc, &opath)?;
        if obj.time_slice_target.is_some() || obj.time_slice_window.is_some() {
            note(
                notes,
                &loc,
                format!(
                    "{opath}: timeSliceTarget/timeSliceWindow only apply to time-slice budgeting methods and were ignored"
                ),
            );
        }
        let slo_name = if multi {
            format!("{name}-{}", objective_suffix(obj, i))
        } else {
            name.to_string()
        };
        let sli = convert_sli(sli_doc, obj, &loc, &opath, &sli_path, notes)?;
        out.push(SloSpec {
            name: slo_name,
            objective,
            description: doc.description.clone(),
            labels: labels.clone(),
            sli,
            alerting: Alerting::default(),
            period: period.clone(),
        });
    }

    Ok((service.to_string(), out))
}

fn convert_labels(
    meta: &Metadata,
    loc: &str,
    notes: &mut Vec<ImportNote>,
) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for (key, value) in &meta.labels {
        let rendered = match value {
            LabelValue::One(s) => s.clone(),
            LabelValue::Many(values) => {
                if values.len() > 1 {
                    note(
                        notes,
                        loc,
                        format!(
                            "metadata.labels['{key}'] has {} values; they were joined with commas (Prometheus labels are single-valued)",
                            values.len()
                        ),
                    );
                }
                values.join(",")
            }
        };
        labels.insert(key.clone(), rendered);
    }
    labels
}

fn convert_time_window(
    windows: &[TimeWindowDoc],
    loc: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<Option<String>> {
    match windows {
        [] => {
            note(
                notes,
                loc,
                "spec.timeWindow is missing; the generation-time default period applies",
            );
            Ok(None)
        }
        [tw] => {
            if tw.calendar.is_some() {
                return Err(err(
                    loc,
                    "spec.timeWindow[0].calendar: calendar-aligned time windows are not representable; slokit periods are rolling windows",
                ));
            }
            if tw.is_rolling == Some(false) {
                return Err(err(
                    loc,
                    "spec.timeWindow[0].isRolling: false is not representable; slokit periods are rolling windows",
                ));
            }
            let duration = tw
                .duration
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .ok_or_else(|| err(loc, "spec.timeWindow[0].duration is required"))?;
            if let Some(unit) = duration.chars().find(|c| matches!(c, 'M' | 'Q' | 'Y')) {
                return Err(err(
                    loc,
                    format!(
                        "spec.timeWindow[0].duration '{duration}': calendar unit '{unit}' (months/quarters/years) is not representable as a fixed-length Prometheus window"
                    ),
                ));
            }
            Window::parse(duration)
                .map_err(|e| err(loc, format!("spec.timeWindow[0].duration: {e}")))?;
            Ok(Some(duration.to_string()))
        }
        many => Err(err(
            loc,
            format!(
                "spec.timeWindow has {} entries; OpenSLO v1 allows one and slokit maps exactly one",
                many.len()
            ),
        )),
    }
}

fn objective_percent(obj: &ObjectiveDoc, loc: &str, opath: &str) -> Result<f64> {
    match (obj.target, obj.target_percent) {
        (Some(_), Some(_)) => Err(err(
            loc,
            format!("{opath}: set either target or targetPercent, not both"),
        )),
        (Some(t), None) => {
            if !t.is_finite() || !(0.0..=1.0).contains(&t) {
                return Err(err(
                    loc,
                    format!(
                        "{opath}.target {t} must be a unit fraction between 0 and 1 (use targetPercent for percent values)"
                    ),
                ));
            }
            Ok(t * 100.0)
        }
        (None, Some(p)) => Ok(p),
        (None, None) => Err(err(
            loc,
            format!("{opath}: target or targetPercent is required"),
        )),
    }
}

/// The per-objective SLO-name suffix for multi-objective documents: the
/// slugified `displayName`, or the 1-based objective index when there is none.
fn objective_suffix(obj: &ObjectiveDoc, index: usize) -> String {
    let slug = slugify(&obj.display_name);
    if slug.is_empty() {
        (index + 1).to_string()
    } else {
        slug
    }
}

fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_dash = false;
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '_' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    out
}

fn convert_sli(
    sli: &SliSpecDoc,
    obj: &ObjectiveDoc,
    loc: &str,
    opath: &str,
    sli_path: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<SliSpec> {
    match (&sli.ratio_metric, &sli.threshold_metric) {
        (Some(_), Some(_)) => Err(err(
            loc,
            format!(
                "{sli_path} sets both ratioMetric and thresholdMetric; exactly one is required"
            ),
        )),
        (None, None) => Err(err(
            loc,
            format!("{sli_path} needs ratioMetric or thresholdMetric"),
        )),
        (Some(ratio), None) => {
            if obj.op.is_some() || obj.value.is_some() {
                note(
                    notes,
                    loc,
                    format!("{opath}: op/value apply to thresholdMetric SLIs and were ignored for this ratioMetric"),
                );
            }
            convert_ratio(ratio, loc, &format!("{sli_path}.ratioMetric"), notes)
        }
        (None, Some(threshold)) => convert_threshold(
            threshold,
            obj,
            loc,
            opath,
            &format!("{sli_path}.thresholdMetric"),
            notes,
        ),
    }
}

fn convert_ratio(
    ratio: &RatioMetricDoc,
    loc: &str,
    path: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<SliSpec> {
    if ratio.raw.is_some() && (ratio.good.is_some() || ratio.bad.is_some() || ratio.total.is_some())
    {
        return Err(err(
            loc,
            format!("{path}: `raw` cannot be combined with good/bad/total"),
        ));
    }

    if let Some(raw) = &ratio.raw {
        let query = metric_query(raw, loc, &format!("{path}.raw"))?;
        let query = windowize(&query, loc, &format!("{path}.raw"), notes)?;
        let error_ratio_query = match ratio.raw_type.as_deref() {
            Some("failure") => query,
            Some("success") => format!("1 - ({query})"),
            Some(other) => {
                return Err(err(
                    loc,
                    format!(
                        "{path}.rawType '{other}' is not supported (expected success or failure)"
                    ),
                ));
            }
            None => {
                return Err(err(
                    loc,
                    format!("{path}.rawType is required alongside `raw` (success or failure)"),
                ));
            }
        };
        return Ok(SliSpec {
            raw: Some(RawSli { error_ratio_query }),
            ..SliSpec::default()
        });
    }

    if ratio.good.is_some() && ratio.bad.is_some() {
        return Err(err(
            loc,
            format!("{path}: set either `good` or `bad`, not both"),
        ));
    }
    let total = ratio
        .total
        .as_ref()
        .ok_or_else(|| err(loc, format!("{path}.total is required with good/bad")))?;
    let total_query = metric_query(total, loc, &format!("{path}.total"))?;
    let total_query = windowize(&total_query, loc, &format!("{path}.total"), notes)?;

    let error_query = if let Some(bad) = &ratio.bad {
        let bad_query = metric_query(bad, loc, &format!("{path}.bad"))?;
        windowize(&bad_query, loc, &format!("{path}.bad"), notes)?
    } else if let Some(good) = &ratio.good {
        let good_query = metric_query(good, loc, &format!("{path}.good"))?;
        let good_query = windowize(&good_query, loc, &format!("{path}.good"), notes)?;
        note(
            notes,
            loc,
            format!("{path}.good: derived the error query as total minus good"),
        );
        format!("({total_query}) - ({good_query})")
    } else {
        return Err(err(loc, format!("{path} needs `good`, `bad`, or `raw`")));
    };

    Ok(SliSpec {
        events: Some(EventsSli {
            error_query,
            total_query,
        }),
        ..SliSpec::default()
    })
}

fn convert_threshold(
    threshold: &MetricDoc,
    obj: &ObjectiveDoc,
    loc: &str,
    opath: &str,
    path: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<SliSpec> {
    let query = metric_query(threshold, loc, path)?;
    let (histogram_metric, selector) = parse_bare_histogram(&query, loc, path)?;
    for suffix in ["_bucket", "_count", "_sum"] {
        if histogram_metric.ends_with(suffix) {
            note(
                notes,
                loc,
                format!(
                    "{path}: metric '{histogram_metric}' ends with '{suffix}'; slokit appends _bucket/_count to the base histogram metric, so this is probably not the base name"
                ),
            );
        }
    }

    match obj.op.as_deref() {
        Some("lte") => {}
        Some("lt") => note(
            notes,
            loc,
            format!("{opath}.op 'lt' is treated as 'lte': Prometheus histogram buckets are cumulative with inclusive upper bounds"),
        ),
        Some(op @ ("gte" | "gt")) => {
            return Err(err(
                loc,
                format!(
                    "{opath}.op '{op}' is not representable; slokit's latency SLI models good events as at or below the threshold (lte)"
                ),
            ));
        }
        Some(other) => {
            return Err(err(
                loc,
                format!("{opath}.op '{other}' is not a known OpenSLO operator (lte, lt, gte, gt)"),
            ));
        }
        None => {
            return Err(err(
                loc,
                format!("{opath}.op is required for thresholdMetric SLIs"),
            ));
        }
    }

    let value = obj.value.ok_or_else(|| {
        err(
            loc,
            format!("{opath}.value is required for thresholdMetric SLIs"),
        )
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(err(
            loc,
            format!("{opath}.value must be a positive number, got {value}"),
        ));
    }

    Ok(SliSpec {
        latency: Some(LatencySli {
            histogram_metric,
            // Rendered with Rust's shortest f64 display (0.3 -> "0.3",
            // 1.0 -> "1"); the histogram must expose that exact `le` bucket.
            threshold: format!("{value}"),
            selector,
        }),
        ..SliSpec::default()
    })
}

/// Extract the query string from a Prometheus metric source, rejecting
/// non-Prometheus sources and unresolvable `metricSourceRef` references.
fn metric_query(metric: &MetricDoc, loc: &str, path: &str) -> Result<String> {
    let source = &metric.metric_source;
    if source.type_.is_empty() {
        if let Some(r) = &source.metric_source_ref {
            return Err(err(
                loc,
                format!(
                    "{path}.metricSource.metricSourceRef '{r}': DataSource references are not resolved; inline `type: Prometheus` with `spec.query`"
                ),
            ));
        }
        return Err(err(
            loc,
            format!("{path}.metricSource.type is missing (expected Prometheus)"),
        ));
    }
    if !source.type_.eq_ignore_ascii_case("prometheus") {
        return Err(err(
            loc,
            format!(
                "{path}.metricSource.type '{}' is not supported; slokit generates Prometheus rules, so only Prometheus metric sources map",
                source.type_
            ),
        ));
    }
    let query = source
        .spec
        .get("query")
        .ok_or_else(|| err(loc, format!("{path}.metricSource.spec.query is missing")))?;
    let query = query.as_str().ok_or_else(|| {
        err(
            loc,
            format!("{path}.metricSource.spec.query must be a string"),
        )
    })?;
    let query = query.trim();
    if query.is_empty() {
        return Err(err(
            loc,
            format!("{path}.metricSource.spec.query must not be empty"),
        ));
    }
    Ok(query.to_string())
}

/// Apply the window convention (see the module docs): keep queries that
/// already carry the `{{.window}}` token, otherwise rewrite every fixed range
/// selector (`[5m]`) to `[{{.window}}]` with a note, and error when there is
/// nothing to rewrite.
fn windowize(query: &str, loc: &str, path: &str, notes: &mut Vec<ImportNote>) -> Result<String> {
    if query.contains(WINDOW_TOKEN) || query.contains("{{ .window }}") {
        return Ok(query.to_string());
    }

    let chars: Vec<char> = query.chars().collect();
    let mut out = String::with_capacity(query.len());
    let mut rewritten: Vec<String> = Vec::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if let Some(quote) = in_quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                in_quote = Some(c);
                out.push(c);
                i += 1;
            }
            '[' => {
                // A fixed range selector: `[` + a plain duration + `]`.
                let close = chars[i + 1..].iter().position(|&x| x == ']' || x == '[');
                match close {
                    Some(offset) if chars[i + 1 + offset] == ']' => {
                        let content: String = chars[i + 1..i + 1 + offset].iter().collect();
                        if is_plain_duration(&content) {
                            out.push('[');
                            out.push_str(WINDOW_TOKEN);
                            out.push(']');
                            let literal = format!("[{content}]");
                            if !rewritten.contains(&literal) {
                                rewritten.push(literal);
                            }
                            i += offset + 2;
                        } else {
                            out.push('[');
                            i += 1;
                        }
                    }
                    _ => {
                        out.push('[');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    if rewritten.is_empty() {
        return Err(err(
            loc,
            format!(
                "{path}: query has no {WINDOW_TOKEN} token and no fixed range selector (like [5m]) to rewrite, so slokit cannot evaluate it per window: {query}"
            ),
        ));
    }
    note(
        notes,
        loc,
        format!(
            "{path}: rewrote fixed range window(s) {} to [{WINDOW_TOKEN}]; every rewritten lookback now follows the generated rule's window",
            rewritten.join(", ")
        ),
    );
    Ok(out)
}

/// True when `s` is a plain Prometheus duration (`5m`, `1h30m`): rewriting it
/// to the window token is safe. Subquery ranges (`1h:5m`) contain `:` and
/// never match.
fn is_plain_duration(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 's' | 'm' | 'h' | 'd' | 'w'))
        && Window::parse(s).is_ok()
}

/// Split a thresholdMetric query into a histogram base metric plus optional
/// selector, per the documented convention: only `metric` or
/// `metric{matchers}` shapes are representable.
fn parse_bare_histogram(query: &str, loc: &str, path: &str) -> Result<(String, Option<String>)> {
    let unrepresentable = || {
        err(
            loc,
            format!(
                "{path}: query '{query}' is not representable; slokit maps thresholdMetric only when the query is a bare Prometheus histogram base metric (without the _bucket/_count suffix), optionally with a selector, e.g. `http_request_duration_seconds{{job=\"api\"}}`"
            ),
        )
    };
    let s = query.trim();
    let (metric, selector) = match s.find('{') {
        Some(idx) => {
            if !s.ends_with('}') {
                return Err(unrepresentable());
            }
            let inner = s[idx + 1..s.len() - 1].trim();
            let selector = if inner.is_empty() {
                None
            } else {
                Some(inner.to_string())
            };
            (&s[..idx], selector)
        }
        None => (s, None),
    };
    if !is_metric_name(metric) {
        return Err(unrepresentable());
    }
    Ok((metric.to_string(), selector))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_flattens_display_names() {
        assert_eq!(slugify("Fast requests (p99)"), "fast-requests-p99");
        assert_eq!(slugify("  fast  "), "fast");
        assert_eq!(slugify("!!!"), "");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn plain_durations_are_recognized() {
        assert!(is_plain_duration("5m"));
        assert!(is_plain_duration("1h30m"));
        assert!(!is_plain_duration("1h:5m"));
        assert!(!is_plain_duration(""));
        assert!(!is_plain_duration("code"));
        assert!(!is_plain_duration("5x"));
    }

    #[test]
    fn windowize_rewrites_fixed_ranges_and_notes() {
        let mut notes = Vec::new();
        let out = windowize(
            "sum(rate(errs[5m])) / sum(rate(reqs[5m]))",
            "slo 'a'",
            "p",
            &mut notes,
        )
        .unwrap();
        assert_eq!(
            out,
            "sum(rate(errs[{{.window}}])) / sum(rate(reqs[{{.window}}]))"
        );
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains("[5m]"), "{}", notes[0].message);
    }

    #[test]
    fn windowize_keeps_existing_tokens_without_notes() {
        let mut notes = Vec::new();
        let q = "sum(rate(errs[{{.window}}]))";
        assert_eq!(windowize(q, "l", "p", &mut notes).unwrap(), q);
        assert!(notes.is_empty());
    }

    #[test]
    fn windowize_skips_strings_and_subqueries() {
        let mut notes = Vec::new();
        let out = windowize(
            "max_over_time(up{note=\"[5m]\"}[1h:5m]) and rate(x[5m])",
            "l",
            "p",
            &mut notes,
        )
        .unwrap();
        assert_eq!(
            out,
            "max_over_time(up{note=\"[5m]\"}[1h:5m]) and rate(x[{{.window}}])"
        );
    }

    #[test]
    fn windowize_errors_when_nothing_can_be_rewritten() {
        let mut notes = Vec::new();
        let msg = windowize("sum(app_errors_ratio)", "slo 'a'", "spec.q", &mut notes)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("no fixed range selector"), "{msg}");
        assert!(msg.contains("spec.q"), "{msg}");
    }

    #[test]
    fn bare_histogram_queries_split_into_metric_and_selector() {
        let (m, sel) = parse_bare_histogram("http_seconds", "l", "p").unwrap();
        assert_eq!(m, "http_seconds");
        assert_eq!(sel, None);

        let (m, sel) = parse_bare_histogram("http_seconds{job=\"api\"}", "l", "p").unwrap();
        assert_eq!(m, "http_seconds");
        assert_eq!(sel.as_deref(), Some("job=\"api\""));

        let (m, sel) = parse_bare_histogram("http_seconds{}", "l", "p").unwrap();
        assert_eq!(m, "http_seconds");
        assert_eq!(sel, None);
    }

    #[test]
    fn non_bare_threshold_queries_are_rejected() {
        for q in [
            "histogram_quantile(0.99, rate(http_seconds_bucket[5m]))",
            "http seconds",
            "http_seconds{job=\"api\"} > 1",
        ] {
            let msg = parse_bare_histogram(q, "l", "p").unwrap_err().to_string();
            assert!(msg.contains("not representable"), "{q}: {msg}");
        }
    }
}
