//! Binary-level proof for `kind: AlertWindows` catalogue input (v1.7.0 PR 2).
//!
//! The v1.7.0 done-when clauses are stated about the CLI, so this file drives
//! the real `slokit` binary rather than `generate_rules_with`. Before this
//! slice, every invocation below either failed at argument parsing
//! (`unexpected argument '--alert-windows'`) or, for the two upstream
//! catalogues, failed inside the CRD importer with
//!
//! ```text
//! sloth-crd: no kind: PrometheusServiceLevel documents in input (nothing to import)
//! ```
//!
//! because auto-detection routed on the `apiVersion` group alone and then
//! nothing knew the `kind`. Those two documents are pinned in
//! `tests/sloth_corpus.rs`, whose `CORPUS` table flips them from `Refused` to
//! `Accepted` in this same PR.
//!
//! **The load-bearing test is [`sloths_own_defaults_as_a_catalogue_are_a_no_op`]**
//! (done-when clause 4). A catalogue that reproduces sloth's built-in defaults
//! must generate byte-identical rules to passing no catalogue at all, because
//! the factor formula maps those percentages exactly onto
//! `MwmbrConfig::sre_default()`. It is the one assertion here that a
//! mis-derived factor, a reordered window list, or an accidental second
//! period-scaling all break, and none of them changes whether the flag
//! "works".
//!
//! The parsing and validation assertions live in
//! `src/spec/alert_windows.rs`'s unit tests; this file only proves the
//! capability is reachable from the installed command, that it CHANGES the
//! output it is applied to, and that it loses to a spec's own
//! `alerting.windows`.

#![cfg(feature = "cli")]

use std::process::{Command, Output};

/// Both upstream catalogues, committed with their upstream sha256 by PR 1.
const UPSTREAM_7D: &str = "tests/fixtures/sloth_corpus/windows/7d.yaml";
const UPSTREAM_30D: &str = "tests/fixtures/sloth_corpus/windows/custom-30d.yaml";
/// The directory holding exactly those two, used for the directory-load path.
const UPSTREAM_DIR: &str = "tests/fixtures/sloth_corpus/windows";

/// sloth's own defaults written as a catalogue: the clause-4 control.
const SLOTH_DEFAULTS_30D: &str = "tests/fixtures/alert_windows/sloth-defaults-30d.yaml";
const MALFORMED_30D: &str = "tests/fixtures/alert_windows/malformed-30d.yaml";
const MIXED_PERIODS: &str = "tests/fixtures/alert_windows/mixed-periods.yaml";
const OWN_WINDOWS: &str = "tests/fixtures/alert_windows/spec-with-own-windows.yaml";
const SAMPLE: &str = "tests/fixtures/sample.yaml";

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Both streams, for asserting on error text without caring which one it went
/// to.
fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn generate(args: &[&str]) -> String {
    let out = slokit(args);
    assert!(
        out.status.success(),
        "slokit {args:?} exited {:?}:\n{}",
        out.status.code(),
        combined(&out)
    );
    stdout(&out)
}

/// Every `sloth_severity`-bearing burn-rate threshold multiplier the rules
/// carry, in emission order.
///
/// Read off the rendered YAML rather than out of the model, because the claim
/// is about what the binary WRITES. The alert expressions embed the factor as
/// `(...) > (<factor> * <budget>)`, so the window durations in the same rule
/// are the cheapest cross-check that the catalogue's windows travelled with
/// its factors — see [`the_upstream_seven_day_catalogue_reaches_the_rules`].
fn alert_exprs(rules: &str) -> Vec<String> {
    rules
        .lines()
        .filter(|l| l.contains("sli_error:ratio_rate"))
        .filter(|l| l.contains(" > ("))
        .map(|l| l.trim().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// done-when clause 4
// ---------------------------------------------------------------------------

/// **Done-when clause 4, first half.** sloth's own defaults, expressed as a
/// 30-day catalogue, are `MwmbrConfig::sre_default()` under the factor
/// formula, so applying the catalogue must change nothing at all.
///
/// This is a byte comparison of the whole rules document, not a spot check:
/// the factors, the lookback windows, the rule order, and the recording rules
/// the alerts reference are all in those bytes.
#[test]
fn sloths_own_defaults_as_a_catalogue_are_a_no_op() {
    let without = generate(&["generate", "-i", SAMPLE, "--period", "30d"]);
    let with = generate(&[
        "generate",
        "-i",
        SAMPLE,
        "--period",
        "30d",
        "--alert-windows",
        SLOTH_DEFAULTS_30D,
    ]);
    assert_eq!(
        with, without,
        "a 30d catalogue carrying sloth's own defaults must map onto \
         MwmbrConfig::sre_default() and generate byte-identical rules"
    );
    // Not vacuous: the comparison would also pass if BOTH sides were empty.
    assert!(
        without.contains("sloth_severity"),
        "the control produced no alert rules at all:\n{without}"
    );
}

/// **Done-when clause 4, second half (7d).** The factors this milestone's
/// scoping pass computed — 13.44 / 3.5 / 1.4 / 0.98 — asserted at the binary,
/// together with the windows they belong to.
#[test]
fn the_upstream_seven_day_catalogue_reaches_the_rules() {
    let rules = generate(&[
        "generate",
        "-i",
        SAMPLE,
        "--period",
        "7d",
        "--alert-windows",
        UPSTREAM_7D,
    ]);
    let exprs = alert_exprs(&rules);
    assert!(!exprs.is_empty(), "no alert expressions found:\n{rules}");

    // (pct/100) x (7d / longWindow), with the catalogue's own lookbacks.
    for (window, factor) in [
        ("1h", "13.44"),
        ("6h", "3.5"),
        ("1d", "1.4"),
        ("3d", "0.98"),
    ] {
        let needle = format!("ratio_rate{window}");
        let hit = exprs
            .iter()
            .find(|e| e.contains(&needle))
            .unwrap_or_else(|| panic!("no alert expression over {window}:\n{exprs:#?}"));
        assert!(
            hit.contains(&format!("({factor} * ")),
            "the {window} condition should carry factor {factor}, got:\n  {hit}"
        );
    }
    // The short windows travelled too: 5m / 30m / 2h / 6h.
    for short in [
        "ratio_rate5m",
        "ratio_rate30m",
        "ratio_rate2h",
        "ratio_rate6h",
    ] {
        assert!(
            rules.contains(short),
            "expected a recording rule at {short}:\n{rules}"
        );
    }
}

/// **Done-when clause 4, second half (30d).** The other upstream catalogue:
/// 14.4 / 4.8 / 3 / 1 over 30m / 3h / 12h / 36h.
///
/// Its page-quick factor is 14.4, the same number `sre_default()` carries, so
/// the factor alone would not distinguish it — the WINDOWS do. `30m` and `36h`
/// appear in no default table at any period.
#[test]
fn the_upstream_thirty_day_catalogue_reaches_the_rules() {
    let rules = generate(&[
        "generate",
        "-i",
        SAMPLE,
        "--period",
        "30d",
        "--alert-windows",
        UPSTREAM_30D,
    ]);
    let exprs = alert_exprs(&rules);
    for (window, factor) in [("30m", "14.4"), ("3h", "4.8"), ("12h", "3"), ("36h", "1")] {
        let needle = format!("ratio_rate{window}");
        let hit = exprs
            .iter()
            .find(|e| e.contains(&needle))
            .unwrap_or_else(|| panic!("no alert expression over {window}:\n{exprs:#?}"));
        assert!(
            hit.contains(&format!("({factor} * ")),
            "the {window} condition should carry factor {factor}, got:\n  {hit}"
        );
    }
}

// ---------------------------------------------------------------------------
// The flag is not decorative
// ---------------------------------------------------------------------------

/// The behaviour difference between the flag on and off, on the same input.
///
/// `sloths_own_defaults_as_a_catalogue_are_a_no_op` deliberately asserts the
/// opposite for a catalogue that encodes the defaults; without this test, an
/// implementation that ignored `--alert-windows` entirely would pass it.
#[test]
fn a_catalogue_changes_the_rules_it_is_applied_to() {
    let without = generate(&["generate", "-i", SAMPLE, "--period", "30d"]);
    let with = generate(&[
        "generate",
        "-i",
        SAMPLE,
        "--period",
        "30d",
        "--alert-windows",
        UPSTREAM_30D,
    ]);
    assert_ne!(
        with, without,
        "--alert-windows with a non-default catalogue must change the output"
    );
    assert!(
        !without.contains("ratio_rate36h") && with.contains("ratio_rate36h"),
        "the 36h lookback is unique to the catalogue and must appear only with it"
    );
}

/// A spec's own `alerting.windows` outranks a catalogue for the same period.
/// The precedence lives in one place (`resolve_mwmbr`), and this is the half
/// of it a catalogue could plausibly have broken.
#[test]
fn a_specs_own_windows_still_outrank_a_catalogue() {
    let rules = generate(&[
        "generate",
        "-i",
        OWN_WINDOWS,
        "--alert-windows",
        UPSTREAM_7D,
    ]);
    assert!(
        rules.contains("(99 * ") && rules.contains("(88 * "),
        "the spec's own factors should survive:\n{rules}"
    );
    assert!(
        !rules.contains("13.44"),
        "the 7d catalogue must not reach an SLO that sets its own windows:\n{rules}"
    );
}

/// A directory of catalogues loads all of them, and each applies to its own
/// period. `tests/fixtures/sloth_corpus/windows/` holds exactly the 7d and 30d
/// upstream catalogues, so the mixed-period spec is fully covered by the
/// directory and by neither file alone.
#[test]
fn a_directory_of_catalogues_covers_every_period_in_it() {
    let rules = generate(&[
        "generate",
        "-i",
        MIXED_PERIODS,
        "--alert-windows",
        UPSTREAM_DIR,
    ]);
    // 7d SLO -> 13.44 over 1h; 30d SLO -> 4.8 over 3h (unique to custom-30d).
    assert!(
        rules.contains("(13.44 * "),
        "7d catalogue missing:\n{rules}"
    );
    assert!(rules.contains("(4.8 * "), "30d catalogue missing:\n{rules}");
}

/// An SLO period no catalogue covers is an error naming the period, not a
/// silent fallback to the default table.
///
/// `resolve_mwmbr` is infallible on purpose (the dashboard shares it), so this
/// is the CLI's job. Without it, `--alert-windows 7d.yaml` on this spec would
/// exit 0 having applied the catalogue to one SLO and the defaults to the
/// other, with nothing said.
#[test]
fn an_uncovered_period_is_refused_naming_it() {
    let out = slokit(&[
        "generate",
        "-i",
        MIXED_PERIODS,
        "--alert-windows",
        UPSTREAM_7D,
    ]);
    assert!(!out.status.success(), "expected a non-zero exit");
    let text = combined(&out);
    assert!(
        text.contains("no catalogue for SLO period 30d"),
        "the error should name the uncovered period:\n{text}"
    );
    assert!(
        text.contains("it covers 7d"),
        "the error should name what IS covered:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// The two input channels
// ---------------------------------------------------------------------------

/// `slokit validate -i <catalogue>` reads a catalogue and reports the factors
/// it would apply. This is the invocation `tests/sloth_corpus.rs` measures, so
/// it is what makes the two `windows/` rows read `Accepted`.
///
/// The factor is rendered the way the emitted rules render it: deriving
/// `20% x 7d/1d` in `f64` lands on `1.4000000000000001`, and a validate line
/// printing that beside a rule saying `1.4` would look like two numbers.
#[test]
fn validate_reads_a_catalogue_and_reports_its_factors() {
    let out = slokit(&["validate", "-i", UPSTREAM_7D]);
    assert!(out.status.success(), "{}", combined(&out));
    let text = stdout(&out);
    assert!(
        text.contains("AlertWindows catalogue for 7d"),
        "expected the kind and period:\n{text}"
    );
    for expected in [
        "page 1h/5m x13.44",
        "ticket 1d/2h x1.4",
        "ticket 3d/6h x0.98",
    ] {
        assert!(text.contains(expected), "expected {expected:?} in:\n{text}");
    }
    assert!(
        !text.contains("1.4000000000000001"),
        "the factor should render as the rules render it:\n{text}"
    );
}

/// The acceptance above is not "we ignored the file": a catalogue with bad
/// numbers is refused, naming the offending path.
#[test]
fn validate_refuses_a_malformed_catalogue() {
    let out = slokit(&["validate", "-i", MALFORMED_30D]);
    assert!(
        !out.status.success(),
        "a catalogue with a zero budget percent must not validate:\n{}",
        combined(&out)
    );
    assert!(
        combined(&out).contains("spec.page.quick.errorBudgetPercent is 0"),
        "the error should name the field:\n{}",
        combined(&out)
    );
}

/// A catalogue handed to `--input` is refused by name rather than counted as
/// zero specs.
///
/// Letting it through would make `slokit generate -i windows/7d.yaml` print an
/// empty rules document and exit 0 — a flag-shaped no-op, the failure class
/// this repo has shipped before.
#[test]
fn a_catalogue_on_the_input_channel_is_refused_naming_the_flag() {
    let out = slokit(&["generate", "-i", UPSTREAM_7D]);
    assert!(!out.status.success(), "expected a non-zero exit");
    let text = combined(&out);
    assert!(
        text.contains("is a sloth alert-window catalogue"),
        "the error should say what the file is:\n{text}"
    );
    assert!(
        text.contains("--alert-windows"),
        "the error should name the flag that DOES take it:\n{text}"
    );
}

/// The dashboard takes the same flag, because it resolves the same windows:
/// its panels query `slo:sli_error:ratio_rate<window>` series that only exist
/// if `generate` ran with the same catalogue. A dashboard without the flag
/// queries 30d-scaled windows the catalogue's rules never record.
#[test]
fn the_dashboard_takes_the_same_catalogue_as_generate() {
    let rules = generate(&[
        "generate",
        "-i",
        SAMPLE,
        "--period",
        "30d",
        "--alert-windows",
        UPSTREAM_30D,
    ]);
    let dash = generate(&[
        "dashboard",
        "-i",
        SAMPLE,
        "--period",
        "30d",
        "--alert-windows",
        UPSTREAM_30D,
    ]);
    for window in [
        "ratio_rate30m",
        "ratio_rate3h",
        "ratio_rate12h",
        "ratio_rate36h",
    ] {
        assert!(rules.contains(window), "rules missing {window}");
        assert!(dash.contains(window), "dashboard missing {window}");
    }
    let dash_without = generate(&["dashboard", "-i", SAMPLE, "--period", "30d"]);
    assert!(
        !dash_without.contains("ratio_rate36h"),
        "without the flag the dashboard must not query the catalogue's windows"
    );
}
