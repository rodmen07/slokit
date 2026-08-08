//! Drift guard between the dashboard and the rule generator (two-source).
//!
//! Every PromQL expression in the emitted dashboard must reference only `slo:`
//! series the generator actually records for the same spec. This is what makes
//! the dashboard checkable without live data: if a panel queries a series no
//! recording rule produces, the panel would render "no data" against a real
//! Prometheus, and this suite fails instead.
//!
//! Both sources are the real artifacts: the dashboard side walks
//! [`slokit::dashboard::dashboard_value_with`] output, and the recorded side
//! parses the YAML that [`slokit::generate::generate_rules_with`] renders (the
//! same bytes `slokit generate` writes), not any internal intermediate.
//!
//! **The guard runs across the OPTION SPACE, not only under default options.**
//! The window-scoped series carry the resolved burn-rate window in their NAME
//! (`slo:sli_error:ratio_rate<window>`), so the two sides agree only while they
//! resolve the same windows — and the resolution reads `GenerateOptions`. A
//! guard pinned to `GenerateOptions::default()` was blind to exactly that: it
//! stayed green while `slokit generate --period 7d` and `slokit dashboard`
//! disagreed about all seven window-scoped series, and `--no-period-scaling`
//! disagreed about the same seven in the opposite direction. Each of those two
//! defects has its own named regression test below; the matrix test is what
//! keeps a third one from being introduced by a future option.

#![cfg(feature = "dashboard")]

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;
use slokit::dashboard::dashboard_value_with;
use slokit::generate::{generate_rules_with, GenerateOptions};
use slokit::spec::Spec;
use slokit::Window;

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

/// The committed specs plus two synthetic ones whose periods are NOT 30d.
///
/// What actually keeps the scaling axis from being vacuous is measured, and it
/// is worth stating because the obvious answer is wrong. Every `examples/`
/// spec declares `period: 30d`, which IS `DEFAULT_PERIOD`, so `scaled(30d,
/// 30d)` is the identity there and no option moves them; the entry that bites
/// is `--period 7d --no-period-scaling` against `tests/fixtures/sample.yaml`,
/// which declares no period at all. A one-sided control (the dashboard forcing
/// `period_aware = true` while the generator honours it) reddens the matrix
/// naming exactly that spec and that invocation, with or without the synthetic
/// pair — so the pair is NOT load-bearing for it.
///
/// They stay for the case the committed corpus cannot state: an SLO whose own
/// `period:` field is non-default, as opposed to one that inherits `--period`.
/// That is a distinct resolution path (`resolve_period`'s `Some` arm rather
/// than its default arm) and nothing committed exercises it.
///
/// A mutation in the SHARED seam cannot be caught here by construction, and
/// that is by design: this file guards AGREEMENT between two sources, and a
/// seam both sides read moves both sides together. The named regression tests
/// below assert the absolute expected series for that reason.
fn drift_specs() -> Vec<(String, Spec)> {
    let mut specs = committed_specs();
    specs.push((
        "<synthetic: no period, so --period decides>".to_string(),
        Spec::from_yaml(NO_PERIOD_SPEC).unwrap(),
    ));
    specs.push((
        "<synthetic: period 7d>".to_string(),
        Spec::from_yaml(SEVEN_DAY_SPEC).unwrap(),
    ));
    specs
}

/// The generator option space a `slokit generate` user can actually reach from
/// the CLI, each entry labelled with the invocation that produces it.
///
/// Every one of these must also be reachable from `slokit dashboard`, which is
/// what `tests/dashboard_cli_options.rs` proves against the real binary; here
/// they exercise the library resolution both commands share.
fn option_matrix() -> Vec<(&'static str, GenerateOptions)> {
    let mut out = Vec::new();

    out.push(("generate", GenerateOptions::default()));

    let mut no_scaling = GenerateOptions::default();
    no_scaling.period_aware = false;
    out.push(("generate --no-period-scaling", no_scaling));

    let mut short_period = GenerateOptions::default();
    short_period.default_period = Window::days(7);
    out.push(("generate --period 7d", short_period));

    let mut long_period = GenerateOptions::default();
    long_period.default_period = Window::days(90);
    out.push(("generate --period 90d", long_period));

    let mut both = GenerateOptions::default();
    both.default_period = Window::days(7);
    both.period_aware = false;
    out.push(("generate --period 7d --no-period-scaling", both));

    out
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

/// Every series name the generator records for `spec` under `opts`, read from
/// the rendered Prometheus rules YAML (`record:` keys).
fn recorded_series(spec: &Spec, opts: &GenerateOptions) -> BTreeSet<String> {
    let yaml = generate_rules_with(spec, opts)
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

/// The dashboard series that `opts` never records: empty is the contract.
fn unrecorded(spec: &Spec, opts: &GenerateOptions) -> Vec<String> {
    let referenced = referenced_series(&dashboard_value_with(spec, opts));
    assert!(
        !referenced.is_empty(),
        "the dashboard for '{}' references no slo: series at all; \
         the extractor or the dashboard is broken",
        spec.service
    );
    let recorded = recorded_series(spec, opts);
    referenced.difference(&recorded).cloned().collect()
}

#[test]
fn every_dashboard_expression_references_only_recorded_series() {
    for (path, spec) in committed_specs() {
        let missing = unrecorded(&spec, &GenerateOptions::default());
        assert!(
            missing.is_empty(),
            "{path}: dashboard for '{}' references series the generator does not \
             record: {missing:?}",
            spec.service
        );
    }
}

#[test]
fn every_dashboard_expression_stays_recorded_under_every_generate_option() {
    let specs = drift_specs();
    assert!(
        specs.len() > 2,
        "only the two synthetic specs survived discovery; the committed set is empty"
    );
    let matrix = option_matrix();
    assert!(
        matrix.len() >= 5,
        "the option matrix collapsed to {} entries; it must cover both \
         --period and --no-period-scaling",
        matrix.len()
    );
    for (invocation, opts) in &matrix {
        for (path, spec) in &specs {
            let missing = unrecorded(spec, opts);
            assert!(
                missing.is_empty(),
                "`slokit {invocation}` + the matching dashboard disagree for {path} \
                 ('{}'): the dashboard references series those rules do not record: \
                 {missing:?}",
                spec.service
            );
        }
    }
}

/// A spec that sets no `period`, so `--period` decides it. Under the defect
/// the dashboard hardcoded 30d here and queried the unscaled default windows
/// while the generator recorded windows scaled to 7d.
const NO_PERIOD_SPEC: &str = r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;

/// A spec with a non-30d `period`. Under the defect the dashboard always
/// scaled while `--no-period-scaling` told the generator not to, so the two
/// disagreed in the opposite direction.
const SEVEN_DAY_SPEC: &str = r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    period: 7d
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;

#[test]
fn a_non_default_period_keeps_the_dashboard_on_recorded_series() {
    let spec = Spec::from_yaml(NO_PERIOD_SPEC).unwrap();
    let mut opts = GenerateOptions::default();
    opts.default_period = Window::days(7);

    // The generator scales the 30d table to 7d: 5m becomes 1m.
    let recorded = recorded_series(&spec, &opts);
    assert!(
        recorded.contains("slo:sli_error:ratio_rate1m"),
        "the 7d-scaled base window must be recorded: {recorded:?}"
    );
    assert!(
        !recorded.contains("slo:sli_error:ratio_rate5m"),
        "the unscaled 30d base window must NOT be recorded under --period 7d: {recorded:?}"
    );

    // ... and the dashboard must follow it there rather than to the 30d table.
    let referenced = referenced_series(&dashboard_value_with(&spec, &opts));
    assert!(
        referenced.contains("slo:sli_error:ratio_rate1m"),
        "the SLI panel must query the 7d-scaled base window: {referenced:?}"
    );
    let missing = unrecorded(&spec, &opts);
    assert!(
        missing.is_empty(),
        "`generate --period 7d` records nothing for these dashboard series: {missing:?}"
    );
}

#[test]
fn no_period_scaling_keeps_the_dashboard_on_recorded_series() {
    let spec = Spec::from_yaml(SEVEN_DAY_SPEC).unwrap();
    let mut opts = GenerateOptions::default();
    opts.period_aware = false;

    // With scaling off the generator records the 30d table verbatim.
    let recorded = recorded_series(&spec, &opts);
    assert!(
        recorded.contains("slo:sli_error:ratio_rate5m"),
        "the verbatim 30d base window must be recorded: {recorded:?}"
    );

    // ... so the dashboard must NOT scale to the SLO's 7d period either.
    let referenced = referenced_series(&dashboard_value_with(&spec, &opts));
    assert!(
        referenced.contains("slo:sli_error:ratio_rate5m"),
        "the SLI panel must query the verbatim base window: {referenced:?}"
    );
    let missing = unrecorded(&spec, &opts);
    assert!(
        missing.is_empty(),
        "`generate --no-period-scaling` records nothing for these dashboard series: {missing:?}"
    );
}

#[test]
fn custom_windows_stay_in_step_between_dashboard_and_generator() {
    // A spec whose custom windows record a non-default window set: if the
    // dashboard fell back to the default table anywhere, it would reference
    // 5m/30m/6h/1d/3d series this generator run never records. Custom windows
    // are option-independent by design (they replace the table outright), so
    // this must hold across the whole matrix.
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
    for (invocation, opts) in option_matrix() {
        let referenced = referenced_series(&dashboard_value_with(&spec, &opts));
        assert!(
            referenced.contains("slo:sli_error:ratio_rate4h"),
            "`slokit {invocation}`: the burn panel must query the custom long window, \
             which no option may rescale: {referenced:?}"
        );
        let missing = unrecorded(&spec, &opts);
        assert!(
            missing.is_empty(),
            "`slokit {invocation}`: dashboard references unrecorded series: {missing:?}"
        );
    }
}

#[test]
fn the_default_dashboard_entry_points_still_mean_default_options() {
    // `dashboard_value` is the 1.x entry point and its output is what every
    // existing consumer already pins; the `_with` form must be a superset, not
    // a replacement that quietly moved the default.
    for (_, spec) in committed_specs() {
        assert_eq!(
            slokit::dashboard::dashboard_value(&spec),
            dashboard_value_with(&spec, &GenerateOptions::default()),
            "dashboard_value must equal dashboard_value_with(.., default) for '{}'",
            spec.service
        );
    }
}
