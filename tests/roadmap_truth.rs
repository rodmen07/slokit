//! Drift guard: `ROADMAP.md` against `Cargo.toml` and `CHANGELOG.md`.
//!
//! The roadmap is the one document in this repo that nothing else checks, and
//! it rotted within a single day of being written: the revision dated
//! 2026-07-18 declared "Current state: v0.7.0", listed v0.8.0 through v1.0.0
//! as upcoming milestones, and carried a BLOCKED row for work that had already
//! shipped, all while the crate sat at 1.0.0 on crates.io. A one-time
//! reconciliation would rot again the same way, so the reconciliation is a
//! test instead: every claim below is derived from a second source, and the
//! build fails when the two disagree.
//!
//! What is guarded:
//!
//! 1. the `## Current state: vX.Y.Z` heading equals `Cargo.toml`'s version,
//! 2. every released `CHANGELOG` version appears in the history table,
//! 3. no released version is listed as an upcoming `### vX.Y.Z` milestone,
//! 4. no released version sits in a `BLOCKED` row, and
//! 5. the "Unreleased on main" section exists exactly when the CHANGELOG has
//!    unreleased entries.
//!
//! The extractors are themselves exercised on synthetic input at the bottom of
//! the file, so a parser that silently stops matching cannot turn these into
//! assertions that always pass.

const ROADMAP: &str = include_str!("../ROADMAP.md");
const CHANGELOG: &str = include_str!("../CHANGELOG.md");
const CARGO_TOML: &str = include_str!("../Cargo.toml");

type Version = (u64, u64, u64);

fn parse_version(raw: &str) -> Option<Version> {
    let raw = raw.trim().trim_start_matches('v');
    let mut parts = raw.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn show(v: Version) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// The lines of `doc` belonging to the `## `-level section with this heading,
/// excluding the heading itself.
fn section<'a>(doc: &'a str, heading: &str) -> Vec<&'a str> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in doc.lines() {
        if line.trim_end() == heading {
            inside = true;
            continue;
        }
        if inside && line.starts_with("## ") {
            break;
        }
        if inside {
            out.push(line);
        }
    }
    out
}

/// The `version = "x.y.z"` of the `[package]` table.
fn crate_version(cargo_toml: &str) -> Option<Version> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix("version") {
                let rest = rest.trim_start().strip_prefix('=')?;
                let value = rest.trim().trim_matches('"');
                return parse_version(value);
            }
        }
    }
    None
}

/// Every version with a `## [x.y.z]` CHANGELOG heading; `[Unreleased]` is not
/// one of them.
fn released_versions(changelog: &str) -> Vec<Version> {
    changelog
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix("## ["))
        .filter_map(|rest| rest.split(']').next())
        .filter_map(parse_version)
        .collect()
}

/// True when the CHANGELOG's `[Unreleased]` section holds anything but blanks.
fn changelog_has_unreleased_entries(changelog: &str) -> bool {
    let mut inside = false;
    for line in changelog.lines() {
        if line.trim_end() == "## [Unreleased]" {
            inside = true;
            continue;
        }
        if inside && line.starts_with("## ") {
            break;
        }
        if inside && !line.trim().is_empty() {
            return true;
        }
    }
    false
}

fn current_state_version(roadmap: &str) -> Option<Version> {
    let line = roadmap
        .lines()
        .find(|line| line.starts_with("## Current state:"))?;
    line.split_whitespace()
        .find_map(|word| word.strip_prefix('v').and_then(parse_version))
}

/// The version (or inclusive version range) named by each history-table row.
fn history_table_ranges(roadmap: &str) -> Vec<(Version, Version)> {
    section(roadmap, "## History and supersession")
        .into_iter()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| {
            let cell = line.trim_start_matches('|').split('|').next()?.trim();
            match cell.split_once('-') {
                Some((lo, hi)) => Some((parse_version(lo)?, parse_version(hi)?)),
                None => {
                    let single = parse_version(cell)?;
                    Some((single, single))
                }
            }
        })
        .collect()
}

/// Versions presented as still-upcoming milestones (`### vX.Y.Z: ...`).
fn milestone_versions(roadmap: &str) -> Vec<Version> {
    roadmap
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix("### v"))
        .filter_map(|rest| parse_version(rest.split(':').next()?))
        .collect()
}

/// Versions named by rows whose status cell is `BLOCKED`.
fn blocked_row_versions(roadmap: &str) -> Vec<Version> {
    let mut out = Vec::new();
    for line in section(roadmap, "## Blocked and USER-ONLY summary") {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let [item, status, ..] = cells.as_slice() else {
            continue;
        };
        if *status != "BLOCKED" {
            continue;
        }
        out.extend(
            item.split_whitespace()
                .filter_map(|word| word.strip_prefix('v').and_then(parse_version)),
        );
    }
    out
}

#[test]
fn roadmap_current_state_matches_the_crate_version() {
    let declared = crate_version(CARGO_TOML).expect("Cargo.toml [package] version parses");
    let claimed = current_state_version(ROADMAP)
        .expect("ROADMAP.md has a `## Current state: vX.Y.Z` heading");
    assert_eq!(
        claimed,
        declared,
        "ROADMAP.md claims current state v{} but Cargo.toml says {}. \
         A release-prep PR must move both.",
        show(claimed),
        show(declared)
    );
}

#[test]
fn roadmap_history_covers_every_released_version() {
    let released = released_versions(CHANGELOG);
    assert!(
        released.len() > 10,
        "parsed only {} released versions from CHANGELOG.md; the heading format changed",
        released.len()
    );
    let ranges = history_table_ranges(ROADMAP);
    assert!(
        !ranges.is_empty(),
        "parsed no version rows from ROADMAP.md's history table; the table format changed"
    );

    let missing: Vec<String> = released
        .iter()
        .filter(|v| !ranges.iter().any(|(lo, hi)| lo <= v && v <= &hi))
        .map(|v| show(*v))
        .collect();
    assert!(
        missing.is_empty(),
        "released per CHANGELOG.md but absent from ROADMAP.md's history table: {}",
        missing.join(", ")
    );
}

#[test]
fn roadmap_does_not_list_a_released_version_as_an_upcoming_milestone() {
    let released = released_versions(CHANGELOG);
    let shipped: Vec<String> = milestone_versions(ROADMAP)
        .into_iter()
        .filter(|v| released.contains(v))
        .map(show)
        .collect();
    assert!(
        shipped.is_empty(),
        "ROADMAP.md lists {} under `## Next milestones`, but CHANGELOG.md says it already \
         shipped; move the section to `## History and supersession`",
        shipped.join(", ")
    );
}

#[test]
fn roadmap_does_not_list_a_released_version_as_blocked() {
    let released = released_versions(CHANGELOG);
    let shipped: Vec<String> = blocked_row_versions(ROADMAP)
        .into_iter()
        .filter(|v| released.contains(v))
        .map(show)
        .collect();
    assert!(
        shipped.is_empty(),
        "ROADMAP.md's blocked table still gates {}, which CHANGELOG.md says has shipped",
        shipped.join(", ")
    );
}

#[test]
fn roadmap_declares_unreleased_work_exactly_when_the_changelog_has_some() {
    let has_entries = changelog_has_unreleased_entries(CHANGELOG);
    let has_section = ROADMAP
        .lines()
        .any(|line| line.trim_end() == "## Unreleased on main");
    assert_eq!(
        has_section,
        has_entries,
        "CHANGELOG.md {} unreleased entries but ROADMAP.md {} an `## Unreleased on main` \
         section. Shipped-but-unpublished work must be visible in the roadmap, and must \
         disappear from it when the release is cut.",
        if has_entries { "has" } else { "has no" },
        if has_section { "has" } else { "lacks" },
    );
}

// The extractors above are the only thing standing between these tests and
// vacuous truth, so each one is exercised on input whose answer is known.
#[test]
fn extractors_find_what_they_are_looking_for() {
    assert_eq!(
        crate_version("[dependencies]\nversion = \"9.9.9\"\n\n[package]\nversion = \"1.2.3\"\n"),
        Some((1, 2, 3)),
        "crate_version must read [package], not the first `version =` in the file"
    );
    assert_eq!(
        released_versions("## [Unreleased]\n## [1.0.0] - 2026-07-19\n## [0.6.1] - 2026-06-27\n"),
        vec![(1, 0, 0), (0, 6, 1)],
        "released_versions must skip [Unreleased] and keep dated releases"
    );
    assert!(changelog_has_unreleased_entries(
        "## [Unreleased]\n\n### Added\n- thing\n\n## [1.0.0] - 2026-07-19\n"
    ));
    assert!(!changelog_has_unreleased_entries(
        "## [Unreleased]\n\n## [1.0.0] - 2026-07-19\n\n### Added\n- thing\n"
    ));
    assert_eq!(
        current_state_version("## Current state: v0.7.0 (released 2026-07-18)\n"),
        Some((0, 7, 0))
    );
    assert_eq!(
        history_table_ranges(
            "## History and supersession\n| 0.6.1-0.6.8 | d | h |\n| 1.0.0 | d | h |\n## Next\n"
        ),
        vec![((0, 6, 1), (0, 6, 8)), ((1, 0, 0), (1, 0, 0))],
        "history_table_ranges must expand patch-series rows into an inclusive range"
    );
    assert_eq!(
        milestone_versions("### v0.8.0: Spec hardening\ntext\n### v1.1.0: publish\n"),
        vec![(0, 8, 0), (1, 1, 0)]
    );
    assert_eq!(
        blocked_row_versions(
            "## Blocked and USER-ONLY summary\n\
             | v0.9.0 SliPlugin implementation (PR 2) | BLOCKED | gated on review |\n\
             | v1.1.0 tag and publish | USER-ONLY | manual by policy |\n\
             ## History and supersession\n"
        ),
        vec![(0, 9, 0)],
        "blocked_row_versions must read the status cell, not every row in the table"
    );
}
