//! The cross-dialect parity contract (v1.8.0 PR 2).
//!
//! slokit reads four input dialects, each added in a different milestone:
//! native slokit specs (0.1.0), OpenSLO `v1` (0.10.0), OpenSLO `v1alpha`
//! (1.5.0) and the sloth Kubernetes CRD (1.6.0). Every one of them was
//! verified against sloth or against a native twin. **Nothing had ever checked
//! that they agree with each other**, and the v1.7.0 corpus census found two
//! constructs whose answer depended on which dialect the document arrived in.
//!
//! [`tests/sloth_corpus.rs`](sloth_corpus) pins what slokit does with the
//! upstream corpus. This file pins something the corpus cannot see, because
//! every upstream document is written in exactly one dialect: for each
//! construct that **more than one importer can receive**, what each of them
//! does with it, and whether those answers agree.
//!
//! # What the contract is made of
//!
//! - Four `base.*` fixtures, one per dialect, describing the SAME SLO. They
//!   generate byte-identical rules
//!   ([`the_four_dialects_generate_identical_rules_from_matched_documents`]),
//!   which is what makes "matched" a measured property rather than a comment.
//! - One fixture per (construct, dialect): the base document plus exactly one
//!   construct, so a difference in the run can only come from the construct.
//! - [`CONSTRUCTS`], the recorded answer each dialect gives, and the recorded
//!   VERDICT on whether they agree.
//!
//! # What it catches that the per-dialect suites do not
//!
//! Every existing OpenSLO / CRD test asserts one route's behaviour against a
//! twin or a fixture, so a change to ONE route is green everywhere as long as
//! that route's own expectations are updated. Here, changing one dialect's
//! answer and leaving the others alone fails
//! [`every_row_has_its_recorded_disposition`] by name; updating the row and
//! leaving the summary alone fails [`the_recorded_verdicts_match_the_rows`].
//!
//! # Why the fixtures live in a subdirectory
//!
//! `tests/schema.rs`'s `native_fixture_files()` (`tests/schema.rs:531`) reads
//! `tests/fixtures` **non-recursively** and validates everything it finds
//! against the native JSON Schema. Twelve of the sixteen documents here are
//! not native specs at all, so dropping them at the fixtures root would redden
//! the schema suite on the very commit that adds them. `dialect_parity/` is
//! invisible to that walker, the same discharge `sloth_crd/` got in v1.6.0 and
//! `sloth_corpus/` in v1.7.0.
//!
//! # Why the table is hand-written while the file list is not
//!
//! The fixtures are glob-discovered (recursively, with a hard failure on a
//! zero-length walk) and reconciled against [`CONSTRUCTS`] in **both**
//! directions, so the table cannot silently stop covering the tree and the
//! tree cannot silently stop matching the table. The dispositions themselves
//! have to be written down — that is the contract.

#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Where the parity fixtures live, relative to the crate root.
const FIXTURE_DIR: &str = "tests/fixtures/dialect_parity";

// ---------------------------------------------------------------------------
// The contract
// ---------------------------------------------------------------------------

/// An input dialect that produces specs.
///
/// `kind: AlertWindows` catalogues are deliberately absent: they are a
/// burn-rate window catalogue rather than a spec, so no construct below can
/// arrive in one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Slokit,
    OpensloV1,
    OpensloV1alpha,
    SlothCrd,
}

use Dialect::{OpensloV1, OpensloV1alpha, Slokit, SlothCrd};

impl Dialect {
    /// How this dialect is named in a failure message.
    fn label(self) -> &'static str {
        match self {
            Slokit => "slokit (native)",
            OpensloV1 => "openslo/v1",
            OpensloV1alpha => "openslo/v1alpha",
            SlothCrd => "sloth.slok.dev/v1 (Kubernetes CRD)",
        }
    }

    /// The matched base document for this dialect.
    fn base_fixture(self) -> &'static str {
        match self {
            Slokit => "base.slokit.yaml",
            OpensloV1 => "base.openslo-v1.yaml",
            OpensloV1alpha => "base.openslo-v1alpha.yaml",
            SlothCrd => "base.sloth-crd.yaml",
        }
    }
}

/// Every dialect, in the order they were added to slokit.
const DIALECTS: [Dialect; 4] = [Slokit, OpensloV1, OpensloV1alpha, SlothCrd];

/// What one dialect does with one construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// Accepted and HONORED: `validate` exits 0 and the construct reaches the
    /// generated rules.
    Imported,
    /// Accepted and dropped, with an import note containing this substring.
    /// The substring carries the REASON, so a note that starts saying
    /// something else fails here rather than passing as "still noted".
    NotedAndIgnored(&'static str),
    /// Accepted and dropped in SILENCE: nothing in the notes mentions the
    /// construct at all. This is a defect shape, not a design: it is recorded
    /// so the next one is a row that disagrees with its siblings rather than
    /// an absence nobody can see.
    ///
    /// No row uses it as of v1.10.0, and that is the point rather than an
    /// oversight: `object-annotations` was the last silent drop and this
    /// milestone closed it. The variant stays because it is the NAME for the
    /// shape — deleting it would leave the next silent drop with nothing to
    /// be recorded as, which is the state (an absence nobody can see) the
    /// contract exists to end. `Refused` and the rest are constructed, so the
    /// allow is scoped to this variant alone.
    #[allow(dead_code)]
    SilentlyIgnored,
    /// Accepted and dropped, reported by `slokit lint` under this code rather
    /// than by an import note.
    Linted(&'static str),
    /// Refused: `validate` exits non-zero and its output contains this
    /// substring.
    Refused(&'static str),
}

use Disposition::{Imported, Linted, NotedAndIgnored, Refused, SilentlyIgnored};

/// Whether the dialects agree about a construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Every dialect that can receive the construct answers identically.
    Uniform,
    /// They do not, and this is why.
    Divergent(&'static str),
}

use Verdict::{Divergent, Uniform};

/// One dialect's row for one construct.
struct Row {
    dialect: Dialect,
    /// The fixture under [`FIXTURE_DIR`]: the dialect's base document plus
    /// exactly this construct.
    fixture: &'static str,
    disposition: Disposition,
}

/// One construct that more than one importer can receive.
struct Construct {
    /// Stable id, used in failure messages.
    id: &'static str,
    /// The construct in one line.
    what: &'static str,
    /// The words an import NOTE about this construct contains. Asserted
    /// PRESENT (inside the row's own substring) on a noted row, and ABSENT
    /// from the notes on every other row — which is what makes
    /// [`SilentlyIgnored`] an assertion rather than a shrug.
    note_key: &'static str,
    /// A string that appears in the generated rules if and only if the
    /// construct was honored.
    rule_marker: &'static str,
    /// What each dialect that can receive the construct does with it.
    rows: &'static [Row],
    /// The recorded summary of those rows.
    verdict: Verdict,
}

/// The note both OpenSLO routes emit for a dropped `metadata.displayName`.
/// Written once so the two rows below cannot drift into "both noted, in
/// different words", which is what the v1.5.0 → 1.8.0 divergence looked like
/// from a distance.
const DISPLAY_NAME_NOTE: &str =
    "metadata.displayName does not map and was ignored (slokit SLOs carry a name and a description)";

/// The note all three object-envelope dialects emit for a dropped
/// `metadata.annotations`, recorded once here for the same reason
/// [`DISPLAY_NAME_NOTE`] is: three rows carrying three literals can be
/// reworded one at a time. Since v1.10.0 the three emission sites read one
/// constant too (`OBJECT_ANNOTATIONS_NOTE`, `src/spec/import.rs`), so this
/// string is the independent expectation that constant is checked against —
/// deriving it from the same source would make the check unfalsifiable.
const OBJECT_ANNOTATIONS_NOTE: &str = "metadata.annotations do not map and were ignored";

/// The unknown-SLI-plugin refusal, shared by the two dialects that can express
/// an `sli.plugin`.
const UNKNOWN_PLUGIN_REFUSAL: &str =
    "unknown SLI plugin 'parity/no/such-plugin' (not in the plugin registry)";

/// Every construct more than one importer can receive, and the answer each
/// gives. A dialect with no row cannot express the construct at all: the
/// native dialect has no object metadata, and neither OpenSLO version has an
/// SLO plugin chain or an `sli.plugin`.
const CONSTRUCTS: &[Construct] = &[
    Construct {
        id: "display-name",
        what: "the object's human-readable name (`metadata.displayName`)",
        note_key: "metadata.displayName",
        rule_marker: "Requests Availability",
        rows: &[
            Row {
                dialect: OpensloV1,
                fixture: "display-name.openslo-v1.yaml",
                // Silent until 1.8.0: the shared envelope parsed the field on
                // both routes and only the v1alpha path mentioned it, so a v1
                // document lost its display name with nothing said. That is
                // the asymmetry this milestone removed, and this row is what
                // stops it coming back.
                disposition: NotedAndIgnored(DISPLAY_NAME_NOTE),
            },
            Row {
                dialect: OpensloV1alpha,
                fixture: "display-name.openslo-v1alpha.yaml",
                disposition: NotedAndIgnored(DISPLAY_NAME_NOTE),
            },
        ],
        verdict: Uniform,
    },
    Construct {
        id: "object-labels",
        what: "labels on the object envelope (`metadata.labels`)",
        note_key: "metadata.labels",
        rule_marker: "team-parity",
        rows: &[
            Row {
                dialect: OpensloV1,
                fixture: "object-labels.openslo-v1.yaml",
                // OpenSLO v1 has no other place to put SLO labels, so here
                // the object envelope IS the labelling surface.
                disposition: Imported,
            },
            Row {
                dialect: OpensloV1alpha,
                fixture: "object-labels.openslo-v1alpha.yaml",
                disposition: NotedAndIgnored(
                    "metadata.labels are not part of openslo/v1alpha and were ignored",
                ),
            },
            Row {
                dialect: SlothCrd,
                fixture: "object-labels.sloth-crd.yaml",
                disposition: NotedAndIgnored(
                    "metadata.labels (owner) are Kubernetes object labels and were ignored",
                ),
            },
        ],
        verdict: Divergent(
            "the same key means different things: openslo/v1 has no other labelling surface, \
             while a CRD's spec.labels is the rule labels and v1alpha has no labels at all. \
             Both dialects that drop them say so and say where the labels belong instead.",
        ),
    },
    Construct {
        id: "object-annotations",
        what: "annotations on the object envelope (`metadata.annotations`)",
        note_key: "metadata.annotations",
        rule_marker: "owner-team",
        rows: &[
            Row {
                dialect: OpensloV1,
                fixture: "object-annotations.openslo-v1.yaml",
                // The only route that ever said it: since 0.10.0.
                disposition: NotedAndIgnored(OBJECT_ANNOTATIONS_NOTE),
            },
            Row {
                dialect: OpensloV1alpha,
                fixture: "object-annotations.openslo-v1alpha.yaml",
                // FOUND BY THIS CONTRACT ON ITS FIRST RUN and filed rather
                // than fixed, because v1.8.0's slice named only the display
                // name; closed in v1.10.0. It was the mirror image of
                // `display-name` above — there v1 was the silent one — and
                // silent for the same mechanical reason: `Metadata` is shared
                // by both OpenSLO routes, so v1alpha parsed annotations it
                // never mentioned.
                disposition: NotedAndIgnored(OBJECT_ANNOTATIONS_NOTE),
            },
            Row {
                dialect: SlothCrd,
                fixture: "object-annotations.sloth-crd.yaml",
                // Silent until v1.10.0 for a DIFFERENT reason than v1alpha's:
                // `ObjectMeta` carried no `annotations` field, so serde
                // dropped the key before the importer could see it.
                disposition: NotedAndIgnored(OBJECT_ANNOTATIONS_NOTE),
            },
        ],
        verdict: Uniform,
    },
    Construct {
        id: "slo-plugin-chain",
        what: "a sloth SLO plugin chain (`slo_plugins` / `sloPlugins`, `plugins`)",
        // No import note carries this: it is a LINT finding on both routes,
        // which is itself part of the contract.
        note_key: "sloth SLO plugin chain",
        rule_marker: "sloth.dev/core/debug/v1",
        rows: &[
            Row {
                dialect: Slokit,
                fixture: "plugin-chain.slokit.yaml",
                disposition: Linted("SLO_PLUGIN_CHAIN_DROPPED"),
            },
            Row {
                dialect: SlothCrd,
                fixture: "plugin-chain.sloth-crd.yaml",
                // A hard error until v1.8.0 PR 1, on the stated grounds that
                // the chain "would rewrite the generated rules" — true of
                // sloth's generator and false of slokit's. This row is what
                // holds the two routes together now.
                disposition: Linted("SLO_PLUGIN_CHAIN_DROPPED"),
            },
        ],
        verdict: Uniform,
    },
    Construct {
        id: "unknown-sli-plugin",
        what: "an `sli.plugin.id` that is not in slokit's registry",
        note_key: "unknown SLI plugin",
        rule_marker: "parity/no/such-plugin",
        rows: &[
            Row {
                dialect: Slokit,
                fixture: "unknown-sli-plugin.slokit.yaml",
                disposition: Refused(UNKNOWN_PLUGIN_REFUSAL),
            },
            Row {
                dialect: SlothCrd,
                fixture: "unknown-sli-plugin.sloth-crd.yaml",
                disposition: Refused(UNKNOWN_PLUGIN_REFUSAL),
            },
        ],
        // Deliberate, and decision D1.8-2 of the v1.8.0 milestone: the plugin
        // registry is a closed set by the 0.9.0 design, so refusing an unknown
        // id is the fail-closed behaviour on every route.
        verdict: Uniform,
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR)
}

/// The path a fixture is passed to the binary as, `/`-separated so the note
/// prefix [`import_notes`] strips is byte-predictable on every platform.
fn fixture_arg(fixture: &str) -> String {
    format!("{FIXTURE_DIR}/{fixture}")
}

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit binary runs")
}

/// Everything the binary wrote, both streams, as one searchable string.
fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The import notes the binary printed, with the `note: <fixture path>: `
/// prefix removed.
///
/// Stripping the path is load-bearing, not tidiness: every note line begins
/// with the fixture path, and these fixtures are NAMED after the construct
/// under test, so a plain `stderr.contains(note_key)` would match the FILENAME
/// and report a silent drop as a reported one.
fn import_notes(out: &Output, fixture: &str) -> Vec<String> {
    let arg = fixture_arg(fixture);
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter_map(|line| line.strip_prefix("note: "))
        .map(|line| match line.split_once(&format!("{arg}: ")) {
            Some((_, rest)) => rest.to_string(),
            None => line.to_string(),
        })
        .collect()
}

/// Every regular file under `dir`, recursively, as `/`-separated paths
/// relative to [`fixture_root`].
fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("fixture directory {} is readable: {e}", dir.display()))
        .map(|e| e.expect("readable dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .expect("dir entry has a name")
            .to_string_lossy()
            .to_string();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk(&path, &rel, out);
        } else {
            out.push(rel);
        }
    }
}

fn discovered_fixtures() -> Vec<String> {
    let mut found = Vec::new();
    walk(&fixture_root(), "", &mut found);
    assert!(
        !found.is_empty(),
        "zero files discovered under {FIXTURE_DIR}; every assertion in this file would be \
         vacuously green, so a moved or emptied fixture set fails here instead"
    );
    found.sort();
    found
}

/// Every fixture the contract claims: the four matched bases plus one per row.
fn listed_fixtures() -> Vec<String> {
    let mut listed: Vec<String> = DIALECTS
        .iter()
        .map(|d| d.base_fixture().to_string())
        .chain(
            CONSTRUCTS
                .iter()
                .flat_map(|c| c.rows.iter().map(|r| r.fixture.to_string())),
        )
        .collect();
    listed.sort();
    listed
}

// ---------------------------------------------------------------------------
// The matched bases: what makes every row below attributable to its construct
// ---------------------------------------------------------------------------

/// Output formats the bases must agree in. `operator` is included because it
/// carries the Kubernetes envelope, which is exactly where a CRD-specific
/// mis-map would hide.
const FORMATS: [&str; 2] = ["prometheus", "operator"];

/// `generate` invocations that pass no `--period`, so every dialect uses the
/// period its own document carries (or the default).
const PERIOD_FREE_OPTIONS: [(&str, &[&str]); 2] = [
    ("generate", &[]),
    ("generate --no-period-scaling", &["--no-period-scaling"]),
];

/// `generate` invocations that DO pass `--period`.
const PERIOD_PINNED_OPTIONS: [(&str, &[&str]); 2] = [
    ("generate --period 7d", &["--period", "7d"]),
    (
        "generate --period 7d --no-period-scaling",
        &["--period", "7d", "--no-period-scaling"],
    ),
];

fn generate(fixture: &str, format: &str, opts: &[&str]) -> Output {
    let path = fixture_arg(fixture);
    let mut args = vec!["generate", "-i", path.as_str(), "--format", format];
    args.extend_from_slice(opts);
    slokit(&args)
}

fn generated_rules(fixture: &str, format: &str, opts: &[&str], label: &str) -> Vec<u8> {
    let out = generate(fixture, format, opts);
    assert!(
        out.status.success(),
        "`{label} --format {format}` failed on {fixture}: {}",
        combined(&out).trim()
    );
    out.stdout
}

/// The four `base.*` documents describe the same SLO, so slokit must emit the
/// same bytes from all four. Without this, "one matched document per dialect"
/// would be a claim in a comment and every per-construct row below would rest
/// on it.
#[test]
fn the_four_dialects_generate_identical_rules_from_matched_documents() {
    for format in FORMATS {
        for (label, opts) in PERIOD_FREE_OPTIONS {
            let reference = generated_rules(Slokit.base_fixture(), format, opts, label);
            for dialect in DIALECTS.iter().skip(1) {
                let other = generated_rules(dialect.base_fixture(), format, opts, label);
                assert_eq!(
                    reference.len(),
                    other.len(),
                    "`{label} --format {format}`: {} generated {} bytes from {} but {} generated \
                     {} bytes from {}",
                    Slokit.label(),
                    reference.len(),
                    Slokit.base_fixture(),
                    dialect.label(),
                    other.len(),
                    dialect.base_fixture()
                );
                assert!(
                    reference == other,
                    "`{label} --format {format}`: {} and {} generated the same number of bytes \
                     from matched documents, but not the same bytes",
                    Slokit.label(),
                    dialect.label()
                );
            }
        }
    }
}

/// The one place the matched bases do NOT agree, recorded rather than avoided:
/// the CRD is the only dialect with no field for a period, so `--period`
/// reaches it and slides off the three documents that pin their own.
///
/// This is why the identity assertion above runs across an option space
/// instead of at `slokit generate` alone — at the default invocation all four
/// agree, and this difference is invisible.
#[test]
fn only_the_crd_base_follows_the_period_flag_because_it_has_no_period_field() {
    for format in FORMATS {
        for (label, opts) in PERIOD_PINNED_OPTIONS {
            let pinned: Vec<(Dialect, Vec<u8>)> = [Slokit, OpensloV1, OpensloV1alpha]
                .iter()
                .map(|d| (*d, generated_rules(d.base_fixture(), format, opts, label)))
                .collect();
            let default = generated_rules(Slokit.base_fixture(), format, &[], "generate");
            for (dialect, rules) in &pinned {
                assert!(
                    *rules == default,
                    "`{label} --format {format}`: {} carries `period: 30d` in its document, so \
                     the flag must not change its rules",
                    dialect.label()
                );
            }

            let crd = generated_rules(SlothCrd.base_fixture(), format, opts, label);
            assert!(
                crd != default,
                "`{label} --format {format}`: {} has no period field, so the flag is the only \
                 thing setting its window and its rules must differ from the 30d default \
                 ({} bytes either way)",
                SlothCrd.label(),
                crd.len()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The contract proper
// ---------------------------------------------------------------------------

/// The committed tree and [`CONSTRUCTS`] must describe the same set of files,
/// in both directions. Adding a fixture without a row, or dropping a fixture a
/// row still claims, fails here by name.
#[test]
fn the_contract_covers_exactly_the_committed_fixtures() {
    let found = discovered_fixtures();
    let listed = listed_fixtures();

    let unlisted: Vec<&String> = found.iter().filter(|p| !listed.contains(p)).collect();
    assert!(
        unlisted.is_empty(),
        "committed under {FIXTURE_DIR} but claimed by no base or row: {unlisted:?}"
    );
    let missing: Vec<&String> = listed.iter().filter(|p| !found.contains(p)).collect();
    assert!(
        missing.is_empty(),
        "claimed by the contract but not committed under {FIXTURE_DIR}: {missing:?}"
    );
}

/// A construct with one row is not a parity statement, and a `note_key` that
/// the recorded note does not contain would make the silence assertions in
/// [`every_row_has_its_recorded_disposition`] test the wrong string.
#[test]
fn every_construct_is_a_statement_about_at_least_two_dialects() {
    for construct in CONSTRUCTS {
        assert!(
            construct.rows.len() >= 2,
            "{}: a construct only one dialect can receive says nothing about parity",
            construct.id
        );
        let mut seen: Vec<Dialect> = Vec::new();
        for row in construct.rows {
            assert!(
                !seen.contains(&row.dialect),
                "{}: {} has two rows",
                construct.id,
                row.dialect.label()
            );
            seen.push(row.dialect);
            if let NotedAndIgnored(note) = row.disposition {
                assert!(
                    note.contains(construct.note_key),
                    "{}: the note recorded for {} does not contain the construct's note_key \
                     {:?}, so the silence assertions on the other rows would be checking an \
                     unrelated string",
                    construct.id,
                    row.dialect.label(),
                    construct.note_key
                );
            }
        }
    }
}

/// The contract proper: what each dialect actually does with each construct,
/// run through the shipped binary.
#[test]
fn every_row_has_its_recorded_disposition() {
    let mut wrong = Vec::new();
    for construct in CONSTRUCTS {
        for row in construct.rows {
            let path = fixture_arg(row.fixture);
            let out = slokit(&["validate", "-i", &path]);
            let text = combined(&out);
            let notes = import_notes(&out, row.fixture);
            let mentions_construct = notes.iter().any(|n| n.contains(construct.note_key));
            let at = format!("{} / {}", construct.id, row.dialect.label());

            match row.disposition {
                Refused(reason) => {
                    if out.status.success() {
                        wrong.push(format!("{at}: recorded Refused, but validate exited 0"));
                    } else if !text.contains(reason) {
                        wrong.push(format!(
                            "{at}: refused for a different reason than {reason:?}:\n      {}",
                            text.trim().replace('\n', "\n      ")
                        ));
                    }
                    continue;
                }
                _ if !out.status.success() => {
                    wrong.push(format!(
                        "{at}: recorded {:?}, but validate exited {:?}:\n      {}",
                        row.disposition,
                        out.status.code(),
                        text.trim().replace('\n', "\n      ")
                    ));
                    continue;
                }
                NotedAndIgnored(note) => {
                    if !notes.iter().any(|n| n.contains(note)) {
                        wrong.push(format!(
                            "{at}: expected an import note containing {note:?}, got {notes:?}"
                        ));
                    }
                }
                SilentlyIgnored => {
                    if mentions_construct {
                        wrong.push(format!(
                            "{at}: recorded SilentlyIgnored, but a note now mentions {:?}: \
                             {notes:?}. If the drop is now reported, this row becomes \
                             NotedAndIgnored and the construct's verdict is revisited.",
                            construct.note_key
                        ));
                    }
                }
                Imported | Linted(_) => {
                    if mentions_construct {
                        wrong.push(format!(
                            "{at}: recorded {:?}, but an import note mentions {:?}: {notes:?}",
                            row.disposition, construct.note_key
                        ));
                    }
                }
            }

            // What reaches the rules is the half a note cannot fake.
            let rules = String::from_utf8_lossy(&generated_rules(
                row.fixture,
                "prometheus",
                &[],
                "generate",
            ))
            .into_owned();
            let honored = rules.contains(construct.rule_marker);
            match row.disposition {
                Imported if !honored => wrong.push(format!(
                    "{at}: recorded Imported, but {:?} never reaches the generated rules",
                    construct.rule_marker
                )),
                Imported => {}
                _ if honored => wrong.push(format!(
                    "{at}: recorded {:?}, but {:?} reached the generated rules anyway",
                    row.disposition, construct.rule_marker
                )),
                _ => {}
            }

            if let Linted(code) = row.disposition {
                let lint = combined(&slokit(&["lint", "-i", &path]));
                if !lint.contains(code) {
                    wrong.push(format!(
                        "{at}: expected `slokit lint` to report {code}, got:\n      {}",
                        lint.trim().replace('\n', "\n      ")
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "dialect dispositions drifted:\n  {}",
        wrong.join("\n  ")
    );
}

/// The recorded verdict must match the rows it summarises.
///
/// This is the clause that makes a one-sided change loud. Editing one
/// dialect's behaviour reddens [`every_row_has_its_recorded_disposition`];
/// updating that row to match and walking away reddens this one, because a
/// construct recorded `Uniform` no longer has identical rows.
#[test]
fn the_recorded_verdicts_match_the_rows() {
    for construct in CONSTRUCTS {
        let first = construct.rows[0].disposition;
        let uniform = construct.rows.iter().all(|r| r.disposition == first);
        match construct.verdict {
            Uniform => assert!(
                uniform,
                "{} ({}) is recorded Uniform, but its dialects answer differently: {:?}",
                construct.id,
                construct.what,
                construct
                    .rows
                    .iter()
                    .map(|r| (r.dialect.label(), r.disposition))
                    .collect::<Vec<_>>()
            ),
            Divergent(reason) => assert!(
                !uniform,
                "{} ({}) is recorded Divergent ({reason}), but every dialect now answers {:?} — \
                 the divergence is gone and the record should say so",
                construct.id, construct.what, first
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// The OpenSLO v1 display-name note (v1.8.0 done-when clause 4)
// ---------------------------------------------------------------------------

/// The behaviour this PR adds, asserted on the document that had none.
#[test]
fn the_openslo_v1_route_emits_the_display_name_note() {
    let fixture = "display-name.openslo-v1.yaml";
    let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
    assert!(
        out.status.success(),
        "a dropped display name is advisory, not fatal: {}",
        combined(&out).trim()
    );
    let notes = import_notes(&out, fixture);
    assert!(
        notes.iter().any(|n| n.contains(DISPLAY_NAME_NOTE)),
        "expected {DISPLAY_NAME_NOTE:?} among {notes:?}"
    );
}

/// The other half of the difference: a v1 document with no display name must
/// not gain a note about one. Without this, a note emitted unconditionally
/// would satisfy the test above.
#[test]
fn a_v1_document_without_a_display_name_gets_no_such_note() {
    let fixture = OpensloV1.base_fixture();
    let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
    assert!(out.status.success(), "{}", combined(&out).trim());
    let notes = import_notes(&out, fixture);
    assert!(
        !notes.iter().any(|n| n.contains("metadata.displayName")),
        "the base document sets no displayName, so nothing should mention one: {notes:?}"
    );
}

/// The parity claim itself, and the reason [`DISPLAY_NAME_NOTE`] is one
/// constant: two routes that both "note the display name" in different words
/// still send a reader to two different answers.
#[test]
fn both_openslo_dialects_word_the_display_name_note_identically() {
    let mut seen = Vec::new();
    for fixture in [
        "display-name.openslo-v1.yaml",
        "display-name.openslo-v1alpha.yaml",
    ] {
        let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
        let notes = import_notes(&out, fixture);
        let note = notes
            .iter()
            .find(|n| n.contains("metadata.displayName"))
            .unwrap_or_else(|| panic!("{fixture} printed no displayName note: {notes:?}"))
            .clone();
        seen.push((fixture, note));
    }
    assert_eq!(
        seen[0].1, seen[1].1,
        "the two OpenSLO routes word the same drop differently: {} says {:?}, {} says {:?}",
        seen[0].0, seen[0].1, seen[1].0, seen[1].1
    );
}

// ---------------------------------------------------------------------------
// The object-annotations note (v1.10.0 done-when clause 4)
// ---------------------------------------------------------------------------
//
// This section replaces `known_gap_object_annotations_are_noted_on_v1_only`,
// deleted in the commit that closed the gap as that test's own failure message
// instructed. It pinned the WRONG behaviour: annotations reported on
// `openslo/v1` and dropped in silence on the other two envelope dialects.
//
// One test per route rather than one loop over both, because a runner aborts a
// test at its first failed assertion: a shared loop would let the v1alpha red
// stand in for the CRD's and report nothing at all about it, and the CRD is
// the route whose fix is the larger one (it had no field for the key).

/// The behaviour this PR adds to `openslo/v1alpha`, asserted on the document
/// that got nothing.
#[test]
fn the_openslo_v1alpha_route_emits_the_object_annotations_note() {
    let fixture = "object-annotations.openslo-v1alpha.yaml";
    let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
    assert!(
        out.status.success(),
        "dropped annotations are advisory, not fatal: {}",
        combined(&out).trim()
    );
    let notes = import_notes(&out, fixture);
    assert!(
        notes.iter().any(|n| n.contains(OBJECT_ANNOTATIONS_NOTE)),
        "expected {OBJECT_ANNOTATIONS_NOTE:?} among {notes:?}"
    );
}

/// The same for the sloth CRD route, where the fix was larger: `ObjectMeta`
/// had no `annotations` field at all, so serde discarded the key before the
/// importer could have said anything about it.
#[test]
fn the_sloth_crd_route_emits_the_object_annotations_note() {
    let fixture = "object-annotations.sloth-crd.yaml";
    let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
    assert!(
        out.status.success(),
        "dropped annotations are advisory, not fatal: {}",
        combined(&out).trim()
    );
    let notes = import_notes(&out, fixture);
    assert!(
        notes.iter().any(|n| n.contains(OBJECT_ANNOTATIONS_NOTE)),
        "expected {OBJECT_ANNOTATIONS_NOTE:?} among {notes:?}"
    );
}

/// The other half of the difference: a document with no annotations must not
/// gain a note about them. Without this, three unconditional notes would
/// satisfy both tests above and the two rows they back.
///
/// The `base.*` documents are the right inputs precisely because the identity
/// assertion at the top of this file already proves they describe the same
/// SLO, so "no annotations" is the only thing they have in common here.
/// Failures are collected rather than asserted in the loop: with four inputs,
/// aborting on the first would hide the other three.
#[test]
fn no_dialect_mentions_annotations_on_a_document_that_has_none() {
    let mut wrong = Vec::new();
    for dialect in DIALECTS {
        let fixture = dialect.base_fixture();
        let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
        if !out.status.success() {
            wrong.push(format!(
                "{}: base document did not validate: {}",
                dialect.label(),
                combined(&out).trim()
            ));
            continue;
        }
        let notes = import_notes(&out, fixture);
        if notes.iter().any(|n| n.contains("metadata.annotations")) {
            wrong.push(format!(
                "{}: {fixture} sets no annotations, so nothing should mention any: {notes:?}",
                dialect.label()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a note about dropped annotations fired without any:\n  {}",
        wrong.join("\n  ")
    );
}

/// The parity claim itself, and the reason the emission sites read one
/// `OBJECT_ANNOTATIONS_NOTE` constant (`src/spec/import.rs`) instead of three
/// literals: three routes that all "note the annotations" in different words
/// still send a reader to three different answers.
///
/// The expectation this compares against is the constant at the top of THIS
/// file, which no production code reads, so the check stays falsifiable — a
/// guard deriving both sides from `src/spec/import.rs` would move both at once
/// and pass on any reword.
#[test]
fn all_three_envelope_dialects_word_the_object_annotations_note_identically() {
    let mut seen = Vec::new();
    for fixture in [
        "object-annotations.openslo-v1.yaml",
        "object-annotations.openslo-v1alpha.yaml",
        "object-annotations.sloth-crd.yaml",
    ] {
        let out = slokit(&["validate", "-i", &fixture_arg(fixture)]);
        let notes = import_notes(&out, fixture);
        let note = notes
            .iter()
            .find(|n| n.contains("metadata.annotations"))
            .unwrap_or_else(|| panic!("{fixture} printed no annotations note: {notes:?}"))
            .clone();
        seen.push((fixture, note));
    }
    for (fixture, note) in &seen {
        // `ImportNote` renders as `<location>: <message>`, and the LOCATION
        // legitimately differs here — the CRD reports against
        // `PrometheusServiceLevel 'parity'` and the OpenSLO routes against
        // `slo 'requests-availability'` — so the equality is over the message
        // alone. (Splitting at the first `": "` is exact for these three
        // locations, each of which is a bare word plus a quoted name.)
        let message = note
            .split_once(": ")
            .map_or(note.as_str(), |(_location, message)| message);
        assert_eq!(
            message, OBJECT_ANNOTATIONS_NOTE,
            "{fixture} words the dropped annotations as {message:?}, not as the recorded \
             {OBJECT_ANNOTATIONS_NOTE:?} — three routes explaining one drop in three ways is \
             the divergence this constant exists to prevent"
        );
    }
}
