//! Drift guards for GitHub Actions token permissions.
//!
//! Every workflow must carry an explicit `permissions:` block at the
//! workflow level, and no permissions grant anywhere in any workflow may
//! exceed `contents: read`. The repository-default token scope is a settings
//! toggle, not a reviewed commit: without an explicit block, flipping that
//! toggle silently widens what every job's GITHUB_TOKEN can do. Measured on
//! main run 31026896599 (2026-08-05): jobs without a block ran with
//! `Contents: read, Metadata: read, Packages: read`, while the one job with
//! an explicit block ran with exactly `Contents: read, Metadata: read`.
//!
//! The workflow list is discovered from `.github/workflows`, never
//! hand-enumerated, and an empty discovery is a hard failure, so a new
//! workflow file enters this contract the moment it is committed. Each file
//! is run through a real YAML parser (serde_norway), so a workflow that no
//! longer parses fails here before it fails on a runner.
//!
//! If a workflow ever legitimately needs a wider grant (for example
//! `id-token: write` for OIDC publishing), widen the assertion here in the
//! same commit that widens the workflow, so the exception is reviewed and
//! named rather than inherited.

use serde_norway::Value;
use std::fs;
use std::path::PathBuf;

/// Discover and parse every workflow file. Empty discovery is a hard
/// failure: a guard that silently scans nothing proves nothing.
fn workflows() -> Vec<(String, Value)> {
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
        found.push((name, value));
    }
    assert!(
        !found.is_empty(),
        "no workflow files discovered under {} - the guard is scanning nothing",
        dir.display()
    );
    found
}

/// Every permissions mapping in `value` (workflow-level and per-job),
/// returned as (location, mapping-or-None-if-missing-at-workflow-level).
fn permissions_blocks(name: &str, value: &Value) -> Vec<(String, Option<Value>)> {
    let mut blocks = Vec::new();
    blocks.push((
        format!("{name} (workflow level)"),
        value.get("permissions").cloned(),
    ));
    if let Some(jobs) = value.get("jobs").and_then(Value::as_mapping) {
        for (job_name, job) in jobs {
            if let Some(perms) = job.get("permissions") {
                let job_name = job_name.as_str().unwrap_or("<non-string job id>");
                blocks.push((format!("{name} job `{job_name}`"), Some(perms.clone())));
            }
        }
    }
    blocks
}

/// Guard 1: every workflow declares an explicit workflow-level
/// `permissions:` block, so no job's token scope is inherited from the
/// repository settings toggle.
#[test]
fn every_workflow_declares_workflow_level_permissions() {
    let mut missing = Vec::new();
    for (name, value) in workflows() {
        match value.get("permissions") {
            Some(Value::Mapping(_)) => {}
            Some(other) => missing.push(format!(
                "{name}: workflow-level `permissions:` is not a mapping (found {other:?}); \
                 use the explicit `contents: read` mapping form"
            )),
            None => missing.push(format!(
                "{name}: no workflow-level `permissions:` block - every job inherits \
                 the repository-default token scope (a settings toggle, not a commit)"
            )),
        }
    }
    assert!(
        missing.is_empty(),
        "workflows missing an explicit least-privilege permissions block:\n{}",
        missing.join("\n")
    );
}

/// Guard 2: no permissions grant anywhere (workflow-level or job-level)
/// exceeds `contents: read`. A wider need must widen this assertion in the
/// same commit, so the exception is a reviewed, named decision.
#[test]
fn no_permissions_grant_exceeds_contents_read() {
    let mut violations = Vec::new();
    for (name, value) in workflows() {
        for (location, perms) in permissions_blocks(&name, &value) {
            let Some(perms) = perms else {
                // Absence at the workflow level is guard 1's finding, not a
                // grant; nothing to check here.
                continue;
            };
            let Some(mapping) = perms.as_mapping() else {
                violations.push(format!(
                    "{location}: `permissions:` is not a mapping (found {perms:?})"
                ));
                continue;
            };
            for (scope, level) in mapping {
                let scope = scope.as_str().unwrap_or("<non-string scope>");
                let level = level.as_str().unwrap_or("<non-string level>");
                if scope != "contents" || level != "read" {
                    violations.push(format!(
                        "{location}: grants `{scope}: {level}`, which exceeds the \
                         `contents: read` ceiling this repo's workflows need"
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "permissions grants beyond `contents: read`:\n{}",
        violations.join("\n")
    );
}
