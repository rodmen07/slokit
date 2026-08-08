//! Binary-level proof for the sloth Kubernetes CRD import (v1.6.0 PR 2).
//!
//! The v1.6.0 done-when clauses are stated about the CLI, not the library, and
//! PR 1 could only answer half of clause 1: it shipped **auto-detection**, so
//! `slokit validate -i <fixture>` started exiting 0, but the clause also asks
//! for the dialect with the explicit `--input-format` value, and
//! `InputFormat` had no third variant to pass. That half is this file's main
//! gate, together with clause 2 — the twin byte-identity — asserted through
//! the real binary rather than through `generate_rules_with`.
//!
//! Before PR 2, every `--input-format sloth-crd` invocation here failed at
//! argument parsing (clap exits 2 with `invalid value 'sloth-crd'`); before
//! PR 1, the auto-detected ones failed with
//!
//! ```text
//! spec error: document 1: missing field `service`
//! ```
//!
//! which named neither the dialect nor the problem. That message is not gone —
//! it is exactly what pinning `--input-format slokit` on a CRD document still
//! produces, and [`the_pinned_dialect_overrides_detection_in_both_directions`]
//! asserts it, because a pin that quietly fell back to auto-detection would
//! pass every other test in this file.
//!
//! The library-level mapping assertions live in `tests/sloth_crd.rs`; this
//! file only proves the capability is reachable from the installed command,
//! on both routes, and that the two routes agree byte for byte.

#![cfg(feature = "cli")]

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const K8S_GETTING_STARTED: &str = "tests/fixtures/sloth_crd/k8s-getting-started.yaml";
const GETTING_STARTED_TWIN: &str = "tests/fixtures/sloth_crd/getting-started-twin.yaml";
const K8S_HOME_WIFI: &str = "tests/fixtures/sloth_crd/k8s-home-wifi.yaml";
const HOME_WIFI_TWIN: &str = "tests/fixtures/sloth_crd/home-wifi-twin.yaml";
const K8S_MULTIFILE: &str = "tests/fixtures/sloth_crd/k8s-multifile.yaml";

/// Every committed CRD fixture, in the order the roadmap lists them.
const FIXTURES: [&str; 3] = [K8S_GETTING_STARTED, K8S_HOME_WIFI, K8S_MULTIFILE];

/// The `--input-format` value this dialect is pinned with. Held in one place
/// so that the flag NAME is a single fact: `src/bin/slokit.rs` fixes it with
/// `#[value(name = "sloth-crd")]` rather than inheriting clap's derivation
/// from the variant identifier, and
/// [`the_pinned_value_is_advertised_under_this_exact_name`] reads it back out
/// of `--help` so a rename cannot happen silently.
const PIN: &str = "sloth-crd";

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

/// `slokit generate` with the given extra arguments, asserting exit 0 and
/// handing back **raw stdout bytes** — clause 2 is a byte comparison, so the
/// bytes never become a `String` on the way.
fn generate_bytes(args: &[&str]) -> Vec<u8> {
    let mut argv = vec!["generate"];
    argv.extend_from_slice(args);
    let out = slokit(&argv);
    assert!(
        out.status.success(),
        "generate {args:?} exited {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    assert!(
        !out.stdout.is_empty(),
        "generate {args:?} exited 0 but wrote nothing to stdout"
    );
    out.stdout
}

/// Every generate configuration a CLI user can reach that changes the rendered
/// rules, labelled by the invocation that reaches it — the same matrix
/// `tests/sloth_crd.rs::option_matrix` crosses at the library level, spelled
/// as real argv here.
///
/// It exists because a mis-mapping that pins a field the twin leaves unset
/// (`period: 30d` is the concrete example) is invisible under defaults and
/// only separates once an option moves. That the matrix really does separate
/// them is not assumed:
/// [`the_option_matrix_is_not_decorative`] asserts it.
fn option_matrix() -> Vec<Vec<&'static str>> {
    vec![
        vec![],
        vec!["--period", "7d"],
        vec!["--no-period-scaling"],
        vec!["--period", "7d", "--no-period-scaling"],
    ]
}

/// The two output formats `generate` can render. `operator` is included
/// because it is the format that makes this dialect a round trip: slokit
/// already emitted a Kubernetes custom resource without being able to read
/// one, and these fixtures are single-spec, so its default `metadata.name`
/// (the spec's service) needs no `--name`.
const FORMATS: [&str; 2] = ["prometheus", "operator"];

/// Assert an imported CRD document and its committed native twin render
/// identical bytes through the real binary, everywhere in the option space,
/// on BOTH the auto-detected and the pinned route.
fn assert_cli_twins_agree(crd: &str, twin: &str) {
    for format in FORMATS {
        for opts in option_matrix() {
            let mut detected = vec!["-i", crd, "--format", format];
            detected.extend_from_slice(&opts);

            let mut pinned = vec!["-i", crd, "--input-format", PIN, "--format", format];
            pinned.extend_from_slice(&opts);

            let mut native = vec!["-i", twin, "--format", format];
            native.extend_from_slice(&opts);

            let detected_bytes = generate_bytes(&detected);
            let pinned_bytes = generate_bytes(&pinned);
            let native_bytes = generate_bytes(&native);

            assert_eq!(
                detected_bytes,
                native_bytes,
                "`slokit generate -i {crd} --format {format} {}` differs from its native twin \
                 ({} vs {} bytes)",
                opts.join(" "),
                detected_bytes.len(),
                native_bytes.len()
            );
            assert_eq!(
                pinned_bytes,
                native_bytes,
                "`slokit generate -i {crd} --input-format {PIN} --format {format} {}` differs \
                 from its native twin ({} vs {} bytes)",
                opts.join(" "),
                pinned_bytes.len(),
                native_bytes.len()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Clause 1: `validate` exits 0, on both routes.
// ---------------------------------------------------------------------------

#[test]
fn every_committed_fixture_validates_by_auto_detection() {
    for fixture in FIXTURES {
        let out = slokit(&["validate", "-i", fixture]);
        assert!(
            out.status.success(),
            "validate {fixture} exited {:?}: {}",
            out.status.code(),
            stderr(&out)
        );
        assert!(
            stdout(&out).contains("is valid"),
            "validate {fixture} exited 0 but said: {}",
            stdout(&out)
        );
    }
}

#[test]
fn every_committed_fixture_validates_with_the_dialect_pinned() {
    // The half of done-when clause 1 that PR 1 could not answer: PR 1 shipped
    // detection only, so this invocation did not parse at all until
    // `InputFormat` grew its third variant.
    for fixture in FIXTURES {
        let out = slokit(&["validate", "-i", fixture, "--input-format", PIN]);
        assert!(
            out.status.success(),
            "validate {fixture} --input-format {PIN} exited {:?}: {}",
            out.status.code(),
            stderr(&out)
        );
        assert!(
            stdout(&out).contains("is valid"),
            "validate {fixture} --input-format {PIN} exited 0 but said: {}",
            stdout(&out)
        );
    }
}

#[test]
fn the_pinned_value_is_advertised_under_this_exact_name() {
    // The flag value is a public CLI surface: scripts spell it literally. It
    // is fixed in `src/bin/slokit.rs` with `#[value(name = "sloth-crd")]`
    // rather than left to clap's derivation, and this reads it back.
    let out = slokit(&["validate", "--help"]);
    assert!(out.status.success());
    let help = stdout(&out);
    assert!(
        help.contains(PIN),
        "`validate --help` does not advertise `{PIN}`:\n{help}"
    );
    assert!(
        help.contains("PrometheusServiceLevel"),
        "`validate --help` names `{PIN}` without saying what it reads:\n{help}"
    );
}

#[test]
fn the_pinned_dialect_overrides_detection_in_both_directions() {
    // The behaviour difference the flag exists for. A pin that silently fell
    // back to auto-detection would pass every other test in this file, because
    // every fixture here detects correctly on its own.

    // Pinned OFF the CRD: the exact pre-v1.6.0 failure, reproduced on purpose.
    let out = slokit(&[
        "validate",
        "-i",
        K8S_GETTING_STARTED,
        "--input-format",
        "slokit",
    ]);
    assert!(
        !out.status.success(),
        "a CRD document pinned as native slokit must not validate"
    );
    assert!(
        stderr(&out).contains("document 1: missing field `service`"),
        "expected the native parser's own failure, got: {}",
        stderr(&out)
    );

    let out = slokit(&[
        "validate",
        "-i",
        K8S_GETTING_STARTED,
        "--input-format",
        "openslo",
    ]);
    assert!(
        !out.status.success(),
        "a CRD document pinned as OpenSLO must not validate"
    );
    assert!(
        stderr(&out).contains("unsupported apiVersion 'sloth.slok.dev/v1'"),
        "expected the OpenSLO importer to name the apiVersion it refused, got: {}",
        stderr(&out)
    );

    // Pinned ONTO a document that is not one: the error names the dialect it
    // was read as, which is what the v1.6.0 grounding pass found missing from
    // the pre-PR-1 message.
    let out = slokit(&[
        "validate",
        "-i",
        GETTING_STARTED_TWIN,
        "--input-format",
        PIN,
    ]);
    assert!(
        !out.status.success(),
        "a native document pinned as {PIN} must not validate"
    );
    let err = stderr(&out);
    assert!(
        err.contains("sloth-crd document 1"),
        "the failure must name the dialect it was read as, got: {err}"
    );
    assert!(
        err.contains("missing field `spec`"),
        "the failure must name the missing envelope field, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Clause 2: the twins, through the binary.
// ---------------------------------------------------------------------------

#[test]
fn the_getting_started_fixture_generates_its_native_twins_bytes() {
    assert_cli_twins_agree(K8S_GETTING_STARTED, GETTING_STARTED_TWIN);
}

#[test]
fn the_home_wifi_fixture_generates_its_native_twins_bytes() {
    assert_cli_twins_agree(K8S_HOME_WIFI, HOME_WIFI_TWIN);
}

#[test]
fn the_option_matrix_is_not_decorative() {
    // A two-source agreement guard whose sides are both produced by the same
    // option-taking pipeline proves nothing if every entry lands on the same
    // bytes: `assert_cli_twins_agree` would then be one comparison wearing a
    // loop, and the mis-map it exists to catch (a mapper pinning `period: 30d`
    // where the twin leaves it unset) passes under defaults.
    //
    // So this reads the SAME `option_matrix()` the twin tests cross, rather
    // than restating its entries, and asserts it still separates. Collapsing
    // the matrix to its default entry therefore reddens this test instead of
    // quietly weakening the other two.
    let rendered: BTreeSet<Vec<u8>> = option_matrix()
        .into_iter()
        .map(|opts| {
            let mut argv = vec!["-i", K8S_GETTING_STARTED];
            argv.extend_from_slice(&opts);
            generate_bytes(&argv)
        })
        .collect();

    // Three, not four: `--no-period-scaling` is a no-op at these fixtures' 30d
    // default period (scaling 30d windows to 30d changes nothing), so it
    // collides with the default entry on purpose. It stops colliding as soon
    // as `--period` moves, which is why the fourth entry exists.
    assert!(
        rendered.len() >= 3,
        "`option_matrix()` renders only {} distinct outputs; it no longer \
         discriminates and clause 2's twin comparison has stopped meaning \
         anything beyond the default invocation",
        rendered.len()
    );
}

#[test]
fn the_multi_document_stream_yields_every_service() {
    // sloth's `k8s-multifile.yml` layout: two `PrometheusServiceLevel`
    // documents in one file, which the importer must not stop reading after
    // the first.
    let out = slokit(&["validate", "-i", K8S_MULTIFILE, "--input-format", PIN]);
    assert!(out.status.success(), "{}", stderr(&out));
    let report = stdout(&out);
    assert!(report.contains("'myservice'"), "{report}");
    assert!(report.contains("'myservice2'"), "{report}");

    let rules = String::from_utf8(generate_bytes(&[
        "-i",
        K8S_MULTIFILE,
        "--input-format",
        PIN,
    ]))
    .expect("generated rules are UTF-8");
    assert!(rules.contains("sloth_service: myservice\n"), "{rules}");
    assert!(rules.contains("sloth_service: myservice2\n"), "{rules}");
}

// ---------------------------------------------------------------------------
// Clause 3: the fidelity notes, on the surface a CLI user actually sees.
// ---------------------------------------------------------------------------

#[test]
fn the_ignored_kubernetes_metadata_is_reported_on_stderr_not_stdout() {
    // `tests/sloth_crd.rs` asserts the notes exist on the `Import`. What the
    // binary owes on top of that is putting them where a shell user and a
    // pipeline both survive them: stderr, so `slokit generate -i crd.yaml >
    // rules.yaml` writes rules only.
    let out = slokit(&["generate", "-i", K8S_HOME_WIFI, "--input-format", PIN]);
    assert!(out.status.success(), "{}", stderr(&out));

    let notes = stderr(&out);
    for field in ["metadata.name", "metadata.namespace", "metadata.labels"] {
        assert!(
            notes.contains(field),
            "no import note named `{field}`:\n{notes}"
        );
    }

    let rules = stdout(&out);
    assert!(
        !rules.contains("note:"),
        "an import note leaked into the rules on stdout:\n{rules}"
    );
    // The object labels are ignored, not merged into the rule labels — the
    // reason that note exists at all.
    assert!(
        !rules.contains("role: alert-rules"),
        "a Kubernetes object label reached the generated rules:\n{rules}"
    );
}

// ---------------------------------------------------------------------------
// v1.8.0 PR 1: an SLO plugin chain is captured and linted, not refused
//
// v1.8.0 done-when clause 2 in ROADMAP.md, and the premise the whole slice
// rests on. The v1.7.0 work proved byte-identity for a chain on the NATIVE
// route; the CRD route is a different importer, so the claim is re-derived
// here on this dialect rather than inherited.
// ---------------------------------------------------------------------------

/// sloth's own CRD document carrying SLO plugin chains: a two-entry
/// `spec.sloPlugins` plus a five-entry `slos[].plugins`. Committed under
/// `sloth_corpus/` with its upstream sha256 pinned by `tests/sloth_corpus.rs`,
/// which is why this file reads it there instead of copying it.
const CORPUS_CHAINED_CRD: &str = "tests/fixtures/sloth_corpus/slo-plugin-k8s-getting-started.yml";

/// The second upstream CRD document with a chain: one `spec.sloPlugins` entry
/// and no SLO-level key, so the two documents between them cover both spellings
/// and both "only one of the two is present" cases.
const CORPUS_CHAINED_CRD_2: &str = "tests/fixtures/sloth_corpus/contrib-denominator-corrected.yaml";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-sloth-crd-cli-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// `yaml` with every `sloPlugins:` / `plugins:` block deleted — the key line
/// and every line indented deeper than it — and nothing else touched.
///
/// Written here rather than committed as a second fixture on purpose: a
/// hand-edited "chain removed" copy is exactly the kind of file that acquires a
/// second difference over time, and then the byte comparison below is measuring
/// two edits instead of one. Deriving it means the only difference is
/// mechanical, and [`chain_removal_removes_the_chain_and_nothing_else`] proves
/// the derivation did something.
fn without_chains(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.split('\n').collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if matches!(line.trim(), "sloPlugins:" | "plugins:") {
            let base = indent_of(line);
            i += 1;
            while i < lines.len() && (lines[i].trim().is_empty() || indent_of(lines[i]) > base) {
                i += 1;
            }
            continue;
        }
        out.push(line);
        i += 1;
    }
    out.join("\n")
}

/// The negative control for the comparison below, and the reason it is not a
/// document compared against itself: the stripped copy must really differ, must
/// carry no chain key, and must still be a document slokit reads.
#[test]
fn chain_removal_removes_the_chain_and_nothing_else() {
    for source in [CORPUS_CHAINED_CRD, CORPUS_CHAINED_CRD_2] {
        let original = fs::read_to_string(source).expect("corpus fixture is readable");
        let stripped = without_chains(&original);

        assert!(
            original.contains("sloPlugins:"),
            "{source} is supposed to carry a spec-level chain"
        );
        assert_ne!(
            stripped, original,
            "{source}: chain removal changed nothing, so the byte comparison would \
             be a document against itself"
        );
        assert!(
            !stripped.contains("sloPlugins"),
            "{source}: a spec-level chain survived removal"
        );
        assert!(
            !stripped.lines().any(|l| l.trim() == "plugins:"),
            "{source}: an SLO-level chain survived removal"
        );
        // Removal must not have eaten the document: `slos:` and the SLI are
        // what everything downstream needs.
        assert!(
            stripped.contains("slos:") && stripped.contains("errorQuery:"),
            "{source}: chain removal took more than the chain:\n{stripped}"
        );
    }
}

/// **Done-when clause 2.** A CRD document carrying a plugin chain generates
/// byte-identical rules to the same document with the chain removed, through
/// the shipped binary, in both output formats, across the whole option space
/// (L-045: a single point of the option space would prove agreement under
/// defaults and nothing else).
///
/// This is the claim the v1.6.0 refusal text denied. It said a chain "would
/// rewrite the generated rules"; that is true of *sloth's* generator and false
/// of slokit's, which has no plugin-chain stage at all.
#[test]
fn a_crd_plugin_chain_changes_no_generated_byte() {
    let dir = temp_dir("nochain");
    for source in [CORPUS_CHAINED_CRD, CORPUS_CHAINED_CRD_2] {
        let original = fs::read_to_string(source).expect("corpus fixture is readable");
        let stripped_path = dir.join(
            PathBuf::from(source)
                .file_name()
                .expect("fixture has a file name"),
        );
        fs::write(&stripped_path, without_chains(&original)).expect("temp fixture is writable");
        let stripped = stripped_path.to_string_lossy().into_owned();

        for format in FORMATS {
            for opts in option_matrix() {
                let mut with = vec!["-i", source, "--format", format];
                with.extend_from_slice(&opts);
                let mut without = vec!["-i", stripped.as_str(), "--format", format];
                without.extend_from_slice(&opts);

                let with_bytes = generate_bytes(&with);
                let without_bytes = generate_bytes(&without);
                assert_eq!(
                    with_bytes,
                    without_bytes,
                    "{source} --format {format} {}: the plugin chain changed the rules \
                     ({} vs {} bytes)",
                    opts.join(" "),
                    with_bytes.len(),
                    without_bytes.len()
                );
            }
        }
    }
}

/// The other half of the behavior difference: the document is not merely
/// accepted, the drop is reported. Before v1.8.0 both of these exited non-zero
/// from the importer with `spec.sloPlugins is a sloth SLO plugin chain and has
/// no slokit equivalent`.
#[test]
fn a_crd_plugin_chain_is_reported_by_lint_rather_than_refused() {
    for (source, expected_findings) in [(CORPUS_CHAINED_CRD, 2), (CORPUS_CHAINED_CRD_2, 1)] {
        let out = slokit(&["lint", "-i", source]);
        let text = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(
            text.matches("SLO_PLUGIN_CHAIN_DROPPED").count(),
            expected_findings,
            "{source}: one finding per chain key the document carries:\n{text}"
        );
        assert!(
            !text.contains("refused rather than dropped"),
            "{source}: the superseded refusal text is still reachable:\n{text}"
        );
    }
}

/// KNOWN GAP, characterising what the code does today rather than what it
/// should do (filed as a LOW bug in the autodev slokit backlog, 2026-08-08).
///
/// The finding names the key with its NATIVE spelling, `slo_plugins`, even when
/// the document that carried it is a CRD spelling it `sloPlugins`. The lint
/// reads a `Spec`, and a `Spec` does not remember which dialect produced it, so
/// the message tells a CRD author to go and delete a key their file does not
/// contain. Pinned here so the day a dialect-aware message ships, this test
/// fails and says so instead of the gap closing unnoticed.
#[test]
fn known_gap_the_crd_lint_finding_names_the_native_spelling() {
    let out = slokit(&["lint", "-i", CORPUS_CHAINED_CRD]);
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("`slo_plugins`"),
        "expected the native spelling in the finding:\n{text}"
    );
    assert!(
        !text.contains("`sloPlugins`"),
        "the finding now names the CRD spelling — the known gap is CLOSED, so \
         delete this test and the backlog item it pins:\n{text}"
    );
}
