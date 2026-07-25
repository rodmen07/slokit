//! Boundary-validation tests for the `slokit simulate` subcommand.
//!
//! `simulate` documents its `error_ratio` as a ratio in `[0, 1]`, but the
//! library API is frozen (1.x) and does not enforce it; the CLI is the trust
//! boundary that must reject out-of-domain input the way `--objective` already
//! does. Before this suite, `--error-rate 150`, `--error-rate=-5`,
//! `--error-rate nan`, negative/NaN `--traffic`, and NaN `--remaining` all
//! exited 0 with nonsensical output ("Burn rate: NaNx", "-259200000 events",
//! and a `null` burn rate in JSON). These tests spawn the real binary and
//! assert the exit code flips to non-zero with a message naming the flag, so
//! they FAIL against the pre-fix binary — the behavior-difference proof.

#![cfg(feature = "cli")]

use std::process::{Command, Output};

fn simulate(extra: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_slokit"));
    cmd.arg("simulate")
        .args(["--objective", "99.9"])
        .args(extra);
    cmd.output().expect("slokit simulate runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ---- valid inputs still succeed and produce a real simulation ----

#[test]
fn a_valid_error_rate_succeeds() {
    let out = simulate(&["--error-rate", "0.5"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("Burn rate:"),
        "expected a real simulation table, got: {}",
        stdout(&out)
    );
}

#[test]
fn the_boundary_values_zero_and_one_hundred_succeed() {
    // 0% failing (no errors) and 100% failing (total outage) are both real,
    // representable scenarios and must not be rejected.
    for rate in ["0", "100"] {
        let out = simulate(&["--error-rate", rate]);
        assert!(
            out.status.success(),
            "--error-rate {rate} should be accepted; stderr: {}",
            stderr(&out)
        );
    }
}

// ---- out-of-domain error rates are rejected, naming the flag ----

#[test]
fn an_error_rate_above_one_hundred_percent_is_rejected() {
    let out = simulate(&["--error-rate", "150"]);
    assert!(
        !out.status.success(),
        "an impossible >100% error rate must not exit 0"
    );
    let err = stderr(&out);
    assert!(
        err.contains("--error-rate"),
        "message must name the flag: {err}"
    );
}

#[test]
fn a_negative_error_rate_is_rejected() {
    // `=` form so clap does not treat `-5` as a flag; the value reaches our
    // validation, which is what we are testing.
    let out = simulate(&["--error-rate=-5"]);
    assert!(
        !out.status.success(),
        "a negative error rate must not exit 0"
    );
    assert!(stderr(&out).contains("--error-rate"));
}

#[test]
fn a_nan_error_rate_is_rejected_in_both_formats() {
    let table = simulate(&["--error-rate", "nan"]);
    assert!(
        !table.status.success(),
        "NaN error rate must not exit 0 (table)"
    );

    // The JSON path previously emitted a `null` burn rate — a machine-readable
    // document a downstream consumer would silently mis-handle.
    let json = simulate(&["--error-rate", "nan", "--output", "json"]);
    assert!(
        !json.status.success(),
        "NaN error rate must not exit 0 (json)"
    );
    assert!(
        !stdout(&json).contains("null"),
        "must reject before emitting a null-valued JSON document"
    );
}

// ---- traffic and remaining are validated too ----

#[test]
fn a_negative_traffic_is_rejected() {
    let out = simulate(&["--error-rate", "1", "--traffic=-100"]);
    assert!(!out.status.success(), "negative traffic must not exit 0");
    assert!(stderr(&out).contains("--traffic"));
}

#[test]
fn a_nan_traffic_is_rejected() {
    let out = simulate(&["--error-rate", "1", "--traffic", "nan"]);
    assert!(!out.status.success(), "NaN traffic must not exit 0");
    assert!(stderr(&out).contains("--traffic"));
}

#[test]
fn a_nan_remaining_is_rejected() {
    // A finite out-of-range `--remaining` stays clamped (documented behavior);
    // only NaN/inf, which escape the clamp and print "NaN% remaining", are
    // rejected.
    let out = simulate(&["--error-rate", "1", "--remaining", "nan"]);
    assert!(!out.status.success(), "NaN remaining must not exit 0");
    assert!(stderr(&out).contains("--remaining"));
}

#[test]
fn an_out_of_range_but_finite_remaining_is_still_clamped_not_rejected() {
    // 150% remaining is clamped to a full budget, per the flag's documented
    // contract — it must NOT become an error.
    let out = simulate(&["--error-rate", "1", "--remaining", "150"]);
    assert!(
        out.status.success(),
        "finite out-of-range --remaining is clamped, not rejected; stderr: {}",
        stderr(&out)
    );
}
