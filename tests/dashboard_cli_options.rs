//! Binary-level proof that `slokit dashboard` can be told what `slokit
//! generate` was told.
//!
//! `tests/dashboard_drift.rs` proves the LIBRARY resolution agrees across the
//! option space. That is necessary and not sufficient: the option space is
//! reached from the CLI, and before this change `slokit dashboard` accepted
//! only `-i/--output`, so there was no invocation of the real binary that
//! produced a dashboard matching `slokit generate --period 7d`. A library-only
//! guard stays green through exactly that gap.
//!
//! Every test here drives the installed command and reads its stdout, so it
//! fails if the flags are removed, renamed, or parsed and discarded — the
//! failure mode svccat's `--filter` shipped with for several releases.

#![cfg(feature = "cli")]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

/// A spec that sets no `period`, so `--period` is what decides it.
const SPEC: &str = r#"
service: myservice
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: r[{{.window}}]
"#;

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn ok(args: &[&str]) -> String {
    let out = slokit(args);
    assert!(
        out.status.success(),
        "slokit {args:?} exited {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    stdout(&out)
}

/// Write `SPEC` into a fresh temp dir and hand back the file path.
fn spec_file(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-dashboard-cli-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("spec.yaml");
    fs::write(&path, SPEC).unwrap();
    path
}

/// Every `slo:` series name a `record:` key in the rules YAML declares.
fn recorded(rules_yaml: &str) -> BTreeSet<String> {
    let doc: serde_norway::Value =
        serde_norway::from_str(rules_yaml).expect("generate emits parseable YAML");
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
        "no recording rules parsed out of:\n{rules_yaml}"
    );
    out
}

/// Every `slo:` series name any dashboard `expr` references.
fn referenced(dashboard_json: &str) -> BTreeSet<String> {
    let value: Value =
        serde_json::from_str(dashboard_json).expect("dashboard emits parseable JSON");
    let mut out = BTreeSet::new();
    collect_exprs(&value, &mut |expr| {
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
    assert!(
        !out.is_empty(),
        "no slo: series referenced by:\n{dashboard_json}"
    );
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

#[test]
fn a_dashboard_built_with_the_same_period_queries_only_recorded_series() {
    let path = spec_file("period");
    let p = path.to_str().unwrap();

    let rules = ok(&["generate", "-i", p, "--period", "7d"]);
    let dash = ok(&["dashboard", "-i", p, "--period", "7d"]);

    let recorded = recorded(&rules);
    let referenced = referenced(&dash);
    let missing: Vec<&String> = referenced.difference(&recorded).collect();
    assert!(
        missing.is_empty(),
        "`slokit dashboard --period 7d` queries series `slokit generate --period 7d` \
         never records: {missing:?}\nrecorded: {recorded:?}"
    );
    // Not a vacuous agreement: 7d really moved the windows off the 30d table.
    assert!(
        referenced.contains("slo:sli_error:ratio_rate1m"),
        "the 7d-scaled base window must appear: {referenced:?}"
    );
}

#[test]
fn a_dashboard_built_with_the_same_no_period_scaling_queries_only_recorded_series() {
    let path = spec_file("noscale");
    let p = path.to_str().unwrap();

    let rules = ok(&["generate", "-i", p, "--period", "7d", "--no-period-scaling"]);
    let dash = ok(&[
        "dashboard",
        "-i",
        p,
        "--period",
        "7d",
        "--no-period-scaling",
    ]);

    let recorded = recorded(&rules);
    let referenced = referenced(&dash);
    let missing: Vec<&String> = referenced.difference(&recorded).collect();
    assert!(
        missing.is_empty(),
        "`slokit dashboard --no-period-scaling` queries series the matching generate \
         never records: {missing:?}\nrecorded: {recorded:?}"
    );
    // With scaling off both sides stay on the verbatim 30d table.
    assert!(
        referenced.contains("slo:sli_error:ratio_rate5m"),
        "the verbatim base window must appear: {referenced:?}"
    );
}

#[test]
fn the_flags_change_the_emitted_dashboard_rather_than_being_parsed_and_discarded() {
    let path = spec_file("difference");
    let p = path.to_str().unwrap();

    let default = ok(&["dashboard", "-i", p]);
    let scaled = ok(&["dashboard", "-i", p, "--period", "7d"]);
    let verbatim = ok(&[
        "dashboard",
        "-i",
        p,
        "--period",
        "7d",
        "--no-period-scaling",
    ]);

    assert_ne!(
        default, scaled,
        "--period 7d left the dashboard byte-identical, so the flag does nothing"
    );
    // `--no-period-scaling` reverts the burn windows to the verbatim 30d
    // table, but the TIME RANGE still follows the resolved 7d period: the flag
    // is about window scaling, not about how much history the dashboard opens
    // on. So the two documents must differ in `time` and in nothing else --
    // which is a sharper claim than the byte-equality this assertion made
    // while the range was hardcoded to `now-30d`.
    let mut default_doc: Value = serde_json::from_str(&default).expect("default parses");
    let mut verbatim_doc: Value = serde_json::from_str(&verbatim).expect("verbatim parses");
    assert_eq!(
        verbatim_doc["time"]["from"], "now-7d",
        "--period 7d --no-period-scaling must still open on the resolved 7d period"
    );
    default_doc["time"] = Value::Null;
    verbatim_doc["time"] = Value::Null;
    assert_eq!(
        default_doc, verbatim_doc,
        "outside the time range, --period 7d --no-period-scaling must reproduce \
         the verbatim 30d table, which is exactly the default dashboard for this \
         spec"
    );

    let default_series = referenced(&default);
    let scaled_series = referenced(&scaled);
    assert!(
        default_series.contains("slo:sli_error:ratio_rate5m")
            && !default_series.contains("slo:sli_error:ratio_rate1m"),
        "default: {default_series:?}"
    );
    assert!(
        scaled_series.contains("slo:sli_error:ratio_rate1m")
            && !scaled_series.contains("slo:sli_error:ratio_rate5m"),
        "--period 7d: {scaled_series:?}"
    );
}

#[test]
fn dashboard_offers_every_window_resolving_flag_generate_does() {
    // A drift guard on the two commands' own `--help`, read from the real
    // binary: the resolution is shared in the library, so a flag that exists
    // on one side and not the other silently reintroduces the divergence.
    let generate_help = ok(&["generate", "--help"]);
    let dashboard_help = ok(&["dashboard", "--help"]);
    for flag in ["--period", "--no-period-scaling"] {
        assert!(
            generate_help.contains(flag),
            "`generate --help` no longer lists {flag}; this guard's premise moved:\n{generate_help}"
        );
        assert!(
            dashboard_help.contains(flag),
            "`dashboard --help` does not list {flag}, so no invocation can make the \
             dashboard match those rules:\n{dashboard_help}"
        );
    }
}
