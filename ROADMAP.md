# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-08-01, when the
v1.2.0 release prep landed and the OpenSLO export milestone closed.
Backward-looking detail lives in [CHANGELOG.md](CHANGELOG.md); this file
covers where the crate is going.

This file is machine-checked. `tests/roadmap_truth.rs` reads it against
`Cargo.toml` and `CHANGELOG.md` on every `cargo test` run and fails when they
drift: a current-state version that does not match the crate version, a
released version missing from the history table, a released version still
listed as an upcoming milestone or as BLOCKED, unreleased CHANGELOG entries
with no "Unreleased on main" section here, or a crate version with no CHANGELOG
entry of its own. The previous revision of this file had four of those defects
at once (it described the pre-0.8 world while the crate was at 1.0.0), which is
why the guard exists; the last check was added with the v1.1.0 prep, because a
version bump with no changelog entry passes all of the others.

The remaining release-prep half-finish, a `Cargo.lock` left at the old version,
is checked in CI instead of here, and the reason is recorded in that test file:
an assertion that reads `Cargo.lock` cannot fail, because cargo repairs a stale
lockfile while building the very test binary that would report it. The step
that does fail is `cargo metadata --locked` in `.github/workflows/ci.yml`.

Item labeling used throughout:

- **agent-doable**: can be implemented autonomously as a normal PR.
- **BLOCKED**: cannot start until the stated blocker clears.
- **USER-ONLY**: requires the maintainer (reviews, scope decisions, secret
  writes).

Releases (git tags, GitHub releases, the publishes they fire) are DELEGATED
per the 2026-07-26 user decision recorded in the autodev skill's Merges and
releases policy, which supersedes the "all releases are USER-ONLY" line that
previously stood here. What gates a cut is the checklist, not a marker:
release prep on `main`, CI green on that commit, this file or the backlog
naming the release, and the registry confirmed afterwards. The v1.1.0 cut on
2026-07-26 ran under exactly that delegation. Secret writes (`gh secret set`)
remain USER-ONLY.

## Current state: v1.2.0

slokit is a stable, published SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget, burn-rate, and
   forward-looking simulation math in services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `simulate`, `check`,
   `dashboard`, and `schema` commands behind feature flags (`cli`, `spec`,
   `check`, `dashboard`).

`Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` all say 1.2.0. Under the standing
release delegation the cut (tag `v1.2.0`, GitHub release, the publish it fires)
follows the prep commit directly, and the history row below records the date it
ran.

**This section deliberately does not claim a registry state.** The 1.1.0 prep
wrote "prepared 2026-07-26, tag not yet cut" here and that sentence was false a
few hours later, which cost a separate PR to correct. Whether crates.io has
caught up is a live fact with a one-line check, not a claim a file can keep
true:

```
curl -s -A "your-name (you@example.com)" https://crates.io/api/v1/crates/slokit \
  | tr ',' '\n' | grep newest_version
```

The public API is frozen per [docs/SEMVER.md](docs/SEMVER.md): 1.x changes are
additive only, generated rule output is byte-stable within a minor line, and
the tag-pinned JSON Schema URLs are immutable. 1.2.0 keeps all three: it adds
the `spec::openslo::export` module and the `slokit export` subcommand without
touching any 1.0.0 or 1.1.0 signature.

MSRV is 1.82 and it **is** CI-enforced: the `MSRV 1.82` job in
`.github/workflows/ci.yml` builds the default, all-features, and lean-core
configurations on a pinned 1.82 toolchain against an MSRV-compatible
resolution. CI additionally runs `fmt, clippy, test` (with `-D warnings`, a
committed-lockfile check via `cargo metadata --locked`, and a lean-core build
and test), `Security audit` (`cargo audit --deny warnings`), `promtool check
generated rules` against a pinned Prometheus release, and `coverage`.

## Next milestones

Nothing is scheduled. The v1.2.0 OpenSLO-export milestone closed with this
release prep (all three D6 slices shipped; see the history table below), and
the theme after it is named but not scoped.

D5 of [docs/design/POST_1_0_EXPANSION.md](docs/design/POST_1_0_EXPANSION.md),
approved 2026-08-01, puts **additional lint rules** next, with each individual
rule gated on citing a real-world spec where it would have fired. Turning that
theme into a milestone is a product increment: it needs the candidate rules
enumerated, each one grounded in a real spec, and a checkable done-when. Until
that lands, the candidates below stay unscheduled and no version number is
promised to any of them.

## Later / candidates (unscheduled)

Ranked with defaults in
[docs/design/POST_1_0_EXPANSION.md](docs/design/POST_1_0_EXPANSION.md)
(D1-D6 approved 2026-08-01; OpenSLO export left this list, became the v1.2.0
milestone, and shipped — see the history table). Listed here until a decision
schedules them:

- Additional lint rules surfaced by real-world specs — the approved next theme
  (D5), still unscoped, and each rule still gated on citing a real spec where
  it would have fired.
- Dashboard enhancements, for example per-severity burn panels — deferred
  until any generated dashboard has live data to render.
- Carrying `examples/infraportal/` from SLO-definitions-as-code to live status,
  which is blocked on the InfraPortal services exposing `/metrics` at all (that
  work lives in the microservices repo, not here).
- USER-ONLY: backfill missing git tags v0.5.0 and v0.6.1 through v0.6.8
  (published to crates.io without tags; opportunistic).

## Blocked and USER-ONLY summary

| Item | Status | Reason |
|------|--------|--------|
| Tag backfill for v0.5.0 and v0.6.1 through v0.6.8 (dated 2026-07-26) | USER-ONLY | predates the release delegation and stays outside it: nine historical versions with no release prep on `main` to verify a cut against (backfill publishes nothing — publish.yml fires on release publication, and those versions are already on crates.io); clears when the nine tags exist on origin or the user directs it |

Nothing is BLOCKED. Every remaining agent-doable item can start today. The
two rows that previously gated releases as USER-ONLY ("Every release cut" and
the v1.1.0 cut row) are gone: the first was superseded by the 2026-07-26
delegation (see the labeling section above), and the second's clearing
condition — the crates.io API reporting `newest_version` 1.1.0 — was met the
same day. The proposal-review row is gone for the same reason: its clearing
condition was met on 2026-08-01 when the maintainer approved D1-D6 as written,
which scheduled the v1.2.0 milestone that has since shipped. Its text is
preserved in the History section below.

For the record, every 1.x cut runs the same three commands against the prep
commit on `main`: `git tag vX.Y.Z && git push origin vX.Y.Z`, then
`gh release create vX.Y.Z --verify-tag --generate-notes`. The v1.1.0 cut ran
it on 2026-07-26 (tag at `8733a07`, release fired
`.github/workflows/publish.yml` run `30202759087`, success). Creating the **release** (not the tag) is what fires publish.yml,
which re-runs fmt, clippy and both test configurations, asserts `Cargo.toml`'s
version equals the tag, then publishes with `secrets.CRATES_IO_TOKEN`.
Publishing is irreversible, which is why the delegation's checklist — prep on
`main`, CI green on that commit, version and changelog re-read before tagging,
registry confirmed after — gates every cut.

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
| 1.1.0 | 2026-07-26 | `slokit simulate` + the public `slokit::simulate` module, `examples/infraportal/`, `cargo audit` CI gate; released and published the same day under the release delegation |
| 1.2.0 | 2026-08-01 | OpenSLO v1 export: the `spec::openslo::export` module (`to_yaml` / `to_yaml_reported`, `Export` / `ExportNote`) and the `slokit export --format openslo` subcommand, with a semantic round-trip property proven over every committed spec; proposed, approved and built the same day |

The v1.2.0 milestone as it was scoped, for the record, since the section itself
is gone from `## Next milestones` now that it shipped. Theme (D1, D4): OpenSLO
v1 export and nothing else, the inverse of the v0.10.0 import. Surface (D2):
the library functions plus a `slokit export --format openslo` subcommand.
Fidelity (D3): a semantic round trip, failing closed with an error naming the
field on any construct OpenSLO cannot represent. Slices (D6): PR 1 the library
export (`#21`), PR 2 the subcommand (`#22`), PR 3 this release prep and the
cut. Its done-when was checkable, and two of its three clauses are already
mechanical: `slokit export --format openslo` on a repo example produces YAML
that `slokit validate` re-imports cleanly (asserted at the binary level by
`tests/export_cli.rs::an_exported_example_reimports_cleanly_through_validate`)
and the round-trip suite is green in CI. The third — crates.io reporting
`newest_version` 1.2.0 — is the registry check above, which nothing in this
repo can assert on its own.

Drift worth recording:

- **2026-08-01: the current-state section stopped asserting a registry state.**
  The 1.1.0 prep had written "prepared 2026-07-26, tag not yet cut" into
  `## Current state`, which the cut falsified hours later and a separate PR had
  to correct. Prep and cut now run in one increment, which makes any "not yet
  cut" wording stale before the day is out, so that section states only what
  the repo itself can prove — the three files agreeing on the version — and
  delegates the registry claim to a one-line `curl`. The whole v1.2.0
  milestone, proposal to build, ran on this one day.
- **2026-08-01: the post-1.0 proposal was approved and v1.2.0 was scheduled.**
  The maintainer approved D1 through D6 of
  [docs/design/POST_1_0_EXPANSION.md](docs/design/POST_1_0_EXPANSION.md) as
  written, so the v1.2.0 section above changed from "proposal-gated" to the
  scoped OpenSLO-export milestone, OpenSLO export left the unscheduled
  candidate list, and the USER-ONLY review row was deleted from the summary
  table. That row read verbatim: `| Review of the post-1.0 expansion proposal
  (dated 2026-07-26) | USER-ONLY | it is a scope decision, not an
  implementation; clears when the maintainer approves or overrides D1-D6 in
  docs/design/POST_1_0_EXPANSION.md |`. Its clearing condition was met, which
  is the only reason it is gone.
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
- 1.0.0 through 1.1.0 is the first time this repo let user-facing work sit
  unpublished: `slokit simulate` merged 2026-07-23 (PR #15) and was documented
  in the README and the roadmap while `cargo install slokit` still delivered a
  binary without it. Four merged PRs accumulated behind the missing cut. The
  "Unreleased on main" section and its guard exist so that gap is at least
  visible in the roadmap; making it visible is not the same as closing it, and
  only the tag closes it. The tag closed it on 2026-07-26, the same day the
  release delegation landed — the gap existed exactly as long as releases
  were held on a USER-ONLY marker.
