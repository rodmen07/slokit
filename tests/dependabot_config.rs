//! Drift guards for the Dependabot version-updates config.
//!
//! `.github/dependabot.yml` is consumed by a GitHub service, not by any job
//! in this repo, so nothing in CI would notice if it vanished or stopped
//! parsing: version updates would silently stop arriving, which from the
//! outside is indistinguishable from "every dependency is current". These
//! guards make the config's absence or decay a named test failure instead.
//!
//! What a test here can NOT prove is the service side -- that Dependabot
//! actually runs against the config. That was proven once, at ship time, by
//! the behavior difference on the default branch (zero version-update PRs
//! ever before the config existed; Dependabot's first run observed after
//! the merge -- see the shipping PR's body for the run evidence). What a
//! test CAN hold durably is that the file exists, parses, and still covers
//! both ecosystems this repo ships from.
//!
//! If a legitimate config change narrows what these guards assert (an
//! ecosystem retired, a schedule removed), widen or retire the assertion in
//! the same commit, so the exception is a reviewed, named decision -- the
//! same convention `tests/workflow_permissions.rs` uses.

use serde_norway::Value;
use std::fs;
use std::path::PathBuf;

/// Read and parse the config. A missing or unparseable file is a hard
/// failure in every guard: a config Dependabot cannot read updates nothing.
fn config() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/dependabot.yml");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e} - Dependabot version updates are configured by this \
             file and silently stop arriving without it",
            path.display()
        )
    });
    serde_norway::from_str(&text)
        .unwrap_or_else(|e| panic!("dependabot.yml is not parseable YAML: {e}"))
}

/// The `updates:` entries. An empty or missing list is a hard failure: a
/// guard that scans nothing proves nothing.
fn update_entries(config: &Value) -> Vec<Value> {
    let entries: Vec<Value> = config
        .get("updates")
        .and_then(Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    assert!(
        !entries.is_empty(),
        "dependabot.yml has no `updates:` entries - the config updates nothing"
    );
    entries
}

/// Guard 1: the config declares `version: 2`, the only version Dependabot
/// accepts. Anything else makes GitHub reject the whole file, which it
/// reports only in the repo's Dependabot UI tab, not in CI.
#[test]
fn the_config_declares_version_2() {
    let version = config().get("version").and_then(Value::as_u64);
    assert_eq!(
        version,
        Some(2),
        "dependabot.yml must declare `version: 2` (found {version:?}); \
         GitHub rejects any other value and version updates stop"
    );
}

/// Guard 2: both ecosystems this repo ships from are covered at the repo
/// root - `cargo` (the crate's dependency tree, Cargo.lock committed) and
/// `github-actions` (the pinned actions every workflow runs). Dropping
/// either silently uncouples that ecosystem from automated bumps.
#[test]
fn both_shipping_ecosystems_are_covered_at_the_root() {
    let config = config();
    let mut covered: Vec<(String, String)> = update_entries(&config)
        .iter()
        .map(|entry| {
            let ecosystem = entry
                .get("package-ecosystem")
                .and_then(Value::as_str)
                .unwrap_or("<missing package-ecosystem>")
                .to_owned();
            let directory = entry
                .get("directory")
                .and_then(Value::as_str)
                .unwrap_or("<missing directory>")
                .to_owned();
            (ecosystem, directory)
        })
        .collect();
    covered.sort();
    assert_eq!(
        covered,
        vec![
            ("cargo".to_owned(), "/".to_owned()),
            ("github-actions".to_owned(), "/".to_owned()),
        ],
        "dependabot.yml must cover exactly the cargo and github-actions \
         ecosystems at the repo root; a repo shipping from an uncovered \
         ecosystem gets no automated bumps for it"
    );
}

/// Guard 3: every update entry carries a `schedule.interval`. Dependabot
/// rejects an entry without one, and the rejection is visible only in the
/// repo's Dependabot UI tab, never in CI.
#[test]
fn every_update_entry_has_a_schedule_interval() {
    let config = config();
    let mut missing = Vec::new();
    for entry in update_entries(&config) {
        let ecosystem = entry
            .get("package-ecosystem")
            .and_then(Value::as_str)
            .unwrap_or("<missing package-ecosystem>")
            .to_owned();
        let interval = entry
            .get("schedule")
            .and_then(|s| s.get("interval"))
            .and_then(Value::as_str);
        if interval.is_none() {
            missing.push(ecosystem);
        }
    }
    assert!(
        missing.is_empty(),
        "update entries missing `schedule.interval` (Dependabot rejects them): {missing:?}"
    );
}
