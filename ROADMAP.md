# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-07-18, immediately
after the v0.7.0 release. Backward-looking detail lives in
[CHANGELOG.md](CHANGELOG.md); this file covers where the crate is going.

Item labeling used throughout:

- **agent-doable**: can be implemented autonomously as a normal PR.
- **BLOCKED**: cannot start until the stated blocker clears.
- **USER-ONLY**: requires the maintainer (releases, tags, publishes, reviews).

All releases (git tags, GitHub releases, `cargo publish`) are USER-ONLY,
including the eventual v1.0.0 freeze publish. Agents prepare release PRs; the
user ships them.

## Current state: v0.7.0 (released 2026-07-18)

slokit is an SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget and burn-rate math in
   services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `check`, and `dashboard`
   commands behind feature flags (`cli`, `spec`, `check`, `dashboard`).

MSRV 1.82 is declared in Cargo.toml (not yet CI-enforced). CI runs fmt, clippy
with `-D warnings`, all-features tests, and a lean-core build/test.

Recently shipped:

- **v0.7.0 (2026-07-18)**: configurable alerting. Per-SLO `alerting.windows`
  spec extension (severity/long/short/factor with validation), period-scaled
  MWMBR defaults (`MwmbrConfig::scaled`, `sre_default_for_period`),
  `generate --no-period-scaling` / `GenerateOptions::period_aware`, new
  `NO_SEVERITY_WINDOWS` lint, and `PERIOD_TOO_SHORT` now evaluates effective
  windows.
- **v0.6.1 through v0.6.8 (2026-06-27)**: check-hardening patch series.
  Non-finite Prometheus sample rejection (NaN/Inf), bearer-token auth
  coverage, HTTP error diagnostics (status plus trimmed body snippet,
  `errorType` propagation, missing/unsupported `resultType` diagnostics),
  extensive regression tests.
- **v0.6.0 (2026-06-07)**: `slokit lint` command (OBJECTIVE_100,
  OBJECTIVE_LOW, PERIOD_TOO_SHORT, NO_ALERT_LABELS, ALL_ALERTS_DISABLED,
  NO_DESCRIPTION; `--strict`, `--output json`) plus the crates.io publish
  workflow.

## Next milestones

Ordered path to 1.0. Each milestone is sized for one or two small PRs and
targets the cadence of roughly one minor version per week. Each release cut at
the end of a milestone is USER-ONLY.

### v0.8.0: Spec hardening + promtool integration

Makes generated output externally validated before the API surface grows
further.

- agent-doable: audit `src/spec/validate.rs` for gaps and tighten with tests:
  duplicate SLO names across multi-spec directories, empty or whitespace-only
  `service`/`name`/query fields, conflicting extension combinations (one small
  PR).
- agent-doable: promtool integration: validate generated rule files with
  `promtool check rules` in tests, skipping gracefully when promtool is absent
  locally; add a CI job that installs promtool and runs it against generated
  fixtures (one small PR, CI job can ride along).

Done when: validation gaps above are rejected with tests, and CI runs promtool
against generated rule fixtures on every push.

### v0.9.0: Plugin/extension system (SliPlugin)

Design-doc-first so the extension surface gets human review before it becomes
API.

- agent-doable now (PR 1): short design doc covering the `SliPlugin` registry,
  the `sli.plugin` spec key, and resolution order versus the built-in
  `events`/`raw`/`latency` SLIs. Committed alone for user review.
- USER-ONLY: review and merge the design-doc PR.
- BLOCKED until the design-doc PR merges (PR 2): implement the `SliPlugin`
  registry and `sli.plugin` spec key per the approved design, with
  validate/lint awareness and README docs.

Done when: a plugin-provided SLI can be registered and used from a spec, and
validate/lint understand `sli.plugin`.

### v0.10.0: OpenSLO import

Widens the input funnel beyond sloth specs.

- agent-doable: accept OpenSLO YAML and map it to the internal spec model (new
  `spec::openslo` module behind the existing `spec` feature).
- agent-doable: input detection or an explicit CLI flag (for example
  `--input-format openslo`) wired through
  `generate`/`validate`/`lint`/`check`/`dashboard`.
- agent-doable: fixtures plus tests documenting which OpenSLO fields map,
  which are ignored, and which error.

Done when: an OpenSLO fixture generates the same rules as its equivalent sloth
spec, and unsupported fields fail or warn with clear messages.

### v0.11.0: Spec JSON Schema

Completes the interop tranche: editor autocomplete and validation for the
sloth-compatible spec including the slokit extensions (`period`, `latency`
SLI, `alerting.windows`).

- agent-doable: author the JSON Schema for the spec format, committed in the
  repo, with a test asserting sample fixtures validate against it.
- agent-doable: expose it via a `slokit schema` subcommand or a documented
  raw-file URL, and add a README section on editor integration
  (yaml-language-server).

Done when: the schema is published in-repo, fixtures validate against it in
tests, and the README documents how to wire it into an editor.

### v0.12.0: 1.0 freeze prep

Agent-doable groundwork so the USER-ONLY v1.0.0 release is a
version-bump-and-publish, not a work item. Small breaking changes land here,
before the freeze.

- agent-doable: add `#[non_exhaustive]` to public enums and audit/finalize
  builder APIs and the public surface.
- agent-doable: docs pass: rustdoc coverage on all public items,
  `deny(missing_docs)` or an equivalent lint, README/CHANGELOG polish.
- agent-doable: CI-enforce the declared MSRV (a 1.82 job) and document the
  MSRV and semver guarantees policy.

Done when: the public API is final, fully documented, and MSRV-checked in CI,
with the semver policy written down.

### v1.0.0: API freeze and release

The stated end state: stable API with semver guarantees.

- agent-doable: prepare the 1.0.0 version-bump PR with the final CHANGELOG
  entry and any last doc tweaks.
- USER-ONLY: tag v1.0.0, create the GitHub release, and `cargo publish`.

Done when: v1.0.0 is live on crates.io with the freeze documented.

## Later / candidates (unscheduled)

- OpenSLO export (the inverse of the v0.10.0 import).
- Additional lint rules surfaced by real-world specs.
- Dashboard enhancements, for example per-severity burn panels.
- USER-ONLY: backfill missing git tags v0.5.0 and v0.6.1 through v0.6.8
  (published to crates.io without tags; opportunistic or post-1.0).

## Blocked and USER-ONLY summary

| Item | Status | Reason |
|------|--------|--------|
| Every release cut (tags, GitHub releases, `cargo publish`) | USER-ONLY | releases are manual by policy, including v1.0.0 |
| v0.9.0 SliPlugin implementation (PR 2) | BLOCKED | gated on user review and merge of the design-doc PR (PR 1, agent-doable now) |
| Tag backfill for v0.5.0 and v0.6.1 through v0.6.8 | USER-ONLY | tag creation and pushes are manual |

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

Drift worth recording:

- Configurable alerting was originally planned for 0.6 but slipped; 0.6.0
  shipped the `lint` command instead, and configurable alerting landed as
  0.7.0 on 2026-07-18.
- The 0.6.1 through 0.6.8 patch series (2026-06-27) was an unplanned
  check-hardening detour driven by autonomous dev runs, not by any roadmap.
- Cadence: minors 0.1.0 through 0.6.0 shipped in one burst (2026-06-04 to
  2026-06-07), then minor releases paused for six weeks (only the 0.6.x patch
  series on 2026-06-27 and 0.7.0 on 2026-07-18). The milestones above are
  sized to resume roughly one minor per week.
