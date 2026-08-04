//! Multi-document spec input (v1.3.0 PR 1).
//!
//! Before this slice, `Spec::from_yaml` was the only YAML entry point for
//! native specs, and it hard-fails on a `---`-separated stream
//! ("deserializing from YAML containing more than one document is not
//! supported"), so sloth's own `examples/multifile.yml` could not be loaded
//! at all while `slokit export` happily WROTE multi-document streams to
//! stdout: the tool emitted a shape it could not re-read. These tests pin the
//! new stream-aware path, `Spec::from_yaml_stream`, and its CLI wiring.
//!
//! The committed fixture `tests/fixtures/multifile.yaml` is derived from
//! sloth's `examples/multifile.yml` (the grounding real-world spec named by
//! the ROADMAP's v1.3.0 slice 1): two `prometheus/v1` services in one file.

use slokit::spec::Spec;

const MULTIFILE: &str = include_str!("fixtures/multifile.yaml");
const SAMPLE: &str = include_str!("fixtures/sample.yaml");

/// A minimal valid single-document spec for inline stream surgery.
const MINI: &str = "service: alpha\nslos:\n  - name: avail\n    objective: 99.9\n    sli:\n      raw:\n        error_ratio_query: a[{{.window}}]\n";

#[test]
fn the_committed_fixture_yields_both_documents_in_stream_order() {
    let specs = Spec::from_yaml_stream(MULTIFILE).unwrap();
    assert_eq!(
        specs.len(),
        2,
        "sloth's multifile layout holds two services"
    );
    assert_eq!(specs[0].service, "myservice");
    assert_eq!(specs[1].service, "myservice2");
    assert_eq!(specs[0].slos[0].objective, 99.9);
    assert_eq!(specs[1].slos[0].objective, 99.99);
    for spec in &specs {
        spec.validate().unwrap();
    }
}

#[test]
fn a_single_document_stream_yields_the_same_spec_as_from_yaml() {
    let stream = Spec::from_yaml_stream(SAMPLE).unwrap();
    let single = Spec::from_yaml(SAMPLE).unwrap();
    assert_eq!(stream.len(), 1);
    assert_eq!(stream[0], single);
}

#[test]
fn empty_documents_are_skipped() {
    let yaml = format!("---\n# a comment-only document\n---\n{MINI}");
    let specs = Spec::from_yaml_stream(&yaml).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].service, "alpha");
}

#[test]
fn a_stream_with_no_documents_is_an_error() {
    for input in ["", "# only a comment\n", "---\n---\n"] {
        let err = Spec::from_yaml_stream(input).unwrap_err();
        assert!(
            err.to_string().contains("no YAML documents"),
            "input {input:?} produced the wrong error: {err}"
        );
    }
}

#[test]
fn parse_errors_name_the_failing_document() {
    // Document 1 is valid; document 2 types `slos` as a scalar.
    let yaml = format!("{MINI}---\nservice: beta\nslos: 3\n");
    let err = Spec::from_yaml_stream(&yaml).unwrap_err();
    assert!(
        err.to_string().contains("document 2"),
        "the error must locate the failing document: {err}"
    );

    let err = Spec::from_yaml_stream("service: [broken\n").unwrap_err();
    assert!(
        err.to_string().contains("document 1"),
        "a first-document failure is located too: {err}"
    );
}

/// The sibling keeps its exactly-one-document contract: `from_yaml` on a
/// stream still fails, so no existing caller's behavior changed (the additive
/// bar of docs/SEMVER.md).
#[test]
fn from_yaml_stays_single_document() {
    assert!(Spec::from_yaml(MULTIFILE).is_err());
}

#[cfg(feature = "cli")]
mod cli {
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

    fn stderr(out: &Output) -> String {
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn fixture() -> String {
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/multifile.yaml").to_string()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("slokit-multidoc-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validate_reads_the_committed_multidoc_fixture() {
        let out = slokit(&["validate", "-i", &fixture()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let text = stdout(&out);
        assert!(text.contains("ok: 'myservice' is valid (1 SLO)"), "{text}");
        assert!(text.contains("ok: 'myservice2' is valid (1 SLO)"), "{text}");
    }

    #[test]
    fn lint_reads_the_committed_multidoc_fixture() {
        let out = slokit(&["lint", "-i", &fixture()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert!(
            stdout(&out).contains("no lint findings"),
            "{}",
            stdout(&out)
        );
    }

    #[test]
    fn a_parse_error_names_the_file_and_the_document() {
        let dir = temp_dir("parse-error");
        let path = dir.join("broken.yaml");
        fs::write(
            &path,
            format!("{}---\nservice: beta\nslos: 3\n", super::MINI),
        )
        .unwrap();

        let out = slokit(&["validate", "-i", path.to_str().unwrap()]);
        assert!(!out.status.success());
        let text = stderr(&out);
        assert!(text.contains("broken.yaml"), "names the file: {text}");
        assert!(text.contains("document 2"), "locates the document: {text}");

        fs::remove_dir_all(&dir).ok();
    }

    /// The ROADMAP slice's close condition: a multi-spec `slokit export`
    /// stream pipes back through `slokit validate` cleanly, with the input
    /// itself a multi-document NATIVE file (both new-path directions in one
    /// run: native stream in, OpenSLO stream out, auto-detected back in).
    #[test]
    fn a_multi_spec_export_stream_pipes_back_through_validate() {
        let out = slokit(&["export", "--format", "openslo", "-i", &fixture()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let exported = stdout(&out);

        let dir = temp_dir("roundtrip");
        let path = dir.join("exported.yaml");
        fs::write(&path, &exported).unwrap();

        let back = slokit(&["validate", "-i", path.to_str().unwrap()]);
        assert!(back.status.success(), "stderr: {}", stderr(&back));
        let text = stdout(&back);
        assert!(text.contains("ok: 'myservice' is valid (1 SLO)"), "{text}");
        assert!(text.contains("ok: 'myservice2' is valid (1 SLO)"), "{text}");

        fs::remove_dir_all(&dir).ok();
    }

    /// A multi-document file generates exactly like the equivalent directory
    /// of single-document files: same services, merged into one rules doc.
    #[test]
    fn generate_merges_a_multidoc_file_like_a_directory() {
        let out = slokit(&["generate", "-i", &fixture()]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let rules = stdout(&out);
        assert!(rules.contains("myservice-requests-availability"), "{rules}");
        assert!(
            rules.contains("myservice2-requests-availability"),
            "{rules}"
        );
    }
}
