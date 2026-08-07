//! Drift guard between the dashboard and the rule generator (two-source).
//!
//! Every PromQL expression in the emitted dashboard must reference only `slo:`
//! series the generator actually records for the same spec. This is what makes
//! the dashboard checkable without live data: if a panel queries a series no
//! recording rule produces, the panel would render "no data" against a real
//! Prometheus, and this suite fails instead.
//!
//! Both sources are the real artifacts: the dashboard side walks
//! [`slokit::dashboard::dashboard_value`] output, and the recorded side parses
//! the YAML that [`slokit::generate::generate_rules`] renders (the same bytes
//! `slokit generate` writes), not any internal intermediate.

#![cfg(feature = "dashboard")]

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;
use slokit::dashboard::dashboard_value;
use slokit::generate::generate_rules;
use slokit::spec::Spec;

/// Every committed spec the guard runs over: the sample fixture, the
/// multi-document fixture, and the whole example set. Glob-discovered with a
/// zero-match hard failure so a new example is covered automatically and an
/// emptied directory cannot pass vacuously.
fn committed_specs() -> Vec<(String, Spec)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![
        root.join("tests/fixtures/sample.yaml"),
        root.join("tests/fixtures/multifile.yaml"),
    ];
    let examples = root.join("examples/infraportal/slos");
    let mut example_files: Vec<PathBuf> = std::fs::read_dir(&examples)
        .unwrap_or_else(|e| panic!("reading {}: {e}", examples.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    assert!(
        !example_files.is_empty(),
        "no example specs discovered under {}",
        examples.display()
    );
    example_files.sort();
    files.extend(example_files);

    let mut specs = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        for spec in Spec::from_yaml_stream(&text)
            .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
        {
            specs.push((path.display().to_string(), spec));
        }
    }
    specs
}

/// Every `slo:` metric name referenced by any expression in `value`, found by
/// walking the JSON tree for `expr` strings.
fn referenced_series(value: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_exprs(value, &mut |expr| {
        let mut rest = expr;
        while let Some(pos) = rest.find("slo:") {
            let tail = &rest[pos..];
            let end = tail
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(tail.len());
            out.insert(tail[..end].to_string());
            rest = &tail[end..];
        }
    });
    out
}

fn collect_exprs(value: &Value, f: &mut impl FnMut(&str)) {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key == "expr" {
                    if let Some(s) = v.as_str() {
                        f(s);
                    }
                }
                collect_exprs(v, f);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_exprs(v, f);
            }
        }
        _ => {}
    }
}

/// Every series name the generator records for `spec`, read from the rendered
/// Prometheus rules YAML (`record:` keys).
fn recorded_series(spec: &Spec) -> BTreeSet<String> {
    let yaml = generate_rules(spec)
        .expect("generation must succeed for a committed spec")
        .to_prometheus_yaml()
        .expect("rendering must succeed");
    let doc: serde_norway::Value = serde_norway::from_str(&yaml).expect("generator output is YAML");
    let mut out = BTreeSet::new();
    for group in doc["groups"].as_sequence().into_iter().flatten() {
        for rule in group["rules"].as_sequence().into_iter().flatten() {
            if let Some(name) = rule.get("record").and_then(|r| r.as_str()) {
                out.insert(name.to_string());
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no recording rules parsed out of the generator's YAML"
    );
    out
}

#[test]
fn every_dashboard_expression_references_only_recorded_series() {
    for (path, spec) in committed_specs() {
        let referenced = referenced_series(&dashboard_value(&spec));
        assert!(
            !referenced.is_empty(),
            "{path}: the dashboard for '{}' references no slo: series at all; \
             the extractor or the dashboard is broken",
            spec.service
        );
        let recorded = recorded_series(&spec);
        let unrecorded: Vec<&String> = referenced.difference(&recorded).collect();
        assert!(
            unrecorded.is_empty(),
            "{path}: dashboard for '{}' references series the generator does not \
             record: {unrecorded:?}\nrecorded: {recorded:?}",
            spec.service
        );
    }
}

#[test]
fn custom_windows_stay_in_step_between_dashboard_and_generator() {
    // A spec whose custom windows record a non-default window set: if the
    // dashboard fell back to the default table anywhere, it would reference
    // 5m/30m/6h/1d/3d series this generator run never records.
    let spec = Spec::from_yaml(
        r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
    alerting:
      windows:
        - severity: page
          long: 4h
          short: 20m
          factor: 7
"#,
    )
    .unwrap();
    let referenced = referenced_series(&dashboard_value(&spec));
    let recorded = recorded_series(&spec);
    assert!(
        referenced.contains("slo:sli_error:ratio_rate4h"),
        "the burn panel must query the custom long window: {referenced:?}"
    );
    let unrecorded: Vec<&String> = referenced.difference(&recorded).collect();
    assert!(
        unrecorded.is_empty(),
        "dashboard references unrecorded series: {unrecorded:?}"
    );
}
