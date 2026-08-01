# slokit post-1.0 expansion: scoping v1.2.0

- Status: **APPROVED 2026-08-01** — the maintainer approved D1 through D6 as
  written, accepting the whole default set with no override. v1.2.0 is
  scheduled at the scope below and the [ROADMAP](../../ROADMAP.md) v1.2.0
  section carries it. This document is now the decision record, not a proposal.
- Date: 2026-07-26 (written against v1.1.0, the version live on crates.io);
  approved 2026-08-01
- Scope: decides the theme of the v1.2.0 milestone and orders the remaining
  expansion candidates, per the [ROADMAP](../../ROADMAP.md) v1.2.0 section
- Decides: D1 through D6 below. Every decision is an overridable default:
  replying "approved" accepts the whole set, and overriding any single one
  does not stall the rest.

## 1. Summary

The agent-doable path to 1.0 is complete and v1.1.0 (`slokit simulate`) is
published. The roadmap carries three expansion candidates with no ranking:
OpenSLO export, additional lint rules, and dashboard enhancements. This
proposal ranks them, picks **OpenSLO v1 export** as the single theme of
v1.2.0, and records why the other two wait. Nothing here changes code; the
implementation is gated on this document's review.

## 2. Where the crate stands (verified 2026-07-26, not inherited)

Each claim below was re-derived from the source named with it, not copied
from an earlier document:

- **Published version**: crates.io reports `newest_version` and
  `max_stable_version` both `1.1.0` (API read 2026-07-26).
- **CLI surface**: `generate`, `validate`, `lint`, `calc`, `simulate`,
  `check`, `dashboard`, `schema` (the `Command` enum in `src/bin/slokit.rs`).
- **OpenSLO support is import-only**: `src/spec/openslo.rs` exposes
  `is_openslo`, `from_yaml`, and `from_path`, and `grep -rni export src/`
  returns zero hits. A spec imported from OpenSLO cannot be written back out.
- **Lint**: 13 codes in `src/spec/lint.rs` (`SPEC_VERSION`, `NO_DESCRIPTION`,
  `OBJECTIVE_100`, `OBJECTIVE_LOW`, `PERIOD_TOO_SHORT`, `LABEL_NAME_CHARS`,
  `RESERVED_LABEL`, `NO_ALERT_LABELS`, `ALL_ALERTS_DISABLED`,
  `DUPLICATE_ALERT_WINDOW`, `NO_SEVERITY_WINDOWS`, `THRESHOLD_UNREACHABLE`,
  `PLUGIN_UNKNOWN_OPTION`).
- **Dashboard**: `src/dashboard.rs` emits five panels per SLO (a row, three
  stats, one timeseries). There are no per-severity burn-rate panels.
- **API freeze**: 1.x is additive-only per [SEMVER.md](../SEMVER.md);
  everything proposed here is additive.

## 3. Decisions

### D1. The v1.2.0 theme is OpenSLO v1 export (default)

The inverse of the v0.10.0 import: serialize a slokit `Spec` as OpenSLO v1
YAML. Why this one first:

- **It completes a one-way door.** Import-only conversion strands users: a
  team can migrate INTO slokit from OpenSLO tooling but cannot hand specs
  back to any OpenSLO consumer (Nobl9, oslo-validating pipelines, other
  generators). Export makes adoption reversible, which lowers the cost of
  trying slokit at all.
- **It is the most checkable candidate.** "Round-trip preserves the spec" is
  a property a test can pin; "the lint rules are useful" and "the dashboard
  is better" are opinions.
- **It needs zero new dependencies.** `serde_norway` already serializes; the
  OpenSLO document structs exist in `src/spec/openslo.rs` for the import.
- **It is additive** (new functions, new subcommand), so it is a legal 1.x
  minor under the freeze.

### D2. Surface: an `export` subcommand plus public library functions (default)

- Library: `spec::openslo::to_yaml(&Spec) -> Result<String>` (sibling of
  `from_yaml`), behind the existing `spec` feature. The lean core
  (`--no-default-features`) is untouched.
- CLI: `slokit export --format openslo <SPEC>...` writing to stdout or
  `--output <dir>`, following the flag conventions the other subcommands
  already use. `--format` takes only `openslo` initially; the flag exists so
  a future format is not a breaking change.

### D3. Fidelity contract: semantic round-trip, fail closed on gaps (default)

`from_yaml(to_yaml(spec))` must yield an equivalent `Spec`, pinned by tests
over the repo's example specs plus insta snapshots. Byte identity is NOT
promised: the two models are not isomorphic and field order differs.

Any slokit construct with **no OpenSLO representation** is a **hard error
naming the field**, not a silent drop and not best-effort YAML that an OpenSLO
consumer would reject downstream. (Overridable to lossy-with-warning; fail
closed is the default because the crate's validation philosophy since 0.8.0
has been "hard error only when the output would break", and unrepresentable
fields break the output's consumer.) PR 1's first deliverable is the exact
field-mapping table, derived by inverting the import's existing mapping, with
the unrepresentable set enumerated in the module docs.

### D4. v1.2.0 stays single-theme (default)

No second feature rides along. Lint rules and dashboard panels remain
unscheduled candidates; see D5. A one-theme minor keeps the release reviewable
and the done-when crisp.

### D5. Candidate order after v1.2.0: lint rules, then dashboard panels (default)

- **Additional lint rules** go next, but only once grounded: each proposed
  rule must cite a real spec (from `examples/`, the InfraPortal set, or a
  user report) where it would have fired usefully. Illustrative candidates,
  named here only so the shape is visible, each needing that grounding first:
  a window-coverage check (severity windows that leave part of the error
  budget unwatched), an objective-precision check (more digits than the
  period's event volume can resolve), and a plugin-option-unused check.
- **Per-severity dashboard burn panels** wait. The generated dashboards have
  no live consumer today: `examples/infraportal/` is SLO-definitions-as-code
  until the InfraPortal services expose `/metrics` (work that lives in the
  microservices repo, not here). Building dashboard polish before any
  dashboard renders live data optimizes an artifact nobody can look at.

Both remain candidates, not commitments; each gets its own scoping when its
turn comes.

### D6. Milestone mechanics (default)

Slices are dependency-ordered; nothing is calendar-sized.

1. **PR 1**: library export (`to_yaml` + mapping table + round-trip tests
   incl. every committed example spec + snapshot tests). Done when the
   round-trip property is CI-green.
2. **PR 2**: `export` subcommand + binary-level tests (the
   `tests/simulate_cli.rs` pattern) + README section.
3. **PR 3**: release prep (CHANGELOG `[1.2.0]`, version bumps, ROADMAP move
   in the same commit, as `roadmap_truth` enforces) and the cut, which is
   delegated per the 2026-07-26 release-delegation decision: cut only with
   prep on main, CI green on that commit, and the registry confirmed
   afterwards.

**v1.2.0 done-when (checkable):** `slokit export --format openslo` on a repo
example produces YAML that `slokit validate` re-imports cleanly; the
round-trip suite is green in CI; crates.io reports `newest_version` 1.2.0.

## 4. Out of scope

- OpenSLO v2 (the import is v1; export matches it).
- Exporting to sloth's own YAML dialect (slokit specs already ARE
  sloth-compatible `prometheus/v1`).
- Any change to generated Prometheus rule bytes (frozen within a minor line
  per SEMVER.md).
- The `examples/infraportal/` live-status work (blocked on `/metrics` in the
  microservices repo; tracked there, not here).

## 5. Review — CLOSED

Maintainer review of D1 through D6 was the only gate. It **closed on
2026-08-01: approved as written**, the whole set, with no decision
overridden.

That schedules v1.2.0 at exactly the scope above, and the
[ROADMAP](../../ROADMAP.md) v1.2.0 section now carries the chosen scope and
the checkable done-when (D6's default), written in PR 1 as this section
required. PR 1 (the library export) shipped with that edit and PR 2 (the
`export` subcommand) followed; PR 3 (release prep and the cut) is next. The
[ROADMAP](../../ROADMAP.md) tracks slice status, not this document.
