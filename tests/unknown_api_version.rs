//! The dialect the dispatch could not name (v1.10.0 PR 3).
//!
//! slokit reads four input dialects, three of which announce themselves with a
//! top-level `apiVersion`. The native format does not: it spells its own
//! format version `version:`, and its parser does not deny unknown fields. So
//! a document carrying an `apiVersion` slokit has never heard of — a
//! Kubernetes manifest, an SLO document written for some other tool, an
//! OpenSLO successor under a new group — fell through to the native parser and
//! was judged entirely in native vocabulary:
//!
//! ```text
//! spec error: document 1: missing field `service`
//! ```
//!
//! A required field reported absent from a document that is in fact
//! well-formed in a format nobody asked slokit to read. Worse when the native
//! parse SUCCEEDS, which it does whenever the document happens to satisfy the
//! native required fields: then there was no message at all, and a spec
//! written for something else generated rules in silence.
//!
//! This milestone REPORTS that, and deliberately does not refuse it
//! (`ROADMAP.md` D1.10-3). Refusal is not available under `docs/SEMVER.md`'s
//! 1.x clause that a document validating under 1.a validates under 1.b:
//! measured at the binary before a line of this slice was written, the
//! committed example `examples/infraportal/slos/accounts-service.yaml` with
//! `apiVersion: apps/v1` prepended already validated and already generated
//! byte-identical rules (18126 bytes, sha256
//! `6005cb83bc02d93bdd5409c7d75c70cba1e9bfd1c87bc57360d5999b6b4776fd`). That
//! acceptance is a live promise, so this file pins it beside the new report
//! rather than trusting the roadmap paragraph that describes it.
//!
//! Two reporting surfaces, one accept-set:
//!
//! - `UNKNOWN_API_VERSION`, a new lint code, for the document that parses;
//! - a dialect-naming parse error for the document that does not.
//!
//! Both print the accepted set beside the value they rejected, and
//! [`every_group_the_message_calls_accepted_is_one_the_detector_routes_away_from`]
//! is why that matters: the set is composed in `src/spec/import.rs` from the
//! dialect modules' own group constants, and a mis-composed set is invisible
//! in BOTH directions on a tree where nothing is wrong. Too broad and the lint
//! silently stops firing for a dialect nothing can read; too narrow and it
//! fires on documents slokit imports perfectly well. The message is the only
//! place that set is ever observed, so the guard reads it back out of the
//! message and holds it against the auto-detector's own public predicates.

#![cfg(feature = "spec")]

use slokit::spec::{lint, openslo, sloth_crd, Spec};

/// The code this slice adds.
const CODE: &str = "UNKNOWN_API_VERSION";

/// An `apiVersion` group slokit does not read, and the one the roadmap names:
/// a reader who points slokit at a Kubernetes workload manifest by mistake.
const FOREIGN_GROUP: &str = "apps/";

/// A minimal valid native spec. Deliberately the same shape
/// `tests/source_dialect.rs` uses, so the two files disagree loudly rather
/// than quietly if the native required set changes.
const NATIVE_MINIMAL: &str = "\
service: api
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(errors[{{.window}}]))
        total_query: sum(rate(total[{{.window}}]))
";

/// A document that is NOT a native spec whatever else it is: it satisfies no
/// native required field. Used wherever the point is the parse FAILURE path.
const NOT_A_NATIVE_SPEC: &str = "\
kind: Deployment
metadata:
  name: accounts-service
";

/// `NATIVE_MINIMAL` with a top-level `apiVersion` prepended.
fn native_declaring(api_version: &str) -> String {
    format!("apiVersion: {api_version}\n{NATIVE_MINIMAL}")
}

/// A non-native document declaring `api_version`.
fn foreign_declaring(api_version: &str) -> String {
    format!("apiVersion: {api_version}\n{NOT_A_NATIVE_SPEC}")
}

/// Every lint code a spec produces, in order.
fn codes(spec: &Spec) -> Vec<&'static str> {
    lint(spec).into_iter().map(|l| l.code).collect()
}

/// The one `UNKNOWN_API_VERSION` message a spec produces, or `None`.
fn unknown_api_version_message(spec: &Spec) -> Option<String> {
    lint(spec)
        .into_iter()
        .find(|l| l.code == CODE)
        .map(|l| l.message)
}

/// The parse error text for a document, or a panic if it unexpectedly parsed.
fn parse_error(yaml: &str) -> String {
    match Spec::from_yaml_stream(yaml) {
        Ok(specs) => panic!("expected a parse failure, got {} spec(s)", specs.len()),
        Err(e) => e.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The lint: the document that parses
// ---------------------------------------------------------------------------

/// **Done-when clause 5, the report half, at the library.**
///
/// The finding must quote the author's own string. A code with a generic
/// message ("this document declares an unknown apiVersion") would pass a
/// contains-the-code assertion while telling the reader nothing they could act
/// on, and the value is the only part of this finding they can search for.
#[test]
fn an_unrecognised_api_version_is_reported_and_quotes_the_value() {
    let spec = Spec::from_yaml(&native_declaring("apps/v1")).expect("still parses");
    assert_eq!(
        codes(&spec).iter().filter(|c| **c == CODE).count(),
        1,
        "expected exactly one {CODE} finding, got {:?}",
        codes(&spec)
    );
    let message = unknown_api_version_message(&spec).expect("the finding exists");
    assert!(
        message.contains("apps/v1"),
        "the finding does not quote the value it is about: {message}"
    );
}

/// The OFF state of the same surface, which is the half a "the code fires"
/// assertion cannot prove.
///
/// Stated as a DIFFERENCE rather than as two absolute lists, because the
/// absolute list is not the claim: `NATIVE_MINIMAL` carries unrelated findings
/// of its own (`NO_ALERT_LABELS`, `NO_DESCRIPTION`) and pinning them here would
/// make this test fail every time an unrelated lint is added. What must hold is
/// that the one prepended line adds exactly the one finding and disturbs
/// nothing else — a lint that fired on every native spec, or one that
/// suppressed a sibling finding on its way past, both satisfy "the code appears
/// when the key is there" and are caught here.
#[test]
fn the_key_adds_exactly_one_finding_and_changes_no_other() {
    let with = codes(&Spec::from_yaml(&native_declaring("apps/v1")).expect("parses"));
    let without = codes(&Spec::from_yaml(NATIVE_MINIMAL).expect("parses"));

    assert!(
        !without.contains(&CODE),
        "the unmodified native document must not carry {CODE}: {without:?}"
    );
    let remainder: Vec<&str> = with.iter().copied().filter(|c| *c != CODE).collect();
    assert_eq!(
        remainder, without,
        "prepending the key changed findings other than {CODE}"
    );
    assert_eq!(
        with.len(),
        without.len() + 1,
        "prepending the key must add exactly one finding: {with:?} vs {without:?}"
    );
}

/// **The accept-set audit (L-093), and the drift guard between the reporter
/// and the dispatcher (L-003).**
///
/// The message names the groups slokit accepts. That list is DERIVED — from
/// `openslo::API_GROUP` and `sloth_crd::API_GROUP` via
/// `import::KNOWN_API_GROUPS` — and a derivation that read the wrong thing
/// produces a message that looks completely normal. So this test does not
/// hand-copy the list; it reads it back out of the finding and holds each
/// entry against the auto-detector's own public predicates, which are what
/// `src/bin/slokit.rs` actually routes on.
///
/// Both directions, because each is blind on its own:
///
/// - every group the message calls accepted must be one the detector really
///   claims (otherwise the lint is silently excusing a dialect nothing reads);
/// - a group the message does NOT list must be claimed by no detector and must
///   produce the finding (otherwise the list is over-broad and the lint has
///   stopped covering the class it exists for).
///
/// Plus a floor, because an empty list passes the first direction vacuously
/// and would read as a clean run.
#[test]
fn every_group_the_message_calls_accepted_is_one_the_detector_routes_away_from() {
    let spec = Spec::from_yaml(&native_declaring("apps/v1")).expect("parses");
    let message = unknown_api_version_message(&spec).expect("the finding exists");

    let listed = accepted_groups(&message);
    assert!(
        listed.len() >= 2,
        "the message must print the accepted set, not a placeholder; parsed {listed:?} \
         out of: {message}"
    );
    assert!(
        listed.iter().all(|g| !g.is_empty() && g.ends_with('/')),
        "every accepted entry must be a real apiVersion group prefix, got {listed:?}"
    );

    for group in &listed {
        let document = foreign_declaring(&format!("{group}v1"));
        assert!(
            openslo::is_openslo(&document) || sloth_crd::is_sloth_crd(&document),
            "the message calls `{group}*` accepted, but no importer's detector claims \
             a document declaring `{group}v1` — the printed set and the dispatch disagree"
        );
        let parsed = Spec::from_yaml(&native_declaring(&format!("{group}v1")))
            .expect("a native document parses whatever apiVersion it carries");
        assert_eq!(
            unknown_api_version_message(&parsed),
            None,
            "`{group}v1` is in the accepted set the message prints, so it must not be reported"
        );
    }

    // The converse. `apps/` is deliberately absent from the list above.
    assert!(
        !listed.iter().any(|g| g == FOREIGN_GROUP),
        "this test's own negative case leaked into the accepted set: {listed:?}"
    );
    let document = foreign_declaring("apps/v1");
    assert!(
        !openslo::is_openslo(&document) && !sloth_crd::is_sloth_crd(&document),
        "no detector may claim an unrecognised group"
    );
}

/// Read the accepted groups back out of a message, as `["openslo/", ...]`.
///
/// Deliberately a parser over the shipped string rather than a second copy of
/// the constant: the point of the test above is that the STRING a reader sees
/// is right, and a helper importing the same constant the code under test uses
/// would agree with it by construction and prove nothing.
fn accepted_groups(message: &str) -> Vec<String> {
    let start = message
        .find("(accepted: ")
        .map(|i| i + "(accepted: ".len())
        .unwrap_or_else(|| panic!("no accepted-set clause in: {message}"));
    let rest = &message[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("unterminated accepted-set clause in: {message}"));
    rest[..end]
        .split(", ")
        .map(|entry| entry.trim_end_matches('*').to_string())
        .collect()
}

/// A recognised GROUP with a version no importer accepts is not this finding's
/// business, and saying so is a decision rather than an oversight.
///
/// `openslo/v2` reaches the OpenSLO importer — the detector routes on the
/// group — and that importer already refuses it by name. Reporting it here too
/// would put two different messages on one document depending on which command
/// the reader ran.
#[test]
fn a_recognised_group_with_an_unreadable_version_is_left_to_its_importer() {
    let spec = Spec::from_yaml(&native_declaring("openslo/v2")).expect("parses");
    assert_eq!(
        unknown_api_version_message(&spec),
        None,
        "`openslo/v2` is inside a group slokit reads; the importer owns that refusal"
    );

    let refusal = openslo::from_yaml(&foreign_declaring("openslo/v2"))
        .expect_err("the OpenSLO importer refuses an unreadable version")
        .to_string();
    assert!(
        refusal.contains("openslo/v2"),
        "the importer's refusal must name the version it refused: {refusal}"
    );
}

// ---------------------------------------------------------------------------
// The parse error: the document that does not parse
// ---------------------------------------------------------------------------

/// **Done-when clause 5, the error half.**
///
/// The failure the bug was filed about. All three parts must be present: the
/// value, the accepted set, and the native error — dropping the last would
/// trade one blind message for another, because a reader who really did mean
/// to write a native spec and prepended a stray key still needs it.
#[test]
fn an_unrecognised_api_version_that_also_fails_natively_names_the_accepted_formats() {
    let err = parse_error(&foreign_declaring("apps/v1"));
    assert!(
        err.contains("apps/v1"),
        "the error must name the apiVersion it could not place: {err}"
    );
    assert!(
        !accepted_groups(&err).is_empty(),
        "the error must name the formats slokit does read: {err}"
    );
    assert!(
        err.contains("missing field `service`"),
        "the native parser's own answer must survive: {err}"
    );
}

/// A dialect slokit DOES read, parsed natively, gets a different answer.
///
/// This is the bug entry's own repro (`--input-format slokit` pinned onto a
/// sloth `PrometheusServiceLevel`, or an embedder calling `Spec::from_yaml` on
/// one). Naming a dialect that exists is a different fact from naming none, so
/// it must not be answered with the unrecognised-group message; the reader's
/// problem here is the route, not the document.
#[test]
fn a_recognised_dialect_read_natively_is_told_which_it_is() {
    let err = parse_error(&foreign_declaring("sloth.slok.dev/v1"));
    assert!(
        err.contains("sloth.slok.dev/v1"),
        "the error must name the dialect the document declared: {err}"
    );
    assert!(
        err.contains("names a dialect slokit imports"),
        "a readable dialect must not be reported as unreadable: {err}"
    );
    assert!(
        err.contains("missing field `service`"),
        "the native parser's own answer must survive: {err}"
    );
}

/// The OFF state of the reword: it must not reach the ordinary native mistake.
///
/// A document with no `apiVersion` at all is an ordinary native document with
/// an ordinary native typo, and its message is unchanged. Without this, a
/// reword that fired unconditionally would pass both tests above.
#[test]
fn a_native_document_with_no_api_version_keeps_its_plain_error() {
    let err = parse_error("service: api\n");
    assert!(
        err.contains("missing field `slos`"),
        "expected the plain native error: {err}"
    );
    assert!(
        !err.contains("apiVersion"),
        "the dialect reword must not reach a document that declared none: {err}"
    );
}

// ---------------------------------------------------------------------------
// Done-when clause 5 at the binary, over the committed example
// ---------------------------------------------------------------------------

#[cfg(feature = "cli")]
mod cli {
    use std::fs;
    use std::path::PathBuf;
    use std::process::{Command, Output};

    /// The committed example the roadmap names, measured through the real
    /// command rather than through `generate_rules_with`.
    const EXAMPLE: &str = "examples/infraportal/slos/accounts-service.yaml";

    /// The line this file's derivation prepends.
    const PREPENDED: &str = "apiVersion: apps/v1";

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
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("slokit-unknown-api-version-{tag}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The committed example with `PREPENDED` as its first line, written into
    /// a temp dir.
    ///
    /// It is DERIVED at test time rather than committed as a second example
    /// because `examples/` is a glob-discovered corpus that four other suites
    /// walk (`tests/examples_infraportal.rs`, `tests/check_generate_agreement.rs`,
    /// `tests/dashboard_drift.rs`, `tests/schema.rs`); a committed twin would
    /// silently join all four.
    fn derived(tag: &str) -> (PathBuf, String, String) {
        let source = fs::read_to_string(EXAMPLE).expect("the committed example is readable");
        let derived = format!("{PREPENDED}\n{source}");
        let path = temp_dir(tag).join("accounts-service.yaml");
        fs::write(&path, &derived).expect("write derived example");
        (path, source, derived)
    }

    /// **The derivation's own control (L-077).**
    ///
    /// Every comparison below is "this document versus the same document with
    /// one line added". An INERT transformation makes those the same document,
    /// and then the byte-identity assertion passes for exactly the reason it
    /// was written to exclude — silently, because a prepend that prepended
    /// nothing looks identical to a prepend whose addition changed no output.
    /// So: the derived text differs, the construct is really in it, and the
    /// derivation did not eat the document.
    #[test]
    fn the_derivation_is_not_inert() {
        let (_, source, derived) = derived("control");
        assert_ne!(derived, source, "the prepend produced the same document");
        assert!(
            derived.starts_with(PREPENDED),
            "the prepended key is not at the top of the derived document"
        );
        assert!(
            !source.contains("apiVersion"),
            "the committed example already declares an apiVersion; this whole \
             derivation is then measuring nothing"
        );
        assert_eq!(
            derived.lines().count(),
            source.lines().count() + 1,
            "the prepend added something other than exactly one line"
        );
        // The document downstream still needs: not eaten.
        for required in ["service: accounts-service", "slos:", "objective:"] {
            assert!(
                derived.contains(required),
                "the derivation lost `{required}` from the example"
            );
        }
    }

    /// **Done-when clause 5, the acceptance half.** Reporting, never refusing.
    #[test]
    fn the_example_with_a_foreign_api_version_still_validates() {
        let (path, _, _) = derived("validate");
        let out = slokit(&["validate", "-i", &path.display().to_string()]);
        assert!(
            out.status.success(),
            "a foreign apiVersion must not change what validates; stderr: {}",
            stderr(&out)
        );
        assert!(
            stdout(&out).contains("is valid"),
            "expected the ok line, got: {}",
            stdout(&out)
        );
    }

    /// **Done-when clause 5, the byte-identity half.**
    ///
    /// The generated rules are the artifact 1.x actually promises. Its own
    /// clause, separate from the exit code above, because one perturbation
    /// (making the lint refuse instead of report) would redden the validate
    /// assertion first and this one would never be reached.
    #[test]
    fn the_example_generates_byte_identical_rules_with_and_without_it() {
        let (path, _, _) = derived("generate");
        let with = slokit(&["generate", "-i", &path.display().to_string()]);
        let without = slokit(&["generate", "-i", EXAMPLE]);
        assert!(with.status.success() && without.status.success());
        assert_eq!(
            with.stdout, without.stdout,
            "a captured, never-applied apiVersion changed the generated rules"
        );
        assert!(
            !with.stdout.is_empty(),
            "both sides generated nothing, which is byte-identical and worthless"
        );
    }

    /// **Done-when clause 5, the report half, at the binary.**
    #[test]
    fn lint_reports_the_unknown_api_version_on_that_document() {
        let (path, _, _) = derived("lint");
        let out = slokit(&["lint", "-i", &path.display().to_string()]);
        assert!(out.status.success(), "plain lint exits 0 on a warning");
        let table = stdout(&out);
        assert!(
            table.contains(super::CODE) && table.contains("apps/v1"),
            "expected the finding with its value, got: {table}"
        );

        // And the same command on the committed example says nothing, so the
        // finding is about the prepended key and not about the example.
        let clean = slokit(&["lint", "-i", EXAMPLE]);
        assert!(
            !stdout(&clean).contains(super::CODE),
            "the committed example must stay lint-clean: {}",
            stdout(&clean)
        );
    }

    /// The `lint --strict` consequence D1.10-4 states rather than hides: a
    /// document that was warning-free now exits non-zero under `--strict`.
    /// Its own test, because it is the part of this milestone a user can feel.
    #[test]
    fn lint_strict_now_fails_a_document_that_used_to_pass() {
        let (path, _, _) = derived("strict");
        let strict = slokit(&["lint", "-i", &path.display().to_string(), "--strict"]);
        assert!(
            !strict.status.success(),
            "--strict must fail on the new finding"
        );
        let clean = slokit(&["lint", "-i", EXAMPLE, "--strict"]);
        assert!(
            clean.status.success(),
            "--strict must still pass the committed example: {}",
            stderr(&clean)
        );
    }
}
