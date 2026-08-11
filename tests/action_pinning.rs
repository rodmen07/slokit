//! Drift guards for GitHub Actions supply-chain pinning.
//!
//! Every third-party action a workflow runs is code this repo executes with
//! the repository's `GITHUB_TOKEN` in scope, and in `publish.yml` alongside
//! `CRATES_IO_TOKEN` and an irreversible crates.io push. A tag ref like
//! `@v7` is MUTABLE: the upstream account can repoint it at any commit at
//! any time, so a tag pin is a standing grant to run whatever that account
//! publishes next. A full-length commit SHA is the only ref GitHub will not
//! move underneath us.
//!
//! The workflow list is discovered from `.github/workflows`, never
//! hand-enumerated, and an empty discovery is a hard failure, so a new
//! workflow file enters this contract the moment it is committed. Each file
//! is both parsed as YAML (the authoritative `uses:` inventory) and scanned
//! as raw text (the only place a `#` comment survives), and guard 2 asserts
//! the two scans see the same set, so the comment guard cannot go blind on a
//! step written in a shape the raw scan does not read.
//!
//! ONE family is exempt, named in `REF_NAME_IS_AN_ARGUMENT` below with its
//! reason: `dtolnay/rust-toolchain` reads its own `github.action_ref` and
//! installs the toolchain the ref NAMES, so `@stable` and `@1.82` are
//! arguments, not versions, and a SHA there would ask for a toolchain called
//! `3d3c42e...`. `.github/dependabot.yml` ignores the same action for the
//! same reason. `taiki-e/install-action` was the same shape via its per-tool
//! tags (`@cargo-audit`) and is NOT exempt: it is pinned by moving the tool
//! name into `with: tool:`, the form its `action.yml` documents.
//!
//! What a test here can NOT prove is that a pinned SHA is the commit its
//! trailing comment names, or that it is reachable from that tag at all --
//! both need the network and GitHub's view of the upstream repo. That was
//! checked once, at pinning time, with `gh api repos/<owner>/<repo>/commits/
//! <tag> --jq .sha` against both the major tag and the exact release tag
//! (see the shipping PR's body for the four resolutions). What a test CAN
//! hold durably is that no ref has drifted back to a mutable one, that every
//! pin still says which release it is, and that no exemption outlives the
//! action it was written for.
//!
//! If a workflow ever legitimately needs another exempt action, add it to
//! `REF_NAME_IS_AN_ARGUMENT` with its reason in the same commit that
//! introduces it, so the exception is reviewed and named rather than
//! inherited -- the same convention `tests/workflow_permissions.rs` uses.

use serde_norway::Value;
use std::fs;
use std::path::PathBuf;

/// Actions whose ref is an ARGUMENT rather than a version, and therefore
/// cannot be SHA-pinned. Each entry carries the reason it is exempt; an
/// entry that no workflow still uses is a hole in guard 1 and fails guard 4.
const REF_NAME_IS_AN_ARGUMENT: &[(&str, &str)] = &[(
    "dtolnay/rust-toolchain",
    "the action reads its own github.action_ref and installs the toolchain it \
     names (@stable, @1.82 = the MSRV gate), so a commit SHA would request a \
     toolchain named by a hex string; .github/dependabot.yml ignores it for \
     the same reason",
)];

/// A `uses:` reference found in a workflow, with where it was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct UseRef {
    workflow: String,
    reference: String,
}

/// Discover every workflow file as (name, raw text, parsed YAML). Empty
/// discovery is a hard failure: a guard that silently scans nothing proves
/// nothing.
fn workflow_sources() -> Vec<(String, String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read workflow dir {}: {e}", dir.display()))
    {
        let path = entry.expect("readable dir entry").path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yml" || ext == "yaml");
        if !is_yaml {
            continue;
        }
        let name = path
            .file_name()
            .expect("workflow file name")
            .to_string_lossy()
            .into_owned();
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let value: Value = serde_norway::from_str(&text)
            .unwrap_or_else(|e| panic!("{name} is not parseable YAML: {e}"));
        found.push((name, text, value));
    }
    assert!(
        !found.is_empty(),
        "no workflow files discovered under {} - the guard is scanning nothing",
        dir.display()
    );
    found
}

/// Every `uses:` in every workflow, read from the PARSED document: step-level
/// `jobs.*.steps[].uses` and job-level `jobs.*.uses` (reusable workflows).
fn uses_from_yaml() -> Vec<UseRef> {
    let mut refs = Vec::new();
    for (name, _text, value) in workflow_sources() {
        let Some(jobs) = value.get("jobs").and_then(Value::as_mapping) else {
            continue;
        };
        for (_job_id, job) in jobs {
            if let Some(reference) = job.get("uses").and_then(Value::as_str) {
                refs.push(UseRef {
                    workflow: name.clone(),
                    reference: reference.to_owned(),
                });
            }
            let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
                continue;
            };
            for step in steps {
                if let Some(reference) = step.get("uses").and_then(Value::as_str) {
                    refs.push(UseRef {
                        workflow: name.clone(),
                        reference: reference.to_owned(),
                    });
                }
            }
        }
    }
    refs.sort();
    refs
}

/// Every `uses:` line in every workflow, read from the RAW text as
/// (workflow, line number, reference, trailing `#` comment). Comment lines
/// are skipped, so prose mentioning `uses:` never enters the inventory.
fn uses_from_raw_text() -> Vec<(String, usize, String, Option<String>)> {
    let mut found = Vec::new();
    for (name, text, _value) in workflow_sources() {
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            let after_dash = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
            let Some(rest) = after_dash.strip_prefix("uses:") else {
                continue;
            };
            let rest = rest.trim();
            let (reference, comment) = match rest.split_once('#') {
                Some((reference, comment)) => {
                    (reference.trim().to_owned(), Some(comment.trim().to_owned()))
                }
                None => (rest.to_owned(), None),
            };
            found.push((name.clone(), index + 1, reference, comment));
        }
    }
    found
}

/// True when `reference` is `owner/repo@<40 lowercase hex>`.
fn is_sha_pinned(reference: &str) -> bool {
    let Some((_action, git_ref)) = reference.rsplit_once('@') else {
        return false;
    };
    git_ref.len() == 40
        && git_ref
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

/// The `owner/repo` half of a `uses:` reference, without its ref.
fn action_name(reference: &str) -> &str {
    reference
        .rsplit_once('@')
        .map_or(reference, |(name, _)| name)
}

fn is_exempt(reference: &str) -> bool {
    REF_NAME_IS_AN_ARGUMENT
        .iter()
        .any(|(action, _reason)| action_name(reference) == *action)
}

/// Guard 1: every third-party action is pinned to a full-length commit SHA,
/// unless it is one of the named `REF_NAME_IS_AN_ARGUMENT` exemptions. A
/// local (`./`) action is repo code, already covered by review; any other
/// shape fails closed rather than being waved through unrecognised.
#[test]
fn every_third_party_action_is_pinned_to_a_commit_sha() {
    let mut unpinned = Vec::new();
    for UseRef {
        workflow,
        reference,
    } in uses_from_yaml()
    {
        if reference.starts_with("./") || is_exempt(&reference) || is_sha_pinned(&reference) {
            continue;
        }
        unpinned.push(format!(
            "{workflow}: `uses: {reference}` is not pinned to a commit SHA - that ref \
             is mutable upstream, so this repo runs whatever {} publishes next",
            action_name(&reference)
        ));
    }
    assert!(
        unpinned.is_empty(),
        "workflow steps running an action from a MUTABLE ref:\n{}",
        unpinned.join("\n")
    );
}

/// Guard 2: the raw-text scan and the YAML scan see the same `uses:` set.
/// Guard 3 can only read comments from raw text, so a step written in a
/// shape the raw scan misses would silently escape it; this is what makes
/// that scan trustworthy rather than assumed.
#[test]
fn the_raw_scan_and_the_yaml_scan_see_the_same_uses_set() {
    let mut from_raw: Vec<UseRef> = uses_from_raw_text()
        .into_iter()
        .map(|(workflow, _line, reference, _comment)| UseRef {
            workflow,
            reference,
        })
        .collect();
    from_raw.sort();
    assert_eq!(
        from_raw,
        uses_from_yaml(),
        "the raw-text `uses:` scan disagrees with the parsed-YAML inventory; \
         guard 3 reads comments from the raw scan, so a mismatch means some \
         step's pin comment is going unchecked"
    );
}

/// Guard 3: every SHA-pinned `uses:` says which release the SHA is, in a
/// trailing `# vX.Y.Z` comment. Without it a reviewer (and Dependabot's own
/// PR titles) cannot tell a current pin from a two-year-old one, and the
/// pin's whole cost is that the version stops being readable.
#[test]
fn every_sha_pinned_use_names_its_release_in_a_trailing_comment() {
    let mut undocumented = Vec::new();
    for (workflow, line, reference, comment) in uses_from_raw_text() {
        if !is_sha_pinned(&reference) {
            continue;
        }
        let names_a_release = comment.as_deref().is_some_and(|c| {
            let mut chars = c.chars();
            chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
        });
        if !names_a_release {
            undocumented.push(format!(
                "{workflow}:{line}: `{reference}` has no trailing `# vX.Y.Z` comment \
                 (found {comment:?}) - the pinned release is unreadable"
            ));
        }
    }
    assert!(
        undocumented.is_empty(),
        "SHA pins that do not name their release:\n{}",
        undocumented.join("\n")
    );
}

/// Guard 4: every pinning exemption is still used by some workflow. An
/// exemption whose action has been removed is a standing hole in guard 1 -
/// the next step that adds that action back gets waved through unpinned, and
/// nothing else in this file would report it.
#[test]
fn every_pinning_exemption_is_still_used_by_some_workflow() {
    let refs = uses_from_yaml();
    let mut stale = Vec::new();
    for (action, _reason) in REF_NAME_IS_AN_ARGUMENT {
        let used = refs
            .iter()
            .any(|UseRef { reference, .. }| action_name(reference) == *action);
        if !used {
            stale.push(format!(
                "`{action}` is exempt from SHA pinning but no workflow uses it - \
                 delete the exemption, or it silently exempts the next step that \
                 adds this action back"
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "stale entries in REF_NAME_IS_AN_ARGUMENT:\n{}",
        stale.join("\n")
    );
}
