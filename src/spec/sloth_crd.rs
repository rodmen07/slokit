//! sloth **Kubernetes CRD** import: `apiVersion: sloth.slok.dev/v1`,
//! `kind: PrometheusServiceLevel`.
//!
//! This is how sloth is used inside Kubernetes, and slokit already *emits* a
//! Kubernetes custom resource (`generate --format operator` writes a
//! `PrometheusRule`) without being able to *read* one. Five of the twenty-one
//! documents in `slok/sloth@main`'s `examples/` are in this dialect.
//!
//! # It is the native model, renamed and wrapped
//!
//! Unlike [`super::openslo`], which is a genuinely different model, the CRD is
//! slokit's own [`Spec`] under an `apiVersion`/`kind`/`metadata` envelope with
//! camelCase JSON names. The field list below is sloth's own
//! `pkg/kubernetes/api/sloth/v1/types.go`:
//!
//! | sloth CRD | native slokit spec |
//! |-----------|--------------------|
//! | `spec.service` | [`Spec::service`] |
//! | `spec.labels` | [`Spec::labels`] |
//! | `spec.slos[]` | [`Spec::slos`] |
//! | `slos[].name` / `.objective` / `.description` / `.labels` | the same [`SloSpec`] fields |
//! | `slos[].sli.events.errorQuery` / `.totalQuery` | [`EventsSli::error_query`] / [`EventsSli::total_query`] |
//! | `slos[].sli.raw.errorRatioQuery` | [`RawSli::error_ratio_query`] |
//! | `slos[].sli.plugin.{id, options}` | [`PluginSli`] (same names; ids still resolve against slokit's registry) |
//! | `slos[].alerting.{name, labels, annotations}` | the same [`Alerting`] fields |
//! | `slos[].alerting.pageAlert` / `.ticketAlert` | [`Alerting::page_alert`] / [`Alerting::ticket_alert`] |
//! | `{page,ticket}Alert.{disable, labels, annotations}` | the same [`AlertMeta`] fields |
//!
//! [`Spec::version`] is set to the native default, and the slokit-only
//! extensions the CRD has no field for ([`SloSpec::period`],
//! [`SliSpec::latency`], [`Alerting::windows`]) are simply absent, exactly as
//! they are in a native spec that omits them. That is why importing a CRD
//! document and parsing sloth's own native twin of it generate byte-identical
//! rules — the property `tests/sloth_crd.rs` asserts.
//!
//! # Ignored with a note (lint-style)
//!
//! - `metadata.name`, `metadata.namespace` and `metadata.labels`. The last one
//!   matters most: those are Kubernetes *object* labels, not rule labels
//!   (`spec.labels` is the rule labels), so honoring them would silently add
//!   labels to every generated rule.
//! - `status`, silently: it is the controller's own writeback, never input.
//!
//! # Errors (fail closed, naming the field)
//!
//! The D3 rule the OpenSLO importer and export already follow: a construct
//! that would silently generate the WRONG rules is an error naming the
//! offending field, never a quiet drop.
//!
//! - `spec.sloPlugins` and `slos[].plugins`, sloth's SLO plugin chains.
//!   slokit has no equivalent (`sli.plugin` is a different mechanism and *is*
//!   mapped), and a chain reorders or rewrites the generated rules.
//! - `alerting.page_alert` / `alerting.ticket_alert`, the **native** snake_case
//!   spellings, inside a CRD document. Unknown keys are otherwise ignored (see
//!   below), and ignoring these two would drop the page/ticket severity labels
//!   without a word. This is not hypothetical: sloth's own
//!   `examples/plugin-k8s-getting-started.yml` is a `kind: PrometheusServiceLevel`
//!   document that writes `page_alert:`/`ticket_alert:`, which is why that
//!   document is deliberately *not* one of the committed fixtures.
//!
//! # Unknown keys are ignored
//!
//! Decided explicitly rather than discovered: unknown keys are ignored, which
//! is both what sloth's own Kubernetes decoder does and what slokit's native
//! parser documents ("newer `sloth` keys do not break parsing"). The two
//! confusable-spelling cases above are the deliberate exceptions, because
//! there the ignore is indistinguishable from data loss.
//!
//! # Documents to specs
//!
//! One [`Spec`] per `PrometheusServiceLevel` document, in stream order —
//! the same one-document-one-spec contract as
//! [`Spec::from_yaml_stream`](super::Spec::from_yaml_stream), because a CRD
//! document *is* a wrapped native spec. (The OpenSLO importer merges by
//! `spec.service` instead, because there a document is one SLO rather than one
//! service.) Documents of another `kind` are ignored with a note; a stream
//! with no `PrometheusServiceLevel` document at all is an error.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_norway::{Deserializer as YamlDeserializer, Value};

use crate::error::{Result, SlokitError};

use super::import::{Import, ImportNote};
use super::{AlertMeta, Alerting, EventsSli, PluginSli, RawSli, SliSpec, SloSpec, Spec};

/// The `apiVersion` prefix every sloth Kubernetes CRD document declares.
const API_GROUP: &str = "sloth.slok.dev/";

/// The only `apiVersion` this importer accepts.
const API_VERSION: &str = "sloth.slok.dev/v1";

/// The only `kind` that maps onto a slokit spec.
const KIND: &str = "PrometheusServiceLevel";

/// Cheap format detection for auto-detecting CLI input: true when the first
/// non-empty YAML document has a top-level `apiVersion` starting with
/// `sloth.slok.dev/`. Returns false on unparseable YAML (the caller's normal
/// spec parser will surface the real error), mirroring
/// [`openslo::is_openslo`](super::openslo::is_openslo).
pub fn is_sloth_crd(yaml: &str) -> bool {
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
            .is_some_and(|v| v.starts_with(API_GROUP));
    }
    false
}

/// Import sloth Kubernetes CRD YAML (a single document or a multi-document
/// stream) into slokit [`Spec`]s. See the module docs for the field mapping,
/// which constructs error, and which produce [`ImportNote`]s.
pub fn from_yaml(yaml: &str) -> Result<Import> {
    let mut notes: Vec<ImportNote> = Vec::new();
    let mut specs: Vec<Spec> = Vec::new();
    let mut saw_document = false;
    let mut any_yaml_document = false;

    for (idx, de) in YamlDeserializer::from_str(yaml).enumerate() {
        let n = idx + 1;
        let value = Value::deserialize(de)
            .map_err(|e| SlokitError::Spec(format!("sloth-crd document {n}: {e}")))?;
        if value.is_null() {
            continue;
        }
        any_yaml_document = true;

        let doc: Document = serde_norway::from_value(value)
            .map_err(|e| SlokitError::Spec(format!("sloth-crd document {n}: {e}")))?;

        if doc.api_version != API_VERSION {
            return Err(err(
                &format!("document {n}"),
                format!(
                    "unsupported apiVersion '{}' (expected {API_VERSION})",
                    doc.api_version
                ),
            ));
        }
        match doc.kind.as_str() {
            "" => return Err(err(&format!("document {n}"), "`kind` is missing")),
            KIND => {
                saw_document = true;
                specs.push(convert(n, &doc, &mut notes)?);
            }
            other => note(
                &mut notes,
                &format!("document {n}"),
                format!(
                    "kind '{other}' does not map to the slokit model and was ignored (only {KIND} imports)"
                ),
            ),
        }
    }

    if !any_yaml_document {
        return Err(SlokitError::Spec(
            "sloth-crd: input contains no YAML documents".to_string(),
        ));
    }
    if !saw_document {
        return Err(SlokitError::Spec(format!(
            "sloth-crd: no kind: {KIND} documents in input (nothing to import)"
        )));
    }

    Ok(Import { specs, notes })
}

/// Read and import sloth Kubernetes CRD YAML from a file. See [`from_yaml`].
pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Import> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path)
        .map_err(|e| SlokitError::Spec(format!("reading {}: {e}", path.display())))?;
    from_yaml(&contents)
}

/// Convert one `PrometheusServiceLevel` document into a [`Spec`].
fn convert(doc_no: usize, doc: &Document, notes: &mut Vec<ImportNote>) -> Result<Spec> {
    let object = doc.metadata.name.trim();
    let loc = if object.is_empty() {
        format!("document {doc_no}")
    } else {
        format!("PrometheusServiceLevel '{object}'")
    };

    if doc.spec.slo_plugins.is_some() {
        return Err(err(
            &loc,
            "spec.sloPlugins is a sloth SLO plugin chain and has no slokit equivalent; \
             it would rewrite the generated rules, so it is refused rather than dropped \
             (sli.plugin is a different mechanism and does import)",
        ));
    }

    let service = doc.spec.service.trim();
    if service.is_empty() {
        return Err(err(&loc, "spec.service must not be empty"));
    }

    // Kubernetes object metadata is not spec data: `spec.labels` is what
    // labels the rules, so honoring `metadata.labels` would change output.
    if !object.is_empty() {
        note(
            notes,
            &loc,
            "metadata.name is Kubernetes object identity and was ignored (SLO names come from spec.slos[].name)",
        );
    }
    if doc.metadata.namespace.is_some() {
        note(
            notes,
            &loc,
            "metadata.namespace was ignored (slokit specs are not namespaced)",
        );
    }
    if !doc.metadata.labels.is_empty() {
        let names = doc
            .metadata
            .labels
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        note(
            notes,
            &loc,
            format!(
                "metadata.labels ({names}) are Kubernetes object labels and were ignored; \
                 spec.labels is what labels the generated rules"
            ),
        );
    }

    let mut slos = Vec::with_capacity(doc.spec.slos.len());
    for slo in &doc.spec.slos {
        slos.push(convert_slo(&loc, slo)?);
    }

    let mut spec = Spec::new(service, slos);
    spec.labels = doc.spec.labels.clone();
    Ok(spec)
}

/// Convert one `spec.slos[]` entry into a [`SloSpec`].
fn convert_slo(doc_loc: &str, slo: &CrdSlo) -> Result<SloSpec> {
    let name = slo.name.trim();
    let loc = if name.is_empty() {
        doc_loc.to_string()
    } else {
        format!("{doc_loc} slo '{name}'")
    };
    if name.is_empty() {
        return Err(err(&loc, "spec.slos[].name must not be empty"));
    }
    if slo.plugins.is_some() {
        return Err(err(
            &loc,
            "slos[].plugins is a sloth SLO plugin chain and has no slokit equivalent; \
             it would rewrite the generated rules, so it is refused rather than dropped \
             (sli.plugin is a different mechanism and does import)",
        ));
    }

    let mut out = SloSpec::new(name, slo.objective, convert_sli(&loc, &slo.sli)?);
    out.description = slo.description.clone();
    out.labels = slo.labels.clone();
    out.alerting = convert_alerting(&loc, &slo.alerting)?;
    Ok(out)
}

/// Convert `slos[].sli`. Exactly one shape must be set, matching the native
/// model's own one-of contract (which [`validate`](super::validate) enforces
/// for native specs too).
fn convert_sli(loc: &str, sli: &CrdSli) -> Result<SliSpec> {
    let set = [
        sli.events.is_some(),
        sli.raw.is_some(),
        sli.plugin.is_some(),
    ]
    .iter()
    .filter(|x| **x)
    .count();
    if set == 0 {
        return Err(err(
            loc,
            "sli must set exactly one of events, raw or plugin (none were set)",
        ));
    }
    if set > 1 {
        return Err(err(
            loc,
            "sli must set exactly one of events, raw or plugin (more than one was set)",
        ));
    }

    if let Some(events) = &sli.events {
        return Ok(SliSpec::events(EventsSli::new(
            events.error_query.clone(),
            events.total_query.clone(),
        )));
    }
    if let Some(raw) = &sli.raw {
        return Ok(SliSpec::raw(RawSli::new(raw.error_ratio_query.clone())));
    }
    let plugin = sli.plugin.as_ref().expect("exactly one SLI shape is set");
    let mut out = PluginSli::new(plugin.id.clone());
    out.options = plugin.options.clone();
    Ok(SliSpec::plugin(out))
}

/// Convert `slos[].alerting`, rejecting the native snake_case spellings of the
/// two per-severity keys (see the module docs: sloth's own
/// `plugin-k8s-getting-started.yml` writes them, and ignoring them silently
/// drops the severity labels).
fn convert_alerting(loc: &str, alerting: &CrdAlerting) -> Result<Alerting> {
    for native in ["page_alert", "ticket_alert"] {
        if alerting.extra.contains_key(native) {
            let camel = match native {
                "page_alert" => "pageAlert",
                _ => "ticketAlert",
            };
            return Err(err(
                loc,
                format!(
                    "alerting.{native} is the native slokit spelling; a {KIND} document must \
                     write alerting.{camel}. Ignoring it would silently drop this severity's \
                     labels and annotations (sloth's own examples/plugin-k8s-getting-started.yml \
                     has this bug)"
                ),
            ));
        }
    }

    Ok(Alerting {
        name: alerting.name.clone(),
        labels: alerting.labels.clone(),
        annotations: alerting.annotations.clone(),
        page_alert: convert_alert_meta(&alerting.page_alert),
        ticket_alert: convert_alert_meta(&alerting.ticket_alert),
        // The CRD has no equivalent of the `alerting.windows` slokit
        // extension, so it stays at the default (the standard MWMBR table).
        ..Alerting::default()
    })
}

fn convert_alert_meta(meta: &CrdAlert) -> AlertMeta {
    AlertMeta {
        disable: meta.disable,
        labels: meta.labels.clone(),
        annotations: meta.annotations.clone(),
    }
}

/// Build an import error whose message names the CRD location and field.
fn err(location: &str, message: impl AsRef<str>) -> SlokitError {
    SlokitError::Spec(format!("sloth-crd {location}: {}", message.as_ref()))
}

fn note(notes: &mut Vec<ImportNote>, location: &str, message: impl Into<String>) {
    notes.push(ImportNote {
        location: location.to_string(),
        message: message.into(),
    });
}

// ---------------------------------------------------------------------------
// Raw CRD document shapes (deserialization only). Field names and optionality
// follow sloth's `pkg/kubernetes/api/sloth/v1/types.go`; unknown keys are
// ignored, which is what sloth's own Kubernetes decoder does.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[serde(default)]
    api_version: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    metadata: ObjectMeta,
    spec: CrdSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObjectMeta {
    #[serde(default)]
    name: String,
    #[serde(default)]
    namespace: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSpec {
    #[serde(default)]
    service: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    /// Refused, never dropped: see [`convert`].
    #[serde(default)]
    slo_plugins: Option<Value>,
    #[serde(default)]
    slos: Vec<CrdSlo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSlo {
    #[serde(default)]
    name: String,
    objective: f64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    /// Refused, never dropped: see [`convert_slo`].
    #[serde(default)]
    plugins: Option<Value>,
    sli: CrdSli,
    #[serde(default)]
    alerting: CrdAlerting,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSli {
    #[serde(default)]
    events: Option<CrdSliEvents>,
    #[serde(default)]
    raw: Option<CrdSliRaw>,
    #[serde(default)]
    plugin: Option<CrdSliPlugin>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSliEvents {
    error_query: String,
    total_query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSliRaw {
    error_ratio_query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdSliPlugin {
    id: String,
    #[serde(default)]
    options: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdAlerting {
    #[serde(default)]
    name: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    #[serde(default)]
    page_alert: CrdAlert,
    #[serde(default)]
    ticket_alert: CrdAlert,
    /// Every other key under `alerting`, kept so the two native snake_case
    /// spellings can be refused by name instead of silently ignored.
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrdAlert {
    #[serde(default)]
    disable: bool,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}
