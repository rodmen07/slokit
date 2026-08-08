//! sloth **alert-window catalogues**: `apiVersion: sloth.slok.dev/v1`,
//! `kind: AlertWindows`.
//!
//! This is how sloth supplies a burn-rate window table for one SLO period
//! instead of the 30-day SRE Workbook defaults. slokit has had per-SLO
//! `alerting.windows` since 0.7.0, but no way to consume a *catalogue*, so
//! both of the catalogues sloth ships (`examples/windows/7d.yaml` and
//! `examples/windows/custom-30d.yaml`) died inside the CRD importer with
//! `sloth-crd: no kind: PrometheusServiceLevel documents in input` — the
//! `apiVersion` routed them there and then nothing knew the `kind`.
//!
//! # The mapping is arithmetic slokit already owns
//!
//! A catalogue states, per severity and speed, how much of the error budget
//! may burn over the long window before the alert fires. slokit states the
//! same thing as a burn-rate multiplier. The two are the same number under a
//! change of units:
//!
//! ```text
//! factor = (errorBudgetPercent / 100) x (sloPeriod / longWindow)
//! ```
//!
//! | sloth catalogue | slokit |
//! |---|---|
//! | `spec.sloPeriod` | the SLO period this catalogue applies to |
//! | `spec.{page,ticket}.{quick,slow}.errorBudgetPercent` | numerator of the factor |
//! | `spec.{page,ticket}.{quick,slow}.longWindow` | [`AlertWindow::long`](crate::burn_rate::AlertWindow::long) |
//! | `spec.{page,ticket}.{quick,slow}.shortWindow` | [`AlertWindow::short`](crate::burn_rate::AlertWindow::short) |
//!
//! The four blocks convert in the order `page.quick`, `page.slow`,
//! `ticket.quick`, `ticket.slow`, which is [`MwmbrConfig::sre_default`]'s own
//! order. That is not cosmetic: a 30-day catalogue carrying sloth's own
//! defaults (page 2% / 5%, ticket 10% / 10%) maps onto exactly
//! `sre_default()`'s `14.4 / 6 / 3 / 1`, so it must generate byte-identical
//! rules to passing no catalogue at all, and rule ORDER is part of those
//! bytes.
//!
//! # Applying a catalogue
//!
//! A loaded [`AlertWindowsSet`] is keyed by SLO period and reaches generation
//! through [`GenerateOptions::alert_windows`](crate::generate::GenerateOptions::alert_windows).
//! The precedence is decided in one place,
//! [`resolve_mwmbr`](crate::generate) — the same seam the dashboard shares:
//!
//! 1. the SLO's own `alerting.windows`, if it sets any;
//! 2. else the catalogue whose `sloPeriod` equals the SLO's resolved period;
//! 3. else the default table, scaled to that period unless scaling is off.
//!
//! A catalogue's windows are used **verbatim**: it already states the windows
//! for its own period, so scaling them again would be a second correction for
//! a difference the author already accounted for.
//!
//! # Errors (fail closed, naming the field)
//!
//! Same D3 posture as [`super::sloth_crd`]: anything that would silently
//! produce the wrong alert thresholds is an error naming the offending path,
//! never a quiet drop. All four `{page,ticket}.{quick,slow}` blocks are
//! required (a catalogue missing one would silently lose an alert condition),
//! `errorBudgetPercent` must be in `(0, 100]`, the windows must parse, the
//! short window must be shorter than the long one, and the long window must
//! not exceed the SLO period. Two catalogues for the same `sloPeriod` are an
//! error rather than a last-one-wins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_norway::{Deserializer as YamlDeserializer, Value};

use crate::burn_rate::{AlertWindow, MwmbrConfig, Severity};
use crate::error::{Result, SlokitError};
use crate::window::Window;

use super::Spec;

/// The `apiVersion` prefix every sloth document declares.
const API_GROUP: &str = "sloth.slok.dev/";

/// The only `apiVersion` this reader accepts.
const API_VERSION: &str = "sloth.slok.dev/v1";

/// The `kind` this module reads.
pub const KIND: &str = "AlertWindows";

/// Cheap format detection for auto-detecting CLI input: true when the first
/// non-empty YAML document declares a `sloth.slok.dev/` `apiVersion` **and**
/// `kind: AlertWindows`.
///
/// Both halves matter. [`is_sloth_crd`](super::sloth_crd::is_sloth_crd) tests
/// only the `apiVersion`, so it answers true for a catalogue too; a caller
/// choosing between them must ask this one first.
pub fn is_alert_windows(yaml: &str) -> bool {
    for de in YamlDeserializer::from_str(yaml) {
        let Ok(value) = Value::deserialize(de) else {
            return false;
        };
        if value.is_null() {
            continue;
        }
        let group_matches = value
            .get("apiVersion")
            .and_then(Value::as_str)
            .is_some_and(|v| v.starts_with(API_GROUP));
        let kind_matches = value
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|k| k == KIND);
        return group_matches && kind_matches;
    }
    false
}

/// One sloth alert-window catalogue: the burn-rate conditions to use for SLOs
/// whose period is [`slo_period`](AlertWindowsCatalogue::slo_period).
///
/// `#[non_exhaustive]`: build one by parsing a document with [`from_yaml`] or
/// [`from_path`]. The fields stay public for reading.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AlertWindowsCatalogue {
    /// The SLO period this catalogue is calibrated for (`spec.sloPeriod`).
    pub slo_period: Window,
    /// The four burn-rate conditions, ordered page-quick, page-slow,
    /// ticket-quick, ticket-slow.
    pub windows: Vec<AlertWindow>,
}

impl AlertWindowsCatalogue {
    /// The burn-rate configuration this catalogue describes.
    pub fn to_mwmbr(&self) -> MwmbrConfig {
        MwmbrConfig::new(self.windows.clone())
    }
}

/// A set of catalogues indexed by SLO period, as loaded from a file or a
/// directory.
///
/// Empty by default, which is what makes
/// [`GenerateOptions::alert_windows`](crate::generate::GenerateOptions::alert_windows)
/// an additive option: an empty set changes nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlertWindowsSet {
    by_period: BTreeMap<Window, MwmbrConfig>,
}

impl AlertWindowsSet {
    /// An empty set: no period has a catalogue.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no catalogue is loaded.
    pub fn is_empty(&self) -> bool {
        self.by_period.is_empty()
    }

    /// How many catalogues are loaded.
    pub fn len(&self) -> usize {
        self.by_period.len()
    }

    /// Add a catalogue, failing when another one already covers the same
    /// period.
    ///
    /// A last-one-wins merge would make the emitted thresholds depend on
    /// directory iteration order, so a collision is an error naming the
    /// period.
    pub fn insert(&mut self, catalogue: AlertWindowsCatalogue) -> Result<()> {
        if self.by_period.contains_key(&catalogue.slo_period) {
            return Err(SlokitError::Spec(format!(
                "two alert-window catalogues both declare sloPeriod {}; \
                 keep one catalogue per period",
                catalogue.slo_period.prometheus()
            )));
        }
        self.by_period
            .insert(catalogue.slo_period, catalogue.to_mwmbr());
        Ok(())
    }

    /// The catalogue covering `period`, if one is loaded.
    pub fn for_period(&self, period: Window) -> Option<&MwmbrConfig> {
        self.by_period.get(&period)
    }

    /// Every period this set covers, ascending.
    pub fn periods(&self) -> Vec<Window> {
        self.by_period.keys().copied().collect()
    }

    /// Load every catalogue in `path`: one file, or every `*.yaml`/`*.yml`
    /// file directly inside a directory.
    ///
    /// Non-recursive, matching how [`crate::spec`] input files are collected.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut set = Self::new();
        for file in catalogue_files(path)? {
            for catalogue in from_path(&file)? {
                set.insert(catalogue)
                    .map_err(|e| SlokitError::Spec(format!("loading {}: {e}", file.display())))?;
            }
        }
        if set.is_empty() {
            return Err(SlokitError::Spec(format!(
                "no kind: {KIND} documents found in {}",
                path.display()
            )));
        }
        Ok(set)
    }

    /// The SLO periods in `specs` this set has NO catalogue for, ascending and
    /// deduplicated.
    ///
    /// SLOs that set their own `alerting.windows` are skipped: they never
    /// consult a catalogue, so a missing one cannot affect them.
    ///
    /// This exists because [`resolve_mwmbr`](crate::generate) must stay
    /// infallible (the dashboard entry points share it and return no
    /// `Result`), so it falls back to the default table for an uncovered
    /// period. Silently falling back is fine as a resolution rule and useless
    /// as a user experience: a mistyped `sloPeriod` would emit default
    /// thresholds while the catalogue looked applied. Callers that took a
    /// catalogue path from a user should turn a non-empty result into an
    /// error.
    pub fn uncovered_periods(&self, specs: &[Spec], default_period: Window) -> Result<Vec<Window>> {
        let mut missing: Vec<Window> = Vec::new();
        for spec in specs {
            for slo in &spec.slos {
                if !slo.alerting.windows.is_empty() {
                    continue;
                }
                let period = slo.to_slo(default_period)?.period;
                if self.for_period(period).is_none() && !missing.contains(&period) {
                    missing.push(period);
                }
            }
        }
        missing.sort();
        Ok(missing)
    }
}

/// Every catalogue file `path` names: itself when it is a file, or the
/// `*.yaml`/`*.yml` files directly inside it when it is a directory.
fn catalogue_files(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.is_dir() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| SlokitError::Spec(format!("reading dir {}: {e}", path.display())))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(SlokitError::Spec(format!(
            "no .yaml/.yml files found in {}",
            path.display()
        )));
    }
    Ok(files)
}

/// Parse alert-window catalogue YAML (a single document or a multi-document
/// stream) into [`AlertWindowsCatalogue`]s, in stream order.
///
/// Every document in the stream must be a `kind: AlertWindows` document: a
/// spec document mixed into the same stream is an error rather than a silent
/// drop, because the caller routed the whole file here on its FIRST document's
/// kind and would otherwise lose the rest.
pub fn from_yaml(yaml: &str) -> Result<Vec<AlertWindowsCatalogue>> {
    let mut out = Vec::new();
    let mut any_yaml_document = false;

    for (idx, de) in YamlDeserializer::from_str(yaml).enumerate() {
        let n = idx + 1;
        let value = Value::deserialize(de)
            .map_err(|e| SlokitError::Spec(format!("alert-windows document {n}: {e}")))?;
        if value.is_null() {
            continue;
        }
        any_yaml_document = true;

        let doc: Document = serde_norway::from_value(value)
            .map_err(|e| SlokitError::Spec(format!("alert-windows document {n}: {e}")))?;

        if doc.api_version != API_VERSION {
            return Err(err(
                n,
                format!(
                    "unsupported apiVersion '{}' (expected {API_VERSION})",
                    doc.api_version
                ),
            ));
        }
        if doc.kind != KIND {
            return Err(err(
                n,
                format!(
                    "kind '{}' cannot share a stream with kind: {KIND}; keep alert-window \
                     catalogues in their own file",
                    doc.kind
                ),
            ));
        }
        out.push(convert(n, &doc.spec)?);
    }

    if !any_yaml_document {
        return Err(SlokitError::Spec(
            "alert-windows: input contains no YAML documents".to_string(),
        ));
    }
    Ok(out)
}

/// Read and parse alert-window catalogue YAML from a file. See [`from_yaml`].
pub fn from_path(path: impl AsRef<Path>) -> Result<Vec<AlertWindowsCatalogue>> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path)
        .map_err(|e| SlokitError::Spec(format!("reading {}: {e}", path.display())))?;
    from_yaml(&contents).map_err(|e| match e {
        SlokitError::Spec(msg) => {
            SlokitError::Spec(format!("reading catalogue {}: {msg}", path.display()))
        }
        other => other,
    })
}

/// Convert one document body into a catalogue.
fn convert(doc_no: usize, spec: &DocSpec) -> Result<AlertWindowsCatalogue> {
    if spec.slo_period.trim().is_empty() {
        return Err(err(doc_no, "spec.sloPeriod is required".to_string()));
    }
    let slo_period =
        Window::parse(&spec.slo_period).map_err(|e| err(doc_no, format!("spec.sloPeriod: {e}")))?;

    let blocks = [
        (Severity::Page, "page", "quick", spec.page.quick.as_ref()),
        (Severity::Page, "page", "slow", spec.page.slow.as_ref()),
        (
            Severity::Ticket,
            "ticket",
            "quick",
            spec.ticket.quick.as_ref(),
        ),
        (
            Severity::Ticket,
            "ticket",
            "slow",
            spec.ticket.slow.as_ref(),
        ),
    ];

    let mut windows = Vec::with_capacity(blocks.len());
    for (severity, sev_name, speed, block) in blocks {
        let path = format!("spec.{sev_name}.{speed}");
        let Some(block) = block else {
            return Err(err(
                doc_no,
                format!(
                    "{path} is required; a catalogue missing one block would silently drop that \
                     alert condition"
                ),
            ));
        };
        windows.push(block.to_alert_window(doc_no, &path, severity, slo_period)?);
    }

    Ok(AlertWindowsCatalogue {
        slo_period,
        windows,
    })
}

fn err(doc_no: usize, msg: String) -> SlokitError {
    SlokitError::Spec(format!("alert-windows document {doc_no}: {msg}"))
}

// ---------------------------------------------------------------------------
// The document model (sloth's own field names)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Document {
    #[serde(default, rename = "apiVersion")]
    api_version: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    spec: DocSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocSpec {
    #[serde(default)]
    slo_period: String,
    #[serde(default)]
    page: SeverityBlocks,
    #[serde(default)]
    ticket: SeverityBlocks,
}

#[derive(Debug, Default, Deserialize)]
struct SeverityBlocks {
    #[serde(default)]
    quick: Option<WindowBlock>,
    #[serde(default)]
    slow: Option<WindowBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowBlock {
    error_budget_percent: f64,
    short_window: String,
    long_window: String,
}

impl WindowBlock {
    fn to_alert_window(
        &self,
        doc_no: usize,
        path: &str,
        severity: Severity,
        slo_period: Window,
    ) -> Result<AlertWindow> {
        if !(self.error_budget_percent > 0.0 && self.error_budget_percent <= 100.0) {
            return Err(err(
                doc_no,
                format!(
                    "{path}.errorBudgetPercent is {}, which is outside (0, 100]",
                    self.error_budget_percent
                ),
            ));
        }
        let long = Window::parse(&self.long_window)
            .map_err(|e| err(doc_no, format!("{path}.longWindow: {e}")))?;
        let short = Window::parse(&self.short_window)
            .map_err(|e| err(doc_no, format!("{path}.shortWindow: {e}")))?;
        if short >= long {
            return Err(err(
                doc_no,
                format!(
                    "{path}.shortWindow ({}) must be shorter than its longWindow ({}): the short \
                     window exists to confirm the long one is still burning",
                    short.prometheus(),
                    long.prometheus()
                ),
            ));
        }
        if long > slo_period {
            return Err(err(
                doc_no,
                format!(
                    "{path}.longWindow ({}) is longer than spec.sloPeriod ({})",
                    long.prometheus(),
                    slo_period.prometheus()
                ),
            ));
        }
        let factor =
            (self.error_budget_percent / 100.0) * (slo_period.as_secs_f64() / long.as_secs_f64());
        Ok(AlertWindow::new(severity, long, short, factor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEVEN_DAY: &str = r#"
apiVersion: sloth.slok.dev/v1
kind: AlertWindows
spec:
  sloPeriod: 7d
  page:
    quick:
      errorBudgetPercent: 8
      shortWindow: 5m
      longWindow: 1h
    slow:
      errorBudgetPercent: 12.5
      shortWindow: 30m
      longWindow: 6h
  ticket:
    quick:
      errorBudgetPercent: 20
      shortWindow: 2h
      longWindow: 1d
    slow:
      errorBudgetPercent: 42
      shortWindow: 6h
      longWindow: 3d
"#;

    #[test]
    fn detection_needs_both_the_group_and_the_kind() {
        assert!(is_alert_windows(SEVEN_DAY));
        assert!(!is_alert_windows(
            "apiVersion: sloth.slok.dev/v1\nkind: PrometheusServiceLevel\n"
        ));
        assert!(!is_alert_windows("apiVersion: openslo/v1\nkind: SLO\n"));
        assert!(!is_alert_windows("version: prometheus/v1\nservice: svc\n"));
    }

    #[test]
    fn the_upstream_seven_day_catalogue_maps_to_its_factors() {
        let cats = from_yaml(SEVEN_DAY).expect("parses");
        assert_eq!(cats.len(), 1);
        let cat = &cats[0];
        assert_eq!(cat.slo_period, Window::days(7));
        let factors: Vec<f64> = cat.windows.iter().map(|w| w.factor).collect();
        // (pct / 100) x (7d / longWindow).
        assert!((factors[0] - 13.44).abs() < 1e-9, "{factors:?}");
        assert!((factors[1] - 3.5).abs() < 1e-9, "{factors:?}");
        assert!((factors[2] - 1.4).abs() < 1e-9, "{factors:?}");
        assert!((factors[3] - 0.98).abs() < 1e-9, "{factors:?}");
        assert_eq!(cat.windows[0].long, Window::hours(1));
        assert_eq!(cat.windows[0].short, Window::minutes(5));
        assert_eq!(cat.windows[0].severity, Severity::Page);
        assert_eq!(cat.windows[3].severity, Severity::Ticket);
    }

    /// The property done-when clause 4 rests on: sloth's own defaults, written
    /// as a 30-day catalogue, ARE `MwmbrConfig::sre_default()`.
    #[test]
    fn sloths_own_defaults_as_a_catalogue_equal_the_sre_default_table() {
        let yaml = r#"
apiVersion: sloth.slok.dev/v1
kind: AlertWindows
spec:
  sloPeriod: 30d
  page:
    quick: {errorBudgetPercent: 2, shortWindow: 5m, longWindow: 1h}
    slow: {errorBudgetPercent: 5, shortWindow: 30m, longWindow: 6h}
  ticket:
    quick: {errorBudgetPercent: 10, shortWindow: 2h, longWindow: 1d}
    slow: {errorBudgetPercent: 10, shortWindow: 6h, longWindow: 3d}
"#;
        let cats = from_yaml(yaml).expect("parses");
        assert_eq!(cats[0].to_mwmbr(), MwmbrConfig::sre_default());
    }

    #[test]
    fn a_missing_block_is_an_error_naming_its_path() {
        let yaml = r#"
apiVersion: sloth.slok.dev/v1
kind: AlertWindows
spec:
  sloPeriod: 30d
  page:
    quick: {errorBudgetPercent: 2, shortWindow: 5m, longWindow: 1h}
    slow: {errorBudgetPercent: 5, shortWindow: 30m, longWindow: 6h}
  ticket:
    quick: {errorBudgetPercent: 10, shortWindow: 2h, longWindow: 1d}
"#;
        let e = from_yaml(yaml).unwrap_err().to_string();
        assert!(e.contains("spec.ticket.slow is required"), "{e}");
    }

    #[test]
    fn an_out_of_range_budget_percent_is_refused() {
        let yaml = SEVEN_DAY.replace("errorBudgetPercent: 8", "errorBudgetPercent: 0");
        let e = from_yaml(&yaml).unwrap_err().to_string();
        assert!(e.contains("spec.page.quick.errorBudgetPercent is 0"), "{e}");
    }

    #[test]
    fn a_short_window_at_least_as_long_as_its_long_window_is_refused() {
        let yaml = SEVEN_DAY.replace(
            "shortWindow: 5m\n      longWindow: 1h",
            "shortWindow: 2h\n      longWindow: 1h",
        );
        let e = from_yaml(&yaml).unwrap_err().to_string();
        assert!(e.contains("must be shorter than its longWindow"), "{e}");
    }

    #[test]
    fn a_long_window_beyond_the_slo_period_is_refused() {
        let yaml = SEVEN_DAY.replace("longWindow: 3d", "longWindow: 30d");
        let e = from_yaml(&yaml).unwrap_err().to_string();
        assert!(e.contains("is longer than spec.sloPeriod (7d)"), "{e}");
    }

    #[test]
    fn a_spec_document_may_not_ride_in_a_catalogue_stream() {
        let yaml = format!("{SEVEN_DAY}---\napiVersion: sloth.slok.dev/v1\nkind: PrometheusServiceLevel\nspec:\n  service: svc\n");
        let e = from_yaml(&yaml).unwrap_err().to_string();
        assert!(
            e.contains("kind 'PrometheusServiceLevel' cannot share a stream"),
            "{e}"
        );
    }

    #[test]
    fn two_catalogues_for_one_period_collide_rather_than_overwrite() {
        let cats = from_yaml(SEVEN_DAY).expect("parses");
        let mut set = AlertWindowsSet::new();
        set.insert(cats[0].clone()).expect("first insert");
        let e = set.insert(cats[0].clone()).unwrap_err().to_string();
        assert!(e.contains("both declare sloPeriod 7d"), "{e}");
    }

    #[test]
    fn lookup_is_by_exact_period() {
        let cats = from_yaml(SEVEN_DAY).expect("parses");
        let mut set = AlertWindowsSet::new();
        set.insert(cats[0].clone()).expect("insert");
        assert!(set.for_period(Window::days(7)).is_some());
        assert!(set.for_period(Window::days(30)).is_none());
        assert_eq!(set.periods(), vec![Window::days(7)]);
        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());
    }
}
