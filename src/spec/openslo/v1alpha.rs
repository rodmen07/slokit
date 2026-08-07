//! OpenSLO **v1alpha** import: the predecessor schema, still what the sloth
//! reference examples declare.
//!
//! [`super::from_yaml`] dispatches per document, so this module only owns the
//! shape difference. Everything downstream of the shape — the `{{.window}}`
//! rewriting ([`windowize`](super::windowize)), the `(total) - (good)`
//! derivation ([`error_query_from_good`](super::error_query_from_good)), the
//! histogram/threshold convention
//! ([`latency_from_threshold_query`](super::latency_from_threshold_query)) and
//! the multi-objective SLO naming ([`objective_suffix`](super::objective_suffix))
//! — is the v1 code, reused rather than re-implemented.
//!
//! # How v1alpha differs from v1
//!
//! | construct | `openslo/v1alpha` | `openslo/v1` |
//! |-----------|-------------------|--------------|
//! | the metric | `spec.objectives[i].ratioMetrics.{good,total}`, one per objective | one `spec.indicator` (or `spec.indicatorRef`) for the document |
//! | a metric source | flat `source` / `queryType` / `query` | nested `metricSource.{type, spec.query}` |
//! | the period | `spec.timeWindows[0].{count, unit}` (`count: 30`, `unit: Day`) | `spec.timeWindow[0].duration` (`30d`) |
//! | the threshold SLI | `spec.indicator.thresholdMetric` (document level) | `spec.indicator.spec.thresholdMetric` |
//! | the target | `spec.objectives[i].target` only | `target` or `targetPercent` |
//! | object metadata | `metadata.displayName` | `metadata.labels` / `metadata.annotations` |
//!
//! v1alpha has no `ratioMetric.bad`, no `raw`/`rawType`, no `indicatorRef` and
//! no standalone `kind: SLI` documents, so those v1 paths simply do not exist
//! here.
//!
//! # Mapping
//!
//! | OpenSLO v1alpha construct | slokit model |
//! |---------------------------|--------------|
//! | `metadata.name` | [`SloSpec::name`](super::super::SloSpec::name) (multi-objective documents produce one SLO per objective, suffixed with the objective `displayName` or 1-based index) |
//! | `spec.description` | [`SloSpec::description`](super::super::SloSpec::description) |
//! | `spec.service` | [`Spec::service`](super::super::Spec::service); SLO documents in one stream that share a service merge into one spec |
//! | `spec.timeWindows[0].{count, unit}` | [`SloSpec::period`](super::super::SloSpec::period), as `<count><s\|m\|h\|d\|w>` |
//! | `spec.budgetingMethod: Occurrences` (or absent) | the slokit model itself (event-ratio error budgets) |
//! | `spec.objectives[i].target` (unit fraction) | [`SloSpec::objective`](super::super::SloSpec::objective) (a percent) |
//! | `spec.objectives[i].ratioMetrics.{good,total}` | `events` SLI: `error_query` = `(total) - (good)`, with a note |
//! | `spec.indicator.thresholdMetric` + objective `op: lte`/`lt` and `value` | `latency` SLI, per the v1 histogram convention |
//!
//! # Ignored with a note (lint-style)
//!
//! - `metadata.displayName` (slokit SLOs have a name and a description, no
//!   separate display name).
//! - `metadata.labels`, which are not part of the v1alpha schema at all: they
//!   are reported rather than silently honored, so a document written against
//!   the wrong version does not quietly gain labels.
//! - `spec.objectives[i].timeSliceTarget` (a time-slice budgeting field).
//! - Objective `op`/`value` on `ratioMetrics` objectives (they only apply to
//!   thresholds).
//! - A missing `spec.timeWindows` (the generation-time default period applies).
//! - `ratioMetrics.counter`, accepted and ignored silently: queries are used
//!   verbatim, so the counter/gauge distinction changes nothing slokit emits.
//! - Documents of any kind other than `SLO`.
//!
//! # Errors (unrepresentable documents)
//!
//! Fail closed, the same contract the v1 importer and the OpenSLO export
//! follow: a construct with no slokit representation is an error naming the
//! offending field, never a silent drop.
//!
//! - Calendar-aligned windows (`timeWindows[0].calendar`, `isRolling: false`)
//!   and calendar units (`Month`, `Quarter`, `Year`), because slokit periods
//!   are fixed-length rolling windows.
//! - More than one `spec.timeWindows` entry, a missing or zero `count`, or an
//!   unknown `unit`.
//! - `budgetingMethod` other than `Occurrences`.
//! - A metric `source` that is not Prometheus, or a `queryType` that is not
//!   PromQL.
//! - An objective with both `ratioMetrics` and a document-level
//!   `spec.indicator`, or with neither.
//! - `ratioMetrics` missing `good` or `total`.
//! - A missing `target`, or one outside the unit interval.
//! - Everything the shared v1 helpers already reject: a query with no window
//!   to rewrite, a non-bare `thresholdMetric` query, and threshold objectives
//!   with `op: gt`/`gte`.

use serde::Deserialize;
use serde_norway::Value;

use crate::error::Result;
use crate::window::Window;

use super::super::{Alerting, EventsSli, SliSpec, SloSpec};
use super::{
    err, error_query_from_good, latency_from_threshold_query, note, objective_suffix, windowize,
    Envelope, ImportNote,
};

/// Convert one `apiVersion: openslo/v1alpha`, `kind: SLO` document into its
/// service name plus one [`SloSpec`] per objective.
pub(super) fn convert_slo(
    doc_no: usize,
    env: &Envelope,
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

    if !env.metadata.display_name.trim().is_empty() {
        note(
            notes,
            &loc,
            "metadata.displayName does not map and was ignored (slokit SLOs carry a name and a description)",
        );
    }
    if !env.metadata.labels.is_empty() {
        note(
            notes,
            &loc,
            "metadata.labels are not part of openslo/v1alpha and were ignored; move them to an openslo/v1 document to import them as SLO labels",
        );
    }

    let period = convert_time_windows(&doc.time_windows, &loc, notes)?;

    if doc.objectives.is_empty() {
        return Err(err(
            &loc,
            "spec.objectives must contain at least one objective",
        ));
    }

    let multi = doc.objectives.len() > 1;
    let mut out = Vec::with_capacity(doc.objectives.len());
    for (i, obj) in doc.objectives.iter().enumerate() {
        let opath = format!("spec.objectives[{i}]");
        let objective = objective_percent(obj, &loc, &opath)?;
        if obj.time_slice_target.is_some() {
            note(
                notes,
                &loc,
                format!(
                    "{opath}.timeSliceTarget only applies to the Timeslices budgeting method and was ignored"
                ),
            );
        }
        let slo_name = if multi {
            format!("{name}-{}", objective_suffix(&obj.display_name, i))
        } else {
            name.to_string()
        };
        let sli = convert_sli(obj, doc.indicator.as_ref(), &loc, &opath, notes)?;
        out.push(SloSpec {
            name: slo_name,
            objective,
            description: doc.description.clone(),
            labels: Default::default(),
            sli,
            alerting: Alerting::default(),
            period: period.clone(),
        });
    }

    Ok((service.to_string(), out))
}

/// Pick the objective's SLI: its own `ratioMetrics`, or the document-level
/// `spec.indicator.thresholdMetric`. Exactly one must be reachable.
fn convert_sli(
    obj: &ObjectiveDoc,
    indicator: Option<&IndicatorDoc>,
    loc: &str,
    opath: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<SliSpec> {
    match (&obj.ratio_metrics, indicator) {
        (Some(_), Some(_)) => Err(err(
            loc,
            format!(
                "{opath}.ratioMetrics is set alongside a document-level spec.indicator.thresholdMetric; exactly one is required"
            ),
        )),
        (None, None) => Err(err(
            loc,
            format!("{opath} needs ratioMetrics, or the document needs spec.indicator.thresholdMetric"),
        )),
        (Some(ratio), None) => {
            if obj.op.is_some() || obj.value.is_some() {
                note(
                    notes,
                    loc,
                    format!(
                        "{opath}: op/value apply to thresholdMetric SLIs and were ignored for this ratioMetrics objective"
                    ),
                );
            }
            convert_ratio_metrics(ratio, loc, &format!("{opath}.ratioMetrics"), notes)
        }
        (None, Some(ind)) => {
            let path = "spec.indicator.thresholdMetric";
            let threshold = ind.threshold_metric.as_ref().ok_or_else(|| {
                err(loc, format!("{path} is required when spec.indicator is set"))
            })?;
            let query = metric_query(threshold, loc, path)?;
            latency_from_threshold_query(
                &query,
                obj.op.as_deref(),
                obj.value,
                loc,
                opath,
                path,
                notes,
            )
        }
    }
}

/// Map `ratioMetrics.{good,total}` onto the events SLI. v1alpha has no `bad`
/// and no `raw`, so the good/total derivation is the only shape here.
fn convert_ratio_metrics(
    ratio: &RatioMetricsDoc,
    loc: &str,
    path: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<SliSpec> {
    let good = ratio
        .good
        .as_ref()
        .ok_or_else(|| err(loc, format!("{path}.good is required")))?;
    let total = ratio
        .total
        .as_ref()
        .ok_or_else(|| err(loc, format!("{path}.total is required")))?;

    let total_query = metric_query(total, loc, &format!("{path}.total"))?;
    let total_query = windowize(&total_query, loc, &format!("{path}.total"), notes)?;
    let good_query = metric_query(good, loc, &format!("{path}.good"))?;
    let good_query = windowize(&good_query, loc, &format!("{path}.good"), notes)?;

    let error_query = error_query_from_good(&good_query, &total_query, loc, path, notes);

    Ok(SliSpec {
        events: Some(EventsSli {
            error_query,
            total_query,
        }),
        ..SliSpec::default()
    })
}

/// Extract the query from a v1alpha metric source, rejecting non-Prometheus
/// sources and non-PromQL query types. The v1 twin of this is
/// [`metric_query`](super::metric_query); the versions differ only in where
/// the three fields live.
fn metric_query(metric: &MetricSourceDoc, loc: &str, path: &str) -> Result<String> {
    let source = metric.source.trim();
    if source.is_empty() {
        return Err(err(
            loc,
            format!("{path}.source is missing (expected prometheus)"),
        ));
    }
    if !source.eq_ignore_ascii_case("prometheus") {
        return Err(err(
            loc,
            format!(
                "{path}.source '{source}' is not supported; slokit generates Prometheus rules, so only Prometheus metric sources map"
            ),
        ));
    }
    let query_type = metric.query_type.trim();
    if !query_type.is_empty() && !query_type.eq_ignore_ascii_case("promql") {
        return Err(err(
            loc,
            format!(
                "{path}.queryType '{query_type}' is not supported; slokit emits PromQL, so only promql maps"
            ),
        ));
    }
    let query = metric.query.trim();
    if query.is_empty() {
        return Err(err(loc, format!("{path}.query must not be empty")));
    }
    Ok(query.to_string())
}

/// Map `spec.timeWindows[0]` onto a slokit period string. The v1 twin reads a
/// duration (`30d`); v1alpha states the same thing as a count plus a unit
/// name, so the fixed-length units are joined into the duration slokit and
/// Prometheus already understand.
fn convert_time_windows(
    windows: &[TimeWindowDoc],
    loc: &str,
    notes: &mut Vec<ImportNote>,
) -> Result<Option<String>> {
    match windows {
        [] => {
            note(
                notes,
                loc,
                "spec.timeWindows is missing; the generation-time default period applies",
            );
            Ok(None)
        }
        [tw] => {
            if tw.calendar.is_some() {
                return Err(err(
                    loc,
                    "spec.timeWindows[0].calendar: calendar-aligned time windows are not representable; slokit periods are rolling windows",
                ));
            }
            // `isRolling` is optional in practice: the sloth reference
            // examples omit it and mean a rolling window, so only an explicit
            // `false` is rejected (matching the v1 importer).
            if tw.is_rolling == Some(false) {
                return Err(err(
                    loc,
                    "spec.timeWindows[0].isRolling: false is not representable; slokit periods are rolling windows",
                ));
            }
            let unit = tw
                .unit
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .ok_or_else(|| err(loc, "spec.timeWindows[0].unit is required"))?;
            let suffix = duration_suffix(unit).ok_or_else(|| {
                err(
                    loc,
                    format!(
                        "spec.timeWindows[0].unit '{unit}' is not representable as a fixed-length Prometheus window (expected Second, Minute, Hour, Day or Week; Month, Quarter and Year are calendar units)"
                    ),
                )
            })?;
            let count = tw
                .count
                .ok_or_else(|| err(loc, "spec.timeWindows[0].count is required"))?;
            if count == 0 {
                return Err(err(
                    loc,
                    "spec.timeWindows[0].count must be greater than zero",
                ));
            }
            let duration = format!("{count}{suffix}");
            Window::parse(&duration)
                .map_err(|e| err(loc, format!("spec.timeWindows[0]: {duration}: {e}")))?;
            Ok(Some(duration))
        }
        many => Err(err(
            loc,
            format!(
                "spec.timeWindows has {} entries; OpenSLO v1alpha allows one and slokit maps exactly one",
                many.len()
            ),
        )),
    }
}

/// The Prometheus duration suffix for an OpenSLO v1alpha time-window unit.
/// `None` for calendar units (`Month`, `Quarter`, `Year`) and anything
/// unknown: both are rejected by the caller with the same message.
fn duration_suffix(unit: &str) -> Option<&'static str> {
    if unit.eq_ignore_ascii_case("second") {
        Some("s")
    } else if unit.eq_ignore_ascii_case("minute") {
        Some("m")
    } else if unit.eq_ignore_ascii_case("hour") {
        Some("h")
    } else if unit.eq_ignore_ascii_case("day") {
        Some("d")
    } else if unit.eq_ignore_ascii_case("week") {
        Some("w")
    } else {
        None
    }
}

/// v1alpha states the target as a unit fraction only; there is no
/// `targetPercent` sibling to disambiguate it.
fn objective_percent(obj: &ObjectiveDoc, loc: &str, opath: &str) -> Result<f64> {
    let target = obj.target.ok_or_else(|| {
        err(
            loc,
            format!("{opath}.target is required (openslo/v1alpha has no targetPercent)"),
        )
    })?;
    if !target.is_finite() || !(0.0..=1.0).contains(&target) {
        return Err(err(
            loc,
            format!("{opath}.target {target} must be a unit fraction between 0 and 1"),
        ));
    }
    Ok(target * 100.0)
}

// ---------------------------------------------------------------------------
// Raw OpenSLO v1alpha document shapes (deserialization only).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SloDocSpec {
    #[serde(default)]
    description: String,
    #[serde(default)]
    service: String,
    indicator: Option<IndicatorDoc>,
    #[serde(default)]
    time_windows: Vec<TimeWindowDoc>,
    budgeting_method: Option<String>,
    #[serde(default)]
    objectives: Vec<ObjectiveDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndicatorDoc {
    threshold_metric: Option<MetricSourceDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeWindowDoc {
    unit: Option<String>,
    count: Option<u64>,
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
    time_slice_target: Option<f64>,
    ratio_metrics: Option<RatioMetricsDoc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RatioMetricsDoc {
    /// The counter/gauge distinction changes nothing slokit emits (queries are
    /// used verbatim), so this field is accepted and ignored, exactly as the
    /// v1 importer treats `ratioMetric.counter`.
    #[serde(default, rename = "counter")]
    _counter: Option<bool>,
    good: Option<MetricSourceDoc>,
    total: Option<MetricSourceDoc>,
}

/// v1alpha's metric source is flat: `source`, `queryType`, `query`. (v1 nests
/// the same information under `metricSource.{type, spec.query}`.)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetricSourceDoc {
    #[serde(default)]
    source: String,
    #[serde(default)]
    query_type: String,
    #[serde(default)]
    query: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(query: &str) -> MetricSourceDoc {
        MetricSourceDoc {
            source: "prometheus".to_string(),
            query_type: "promql".to_string(),
            query: query.to_string(),
        }
    }

    fn window(unit: &str, count: u64) -> TimeWindowDoc {
        TimeWindowDoc {
            unit: Some(unit.to_string()),
            count: Some(count),
            is_rolling: None,
            calendar: None,
        }
    }

    #[test]
    fn fixed_length_units_join_count_and_suffix() {
        assert_eq!(duration_suffix("Second"), Some("s"));
        assert_eq!(duration_suffix("Minute"), Some("m"));
        assert_eq!(duration_suffix("Hour"), Some("h"));
        assert_eq!(duration_suffix("Day"), Some("d"));
        assert_eq!(duration_suffix("week"), Some("w"));
    }

    #[test]
    fn calendar_and_unknown_units_have_no_suffix() {
        for unit in ["Month", "Quarter", "Year", "Fortnight", ""] {
            assert_eq!(duration_suffix(unit), None, "{unit}");
        }
    }

    #[test]
    fn a_single_rolling_window_becomes_a_duration() {
        let mut notes = Vec::new();
        let period = convert_time_windows(&[window("Day", 30)], "slo 'a'", &mut notes).unwrap();
        assert_eq!(period.as_deref(), Some("30d"));
        assert!(notes.is_empty());
    }

    #[test]
    fn a_missing_window_notes_and_defaults() {
        let mut notes = Vec::new();
        assert_eq!(
            convert_time_windows(&[], "slo 'a'", &mut notes).unwrap(),
            None
        );
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains("timeWindows is missing"));
    }

    #[test]
    fn calendar_units_windows_and_counts_are_errors() {
        let mut notes = Vec::new();
        let cases: Vec<(TimeWindowDoc, &str)> = vec![
            (window("Month", 1), "not representable as a fixed-length"),
            (window("Year", 1), "not representable as a fixed-length"),
            (window("Day", 0), "count must be greater than zero"),
            (
                TimeWindowDoc {
                    unit: Some("Day".into()),
                    count: None,
                    is_rolling: None,
                    calendar: None,
                },
                "count is required",
            ),
            (
                TimeWindowDoc {
                    unit: None,
                    count: Some(30),
                    is_rolling: None,
                    calendar: None,
                },
                "unit is required",
            ),
            (
                TimeWindowDoc {
                    unit: Some("Day".into()),
                    count: Some(30),
                    is_rolling: Some(false),
                    calendar: None,
                },
                "isRolling",
            ),
            (
                TimeWindowDoc {
                    unit: Some("Day".into()),
                    count: Some(30),
                    is_rolling: None,
                    calendar: Some(Value::String("2020-01-21".into())),
                },
                "calendar",
            ),
        ];
        for (tw, expected) in cases {
            let msg = convert_time_windows(&[tw], "slo 'a'", &mut notes)
                .unwrap_err()
                .to_string();
            assert!(msg.contains(expected), "expected {expected:?} in {msg}");
        }

        let msg = convert_time_windows(
            &[window("Day", 30), window("Day", 7)],
            "slo 'a'",
            &mut notes,
        )
        .unwrap_err()
        .to_string();
        assert!(msg.contains("has 2 entries"), "{msg}");
    }

    #[test]
    fn metric_sources_must_be_prometheus_promql_with_a_query() {
        assert_eq!(
            metric_query(&source("up"), "slo 'a'", "p").unwrap(),
            "up".to_string()
        );

        let mut datadog = source("up");
        datadog.source = "datadog".into();
        let msg = metric_query(&datadog, "slo 'a'", "p")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("'datadog' is not supported"), "{msg}");

        let mut flux = source("up");
        flux.query_type = "flux".into();
        let msg = metric_query(&flux, "slo 'a'", "p").unwrap_err().to_string();
        assert!(msg.contains("queryType 'flux'"), "{msg}");

        let mut blank = source("   ");
        blank.source = "prometheus".into();
        let msg = metric_query(&blank, "slo 'a'", "p")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("query must not be empty"), "{msg}");

        let mut sourceless = source("up");
        sourceless.source = String::new();
        let msg = metric_query(&sourceless, "slo 'a'", "p")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("source is missing"), "{msg}");

        // queryType is optional: a document that omits it still imports.
        let mut untyped = source("up");
        untyped.query_type = String::new();
        assert_eq!(metric_query(&untyped, "slo 'a'", "p").unwrap(), "up");
    }

    #[test]
    fn targets_are_unit_fractions() {
        let obj = |target: Option<f64>| ObjectiveDoc {
            display_name: String::new(),
            op: None,
            value: None,
            target,
            time_slice_target: None,
            ratio_metrics: None,
        };
        assert!((objective_percent(&obj(Some(0.999)), "l", "o").unwrap() - 99.9).abs() < 1e-9);

        let msg = objective_percent(&obj(None), "l", "o")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("target is required"), "{msg}");

        let msg = objective_percent(&obj(Some(99.9)), "l", "o")
            .unwrap_err()
            .to_string();
        assert!(msg.contains("must be a unit fraction"), "{msg}");
    }

    #[test]
    fn ratio_metrics_derive_the_error_query_from_good_and_total() {
        let mut notes = Vec::new();
        let ratio = RatioMetricsDoc {
            _counter: Some(true),
            good: Some(source("sum(rate(ok[{{.window}}]))")),
            total: Some(source("sum(rate(all[{{.window}}]))")),
        };
        let sli = convert_ratio_metrics(&ratio, "slo 'a'", "o.ratioMetrics", &mut notes).unwrap();
        let events = sli.events.expect("events SLI");
        assert_eq!(events.total_query, "sum(rate(all[{{.window}}]))");
        assert_eq!(
            events.error_query,
            "(sum(rate(all[{{.window}}]))) - (sum(rate(ok[{{.window}}])))"
        );
        assert_eq!(notes.len(), 1);
        assert!(notes[0].message.contains("total minus good"));
    }

    #[test]
    fn ratio_metrics_need_both_good_and_total() {
        let mut notes = Vec::new();
        let only_total = RatioMetricsDoc {
            _counter: None,
            good: None,
            total: Some(source("sum(rate(all[{{.window}}]))")),
        };
        let msg = convert_ratio_metrics(&only_total, "slo 'a'", "o.ratioMetrics", &mut notes)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("good is required"), "{msg}");

        let only_good = RatioMetricsDoc {
            _counter: None,
            good: Some(source("sum(rate(ok[{{.window}}]))")),
            total: None,
        };
        let msg = convert_ratio_metrics(&only_good, "slo 'a'", "o.ratioMetrics", &mut notes)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("total is required"), "{msg}");
    }
}
