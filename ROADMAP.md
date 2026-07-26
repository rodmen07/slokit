# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-07-25, after the
v1.0.0 freeze release. Backward-looking detail lives in
[CHANGELOG.md](CHANGELOG.md); this file covers where the crate is going.

This file is machine-checked. `tests/roadmap_truth.rs` reads it against
`Cargo.toml` and `CHANGELOG.md` on every `cargo test` run and fails when the
two drift: a current-state version that does not match the crate version, a
released version missing from the history table, a released version still
listed as an upcoming milestone or as BLOCKED, or unreleased CHANGELOG entries
with no "Unreleased on main" section here. The previous revision of this file
had four of those defects at once (it described the pre-0.8 world while the
crate was at 1.0.0), which is why the guard exists.

Item labeling used throughout:

- **agent-doable**: can be implemented autonomously as a normal PR.
- **BLOCKED**: cannot start until the stated blocker clears.
- **USER-ONLY**: requires the maintainer (releases, tags, publishes, reviews).

All releases (git tags, GitHub releases, `cargo publish`) are USER-ONLY.
Agents prepare release PRs; the user ships them.

## Current state: v1.0.0 (released 2026-07-19)

slokit is a stable, published SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget, burn-rate, and
   forward-looking simulation math in services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `simulate`, `check`,
   `dashboard`, and `schema` commands behind feature flags (`cli`, `spec`,
   `check`, `dashboard`).

v1.0.0 is live on crates.io (`newest_version` 1.0.0, confirmed 2026-07-25) and
the public API is frozen per [docs/SEMVER.md](docs/SEMVER.md): 1.x changes are
additive only, generated rule output is byte-stable within a minor line, and
the tag-pinned JSON Schema URLs are immutable.

MSRV is 1.82 and it **is** CI-enforced: the `MSRV 1.82` job in
`.github/workflows/ci.yml` builds the default, all-features, and lean-core
configurations on a pinned 1.82 toolchain against an MSRV-compatible
resolution. CI additionally runs `fmt, clippy, test` (with `-D warnings` plus a
lean-core build and test), `Security audit` (`cargo audit --deny warnings`),
`promtool check generated rules` against a pinned Prometheus release, and
`coverage`.

## Unreleased on main

Four commits have merged since the v1.0.0 tag (`20f3125`) and are **not yet
published**. Everything here is additive per docs/SEMVER.md, so it is a minor
release, not a patch:

| PR | Commit | Merged | What |
|----|--------|--------|------|
| #14 | `5d1086f` | 2026-07-22 | `cargo audit --deny warnings` CI gate; cleared the HIGH quinn-proto advisory |
| #15 | `96946b8` | 2026-07-23 | `slokit simulate` plus the public `slokit::simulate` module (lean core, no feature flag) |
| #16 | `dc0468a` | 2026-07-24 | `examples/infraportal/`: 16 dogfooded SLOs with byte-identity drift tests |
| #17 | `70bb1dd` | 2026-07-25 | `simulate` numeric input validation at the CLI boundary |

The user-facing consequence of leaving this unshipped: `slokit simulate` exists
on `main` and in the docs but cannot be obtained from crates.io. That is the
next milestone.

## Next milestones

### v1.1.0: publish the post-1.0 work

The first minor of the 1.x line. No new development is required; the work is
already on `main` and green.

- agent-doable: release-prep PR bumping `Cargo.toml` to 1.1.0, converting the
  CHANGELOG `## [Unreleased]` section to `## [1.1.0] - <date>`, and updating
  this file's current-state and history sections in the same commit (the drift
  guard fails the build otherwise).
- USER-ONLY: tag v1.1.0, create the GitHub release, and let the publish
  workflow run.

Done when: the crates.io API reports `newest_version` 1.1.0, and
`cargo install slokit && slokit simulate --help` succeeds from the registry
rather than from a git checkout.

### v1.2.0: post-1.0 expansion (proposal-gated)

The agent-doable path to 1.0 is complete, so what comes next is a product
decision rather than a queue. The candidates below are unranked and none is
scheduled until a design doc picks one.

- agent-doable: a post-1.0 expansion proposal design doc under `docs/design/`
  covering the candidates in the section below, each written as an overridable
  default so the whole set can be accepted in one word.
- USER-ONLY: review and merge the proposal, which schedules v1.2.0.

Done when: a design doc exists under `docs/design/`, is merged, and this file's
v1.2.0 section names the chosen scope with a checkable done-when.

## Later / candidates (unscheduled)

- OpenSLO **export** (the inverse of the v0.10.0 import, which shipped).
- Additional lint rules surfaced by real-world specs.
- Dashboard enhancements, for example per-severity burn panels.
- Carrying `examples/infraportal/` from SLO-definitions-as-code to live status,
  which is blocked on the InfraPortal services exposing `/metrics` at all (that
  work lives in the microservices repo, not here).
- USER-ONLY: backfill missing git tags v0.5.0 and v0.6.1 through v0.6.8
  (published to crates.io without tags; opportunistic).

## Blocked and USER-ONLY summary

| Item | Status | Reason |
|------|--------|--------|
| Every release cut (tags, GitHub releases, `cargo publish`) | USER-ONLY | releases are manual by policy |
| v1.1.0 tag, GitHub release, and publish | USER-ONLY | the release-prep PR is agent-doable; the cut is not |
| Review and merge of the post-1.0 expansion proposal | USER-ONLY | it is a scope decision, not an implementation |
| Tag backfill for v0.5.0 and v0.6.1 through v0.6.8 | USER-ONLY | tag creation and pushes are manual |

Nothing is BLOCKED. Every remaining agent-doable item can start today.

Not blocked by anything: the 2026-06-04 infrastructure decommission does not
affect slokit. The crate has no cloud runtime; CI and publishing run on GitHub
Actions.

## History and supersession

No in-repo roadmap existed before this file (2026-07-18); planning previously
lived only in an out-of-repo backlog. Shipped history, as it actually
happened:

| Version | Date | Highlights |
|---------|------|------------|
| 0.1.0 | 2026-06-04 | error-budget/burn-rate core, sloth-compatible spec parsing, MWMBR rule generation |
| 0.2.0 | 2026-06-04 | live `check` command against a Prometheus HTTP API |
| 0.3.0 | 2026-06-04 | latency SLI (histogram-bucket based) |
| 0.4.0 | 2026-06-06 | Grafana dashboard generation |
| 0.5.0 | 2026-06-07 | multi-spec directory loading, richer `check` output |
| 0.6.0 | 2026-06-07 | `lint` command, crates.io publish workflow |
| 0.6.1-0.6.8 | 2026-06-27 | check-hardening patch series |
| 0.7.0 | 2026-07-18 | configurable alerting (custom windows, period scaling) |
| 0.8.0 | 2026-07-19 | spec-validation hardening plus promtool CI validation of generated rules |
| 0.9.0 | 2026-07-19 | `SliPlugin` registry and the `sli.plugin` spec key, validate/lint aware |
| 0.10.0 | 2026-07-19 | OpenSLO v1 import with `--input-format` and auto-detection |
| 0.11.0 | 2026-07-19 | spec JSON Schema, the `schema` subcommand, byte-identical schema pins |
| 0.12.0 | 2026-07-19 | 1.0 freeze prep: `#[non_exhaustive]` audit, constructors, `deny(missing_docs)`, docs/SEMVER.md, MSRV 1.82 CI job |
| 1.0.0 | 2026-07-19 | API freeze; content identical to 0.12.0, guarantees documented |

Drift worth recording:

- Configurable alerting was originally planned for 0.6 but slipped; 0.6.0
  shipped the `lint` command instead, and configurable alerting landed as
  0.7.0 on 2026-07-18.
- The 0.6.1 through 0.6.8 patch series (2026-06-27) was an unplanned
  check-hardening detour driven by autonomous dev runs, not by any roadmap.
- Cadence: minors 0.1.0 through 0.6.0 shipped in one burst (2026-06-04 to
  2026-06-07), then minors paused for six weeks, then **0.8.0 through 1.0.0
  all shipped on a single day, 2026-07-19**. The "roughly one minor per week"
  sizing in the pre-1.0 revision of this file was wrong by more than an order
  of magnitude in the other direction; agent throughput, not calendar time, is
  what sizes these milestones.
- This file went stale within one day of being written. The revision dated
  2026-07-18 declared "Current state: v0.7.0", listed v0.8.0 through v1.0.0 as
  upcoming milestones, said MSRV was "not yet CI-enforced", and carried a
  BLOCKED row for v0.9.0 PR 2 gated on a design-doc review that had already
  happened. All of those were false by 2026-07-19; preflight caught only one of
  them (the BLOCKED row) seven days later. The verbatim row it caught was
  `| v0.9.0 SliPlugin implementation (PR 2) | BLOCKED | gated on user review
  and merge of the design-doc PR (PR 1, agent-doable now) |`; PR 2 shipped as
  slokit PR #6 and v0.9.0 was tagged the next day. That failure is what
  `tests/roadmap_truth.rs` now guards against.
