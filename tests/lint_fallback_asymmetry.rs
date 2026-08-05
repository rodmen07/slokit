//! `SLI_FALLBACK_ASYMMETRY` (v1.3.0 PR 2), at the binary level.
//!
//! Grounding (ROADMAP v1.3.0 slice 2): both SLOs of sloth's real
//! `examples/home-wifi.yml` guard `events.error_query` with `OR on()
//! vector(0)` while `total_query` has no fallback, so the moment
//! `unifipoller_client_satisfaction_ratio` stops being scraped the ratio
//! evaluates to empty and every burn-rate alert silently stops evaluating.
//! The committed fixture `tests/fixtures/fallback_asymmetry.yaml` carries
//! that exact pattern; these tests pin the rule's CLI surface and the
//! milestone's done-when: fires on the fixture, silent on symmetric and
//! no-fallback specs, and the whole committed example set stays lint-clean.
//!
//! Per-rule unit tests live in `src/spec/lint.rs`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn manifest_path(rel: &str) -> String {
    format!("{}/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

fn temp_spec(tag: &str, contents: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-fallback-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("spec.yaml");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn the_committed_fixture_fires_the_rule_for_both_slos() {
    let out = slokit(&[
        "lint",
        "-i",
        &manifest_path("tests/fixtures/fallback_asymmetry.yaml"),
    ]);
    assert!(out.status.success(), "lint without --strict exits 0");
    let text = stdout(&out);
    assert_eq!(
        text.matches("SLI_FALLBACK_ASYMMETRY").count(),
        2,
        "one finding per SLO (the home-wifi pattern repeats):\n{text}"
    );
    assert!(
        text.contains("`sli.events.error_query` has a `vector(` no-data fallback"),
        "the finding names the guarded query:\n{text}"
    );
}

#[test]
fn strict_mode_fails_on_the_committed_fixture() {
    let out = slokit(&[
        "lint",
        "--strict",
        "-i",
        &manifest_path("tests/fixtures/fallback_asymmetry.yaml"),
    ]);
    assert!(
        !out.status.success(),
        "--strict exits non-zero on the warning"
    );
}

#[test]
fn a_symmetric_fallback_spec_is_clean() {
    let path = temp_spec(
        "symmetric",
        r#"service: api
slos:
  - name: avail
    objective: 99.9
    description: d
    sli:
      events:
        error_query: sum(rate(err[{{.window}}])) OR on() vector(0)
        total_query: sum(rate(tot[{{.window}}])) OR on() vector(1)
    alerting:
      labels: { severity: page }
"#,
    );
    let out = slokit(&["lint", "--strict", "-i", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "symmetric fallbacks must stay clean: {}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("no lint findings"));
}

#[test]
fn a_no_fallback_spec_is_clean() {
    let path = temp_spec(
        "bare",
        r#"service: api
slos:
  - name: avail
    objective: 99.9
    description: d
    sli:
      events:
        error_query: sum(rate(err[{{.window}}]))
        total_query: sum(rate(tot[{{.window}}]))
    alerting:
      labels: { severity: page }
"#,
    );
    let out = slokit(&["lint", "--strict", "-i", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "no-fallback specs must stay clean: {}",
        stdout(&out)
    );
    assert!(stdout(&out).contains("no lint findings"));
}

#[test]
fn the_committed_example_set_stays_lint_clean() {
    // Glob-discovered, never hand-enumerated, with a zero-match hard failure:
    // a moved or emptied example directory must fail this test loudly rather
    // than let the clean-set claim go vacuously green.
    let dir = PathBuf::from(manifest_path("examples/infraportal/slos"));
    let specs: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("examples/infraportal/slos exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    assert!(
        !specs.is_empty(),
        "zero yaml specs discovered in {}; the clean-set guard would be vacuous",
        dir.display()
    );

    for spec in specs
        .iter()
        .cloned()
        .chain([PathBuf::from(manifest_path("tests/fixtures/sample.yaml"))])
    {
        let out = slokit(&["lint", "--strict", "-i", spec.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{} must stay lint-clean: {}",
            spec.display(),
            stdout(&out)
        );
        assert!(
            stdout(&out).contains("no lint findings"),
            "{} must report no findings: {}",
            spec.display(),
            stdout(&out)
        );
    }
}
