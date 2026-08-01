//! Binary-level tests for the `slokit export` subcommand (v1.2.0 PR 2).
//!
//! PR 1 shipped `spec::openslo::to_yaml` / `to_yaml_reported` as library
//! functions with no way to reach them from the command line: `slokit export`
//! exited non-zero with `error: unrecognized subcommand 'export'`, so a user
//! who had installed the CLI could import OpenSLO and never hand a spec back.
//! Every test here spawns the real binary, so all of them fail against the
//! pre-PR one — that is the behavior-difference proof for the new surface.
//!
//! The suite is deliberately about what the *subcommand* adds on top of the
//! library (the round trip through the real files, the stdout/stderr split,
//! directory output, and the guards that stop `--output` from writing outside
//! the directory or losing a spec). The mapping itself is proven as a property
//! in `tests/openslo_export.rs`.

#![cfg(feature = "cli")]

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

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-export-cli-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A committed example spec, exercised through the real file the repo ships.
const EXAMPLE: &str = "examples/infraportal/slos/accounts-service.yaml";

const RAW_SPEC: &str = r#"
service: alpha
slos:
  - name: avail
    objective: 99.9
    sli:
      raw:
        error_ratio_query: sum(rate(a_errors[{{.window}}])) / sum(rate(a_total[{{.window}}]))
"#;

const PLUGIN_SPEC: &str = r#"
service: plugged
slos:
  - name: avail
    objective: 99.9
    sli:
      plugin:
        id: slokit/availability/http-requests-total
        options:
          selector: job="api"
"#;

// ---- the milestone's done-when: export, then re-import through `validate` ----

#[test]
fn an_exported_example_reimports_cleanly_through_validate() {
    // This is v1.2.0's checkable done-when, run end to end through the two
    // real binaries rather than through library calls.
    let out = slokit(&["export", "--format", "openslo", "-i", EXAMPLE]);
    assert!(out.status.success(), "export failed: {}", stderr(&out));

    let dir = temp_dir("reimport");
    let path = dir.join("exported.yaml");
    fs::write(&path, stdout(&out)).unwrap();

    // No --input-format: auto-detection must recognize the emitted document as
    // OpenSLO on its own, which is what any downstream consumer would rely on.
    let back = slokit(&["validate", "-i", path.to_str().unwrap()]);
    assert!(
        back.status.success(),
        "the exported YAML did not re-import: {}",
        stderr(&back)
    );
    assert!(
        stdout(&back).contains("ok: 'accounts-service' is valid (2 SLOs)"),
        "unexpected validate output: {}",
        stdout(&back)
    );
}

#[test]
fn export_emits_one_openslo_document_per_slo() {
    let out = slokit(&["export", "-i", EXAMPLE]);
    assert!(out.status.success(), "{}", stderr(&out));
    let yaml = stdout(&out);

    // accounts-service has two SLOs, so: two `kind: SLO` documents separated
    // by one `---`.
    assert_eq!(yaml.matches("apiVersion: openslo/v1").count(), 2, "{yaml}");
    assert_eq!(yaml.matches("kind: SLO").count(), 2, "{yaml}");
    assert!(yaml.contains("name: requests-availability"), "{yaml}");
    assert!(yaml.contains("name: requests-latency"), "{yaml}");
}

// ---- the stdout/stderr split ----

#[test]
fn notes_go_to_stderr_so_stdout_stays_a_pipeable_stream() {
    // accounts-service drops per-SLO `alerting` and relocates service-level
    // labels, so it produces notes. They must not contaminate the YAML: a
    // `slokit export ... > out.yaml` has to yield a parseable document.
    let out = slokit(&["export", "-i", EXAMPLE]);
    assert!(out.status.success(), "{}", stderr(&out));

    let err = stderr(&out);
    assert!(
        err.contains("note: ") && err.contains("alerting"),
        "expected the dropped-alerting note on stderr, got: {err}"
    );
    assert!(
        !stdout(&out).contains("note: "),
        "notes leaked into stdout: {}",
        stdout(&out)
    );

    // And the proof that stdout really is clean: re-importing it works.
    let dir = temp_dir("pipe");
    let path = dir.join("piped.yaml");
    fs::write(&path, stdout(&out)).unwrap();
    assert!(
        slokit(&["validate", "-i", path.to_str().unwrap()])
            .status
            .success(),
        "stdout was not a valid OpenSLO stream"
    );
}

#[test]
fn notes_name_only_what_was_actually_dropped() {
    // The negative side of the note behavior, without which "notes are
    // printed" could be satisfied by printing them unconditionally. A spec
    // with no service labels and no alerting must report neither.
    //
    // Note-FREE output is not reachable: validation requires the {{.window}}
    // token in every query (src/spec/validate.rs), and that token always earns
    // its own note, so the assertion is per-note rather than "stderr is empty".
    let dir = temp_dir("only-dropped");
    let spec = dir.join("raw.yaml");
    fs::write(&spec, RAW_SPEC).unwrap();

    let out = slokit(&["export", "-i", spec.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        !err.contains("alerting") && !err.contains("service-level label"),
        "reported a drop that never happened: {err}"
    );
    assert!(
        err.contains("{{.window}}"),
        "the one real transformation must still be reported: {err}"
    );
}

// ---- directory input and directory output ----

#[test]
fn a_directory_of_specs_exports_every_spec_to_stdout() {
    let out = slokit(&["export", "-i", "examples/infraportal/slos"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let yaml = stdout(&out);

    // Eight committed specs, 16 SLOs.
    assert_eq!(yaml.matches("apiVersion: openslo/v1").count(), 16, "{yaml}");
    assert!(yaml.contains("service: accounts-service"), "{yaml}");
    assert!(yaml.contains("service: search-service"), "{yaml}");

    // Still one valid stream after joining, not eight concatenated blobs.
    let dir = temp_dir("dirin");
    let path = dir.join("all.yaml");
    fs::write(&path, &yaml).unwrap();
    let back = slokit(&["validate", "-i", path.to_str().unwrap()]);
    assert!(
        back.status.success(),
        "the joined stream did not re-import: {}",
        stderr(&back)
    );
    assert_eq!(stdout(&back).lines().count(), 8, "{}", stdout(&back));
}

#[test]
fn output_writes_one_file_per_service_into_a_directory_it_creates() {
    let dir = temp_dir("outdir");
    // A path that does not exist yet: the flag documents that it is created.
    let out_dir = dir.join("nested").join("openslo");

    let out = slokit(&[
        "export",
        "-i",
        "examples/infraportal/slos",
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let mut written: Vec<String> = fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    written.sort();
    assert_eq!(
        written,
        vec![
            "accounts-service.yaml",
            "activities-service.yaml",
            "automation-service.yaml",
            "contacts-service.yaml",
            "integrations-service.yaml",
            "opportunities-service.yaml",
            "reporting-service.yaml",
            "search-service.yaml",
        ]
    );

    // Nothing went to stdout in this mode, and each file re-imports on its own.
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    let one = out_dir.join("search-service.yaml");
    assert!(
        slokit(&["validate", "-i", one.to_str().unwrap()])
            .status
            .success(),
        "a per-service file did not re-import"
    );
}

// ---- the guards on --output ----

#[test]
fn a_service_name_that_is_not_a_file_name_is_rejected_under_output() {
    // `service` is only checked non-empty by validation, so it can hold a path
    // separator. Under --output that would write outside the directory.
    let dir = temp_dir("traversal");
    let spec = dir.join("evil.yaml");
    fs::write(
        &spec,
        RAW_SPEC.replace("service: alpha", "service: ../escaped"),
    )
    .unwrap();
    let out_dir = dir.join("out");

    let out = slokit(&[
        "export",
        "-i",
        spec.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(
        !out.status.success(),
        "a service name with a path separator must not exit 0"
    );
    let err = stderr(&out);
    assert!(
        err.contains("--output"),
        "message must name the flag: {err}"
    );
    // `<out_dir>/../escaped.yaml` is exactly `<dir>/escaped.yaml`: the file the
    // unguarded path would have created outside the output directory.
    assert!(
        !dir.join("escaped.yaml").exists(),
        "nothing may be written outside the output directory"
    );

    // The same spec still exports fine to stdout: the guard belongs to
    // --output, not to the mapping.
    let piped = slokit(&["export", "-i", spec.to_str().unwrap()]);
    assert!(piped.status.success(), "{}", stderr(&piped));
}

#[test]
fn two_specs_sharing_a_service_are_rejected_before_anything_is_written() {
    let dir = temp_dir("collide");
    let specs = dir.join("specs");
    fs::create_dir_all(&specs).unwrap();
    fs::write(specs.join("a.yaml"), RAW_SPEC).unwrap();
    // Same service, different SLO name: legal to slokit, but both would map to
    // alpha.yaml and one would be silently lost.
    fs::write(specs.join("b.yaml"), RAW_SPEC.replace("avail", "latency")).unwrap();
    let out_dir = dir.join("out");

    let out = slokit(&[
        "export",
        "-i",
        specs.to_str().unwrap(),
        "-o",
        out_dir.to_str().unwrap(),
    ]);
    assert!(!out.status.success(), "a name collision must not exit 0");
    assert!(
        stderr(&out).contains("alpha"),
        "message must name the service: {}",
        stderr(&out)
    );
    assert!(
        !out_dir.exists(),
        "the batch is rejected before any file is written"
    );
}

#[test]
fn an_output_path_that_is_an_existing_file_is_rejected() {
    let dir = temp_dir("notadir");
    let file = dir.join("rules.yaml");
    fs::write(&file, "pre-existing").unwrap();

    let out = slokit(&["export", "-i", EXAMPLE, "-o", file.to_str().unwrap()]);
    assert!(!out.status.success(), "a file target must not exit 0");
    // Asserted against the GUARD's own wording, not just the word "directory":
    // removing the guard still exits non-zero, because `create_dir_all` then
    // fails with "creating output directory <path>". A looser assertion passes
    // either way and proves nothing (this test did, until the mutation probe
    // caught it).
    assert!(
        stderr(&out).contains("--output") && stderr(&out).contains("exists and is not a directory"),
        "message must name the flag and explain it takes a directory: {}",
        stderr(&out)
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "pre-existing",
        "the existing file must not be overwritten"
    );
}

// ---- fail-closed and flag surface ----

#[test]
fn an_unrepresentable_construct_fails_closed_naming_the_field() {
    let dir = temp_dir("plugin");
    let spec = dir.join("plugin.yaml");
    fs::write(&spec, PLUGIN_SPEC).unwrap();

    // The spec is valid slokit (validate accepts it), so this is the export
    // refusing, not the loader.
    assert!(
        slokit(&["validate", "-i", spec.to_str().unwrap()])
            .status
            .success(),
        "the fixture must be a valid slokit spec for this test to mean anything"
    );

    let out = slokit(&["export", "-i", spec.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "an unrepresentable construct must not exit 0"
    );
    let err = stderr(&out);
    assert!(
        err.contains("sli.plugin") && err.contains("slokit/availability/http-requests-total"),
        "the error must name the field and the offending value: {err}"
    );
    assert!(
        stdout(&out).is_empty(),
        "nothing may be emitted for a failed export: {}",
        stdout(&out)
    );
}

#[test]
fn the_format_flag_accepts_only_openslo() {
    // The flag exists so a second format is not a breaking change; today an
    // unknown value must be rejected rather than silently ignored.
    let out = slokit(&["export", "--format", "sloth", "-i", EXAMPLE]);
    assert!(!out.status.success(), "an unknown format must not exit 0");
    assert!(
        stderr(&out).contains("openslo"),
        "clap must list the accepted value: {}",
        stderr(&out)
    );

    // And the flag is optional, defaulting to the only value there is.
    assert!(slokit(&["export", "-i", EXAMPLE]).status.success());
}

#[test]
fn export_appears_in_the_top_level_help() {
    let out = slokit(&["--help"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("export"),
        "the subcommand must be discoverable: {}",
        stdout(&out)
    );
}
