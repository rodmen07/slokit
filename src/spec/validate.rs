//! Semantic validation for [`Spec`](super::Spec).
//!
//! Parsing only guarantees the YAML shape is right. Validation catches the
//! mistakes that would otherwise produce broken or misleading Prometheus rules:
//! out-of-range objectives, missing SLIs, duplicate names, and queries that
//! forgot the `{{.window}}` template token.

use std::collections::HashSet;

use crate::error::{Result, SlokitError};
use crate::sli::WINDOW_TOKEN;
use crate::slo::Objective;
use crate::window::Window;

use super::Spec;

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
            }
            match lat.threshold.trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v > 0.0 => {}
                _ => errors.push(format!(
                    "{where_}: latency `threshold` must be a positive number, got '{}'",
                    lat.threshold
                )),
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
}
