//! Drift guards for the Codecov upload step in `.github/workflows`.
//!
//! Codecov stopped accepting tokenless uploads. This repository has no
//! `CODECOV_TOKEN` secret, so the upload step ran on every push to `main`,
//! logged `-> Token length: 0` and then
//! `Upload queued for processing failed: {"message":"Token required - not
//! valid tokenless upload"}`, and reported SUCCESS anyway because it carried
//! `fail_ci_if_error: false`. Measured on main run 31270524770, job
//! 93135535624, head `354ad11` (2026-08-08). The result was a permanently
//! green job advertising a coverage integration that had never published a
//! single byte -- a shipped surface that silently does nothing.
//!
//! The contract these guards pin has three parts, deliberately split into
//! three tests so a regression in one does not hide behind another:
//!
//! 1. the upload runs only when a token is actually configured, so it can
//!    never again "succeed" at an upload the server rejected;
//! 2. when it does run it does not swallow its own failure; and
//! 3. the condition in (1) reads the secret it claims to read, rather than an
//!    environment variable nothing defines -- which would skip forever and be
//!    indistinguishable from (1) working.
//!
//! Workflow files are glob-discovered, never hand-enumerated, and both an
//! empty workflow directory and an absent upload step are hard failures: a
//! guard that scans nothing proves nothing. If the upload step is ever
//! removed deliberately (the other legal fix for the same defect), delete
//! this file in the SAME commit rather than letting it fail.

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

/// One `codecov/codecov-action` step, with the job that contains it.
struct UploadStep {
    /// `<workflow file> job `<job id>` step `<step name>``, for messages.
    location: String,
    step: Value,
    job: Value,
}

/// Every step in every workflow that runs `codecov/codecov-action`. An empty
/// result is a hard failure rather than a silent pass, so this suite cannot
/// go vacuously green if the step is renamed, moved, or dropped.
fn codecov_upload_steps() -> Vec<UploadStep> {
    let mut found = Vec::new();
    for (name, workflow) in workflows() {
        let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
            continue;
        };
        for (job_id, job) in jobs {
            let job_id = job_id.as_str().unwrap_or("<non-string job id>");
            let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
                continue;
            };
            for (index, step) in steps.iter().enumerate() {
                let uses = step.get("uses").and_then(Value::as_str).unwrap_or_default();
                if !uses.starts_with("codecov/codecov-action") {
                    continue;
                }
                let step_name = step
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("step #{index}"));
                found.push(UploadStep {
                    location: format!("{name} job `{job_id}` step `{step_name}`"),
                    step: step.clone(),
                    job: job.clone(),
                });
            }
        }
    }
    assert!(
        !found.is_empty(),
        "no `codecov/codecov-action` step found in any workflow - either the \
         upload was removed (in which case delete tests/coverage_upload.rs in \
         the same commit) or it was renamed out of this guard's reach"
    );
    found
}

/// Guard 1: the upload runs only when a token is actually configured.
///
/// Without this condition the action runs unconditionally, is rejected by the
/// server with `Token required - not valid tokenless upload`, and the step
/// still reports success. A skipped step is honest; a "successful" upload
/// that never landed is not.
#[test]
fn codecov_upload_runs_only_when_a_token_is_configured() {
    let mut violations = Vec::new();
    for upload in codecov_upload_steps() {
        match upload.step.get("if").and_then(Value::as_str) {
            Some(cond) if cond.contains("env.CODECOV_TOKEN") && cond.contains("!= ''") => {}
            Some(cond) => violations.push(format!(
                "{}: `if: {cond}` does not gate on the token being non-empty; \
                 expected a condition containing `env.CODECOV_TOKEN` and `!= ''`",
                upload.location
            )),
            None => violations.push(format!(
                "{}: has no `if:` condition, so it uploads even with no token \
                 configured and reports success on the server's rejection",
                upload.location
            )),
        }
    }
    assert!(
        violations.is_empty(),
        "Codecov upload steps that are not gated on a configured token:\n{}",
        violations.join("\n")
    );
}

/// Guard 2: when the upload does run, it does not swallow its own failure.
///
/// `fail_ci_if_error: false` is what made the tokenless rejection invisible.
/// Once a token exists, a rejected upload must fail the job.
#[test]
fn codecov_upload_never_swallows_its_own_failure() {
    let mut violations = Vec::new();
    for upload in codecov_upload_steps() {
        let setting = upload
            .step
            .get("with")
            .and_then(|w| w.get("fail_ci_if_error"))
            .cloned();
        let ok = match &setting {
            Some(Value::Bool(true)) => true,
            Some(Value::String(s)) => s == "true",
            _ => false,
        };
        if !ok {
            violations.push(format!(
                "{}: `with.fail_ci_if_error` is {}, so a rejected upload leaves \
                 the job green and the failure is only visible by reading the log",
                upload.location,
                setting.map_or("absent (defaults to false)".to_owned(), |v| format!(
                    "{v:?}"
                )),
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "Codecov upload steps that hide their own failure:\n{}",
        violations.join("\n")
    );
}

/// Guard 3: the condition in guard 1 reads the secret it claims to read.
///
/// `secrets` is not available in a step-level `if:`, so the token has to be
/// lifted into the job's `env:`. If that binding is missing, `env.CODECOV_TOKEN`
/// is unset, the condition is false forever, and the upload silently never
/// runs even once the secret exists -- a failure mode guard 1 cannot see,
/// because a permanently skipped step looks exactly like a correctly gated one.
#[test]
fn the_codecov_gate_reads_the_secret_it_claims_to_read() {
    let mut violations = Vec::new();
    for upload in codecov_upload_steps() {
        let binding = upload
            .job
            .get("env")
            .and_then(|e| e.get("CODECOV_TOKEN"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        match binding {
            Some(expr) if expr.contains("secrets.CODECOV_TOKEN") => {}
            Some(expr) => violations.push(format!(
                "{}: the job binds `env.CODECOV_TOKEN` to `{expr}`, which does \
                 not read `secrets.CODECOV_TOKEN`",
                upload.location
            )),
            None => violations.push(format!(
                "{}: the job declares no `env.CODECOV_TOKEN`, so the step's \
                 `if:` compares an unset variable and the upload can never run",
                upload.location
            )),
        }
    }
    assert!(
        violations.is_empty(),
        "Codecov gates wired to something other than the secret:\n{}",
        violations.join("\n")
    );
}
