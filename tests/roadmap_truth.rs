//! Drift guard: the release documents against `Cargo.toml` and `Cargo.lock`.
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
//!    every such heading is one this file can actually read a version out of,
//!    the sections it reads them from still exist, and no version heading
//!    anywhere in the document sits in a section this file does not classify,
//! 4. no released version sits in a `BLOCKED` row,
//! 5. the "Unreleased on main" section exists exactly when the CHANGELOG has
//!    unreleased entries, and
//! 6. `Cargo.toml`'s version has a `## [x.y.z]` CHANGELOG entry of its own.
//!
//! Check 6 was added with the v1.1.0 prep. It closes the way a release-prep PR
//! can be half-finished while checks 1 through 5 all stay green: bumping
//! `Cargo.toml` and forgetting the changelog passes every one of them, because
//! check 1 simply follows `Cargo.toml` wherever it goes and check 2 only
//! constrains versions the changelog already names.
//!
//! The sibling half-finish, a `Cargo.lock` left behind at the old version, is
//! deliberately NOT guarded here, and the reason is worth keeping. A test that
//! `include_str!`s `Cargo.lock` cannot fail: cargo rewrites a stale lockfile
//! during the build that produces the test binary, so by the time any
//! assertion runs the file on disk has already been repaired. Measured, not
//! assumed — reverting the lock to 1.0.0 against a 1.1.0 `Cargo.toml` and
//! running `cargo test` left the lock reading 1.1.0 again and the assertion
//! passing. That check lives in `.github/workflows/ci.yml` as
//! `cargo metadata --locked`, the one invocation that refuses to repair.
//!
//! The extractors are themselves exercised on synthetic input at the bottom of
//! the file, so a parser that silently stops matching cannot turn these into
//! assertions that always pass.
//!
//! That claim was too strong until 2026-08-08, and check 3 is where it broke.
//! The milestone extractor *was* exercised — on `### v0.8.0: Spec hardening`,
//! a colon and nothing else — while the real parser split on a colon and
//! nothing else too, so the synthetic input agreed with the bug instead of
//! catching it. A heading written `### v1.7.0 — sloth corpus parity` yielded
//! zero versions, zero versions is also what an empty section yields, and the
//! check passed on a roadmap listing a shipped release as upcoming. An
//! extractor test is only a guard against vacuity when its inputs span the
//! shapes the real document is allowed to take, so check 3 now asserts that
//! every milestone heading present is READABLE, separately from asserting
//! what the readable ones say.

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

/// The `## `-level sections that present work as still to come. A released
/// version has no business in either of them. Naming them beats scanning the
/// whole document, which would turn a `### v1.6.0` subheading under
/// `## History and supersession` into a false positive the day that table
/// grows one; both names are asserted to exist below, so a rename fails
/// loudly instead of quietly emptying the guard.
///
/// What naming them cannot see is an ADDITION: a brand-new `## ` section
/// holding a version heading is invisible to an inclusion list from the day
/// it is written. Measured on this tree before 2026-08-08: appending
/// `## Under consideration` with `### v1.7.0` under it — a version the
/// CHANGELOG says shipped — left all seven tests green. So check 3 also walks
/// the WHOLE document via `version_headings_by_section` and fails on any
/// version heading in a section neither this list nor
/// `SHIPPED_WORK_SECTIONS` classifies.
const FORWARD_LOOKING_SECTIONS: [&str; 2] =
    ["## Next milestones", "## Later / candidates (unscheduled)"];

/// Sections whose version headings describe SHIPPED work, where a released
/// version is exactly what belongs. The history table holds no `### v`
/// subheadings today, but growing some is the documented reason check 3
/// names its sections instead of scanning the document, so the
/// classification has to recognise them the day they appear.
const SHIPPED_WORK_SECTIONS: [&str; 1] = ["## History and supersession"];

/// True for a line shaped like a version heading: `### v` followed by a
/// digit, which is what separates `### v1.8.0 — title` from prose like
/// `### various notes`.
fn is_version_heading(line: &str) -> bool {
    line.strip_prefix("### v")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// The `### v…` headings inside the forward-looking sections, verbatim.
///
/// Headings come back UNPARSED on purpose: the caller has to be able to tell
/// "nothing is scheduled" from "a milestone is listed and this file could not
/// read it", and those two used to be the same answer.
fn milestone_headings(roadmap: &str) -> Vec<&str> {
    FORWARD_LOOKING_SECTIONS
        .iter()
        .flat_map(|heading| section(roadmap, heading))
        .map(str::trim_end)
        .filter(|line| is_version_heading(line))
        .collect()
}

/// Every version heading in the document, paired with the `## `-level heading
/// of the section holding it (`None` for one before the first section).
///
/// This walks the WHOLE document, unlike `milestone_headings`, because its
/// job is the opposite one: finding version headings in sections this guard
/// does NOT read, so a new section cannot park one out of sight.
fn version_headings_by_section(roadmap: &str) -> Vec<(Option<&str>, &str)> {
    let mut current = None;
    let mut out = Vec::new();
    for line in roadmap.lines().map(str::trim_end) {
        if line.starts_with("## ") {
            current = Some(line);
        } else if is_version_heading(line) {
            out.push((current, line));
        }
    }
    out
}

/// The version named by a `### v…` milestone heading, whatever separator
/// follows it: a colon, a hyphen, an em dash, a parenthesis, or nothing.
///
/// The previous parser did `rest.split(':').next()`, so it could read a
/// version only when the heading used a colon. `### v1.7.0 — sloth corpus
/// parity` parsed to *no version at all*, and because zero versions is what
/// an empty section also yields, the released-version guard below passed on a
/// roadmap that was actively wrong. Measured on this tree at `5102ffb`: that
/// exact heading gave `7 passed`, and the identical line rewritten with a
/// colon gave `1 failed`.
fn milestone_version(heading: &str) -> Option<Version> {
    let rest = heading.strip_prefix("### v")?;
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    parse_version(&token)
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
    // The sections have to be there. `section()` answers "no lines" for a
    // heading that does not exist, which is the same answer it gives for a
    // section holding nothing, so a rename would empty this guard without
    // failing anything at all.
    for heading in FORWARD_LOOKING_SECTIONS {
        assert!(
            ROADMAP.lines().any(|line| line.trim_end() == heading),
            "ROADMAP.md has no `{heading}` heading. This guard finds upcoming work by \
             section name, so renaming one silently empties it: restore the heading, or \
             update FORWARD_LOOKING_SECTIONS in this file."
        );
    }

    // Every version heading has to sit in a section this guard CLASSIFIES:
    // forward-looking (checked below) or shipped work (where released
    // versions belong). Without this, the two section lists are an inclusion
    // list, and an inclusion list cannot see an ADDITION: a brand-new
    // section could park a released version where no assertion reads, and
    // nothing would fail — measured before 2026-08-08, when exactly that
    // perturbation left every test in this file green.
    let unclassified: Vec<String> = version_headings_by_section(ROADMAP)
        .into_iter()
        .filter(|(sec, _)| {
            !sec.is_some_and(|s| {
                FORWARD_LOOKING_SECTIONS.contains(&s) || SHIPPED_WORK_SECTIONS.contains(&s)
            })
        })
        .map(|(sec, heading)| {
            format!(
                "`{heading}` under `{}`",
                sec.unwrap_or("no ## section at all")
            )
        })
        .collect();
    assert!(
        unclassified.is_empty(),
        "ROADMAP.md version heading(s) in sections this guard does not read: {}. \
         A version heading it cannot classify is one it cannot check, so the check \
         would go quiet rather than fail. Move the heading into a section named in \
         FORWARD_LOOKING_SECTIONS or SHIPPED_WORK_SECTIONS in this file, or add the \
         new section to whichever list describes it.",
        unclassified.join(", ")
    );

    // Every milestone heading present has to be READABLE. Without this, a
    // heading the parser cannot decode is indistinguishable from no milestones
    // at all, and the assertion below passes vacuously — which is exactly what
    // happened between the v1.7.0 prep and 2026-08-08.
    let unreadable: Vec<&str> = milestone_headings(ROADMAP)
        .into_iter()
        .filter(|heading| milestone_version(heading).is_none())
        .collect();
    assert!(
        unreadable.is_empty(),
        "ROADMAP.md milestone heading(s) this guard cannot read a version out of: {}. \
         A heading it cannot read is a heading it cannot check, so the check would go \
         quiet rather than fail. Write the version as `### vX.Y.Z`, followed by any \
         separator you like.",
        unreadable.join(" | ")
    );

    let released = released_versions(CHANGELOG);
    let shipped: Vec<String> = milestone_headings(ROADMAP)
        .into_iter()
        .filter_map(milestone_version)
        .filter(|v| released.contains(v))
        .map(show)
        .collect();
    assert!(
        shipped.is_empty(),
        "ROADMAP.md lists {} under a forward-looking section, but CHANGELOG.md says it \
         already shipped; move the section to `## History and supersession`",
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

#[test]
fn the_crate_version_has_its_own_changelog_entry() {
    let declared = crate_version(CARGO_TOML).expect("Cargo.toml [package] version parses");
    let released = released_versions(CHANGELOG);
    assert!(
        released.contains(&declared),
        "Cargo.toml declares {} but CHANGELOG.md has no `## [{}]` heading. A release-prep \
         PR that bumps the version and forgets the changelog passes every other guard in \
         this file: the current-state check follows Cargo.toml wherever it goes, and the \
         history check only constrains versions the changelog already names. Either write \
         the `## [{}] - <date>` section or put the version back.",
        show(declared),
        show(declared),
        show(declared)
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
    // Separator tolerance is the whole point: each of these headings names a
    // version, and before 2026-08-08 only the colon form parsed at all.
    let milestones = "## Next milestones\n\
                      ### v0.8.0: colon\n\
                      prose between headings\n\
                      ### v0.9.0 - hyphen\n\
                      ### v1.0.0 — em dash\n\
                      ### v1.1.0\n\
                      ### various notes, not a version at all\n\
                      ## Later / candidates (unscheduled)\n\
                      ### v1.2.0 (parenthetical)\n\
                      ## History and supersession\n\
                      ### v0.1.0: shipped long ago, not upcoming\n";
    assert_eq!(
        milestone_headings(milestones)
            .into_iter()
            .filter_map(milestone_version)
            .collect::<Vec<_>>(),
        vec![(0, 8, 0), (0, 9, 0), (1, 0, 0), (1, 1, 0), (1, 2, 0)],
        "milestone headings must parse whatever separator follows the version, must skip \
         `### v`-prefixed prose, and must not reach into the history section"
    );
    // An unreadable heading must be DETECTED and reported, never skipped: the
    // difference between the guard failing and the guard evaporating is
    // entirely here.
    assert_eq!(milestone_version("### v1.7 — only two components"), None);
    assert_eq!(milestone_version("### v1.7.0.1: four components"), None);
    assert_eq!(
        milestone_headings("## Next milestones\n### v1.7 — only two components\n"),
        vec!["### v1.7 — only two components"],
        "a heading whose version cannot be parsed must still be collected, or the guard \
         has nothing to complain about"
    );
    assert!(
        milestone_headings("## Not a section this guard reads\n### v1.7.0: title\n").is_empty(),
        "milestone_headings must read only the forward-looking sections"
    );
    // The whole-document classifier: every version heading is reported with
    // the section holding it, wherever that is — including before the first
    // `## ` heading and inside sections the milestone reader never opens,
    // because "a section this file does not read" is exactly what it exists
    // to surface.
    assert_eq!(
        version_headings_by_section(
            "### v0.0.1: before any section\n\
             ## Next milestones\n\
             ### v1.9.0: forward\n\
             ### various notes, not a version at all\n\
             ## Under consideration\n\
             ### v1.7.0: parked out of sight\n\
             ## History and supersession\n\
             ### v0.1.0: shipped long ago\n"
        ),
        vec![
            (None, "### v0.0.1: before any section"),
            (Some("## Next milestones"), "### v1.9.0: forward"),
            (
                Some("## Under consideration"),
                "### v1.7.0: parked out of sight"
            ),
            (
                Some("## History and supersession"),
                "### v0.1.0: shipped long ago"
            ),
        ],
        "version_headings_by_section must walk every section, keep the pairing, and \
         skip non-version `###` prose"
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
