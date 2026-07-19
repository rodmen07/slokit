//! Advisory linting for a [`Spec`](super::Spec).
//!
//! Linting is distinct from [`validate`](super::validate): validation reports
//! *errors* that make rule generation wrong or impossible (out-of-range
//! objectives, missing SLIs, queries without the `{{.window}}` token), while
//! linting reports *advisory* findings — configurations that are legal but
//! probably not what an SRE intended, such as an objective with no error budget,
//! alerts without routing labels, or an SLO period shorter than the burn-rate
//! windows.
//!
//! [`lint`] never fails; it returns every finding and lets the caller decide
//! whether to treat them as fatal (the CLI's `--strict` flag does).

use std::collections::{BTreeMap, HashSet};

use crate::burn_rate::{MwmbrConfig, Severity};
use crate::window::Window;

use super::{Spec, DEFAULT_PERIOD};

/// How serious a [`Lint`] finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintLevel {
    /// A likely misconfiguration an SRE should review.
    Warning,
    /// A minor, purely informational suggestion.
    Info,
}

impl LintLevel {
    /// A short uppercase label for table output (`WARN` / `INFO`).
    pub fn label(&self) -> &'static str {
        match self {
            LintLevel::Warning => "WARN",
            LintLevel::Info => "INFO",
        }
    }
}

/// A single advisory finding produced by [`lint`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lint {
    /// How serious the finding is.
    pub level: LintLevel,
    /// A stable, machine-readable identifier, e.g. `OBJECTIVE_100`.
    pub code: &'static str,
    /// Where the finding applies, e.g. `slo 'requests-availability'`.
    pub location: String,
    /// A human-readable explanation of the finding.
    pub message: String,
}

/// The legacy Prometheus label-name charset (`[a-zA-Z_][a-zA-Z0-9_]*`).
///
/// Prometheus 3.0 accepts UTF-8 names by default, so names outside this set
/// are advisory (older servers and legacy name validation reject them), not
/// validation errors.
fn is_legacy_label_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Advisory checks on one label/annotation map: legacy-charset portability for
/// every name and, for label maps (`reserved`), collisions with the generated
/// `sloth_*` identity labels.
fn label_name_lints(
    out: &mut Vec<Lint>,
    location: &str,
    field: &str,
    map: &BTreeMap<String, String>,
    reserved: bool,
) {
    for key in map.keys() {
        if !is_legacy_label_name(key) {
            out.push(Lint {
                level: LintLevel::Warning,
                code: "LABEL_NAME_CHARS",
                location: location.to_string(),
                message: format!(
                    "`{field}` name '{key}' is outside [a-zA-Z_][a-zA-Z0-9_]*; Prometheus releases before 3.0 (and legacy name validation) reject rules that use it"
                ),
            });
        }
        if reserved && key.starts_with("sloth_") {
            out.push(Lint {
                level: LintLevel::Warning,
                code: "RESERVED_LABEL",
                location: location.to_string(),
                message: format!(
                    "`{field}` name '{key}' uses the reserved `sloth_` prefix; slokit's generated identity labels may overwrite it"
                ),
            });
        }
    }
}

/// Run every advisory check against `spec` and return all findings, ordered by
/// SLO and then by check. An empty vec means the spec is clean.
///
/// Linting assumes nothing about structural validity, so it is safe to call on
/// a spec that would fail [`validate`](super::validate); the checks only read
/// the version, objective, period, labels, alerting, and description fields.
pub fn lint(spec: &Spec) -> Vec<Lint> {
    let mut out = Vec::new();

    // Unknown spec version: parsing ignores the field entirely, so anything
    // other than prometheus/v1 is treated as prometheus/v1 without notice.
    if spec.version != "prometheus/v1" {
        out.push(Lint {
            level: LintLevel::Warning,
            code: "SPEC_VERSION",
            location: "spec".to_string(),
            message: format!(
                "version '{}' is not 'prometheus/v1'; slokit ignores the field and generates prometheus/v1 rules regardless",
                spec.version
            ),
        });
    }

    label_name_lints(&mut out, "spec", "labels", &spec.labels, true);

    for slo in &spec.slos {
        let loc = format!("slo '{}'", slo.name);

        // The effective MWMBR configuration for this SLO: custom windows when
        // set, else the default table scaled to the SLO period (mirroring what
        // generation does). Unparseable custom windows are validate's problem;
        // window-based checks are skipped for them here.
        let custom = slo.custom_mwmbr().ok().flatten();
        let effective_mwmbr = slo.resolve_period(DEFAULT_PERIOD).ok().map(|period| {
            custom
                .clone()
                .unwrap_or_else(|| MwmbrConfig::sre_default_for_period(period))
        });

        // Objective with no — or implausibly little — error budget.
        if slo.objective >= 100.0 {
            out.push(Lint {
                level: LintLevel::Warning,
                code: "OBJECTIVE_100",
                location: loc.clone(),
                message:
                    "objective is 100%: there is no error budget, so burn-rate alerts can never fire"
                        .to_string(),
            });
        } else if slo.objective > 0.0 && slo.objective < 50.0 {
            out.push(Lint {
                level: LintLevel::Warning,
                code: "OBJECTIVE_LOW",
                location: loc.clone(),
                message: format!(
                    "objective {}% is implausibly low; confirm this is intended",
                    slo.objective
                ),
            });
        }

        // A burn-rate condition whose threshold (factor * error budget) is at
        // least 1 asks for an error ratio above 100%, which can never happen:
        // the alert is generated but can never fire.
        if slo.objective > 0.0 && slo.objective < 100.0 {
            if let Some(mwmbr) = &effective_mwmbr {
                let budget = 1.0 - slo.objective / 100.0;
                for w in &mwmbr.windows {
                    let threshold = w.factor * budget;
                    if threshold >= 1.0 {
                        out.push(Lint {
                            level: LintLevel::Warning,
                            code: "THRESHOLD_UNREACHABLE",
                            location: loc.clone(),
                            message: format!(
                                "{} condition (factor {}, long {}) fires at error ratio {threshold}, but an error ratio cannot exceed 1; this condition can never fire",
                                w.severity.label(),
                                w.factor,
                                w.long
                            ),
                        });
                    }
                }
            }
        }

        // Repeated identical custom conditions (same severity and windows) are
        // redundant: OR-ed together, only the lowest threshold matters.
        let mut seen_windows: HashSet<(&str, Window, Window)> = HashSet::new();
        for w in &slo.alerting.windows {
            if let (Ok(long), Ok(short)) = (Window::parse(&w.long), Window::parse(&w.short)) {
                if !seen_windows.insert((w.severity.as_str(), long, short)) {
                    out.push(Lint {
                        level: LintLevel::Warning,
                        code: "DUPLICATE_ALERT_WINDOW",
                        location: loc.clone(),
                        message: format!(
                            "duplicate {} condition with long {long} and short {short} in `alerting.windows`; only the lowest-factor duplicate has any effect",
                            w.severity
                        ),
                    });
                }
            }
        }

        for (field, map, reserved) in [
            ("labels", &slo.labels, true),
            ("alerting.labels", &slo.alerting.labels, true),
            ("alerting.annotations", &slo.alerting.annotations, false),
            (
                "alerting.page_alert.labels",
                &slo.alerting.page_alert.labels,
                true,
            ),
            (
                "alerting.page_alert.annotations",
                &slo.alerting.page_alert.annotations,
                false,
            ),
            (
                "alerting.ticket_alert.labels",
                &slo.alerting.ticket_alert.labels,
                true,
            ),
            (
                "alerting.ticket_alert.annotations",
                &slo.alerting.ticket_alert.annotations,
                false,
            ),
        ] {
            label_name_lints(&mut out, &loc, field, map, reserved);
        }

        // SLO period shorter than (or equal to) the longest effective
        // burn-rate window. With period-aware scaling the default table always
        // fits; this mostly catches custom windows that outgrow the period.
        if let (Ok(period), Some(mwmbr)) = (slo.resolve_period(DEFAULT_PERIOD), &effective_mwmbr) {
            if let Some(longest_window) = mwmbr.windows.iter().map(|w| w.long).max() {
                if period <= longest_window {
                    out.push(Lint {
                        level: LintLevel::Warning,
                        code: "PERIOD_TOO_SHORT",
                        location: loc.clone(),
                        message: format!(
                            "period {period} is not longer than the longest burn-rate window ({longest_window}); long-window alerts will not be meaningful"
                        ),
                    });
                }
            }
        }

        // Alert routing.
        let page_disabled = slo.alerting.page_alert.disable;
        let ticket_disabled = slo.alerting.ticket_alert.disable;
        if page_disabled && ticket_disabled {
            out.push(Lint {
                level: LintLevel::Warning,
                code: "ALL_ALERTS_DISABLED",
                location: loc.clone(),
                message:
                    "both page and ticket alerts are disabled; no burn-rate alerts will be generated for this SLO"
                        .to_string(),
            });
        } else {
            let has_shared_labels = !slo.alerting.labels.is_empty();
            if !page_disabled && slo.alerting.page_alert.labels.is_empty() && !has_shared_labels {
                out.push(Lint {
                    level: LintLevel::Warning,
                    code: "NO_ALERT_LABELS",
                    location: loc.clone(),
                    message:
                        "page alert has no labels (e.g. `severity`); Alertmanager routing may not match it"
                            .to_string(),
                });
            }
            if !ticket_disabled && slo.alerting.ticket_alert.labels.is_empty() && !has_shared_labels
            {
                out.push(Lint {
                    level: LintLevel::Warning,
                    code: "NO_ALERT_LABELS",
                    location: loc.clone(),
                    message:
                        "ticket alert has no labels (e.g. `severity`); Alertmanager routing may not match it"
                            .to_string(),
                });
            }

            // Custom windows that leave an enabled severity with no conditions
            // silently drop that alert.
            if let Some(custom) = &custom {
                for (disabled, severity) in [
                    (page_disabled, Severity::Page),
                    (ticket_disabled, Severity::Ticket),
                ] {
                    if !disabled && custom.for_severity(severity).next().is_none() {
                        out.push(Lint {
                            level: LintLevel::Warning,
                            code: "NO_SEVERITY_WINDOWS",
                            location: loc.clone(),
                            message: format!(
                                "custom `alerting.windows` has no {} conditions, so no {} alert will be generated; add one or disable the alert",
                                severity.label(),
                                severity.label(),
                            ),
                        });
                    }
                }
            }
        }

        // Missing description (informational).
        if slo.description.trim().is_empty() {
            out.push(Lint {
                level: LintLevel::Info,
                code: "NO_DESCRIPTION",
                location: loc.clone(),
                message:
                    "SLO has no description; add one so generated alerts and dashboards are self-explanatory"
                        .to_string(),
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(spec: &Spec) -> Vec<&'static str> {
        lint(spec).into_iter().map(|l| l.code).collect()
    }

    const CLEAN: &str = r#"
service: api
slos:
  - name: availability
    objective: 99.9
    description: "99.9% of requests succeed"
    sli:
      events:
        error_query: sum(rate(err[{{.window}}]))
        total_query: sum(rate(tot[{{.window}}]))
    alerting:
      labels: { severity: page }
"#;

    #[test]
    fn clean_spec_has_no_findings() {
        let spec = Spec::from_yaml(CLEAN).unwrap();
        assert!(lint(&spec).is_empty(), "{:?}", lint(&spec));
    }

    #[test]
    fn objective_100_warns() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 100
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(codes(&spec).contains(&"OBJECTIVE_100"));
    }

    #[test]
    fn low_objective_warns() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 40
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(codes(&spec).contains(&"OBJECTIVE_LOW"));
    }

    #[test]
    fn short_period_with_scaled_defaults_does_not_warn() {
        // Default windows scale with the period, so even a 1d period gets
        // windows that fit inside it.
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    period: 1d
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(!codes(&spec).contains(&"PERIOD_TOO_SHORT"));
    }

    #[test]
    fn custom_window_longer_than_period_warns() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    period: 1d
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 3d
          short: 6h
          factor: 1
        - severity: ticket
          long: 1h
          short: 5m
          factor: 2
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(codes(&spec).contains(&"PERIOD_TOO_SHORT"));
    }

    #[test]
    fn custom_windows_missing_a_severity_warn() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 14.4
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let c = codes(&spec);
        assert!(c.contains(&"NO_SEVERITY_WINDOWS"));
    }

    #[test]
    fn custom_windows_for_both_severities_are_clean() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 14.4
        - severity: ticket
          long: 1d
          short: 2h
          factor: 3
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(!codes(&spec).contains(&"NO_SEVERITY_WINDOWS"));
        assert!(!codes(&spec).contains(&"PERIOD_TOO_SHORT"));
    }

    #[test]
    fn disabled_severity_needs_no_custom_windows() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      ticket_alert: { disable: true }
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 14.4
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(!codes(&spec).contains(&"NO_SEVERITY_WINDOWS"));
    }

    #[test]
    fn long_period_does_not_warn_on_period() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    period: 30d
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(!codes(&spec).contains(&"PERIOD_TOO_SHORT"));
    }

    #[test]
    fn missing_alert_labels_warn() {
        // No shared labels and no per-severity labels.
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let n = lint(&spec)
            .iter()
            .filter(|l| l.code == "NO_ALERT_LABELS")
            .count();
        assert_eq!(n, 2, "expected page + ticket findings");
    }

    #[test]
    fn all_alerts_disabled_warns_and_skips_label_check() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      page_alert: { disable: true }
      ticket_alert: { disable: true }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let c = codes(&spec);
        assert!(c.contains(&"ALL_ALERTS_DISABLED"));
        assert!(!c.contains(&"NO_ALERT_LABELS"));
    }

    #[test]
    fn missing_description_is_info() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let found = lint(&spec)
            .into_iter()
            .find(|l| l.code == "NO_DESCRIPTION")
            .expect("expected NO_DESCRIPTION");
        assert_eq!(found.level, LintLevel::Info);
    }

    #[test]
    fn unknown_spec_version_warns() {
        let yaml = r#"
version: "prometheus/v2"
service: api
slos:
  - name: a
    objective: 99.9
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let found = lint(&spec)
            .into_iter()
            .find(|l| l.code == "SPEC_VERSION")
            .expect("expected SPEC_VERSION");
        assert_eq!(found.location, "spec");
        assert!(found.message.contains("prometheus/v2"));
    }

    #[test]
    fn default_and_explicit_v1_versions_do_not_warn() {
        let spec = Spec::from_yaml(CLEAN).unwrap();
        assert!(!codes(&spec).contains(&"SPEC_VERSION"));
        let explicit = format!("version: \"prometheus/v1\"\n{CLEAN}");
        let spec = Spec::from_yaml(&explicit).unwrap();
        assert!(!codes(&spec).contains(&"SPEC_VERSION"));
    }

    #[test]
    fn label_names_outside_the_legacy_charset_warn() {
        let yaml = r#"
service: api
labels:
  "team name": platform
slos:
  - name: a
    objective: 99.9
    description: d
    labels:
      foo-bar: x
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      annotations:
        "runbook url": https://example.com
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let findings: Vec<_> = lint(&spec)
            .into_iter()
            .filter(|l| l.code == "LABEL_NAME_CHARS")
            .collect();
        assert_eq!(findings.len(), 3, "{findings:?}");
        assert!(findings.iter().any(|l| l.location == "spec"));
        assert!(findings.iter().any(|l| l.message.contains("foo-bar")));
        assert!(findings.iter().any(|l| l.message.contains("runbook url")));
    }

    #[test]
    fn reserved_sloth_label_warns() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.9
    description: d
    labels:
      sloth_service: impostor
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let found = lint(&spec)
            .into_iter()
            .find(|l| l.code == "RESERVED_LABEL")
            .expect("expected RESERVED_LABEL");
        assert!(found.message.contains("sloth_service"));
    }

    #[test]
    fn sloth_prefixed_annotations_are_not_reserved() {
        // Annotations never collide with the generated identity labels.
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.9
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      annotations: { sloth_note: fine }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let c = codes(&spec);
        assert!(!c.contains(&"RESERVED_LABEL"));
        assert!(!c.contains(&"LABEL_NAME_CHARS"));
    }

    #[test]
    fn unreachable_custom_threshold_warns() {
        // Objective 99% => budget 0.01; factor 150 => threshold 1.5 (> 1).
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 150
        - severity: ticket
          long: 1d
          short: 2h
          factor: 2
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let findings: Vec<_> = lint(&spec)
            .into_iter()
            .filter(|l| l.code == "THRESHOLD_UNREACHABLE")
            .collect();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("can never fire"));
    }

    #[test]
    fn low_objective_makes_default_page_thresholds_unreachable() {
        // Objective 60% => budget 0.4; default factors 14.4, 6, and 3 all give
        // thresholds >= 1, while factor 1 stays reachable (0.4).
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 60
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting: { labels: { severity: page } }
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let n = lint(&spec)
            .iter()
            .filter(|l| l.code == "THRESHOLD_UNREACHABLE")
            .count();
        assert_eq!(n, 3);
    }

    #[test]
    fn reachable_thresholds_do_not_warn() {
        let spec = Spec::from_yaml(CLEAN).unwrap();
        assert!(!codes(&spec).contains(&"THRESHOLD_UNREACHABLE"));
    }

    #[test]
    fn duplicate_alert_windows_warn_even_across_spellings() {
        // 1800s and 30m are the same window; the duplicate must be caught
        // after parsing, not by raw string comparison.
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 30m
          short: 5m
          factor: 10
        - severity: page
          long: 1800s
          short: 5m
          factor: 6
        - severity: ticket
          long: 12h
          short: 1h
          factor: 2
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let findings: Vec<_> = lint(&spec)
            .into_iter()
            .filter(|l| l.code == "DUPLICATE_ALERT_WINDOW")
            .collect();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("30m"));
    }

    #[test]
    fn distinct_alert_windows_do_not_warn() {
        let yaml = r#"
service: api
slos:
  - name: a
    objective: 99.0
    description: d
    sli: { raw: { error_ratio_query: "r[{{.window}}]" } }
    alerting:
      labels: { severity: page }
      windows:
        - severity: page
          long: 1h
          short: 5m
          factor: 14.4
        - severity: ticket
          long: 1h
          short: 5m
          factor: 3
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(!codes(&spec).contains(&"DUPLICATE_ALERT_WINDOW"));
    }
}
