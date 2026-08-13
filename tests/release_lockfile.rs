//! Drift guards for the lockfile the RELEASE path resolves against.
//!
//! `cargo publish` does not imply `--locked`. Measured on cargo 1.96.0
//! against this crate with `Cargo.toml` bumped one patch ahead of
//! `Cargo.lock`:
//!
//! * `cargo package --no-verify --allow-dirty` exited **0**, logged
//!   `Updating crates.io index`, and rewrote `Cargo.lock` in place;
//! * `cargo package --no-verify --allow-dirty --locked`, on the identical
//!   stale tree, exited **101** with `cannot update the lock file ... because
//!   --locked was passed to prevent this`.
//!
//! So without the flag the publish job reaches the live index at release
//! time and re-resolves, which makes the artifact it uploads a build of a
//! dependency graph no CI run ever verified. That is not confined to the
//! runner either: `cargo package --list` includes `Cargo.lock`, so the
//! lockfile resolved inside the publish job is the one shipped inside the
//! `.crate` and handed to every `cargo install --locked slokit`.
//!
//! `ci.yml` already refuses a stale lockfile on pull requests
//! (`cargo metadata --locked`, and the comment there explains why every other
//! cargo invocation would silently REPAIR one instead). These guards hold the
//! same property on the path that actually ships, where a repair is
//! irreversible rather than merely unnoticed.
//!
//! Scope is discovered by TRIGGER, not by filename: every workflow whose
//! `on:` names `release` is a release path, so a second publishing workflow
//! enters this contract the moment it is committed rather than needing to be
//! remembered. Empty discovery is a hard failure at every level -- no
//! workflows, no release-triggered workflow, or no cargo step inside one all
//! fail loudly, because a guard whose corpus emptied passes for exactly the
//! same reason a clean tree does.

use serde_norway::Value;
use std::fs;
use std::path::PathBuf;

/// Cargo subcommands that RESOLVE DEPENDENCIES, and therefore read (and
/// without `--locked` silently rewrite) `Cargo.lock`.
const RESOLVES_DEPENDENCIES: &[&str] = &[
    "bench", "build", "check", "clippy", "doc", "metadata", "package", "publish", "run", "rustc",
    "test", "tree",
];

/// Subcommands exempt from the `--locked` requirement, each with its reason.
/// Anything in NEITHER list fails closed: a subcommand nobody has classified
/// is waved through by default otherwise, which is the hole this whole file
/// exists to close one level down.
const DOES_NOT_READ_THE_LOCKFILE: &[(&str, &str)] = &[(
    "fmt",
    "rustfmt reads source files and never resolves the dependency graph, so \
     --locked is accepted but asserts nothing",
)];

/// One cargo invocation found in a release-path `run:` body.
struct CargoStep {
    /// `<workflow> job `<job>` step `<step>``, for failure messages.
    location: String,
    /// The shell line it was read from, quoted back in failures.
    line: String,
    /// The invocation's tokens, starting at `cargo`.
    tokens: Vec<String>,
}

impl CargoStep {
    /// The subcommand: the first token that is neither a flag nor a
    /// `+toolchain` override.
    fn subcommand(&self) -> Option<&str> {
        self.tokens[1..]
            .iter()
            .filter(|t| !t.starts_with('+'))
            .find(|t| !t.starts_with('-'))
            .map(String::as_str)
    }

    /// Whether `--locked` is passed to cargo itself. Tokens after a bare `--`
    /// belong to the subcommand's own callee (clippy's lint flags, a test
    /// binary's arguments) and are not cargo's to read.
    fn carries_locked(&self) -> bool {
        self.tokens
            .iter()
            .take_while(|t| t.as_str() != "--")
            .any(|t| t.as_str() == "--locked")
    }
}

/// Discover and parse every workflow file. Empty discovery is a hard failure:
/// a guard that silently scans nothing proves nothing.
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

/// The event names a workflow triggers on.
///
/// The key is read by scanning the mapping rather than by lookup because
/// YAML 1.1 resolves the bare key `on` to the BOOLEAN `true`, so a parser can
/// hand back a document in which `workflow["on"]` is absent while the trigger
/// is plainly there in the file. Both spellings are accepted.
fn triggers(workflow: &Value) -> Vec<String> {
    let Some(mapping) = workflow.as_mapping() else {
        return Vec::new();
    };
    let Some(node) = mapping.iter().find_map(|(key, value)| {
        let is_on = key.as_str() == Some("on") || key.as_bool() == Some(true);
        is_on.then_some(value)
    }) else {
        return Vec::new();
    };
    match node {
        Value::Mapping(map) => map
            .iter()
            .filter_map(|(key, _)| key.as_str().map(str::to_owned))
            .collect(),
        Value::Sequence(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        Value::String(one) => vec![one.clone()],
        _ => Vec::new(),
    }
}

/// Split a `run:` body into shell lines worth reading, dropping blanks and
/// `#` comments so prose in a script never enters the inventory.
fn shell_lines(run: &str) -> Vec<&str> {
    run.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// Every cargo invocation on one shell line, as token lists.
///
/// Shell substitution and separator punctuation is blanked first so an
/// invocation wrapped in `$( )` -- which is how the publish step captures its
/// own output -- tokenises as `cargo`, `publish`, ... rather than as a single
/// `$(cargo` token that no `== "cargo"` test would ever match.
fn cargo_invocations(line: &str) -> Vec<Vec<String>> {
    let cleaned: String = line
        .chars()
        .map(|c| {
            if matches!(c, '$' | '(' | ')' | '`' | ';' | '&' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let tokens: Vec<String> = cleaned.split_whitespace().map(str::to_owned).collect();
    let mut invocations = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] == "cargo" {
            let mut end = index + 1;
            while end < tokens.len() && tokens[end] != "cargo" {
                end += 1;
            }
            invocations.push(tokens[index..end].to_vec());
            index = end;
        } else {
            index += 1;
        }
    }
    invocations
}

/// Workflows that a `release` event triggers, as (file name, parsed document).
fn release_workflows() -> Vec<(String, Value)> {
    workflows()
        .into_iter()
        .filter(|(_, workflow)| triggers(workflow).iter().any(|event| event == "release"))
        .collect()
}

/// Every cargo invocation reachable on the release path.
fn release_path_cargo_steps() -> Vec<CargoStep> {
    let mut found = Vec::new();
    for (name, workflow) in release_workflows() {
        let Some(jobs) = workflow.get("jobs").and_then(Value::as_mapping) else {
            continue;
        };
        for (job_id, job) in jobs {
            let job_id = job_id.as_str().unwrap_or("<non-string job id>");
            let Some(steps) = job.get("steps").and_then(Value::as_sequence) else {
                continue;
            };
            for (index, step) in steps.iter().enumerate() {
                let Some(run) = step.get("run").and_then(Value::as_str) else {
                    continue;
                };
                let step_name = step
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("step #{index}"));
                for line in shell_lines(run) {
                    for tokens in cargo_invocations(line) {
                        found.push(CargoStep {
                            location: format!("{name} job `{job_id}` step `{step_name}`"),
                            line: line.to_owned(),
                            tokens,
                        });
                    }
                }
            }
        }
    }
    found
}

/// Guard 1: the corpus these guards read is real.
///
/// Every assertion below is of the form "no violations found", which a corpus
/// that emptied satisfies just as well as a correct release path does -- and
/// it gets EASIER to satisfy as the discovery breaks harder. So the selection
/// is asserted on its own, before any outcome: a workflow set, a
/// release-triggered subset, and cargo work inside it.
#[test]
fn the_release_path_is_discovered_and_carries_cargo_steps() {
    let releases = release_workflows();
    assert!(
        !releases.is_empty(),
        "no workflow is triggered by a `release` event, so every other guard \
         in this file passes vacuously; if publishing genuinely moved off \
         GitHub releases, delete tests/release_lockfile.rs in the SAME commit \
         rather than leaving it green over nothing (workflows seen: {:?})",
        workflows()
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    );

    let steps = release_path_cargo_steps();
    assert!(
        !steps.is_empty(),
        "the release path {:?} runs no cargo command at all - the `run:` \
         scan found nothing, so the `--locked` guards below have no corpus",
        releases
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    );
}

/// Guard 2: every dependency-resolving cargo step on the release path is
/// `--locked`.
///
/// Without it the step re-resolves against the live index and rewrites
/// `Cargo.lock`, exiting 0 either way, so the release is built and tested
/// against a graph that no green CI run ever covered.
#[test]
fn every_dependency_resolving_cargo_step_in_the_release_path_is_locked() {
    let mut violations = Vec::new();
    for step in release_path_cargo_steps() {
        let Some(subcommand) = step.subcommand() else {
            violations.push(format!(
                "{}: `{}` invokes cargo with no subcommand this guard can \
                 read; it cannot be classified, so it fails closed",
                step.location, step.line
            ));
            continue;
        };
        if DOES_NOT_READ_THE_LOCKFILE
            .iter()
            .any(|(name, _reason)| *name == subcommand)
        {
            continue;
        }
        if !RESOLVES_DEPENDENCIES.contains(&subcommand) {
            violations.push(format!(
                "{}: `cargo {subcommand}` is in neither RESOLVES_DEPENDENCIES \
                 nor DOES_NOT_READ_THE_LOCKFILE - classify it (with its \
                 reason) in the same commit that adds it, rather than letting \
                 an unclassified subcommand default to unchecked",
                step.location
            ));
            continue;
        }
        if !step.carries_locked() {
            violations.push(format!(
                "{}: `{}` resolves dependencies without `--locked`, so it \
                 reaches the live index at release time and rewrites \
                 Cargo.lock instead of failing",
                step.location, step.line
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "release-path cargo steps that do not pin the dependency graph:\n{}",
        violations.join("\n")
    );
}

/// Guard 3: the crates.io publish itself is `--locked`, named rather than
/// merely covered by guard 2.
///
/// Guard 2 is a "no violations" assertion over whatever it finds, so deleting
/// the publish step outright satisfies it perfectly. This one names the
/// invocation the whole workflow exists for and requires it to be present,
/// singular, and locked -- the lockfile it resolves is the one packaged into
/// the `.crate` and shipped to every `cargo install --locked` user.
#[test]
fn the_crates_io_publish_itself_is_locked() {
    let publishes: Vec<CargoStep> = release_path_cargo_steps()
        .into_iter()
        .filter(|step| step.subcommand() == Some("publish"))
        .collect();
    assert_eq!(
        publishes.len(),
        1,
        "expected exactly one `cargo publish` on the release path, found {} \
         ({:?}) - zero means this guard and guard 2 are both watching a \
         workflow that no longer publishes, and more than one means two \
         uploads race for the same version",
        publishes.len(),
        publishes
            .iter()
            .map(|step| step.location.clone())
            .collect::<Vec<_>>()
    );
    let publish = &publishes[0];
    assert!(
        publish.carries_locked(),
        "{}: `{}` publishes without `--locked`, so the .crate is packaged \
         from a lockfile re-resolved at publish time; that file is included \
         by `cargo package --list` and is what `cargo install --locked \
         slokit` consumes",
        publish.location,
        publish.line
    );
}

/// Guard 4: every `--locked` exemption is still used by the release path.
///
/// An exemption whose subcommand has left the workflow is a standing hole in
/// guard 2: the next step that adds that subcommand back is waved through
/// unlocked, and nothing else here would report it.
#[test]
fn every_lockfile_exemption_is_still_used_by_the_release_path() {
    let steps = release_path_cargo_steps();
    let mut stale = Vec::new();
    for (subcommand, _reason) in DOES_NOT_READ_THE_LOCKFILE {
        let used = steps
            .iter()
            .any(|step| step.subcommand() == Some(*subcommand));
        if !used {
            stale.push(format!(
                "`cargo {subcommand}` is exempt from `--locked` but the \
                 release path no longer runs it - delete the exemption, or it \
                 silently exempts the next step that adds this subcommand back"
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "stale entries in DOES_NOT_READ_THE_LOCKFILE:\n{}",
        stale.join("\n")
    );
}
