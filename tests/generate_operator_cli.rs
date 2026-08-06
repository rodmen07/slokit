//! Binary-level tests for `slokit generate --format operator` resource naming.
//!
//! Found by QA adversarial review (2026-08-06) of the v1.3.0 multi-document
//! input: `--format operator` named every emitted `PrometheusRule` resource
//! `args.name.unwrap_or(spec.service)`, a per-spec rule written when input was
//! one spec. With several specs (one `-i` directory, or since v1.3.0 one
//! multi-document file) that produced silent `metadata.name` collisions two
//! ways:
//!
//! 1. `--name X` stamped X onto EVERY resource in the stream.
//! 2. Two specs sharing a service (legal input: validation rejects duplicate
//!    service/SLO PAIRS, not duplicate services) both defaulted to the
//!    service name.
//!
//! Two resources of the same kind and `metadata.name` are ONE resource to a
//! cluster: `kubectl apply` of the emitted stream keeps only the last document
//! and silently drops every other spec's rules — the same silent-loss class
//! `write_export_dir` already fails closed on for files. A third sibling:
//! `--name` with `--format prometheus` was parsed and discarded (the svccat
//! `--filter` class), because only the operator arm ever read it.
//!
//! Every test spawns the real binary; the rejection tests assert each guard's
//! own distinctive wording, not a shared common word, so a different failure
//! path cannot satisfy them.

#![cfg(feature = "cli")]

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

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Two distinct services in one multi-document stream (the v1.3.0 fixture).
const MULTIFILE: &str = "tests/fixtures/multifile.yaml";
/// Two specs declaring the SAME service with different SLO names.
const DUP_SERVICE: &str = "tests/fixtures/dup_service.yaml";
/// A single-spec file.
const SINGLE: &str = "tests/fixtures/sample.yaml";

#[test]
fn name_with_multiple_specs_is_rejected_before_any_output() {
    let out = slokit(&[
        "generate", "-i", MULTIFILE, "--format", "operator", "--name", "shared",
    ]);
    assert!(
        !out.status.success(),
        "--name over two specs must be rejected, got stdout:\n{}",
        stdout(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains(
            "--name shared would give all 2 PrometheusRule resources the same metadata.name"
        ),
        "the rejection must carry the collision guard's own wording, got:\n{err}"
    );
    // The guard runs before rendering: a rejected batch emits nothing, so a
    // shell redirect cannot capture a half-written stream.
    assert!(
        stdout(&out).is_empty(),
        "a rejected batch must write nothing to stdout, got:\n{}",
        stdout(&out)
    );
}

#[test]
fn duplicate_service_names_without_name_are_rejected() {
    let out = slokit(&["generate", "-i", DUP_SERVICE, "--format", "operator"]);
    assert!(
        !out.status.success(),
        "duplicate service names must be rejected, got stdout:\n{}",
        stdout(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("two specs declare service 'payments'"),
        "the rejection must name the colliding service, got:\n{err}"
    );
    assert!(
        stdout(&out).is_empty(),
        "a rejected batch must write nothing to stdout, got:\n{}",
        stdout(&out)
    );
}

/// The discriminator between "the guard fired" and "validation did the work":
/// the duplicate-service fixture is legal input everywhere else, so the
/// operator-format rejection above is proven to come from the naming guard.
#[test]
fn the_duplicate_service_fixture_is_otherwise_valid() {
    let out = slokit(&["validate", "-i", DUP_SERVICE]);
    assert!(
        out.status.success(),
        "the duplicate-service fixture must validate cleanly, got:\n{}",
        stderr(&out)
    );

    // And the merged Prometheus format accepts it too: group names embed the
    // service/SLO pair, so nothing collides there.
    let out = slokit(&["generate", "-i", DUP_SERVICE]);
    assert!(
        out.status.success(),
        "the merged prometheus format must accept it, got:\n{}",
        stderr(&out)
    );
}

#[test]
fn name_with_a_single_spec_is_honored() {
    let out = slokit(&[
        "generate",
        "-i",
        SINGLE,
        "--format",
        "operator",
        "--name",
        "custom-name",
    ]);
    assert!(
        out.status.success(),
        "--name with one spec must still work, got:\n{}",
        stderr(&out)
    );
    let yaml = stdout(&out);
    assert!(
        yaml.contains("name: custom-name"),
        "the single resource must carry the requested metadata.name, got:\n{yaml}"
    );
}

#[test]
fn multi_spec_without_name_names_each_resource_after_its_service() {
    let out = slokit(&["generate", "-i", MULTIFILE, "--format", "operator"]);
    assert!(
        out.status.success(),
        "distinct services without --name must generate, got:\n{}",
        stderr(&out)
    );
    let yaml = stdout(&out);
    assert_eq!(
        yaml.matches("kind: PrometheusRule").count(),
        2,
        "one resource per spec, got:\n{yaml}"
    );
    // Newline-anchored so `name: myservice2` cannot satisfy the first probe.
    assert!(
        yaml.contains("name: myservice\n"),
        "first resource named after its service, got:\n{yaml}"
    );
    assert!(
        yaml.contains("name: myservice2\n"),
        "second resource named after its service, got:\n{yaml}"
    );
}

#[test]
fn name_with_the_prometheus_format_is_rejected() {
    let out = slokit(&["generate", "-i", SINGLE, "--name", "wasted"]);
    assert!(
        !out.status.success(),
        "--name with the default prometheus format was a parsed-and-discarded \
         no-op and must now be rejected, got stdout:\n{}",
        stdout(&out)
    );
    let err = stderr(&out);
    assert!(
        err.contains("no effect on --format prometheus"),
        "the rejection must carry the format guard's own wording, got:\n{err}"
    );
}
