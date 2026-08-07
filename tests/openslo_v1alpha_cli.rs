//! Binary-level proof for the OpenSLO v1alpha import (v1.5.0 PR 1).
//!
//! The roadmap's done-when clause 1 is stated about the CLI, not the library:
//! `slokit validate -i <fixture>` must exit 0 on the committed sloth
//! documents. Before this PR both invocations exited 1 with
//!
//! ```text
//! spec error: openslo document 1: unsupported apiVersion 'openslo/v1alpha' (expected openslo/v1)
//! ```
//!
//! so every test here fails against the pre-PR binary. The library-level
//! mapping assertions live in `tests/openslo_v1alpha.rs`; this file only
//! proves the capability is reachable from the installed command, on both the
//! auto-detected route and the explicit `--input-format openslo` one (the
//! roadmap recorded the rejection reproducing through both).

#![cfg(feature = "cli")]

use std::process::{Command, Output};

const GETTING_STARTED: &str = "tests/fixtures/openslo/v1alpha/getting-started.yaml";
const APISERVER: &str = "tests/fixtures/openslo/v1alpha/kubernetes-apiserver.yaml";

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn validate_accepts_both_sloth_v1alpha_fixtures_by_auto_detection() {
    for fixture in [GETTING_STARTED, APISERVER] {
        let out = slokit(&["validate", "-i", fixture]);
        let err = stderr(&out);
        assert!(
            out.status.success(),
            "validate {fixture} exited {:?}: {err}",
            out.status.code()
        );
        assert!(!err.contains("unsupported apiVersion"), "{fixture}: {err}");
    }
}

#[test]
fn validate_accepts_both_sloth_v1alpha_fixtures_with_an_explicit_format() {
    for fixture in [GETTING_STARTED, APISERVER] {
        let out = slokit(&["validate", "-i", fixture, "--input-format", "openslo"]);
        assert!(
            out.status.success(),
            "validate --input-format openslo {fixture} exited {:?}: {}",
            out.status.code(),
            stderr(&out)
        );
    }
}

#[test]
fn import_notes_reach_stderr_rather_than_being_swallowed() {
    // The dropped `metadata.displayName` is advisory, not fatal: it must be
    // visible to the operator and must not change the exit code.
    let out = slokit(&["validate", "-i", GETTING_STARTED]);
    assert!(out.status.success());
    let err = stderr(&out);
    assert!(err.contains("note:"), "{err}");
    assert!(err.contains("metadata.displayName"), "{err}");
}

#[test]
fn an_unknown_api_version_still_fails_and_names_both_supported_versions() {
    // The widening must not turn the fail-closed apiVersion check into a
    // shrug: an unknown version is still an error, and the message now tells
    // the operator which two versions are accepted.
    let dir = std::env::temp_dir().join(format!(
        "slokit-v1alpha-cli-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("unknown.yaml");
    let contents = std::fs::read_to_string(GETTING_STARTED)
        .unwrap()
        .replace("openslo/v1alpha", "openslo/v9");
    std::fs::write(&path, contents).unwrap();

    let out = slokit(&["validate", "-i", path.to_str().unwrap()]);
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("unsupported apiVersion 'openslo/v9'"), "{err}");
    assert!(err.contains("openslo/v1 or openslo/v1alpha"), "{err}");

    std::fs::remove_dir_all(&dir).ok();
}
