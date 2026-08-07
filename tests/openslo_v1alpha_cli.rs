//! Binary-level proof for the OpenSLO v1alpha import (v1.5.0 PRs 1 and 2).
//!
//! The roadmap's done-when clause 1 is stated about the CLI, not the library:
//! `slokit validate -i <fixture>` must exit 0 on the committed sloth
//! documents. Before PR 1 both invocations exited 1 with
//!
//! ```text
//! spec error: openslo document 1: unsupported apiVersion 'openslo/v1alpha' (expected openslo/v1)
//! ```
//!
//! so every test here fails against the pre-PR-1 binary. The library-level
//! mapping assertions live in `tests/openslo_v1alpha.rs`; this file only
//! proves the capability is reachable from the installed command, on both the
//! auto-detected route and the explicit `--input-format openslo` one (the
//! roadmap recorded the rejection reproducing through both).
//!
//! PR 2 adds the rest of the end-to-end story, which `validate` cannot state:
//! `generate` really emits rules from these documents (clause 2's twin
//! comparison, run through the real binary rather than through
//! `generate_rules`), and an imported v1alpha document survives
//! `export --format openslo` and a re-import. The round trip is deliberately
//! **asymmetric** — v1alpha in, `openslo/v1` out, because `to_yaml` only
//! writes the current version — so it is asserted as *semantic* stability
//! (the rules generated on both sides are byte-identical) plus an explicit
//! check that the emitted `apiVersion` is the newer one. Asserting apiVersion
//! equality would be asserting a round trip slokit does not offer.
//!
//! The v1 half of that property lives in `tests/export_cli.rs`
//! (`an_exported_example_reimports_cleanly_through_validate`); it stays there,
//! and the v1alpha half lives here, so each dialect's binary-level proofs sit
//! in one file.

#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GETTING_STARTED: &str = "tests/fixtures/openslo/v1alpha/getting-started.yaml";
const GETTING_STARTED_TWIN: &str = "tests/fixtures/openslo/v1alpha/getting-started-twin.yaml";
const APISERVER: &str = "tests/fixtures/openslo/v1alpha/kubernetes-apiserver.yaml";

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

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-v1alpha-cli-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `slokit generate -i <path>`, asserting exit 0 and handing back stdout.
fn generate(path: &str) -> String {
    let out = slokit(&["generate", "-i", path]);
    assert!(
        out.status.success(),
        "generate {path} exited {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    stdout(&out)
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
    let dir = temp_dir("unknown");
    let path = dir.join("unknown.yaml");
    let contents = fs::read_to_string(GETTING_STARTED)
        .unwrap()
        .replace("openslo/v1alpha", "openslo/v9");
    fs::write(&path, contents).unwrap();

    let out = slokit(&["validate", "-i", path.to_str().unwrap()]);
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("unsupported apiVersion 'openslo/v9'"), "{err}");
    assert!(err.contains("openslo/v1 or openslo/v1alpha"), "{err}");

    fs::remove_dir_all(&dir).ok();
}

// ---- PR 2: `generate` end to end, the done-when clause `validate` cannot state ----

#[test]
fn generating_from_the_sloth_getting_started_matches_its_native_twin_byte_for_byte() {
    // Roadmap done-when clause 2, run through the real binary instead of
    // through `generate_rules` (`tests/openslo_v1alpha.rs` covers the library
    // call). An importer that parses the document but mis-maps it — a wrong
    // period, the good/total derivation inverted, the objective read as a
    // percent rather than a unit fraction — passes `validate` and fails here.
    //
    // It is also the cheapest possible proof that import notes never
    // contaminate the rules stream: the twin is native slokit and emits no
    // notes at all, so any note leaking into stdout would break the equality.
    let from_openslo = generate(GETTING_STARTED);
    let from_twin = generate(GETTING_STARTED_TWIN);

    assert_eq!(
        from_openslo, from_twin,
        "imported v1alpha rules diverged from the hand-written twin"
    );
    // Guard against the equality being vacuous if both sides ever produced
    // nothing: these fixtures are one SLO, so exactly three rule groups.
    assert_eq!(
        from_twin.matches("- name: slokit-slo-").count(),
        3,
        "{from_twin}"
    );

    // The notes are still reported, on the channel that is not the stream.
    let err = stderr(&slokit(&["generate", "-i", GETTING_STARTED]));
    assert!(err.contains("metadata.displayName"), "{err}");
}

#[test]
fn generating_from_the_apiserver_stream_emits_one_group_trio_per_imported_objective() {
    // The second sloth fixture is the interesting one: two documents sharing a
    // service, one of which carries two objectives. It must come out as three
    // SLOs (1 + 2), each with its recordings / meta / alerts trio, and the
    // two-objective SLO's members must carry the 1-based suffixes.
    let rules = generate(APISERVER);

    let groups: Vec<&str> = rules
        .lines()
        .filter(|l| l.starts_with("- name: slokit-slo-"))
        .collect();
    assert_eq!(groups.len(), 9, "{rules}");

    for kind in ["sli-recordings", "meta-recordings", "alerts"] {
        for slo in [
            "requests-availability-openslo",
            "requests-latency-openslo-1",
            "requests-latency-openslo-2",
        ] {
            let expected = format!("- name: slokit-slo-{kind}-k8s-apiserver-{slo}");
            assert!(
                groups.contains(&expected.as_str()),
                "missing {expected}: {groups:?}"
            );
        }
    }

    // The per-objective metric really did travel with its objective: the two
    // latency objectives differ only in their `le` bucket and their target.
    assert!(rules.contains(r#"le="0.4""#), "{rules}");
    assert!(rules.contains(r#"le="5""#), "{rules}");
    assert!(rules.contains("vector(0.99)"), "{rules}");
    assert!(rules.contains("vector(0.999)"), "{rules}");
}

// ---- PR 2: the round trip, v1alpha in and openslo/v1 out ----

/// `slokit export --format openslo -i <src>`, written to a file so the second
/// leg reads it exactly as a downstream consumer would.
fn export_to_file(dir: &Path, src: &str) -> PathBuf {
    let out = slokit(&["export", "--format", "openslo", "-i", src]);
    assert!(
        out.status.success(),
        "export {src} exited {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    let path = dir.join("exported.yaml");
    fs::write(&path, stdout(&out)).unwrap();
    path
}

#[test]
fn a_v1alpha_document_exports_as_openslo_v1_and_regenerates_the_same_rules() {
    for (tag, fixture, documents) in [
        ("gs", GETTING_STARTED, 1),
        // The apiserver stream imports as three SLOs, so it exports as three
        // documents: the objective split survives the trip.
        ("apiserver", APISERVER, 3),
    ] {
        let dir = temp_dir(tag);
        let exported = export_to_file(&dir, fixture);
        let yaml = fs::read_to_string(&exported).unwrap();

        // The asymmetry, asserted rather than glossed: the input said
        // v1alpha, the output says v1, because `to_yaml` writes the current
        // version only. A future exporter that learned to emit v1alpha would
        // have to change this line deliberately.
        assert_eq!(
            yaml.matches("apiVersion: openslo/v1\n").count(),
            documents,
            "{fixture} did not export {documents} openslo/v1 document(s): {yaml}"
        );
        assert!(
            !yaml.contains("openslo/v1alpha"),
            "the export must not echo the input version: {yaml}"
        );

        // Leg two: the emitted stream re-imports through auto-detection, with
        // no --input-format hint, exactly as a downstream tool would read it.
        let back = slokit(&["validate", "-i", exported.to_str().unwrap()]);
        assert!(
            back.status.success(),
            "the exported YAML did not re-import: {}",
            stderr(&back)
        );

        // And the property that makes the trip worth anything: the rules are
        // the same on both sides. Byte identity here is a much stronger claim
        // than "it parsed" — it says the period, the queries, the objective
        // and the SLO names all survived both mappings.
        let before = generate(fixture);
        let after = generate(exported.to_str().unwrap());
        assert_eq!(
            before, after,
            "{fixture}: rules changed across the v1alpha -> v1 round trip"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
