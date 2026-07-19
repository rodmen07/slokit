//! Semantic validation for [`Spec`](super::Spec).
//!
//! Parsing only guarantees the YAML shape is right. Validation catches the
//! mistakes that would otherwise produce broken or misleading Prometheus rules:
//! out-of-range objectives, missing SLIs, duplicate names, and queries that
//! forgot the `{{.window}}` template token.

use std::collections::{BTreeMap, HashSet};

use crate::error::{Result, SlokitError};
use crate::sli::WINDOW_TOKEN;
use crate::slo::Objective;
use crate::window::Window;

use super::Spec;

/// The classic Prometheus metric-name charset (`[a-zA-Z_:][a-zA-Z0-9_:]*`).
///
/// The latency SLI embeds `histogram_metric` unquoted in the generated PromQL,
/// so anything outside this set is a syntax error regardless of the Prometheus
/// version (UTF-8 metric names would require quoted selector syntax).
fn is_metric_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

/// True when every double quote in `s` is paired (backslash-escaped quotes
/// inside a string do not count as delimiters).
fn quotes_balanced(s: &str) -> bool {
    let mut open = false;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if open => escaped = true,
            '"' => open = !open,
            _ => {}
        }
    }
    !open
}

/// Report an empty key in a label/annotation map. Empty names are rejected by
/// Prometheus under every name-validation scheme, so the generated rules file
/// would refuse to load.
fn empty_key_error(
    errors: &mut Vec<String>,
    where_: &str,
    field: &str,
    map: &BTreeMap<String, String>,
) {
    if map.keys().any(|k| k.is_empty()) {
        errors.push(format!(
            "{where_}: `{field}` must not contain an empty name"
        ));
    }
}

/// Validate a spec. Returns [`SlokitError::Validation`] with one line per
/// problem, or `Ok(())` when the spec is sound.
pub fn validate(spec: &Spec) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    if spec.service.trim().is_empty() {
        errors.push("`service` must not be empty".to_string());
    }
    if spec.slos.is_empty() {
        errors.push("`slos` must contain at least one SLO".to_string());
    }
    empty_key_error(&mut errors, "spec", "labels", &spec.labels);

    let mut seen: HashSet<&str> = HashSet::new();
    for (i, slo) in spec.slos.iter().enumerate() {
        let where_ = if slo.name.is_empty() {
            format!("slos[{i}]")
        } else {
            format!("slo '{}'", slo.name)
        };

        if slo.name.trim().is_empty() {
            errors.push(format!("{where_}: `name` must not be empty"));
        } else if !seen.insert(slo.name.as_str()) {
            errors.push(format!("{where_}: duplicate SLO name"));
        }

        if let Err(e) = Objective::percent(slo.objective) {
            errors.push(format!("{where_}: {e}"));
        }

        for (field, map) in [
            ("labels", &slo.labels),
            ("alerting.labels", &slo.alerting.labels),
            ("alerting.annotations", &slo.alerting.annotations),
            (
                "alerting.page_alert.labels",
                &slo.alerting.page_alert.labels,
            ),
            (
                "alerting.page_alert.annotations",
                &slo.alerting.page_alert.annotations,
            ),
            (
                "alerting.ticket_alert.labels",
                &slo.alerting.ticket_alert.labels,
            ),
            (
                "alerting.ticket_alert.annotations",
                &slo.alerting.ticket_alert.annotations,
            ),
        ] {
            empty_key_error(&mut errors, &where_, field, map);
        }

        if !slo.alerting.name.is_empty() && slo.alerting.name.trim().is_empty() {
            errors.push(format!(
                "{where_}: `alerting.name` must not be whitespace-only (omit it to fall back to the SLO name)"
            ));
        }

        if let Some(period) = &slo.period {
            if let Err(e) = Window::parse(period) {
                errors.push(format!("{where_}: {e}"));
            }
        }

        match slo.to_sli() {
            Ok(sli) => {
                for query in sli.queries() {
                    if !query.contains(WINDOW_TOKEN) && !query.contains("{{ .window }}") {
                        errors.push(format!(
                            "{where_}: query is missing the {WINDOW_TOKEN} template token: {query}"
                        ));
                    }
                }
            }
            Err(e) => errors.push(format!("{where_}: {e}")),
        }

        for (wi, w) in slo.alerting.windows.iter().enumerate() {
            let where_w = format!("{where_}: alerting.windows[{wi}]");
            if !matches!(w.severity.as_str(), "page" | "ticket") {
                errors.push(format!(
                    "{where_w}: unknown severity '{}' (expected `page` or `ticket`)",
                    w.severity
                ));
            }
            if !w.factor.is_finite() || w.factor <= 0.0 {
                errors.push(format!(
                    "{where_w}: `factor` must be a positive number, got {}",
                    w.factor
                ));
            }
            let long = Window::parse(&w.long);
            let short = Window::parse(&w.short);
            if let Err(e) = &long {
                errors.push(format!("{where_w}: `long`: {e}"));
            }
            if let Err(e) = &short {
                errors.push(format!("{where_w}: `short`: {e}"));
            }
            if let (Ok(long), Ok(short)) = (long, short) {
                if short >= long {
                    errors.push(format!(
                        "{where_w}: `short` ({short}) must be shorter than `long` ({long})"
                    ));
                }
            }
        }

        if let Some(lat) = &slo.sli.latency {
            if lat.histogram_metric.trim().is_empty() {
                errors.push(format!(
                    "{where_}: latency `histogram_metric` must not be empty"
                ));
            } else if !is_metric_name(&lat.histogram_metric) {
                errors.push(format!(
                    "{where_}: latency `histogram_metric` '{}' is not a valid Prometheus metric name ([a-zA-Z_:][a-zA-Z0-9_:]*); it is embedded unquoted in the generated query",
                    lat.histogram_metric
                ));
            }
            // No trim before parsing: the threshold is embedded verbatim in the
            // `le="..."` matcher, so surrounding whitespace would generate a
            // matcher that can never match a real bucket label.
            match lat.threshold.parse::<f64>() {
                Ok(v) if v.is_finite() && v > 0.0 => {}
                _ => errors.push(format!(
                    "{where_}: latency `threshold` must be a positive number, got '{}'",
                    lat.threshold
                )),
            }
            if let Some(sel) = lat
                .selector
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if sel.contains('{') || sel.contains('}') {
                    errors.push(format!(
                        "{where_}: latency `selector` must not contain braces; write only the matchers, e.g. `job=\"api\"`"
                    ));
                }
                if sel.starts_with(',') || sel.ends_with(',') {
                    errors.push(format!(
                        "{where_}: latency `selector` must not start or end with a comma"
                    ));
                }
                if !quotes_balanced(sel) {
                    errors.push(format!(
                        "{where_}: latency `selector` has an unbalanced double quote"
                    ));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SlokitError::Validation(errors.join("\n")))
    }
}

/// Validate several specs together: every spec individually (each finding
/// prefixed with its service), plus cross-spec checks that only matter when
/// the specs' rules are merged into one file.
///
/// The cross-spec check rejects a service/SLO-name pair that appears in more
/// than one spec: merged output would repeat the rule-group names (and
/// `sloth_id`), and Prometheus refuses to load a rules file with duplicate
/// group names. Duplicates within a single spec are reported once, by the
/// per-spec pass.
pub fn validate_all(specs: &[Spec]) -> Result<()> {
    let mut errors: Vec<String> = Vec::new();

    for spec in specs {
        if let Err(e) = validate(spec) {
            match e {
                SlokitError::Validation(msg) => errors.extend(
                    msg.lines()
                        .map(|line| format!("service '{}': {line}", spec.service)),
                ),
                other => errors.push(format!("service '{}': {other}", spec.service)),
            }
        }
    }

    let mut seen: HashSet<(&str, &str)> = HashSet::new();
    for spec in specs {
        // Collapse within-spec duplicates first; `validate` already reports
        // those, so only cross-spec repeats are added here.
        let mut in_spec: HashSet<&str> = HashSet::new();
        for slo in &spec.slos {
            if !in_spec.insert(slo.name.as_str()) {
                continue;
            }
            if !seen.insert((spec.service.as_str(), slo.name.as_str())) {
                errors.push(format!(
                    "service '{}': slo '{}': duplicate service/SLO pair across specs; merged rule-group names would collide",
                    spec.service, slo.name
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SlokitError::Validation(errors.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_sound_spec() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(err[{{.window}}]))
        total_query: sum(rate(tot[{{.window}}]))
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn reports_multiple_problems() {
        let yaml = r#"
service: ""
slos:
  - name: a
    objective: 150
    sli:
      events:
        error_query: sum(rate(err[5m]))
        total_query: sum(rate(tot[{{.window}}]))
  - name: a
    objective: 99.0
    sli: {}
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let err = spec.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`service` must not be empty"));
        assert!(msg.contains("not a percentage")); // objective 150
        assert!(msg.contains("missing the")); // error_query has no token
        assert!(msg.contains("duplicate SLO name"));
        assert!(msg.contains("has no `events`, `raw`, or `latency` SLI"));
    }

    #[test]
    fn reports_bad_custom_alert_windows() {
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
          short: 1h
          factor: 0
        - severity: page
          long: nonsense
          short: 5m
          factor: 10
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("unknown severity 'critical'"));
        assert!(msg.contains("`factor` must be a positive number"));
        assert!(msg.contains("must be shorter than"));
        assert!(msg.contains("`long`:"));
    }

    #[test]
    fn accepts_sound_custom_alert_windows() {
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
          long: 1h
          short: 5m
          factor: 14.4
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn reports_bad_latency_fields() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: ""
        threshold: abc
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("`histogram_metric` must not be empty"));
        assert!(msg.contains("`threshold` must be a positive number"));
    }

    #[test]
    fn whitespace_only_alerting_name_is_an_error() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      name: "   "
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("`alerting.name` must not be whitespace-only"));
    }

    #[test]
    fn empty_label_and_annotation_names_are_errors() {
        let yaml = r#"
service: s
labels:
  "": spec-level
slos:
  - name: a
    objective: 99.0
    labels:
      "": slo-level
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      annotations:
        "": note
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("spec: `labels` must not contain an empty name"));
        assert!(msg.contains("slo 'a': `labels` must not contain an empty name"));
        assert!(msg.contains("`alerting.annotations` must not contain an empty name"));
    }

    #[test]
    fn invalid_histogram_metric_name_is_an_error() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: "http duration seconds"
        threshold: "0.3"
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("is not a valid Prometheus metric name"));
    }

    #[test]
    fn histogram_metric_with_colons_is_accepted() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: "job:latency_seconds"
        threshold: "0.3"
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn padded_latency_threshold_is_an_error() {
        // The threshold is embedded verbatim in the `le="..."` matcher, so
        // surrounding whitespace would silently match nothing.
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: m
        threshold: " 0.3 "
"#;
        let spec = Spec::from_yaml(yaml).unwrap();
        let msg = spec.validate().unwrap_err().to_string();
        assert!(msg.contains("`threshold` must be a positive number, got ' 0.3 '"));
    }

    #[test]
    fn broken_latency_selectors_are_errors() {
        let cases = [
            ("'{job=\"x\"}'", "must not contain braces"),
            ("'job=\"x\",'", "must not start or end with a comma"),
            ("'job=\"x'", "unbalanced double quote"),
        ];
        for (selector, needle) in cases {
            let yaml = format!(
                r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: m
        threshold: "0.3"
        selector: {selector}
"#
            );
            let spec = Spec::from_yaml(&yaml).unwrap();
            let msg = spec.validate().unwrap_err().to_string();
            assert!(msg.contains(needle), "selector {selector}: {msg}");
        }
    }

    #[test]
    fn sound_selectors_including_escaped_quotes_are_accepted() {
        for selector in ["'job=\"api\", code=~\"5..\"'", r#"'job="a\"b"'"#] {
            let yaml = format!(
                r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      latency:
        histogram_metric: m
        threshold: "0.3"
        selector: {selector}
"#
            );
            let spec = Spec::from_yaml(&yaml).unwrap();
            assert!(spec.validate().is_ok(), "selector {selector} should pass");
        }
    }

    fn one_slo_spec(service: &str, name: &str) -> Spec {
        let yaml = format!(
            r#"
service: {service}
slos:
  - name: {name}
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{{{.window}}}}]
"#
        );
        Spec::from_yaml(&yaml).unwrap()
    }

    #[test]
    fn validate_all_accepts_distinct_identities() {
        // Same SLO name under different services, and same service with
        // different SLO names, are both fine.
        let specs = [
            one_slo_spec("alpha", "avail"),
            one_slo_spec("beta", "avail"),
            one_slo_spec("alpha", "latency"),
        ];
        assert!(validate_all(&specs).is_ok());
    }

    #[test]
    fn validate_all_rejects_duplicate_service_slo_pairs() {
        let specs = [
            one_slo_spec("alpha", "avail"),
            one_slo_spec("alpha", "avail"),
        ];
        let msg = validate_all(&specs).unwrap_err().to_string();
        assert!(msg.contains("duplicate service/SLO pair across specs"));
        assert!(msg.contains("service 'alpha'"));
        assert!(msg.contains("slo 'avail'"));
    }

    #[test]
    fn validate_all_prefixes_per_spec_errors_with_the_service() {
        let yaml = r#"
service: gamma
slos:
  - name: a
    objective: 150
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;
        let specs = [Spec::from_yaml(yaml).unwrap()];
        let msg = validate_all(&specs).unwrap_err().to_string();
        assert!(msg.contains("service 'gamma': slo 'a':"));
        assert!(msg.contains("not a percentage"));
    }

    #[test]
    fn validate_all_reports_in_spec_duplicates_once() {
        let yaml = r#"
service: s
slos:
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
  - name: a
    objective: 99.0
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;
        let specs = [Spec::from_yaml(yaml).unwrap()];
        let msg = validate_all(&specs).unwrap_err().to_string();
        assert_eq!(msg.matches("duplicate SLO name").count(), 1);
        assert!(!msg.contains("across specs"));
    }
}
