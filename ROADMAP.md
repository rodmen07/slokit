# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-08-08, when v1.8.0
(import dialect parity) was scoped; the v1.7.0 (sloth corpus parity) release
prep had closed the previous milestone earlier the same day, and the v1.6.0
(sloth Kubernetes CRD input) prep the one before it. Backward-looking detail
lives in [CHANGELOG.md](CHANGELOG.md); this file covers where the crate is
going.

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

## Current state: v1.7.0

slokit is a stable, published SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget, burn-rate, and
   forward-looking simulation math in services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `simulate`, `check`,
   `dashboard`, and `schema` commands behind feature flags (`cli`, `spec`,
   `check`, `dashboard`).

`Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` all say 1.7.0. Under the standing
release delegation the cut (tag `v1.7.0`, GitHub release, the publish it fires)
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
the tag-pinned JSON Schema URLs are immutable. 1.7.0 keeps all three, on the
same "the spec format only grows" clause v1.5.0 and v1.6.0 ran on: unquoted
YAML scalars in `labels` and `annotations` used to be a hard parse error and
now coerce, `kind: AlertWindows` documents used to die in the CRD importer and
now import, no earlier 1.x signature changes, and generated Prometheus rule
output for every existing spec is byte-identical to 1.6.0 — including through
the new catalogue path, where a 30-day catalogue carrying sloth's own defaults
generates exactly what no catalogue generates. The two additions to the
library surface are additive in the same sense: `GenerateOptions` gains an
`alert_windows` field (the type is `#[non_exhaustive]` and built through
`Default`), and `spec::alert_windows` is a new sibling module. The new
`SLO_PLUGIN_CHAIN_DROPPED` lint reports a construct that was already being
dropped; it changes no document's generated output, which is why the plugin
chain is linted rather than refused. It adds no dependency, so the lean core is
untouched.

MSRV is 1.82 and it **is** CI-enforced: the `MSRV 1.82` job in
`.github/workflows/ci.yml` builds the default, all-features, and lean-core
configurations on a pinned 1.82 toolchain against an MSRV-compatible
resolution. CI additionally runs `fmt, clippy, test` (with `-D warnings`, a
committed-lockfile check via `cargo metadata --locked`, and a lean-core build
and test), `Security audit` (`cargo audit --deny warnings`), `promtool check
generated rules` against a pinned Prometheus release, and `coverage`.

## Unreleased on main

Merged since v1.7.0 and not yet released. Kept in step with
[CHANGELOG.md](CHANGELOG.md)'s `[Unreleased]` section by
`tests/roadmap_truth.rs::roadmap_declares_unreleased_work_exactly_when_the_changelog_has_some`.

- **v1.8.0 PR 1 — the CRD stops refusing what the native route lints.**
  `src/spec/sloth_crd.rs` captures `spec.sloPlugins` and `slos[].plugins` into
  `Spec::slo_plugins` / `SloSpec::plugins`, so `SLO_PLUGIN_CHAIN_DROPPED`
  reports them and the two CRD corpus rows move `Refused` → `Accepted`; corpus
  refusals 4 → 2, asserted by
  `tests/sloth_corpus.rs::exactly_two_upstream_documents_are_still_refused`.
  Done-when clauses 1 and 2 hold: clause 2's byte-identity claim was re-derived
  **on this dialect** rather than inherited from the v1.7.0 native probe, and
  it held (8729 bytes prometheus / 9337 operator, `cmp` clean).
  **PR 2 (`tests/dialect_parity.rs` plus the OpenSLO `v1`
  `metadata.displayName` note) and PR 3 (release prep and the cut) are what
  remain before v1.8.0 ships.**

## Next milestones

### v1.8.0 — import dialect parity

slokit reads four input dialects today, each added in a different milestone:
native slokit specs since 0.1.0, OpenSLO `v1` in 0.10.0, OpenSLO `v1alpha` in
1.5.0, the sloth Kubernetes CRD in 1.6.0, plus `kind: AlertWindows` catalogues
in 1.7.0. Every one of them was verified against sloth or against a native
twin. **Nothing has ever checked that they agree with each other**, and the
v1.7.0 corpus census — re-run to scope this milestone — found two constructs
where the answer slokit gives depends on which dialect the document arrived
in. In one of them the two shipped surfaces state opposite reasons for it, and
one of those reasons is measurably false.

**Grounding, run 2026-08-08 before this section was written.** The census
needed no refresh: `slok/sloth@main` is
`8a3be4fab79defa4448d09d91b48422615980b05`, the same commit
`tests/sloth_corpus.rs:51` pins, so no upstream document has appeared since the
corpus was committed; and `cargo test --all-features --test sloth_corpus` is
8/8 green, so all 20 recorded dispositions still hold. Those 20 are 16 accepted
(3 lossy, reporting `SLO_PLUGIN_CHAIN_DROPPED`) and 4 refused — and the four
refusals are not one class:

1. **A sloth SLO plugin chain is a hard error in the CRD dialect and a warning
   in the native one.** `slokit lint -i
   tests/fixtures/sloth_corpus/slo-plugin-getting-started.yml` exits 0 with two
   `WARN … SLO_PLUGIN_CHAIN_DROPPED` rows; the CRD twin
   `slo-plugin-k8s-getting-started.yml` (and `contrib-denominator-corrected.yaml`)
   dies in the importer at `src/spec/sloth_crd.rs:199` and `:268`. The refusal
   text argues the chain "would rewrite the generated rules, so it is refused
   rather than dropped". Against slokit's own generator that is **false**, and
   one command shows it: generating from the native document with the chain and
   from the same document with `slo_plugins` and `plugins` deleted produced
   **byte-identical** output, 8729 bytes each, `cmp` clean. The v1.6.0 refusal
   and the v1.7.0 lint were each right about a different referent — sloth's
   output versus slokit's — and shipped as a contradiction because no test
   compares dialects.
2. **`metadata.displayName` is noted on OpenSLO `v1alpha` import and dropped in
   silence on `v1`.** The shared envelope parses it either way
   (`src/spec/openslo.rs:322`), but only `src/spec/openslo/v1alpha.rs:128`
   mentions it. Probed at the binary level on two documents differing in
   nothing but `apiVersion`: the v1alpha run printed `metadata.displayName does
   not map and was ignored`, the v1 run printed no note at all. This is the
   open follow-up filed 2026-08-07 and it belongs to this theme rather than to
   a loose one-line fix. **Settled by PR 2** (2026-08-08): the `v1` path emits
   the same note, from the same string constant the contract asserts both
   routes against.

The other two refusals are **deliberate and stay refusals**:
`plugin-getting-started.yml` names an SLI plugin id (`getting_started_availability`)
that is user-supplied Go code upstream, and slokit's `SliPlugin` registry is a
closed set by the 0.9.0 design — refusing an unknown id is the fail-closed
behaviour, not a gap. `plugin-k8s-getting-started.yml` writes the native
`page_alert` spelling inside a CRD document; slokit's message names it as
sloth's own bug and refuses rather than silently dropping that severity's
labels, which is exactly what v1.6.0 chose.

**Slices, dependency-ordered.**

- **PR 1 (agent-doable): the CRD stops refusing what the native route lints.**
  `src/spec/sloth_crd.rs` captures `spec.sloPlugins` and `slos[].plugins` into
  the same fields the native parser fills, so the existing
  `SLO_PLUGIN_CHAIN_DROPPED` lint reports them, and the two corpus rows move
  `Refused` → `Accepted` with lint codes in `tests/sloth_corpus.rs`. The
  byte-identity claim is **proven for the CRD route, not inherited** from the
  native probe above: if generating from a CRD document with its chain is not
  byte-identical to generating from the same document with the chain removed,
  PR 1's premise is wrong and this milestone re-scopes instead of shipping. The
  false half of the old refusal text is deleted rather than reworded.
- **PR 2 (agent-doable): the parity contract, and the OpenSLO `v1` note.**
  A committed `tests/dialect_parity.rs` running the shipped binary over one
  matched document per dialect for every construct more than one importer can
  receive, recording the disposition each gives; plus the `metadata.displayName`
  note on the `v1` path, which closes the 2026-08-07 follow-up. The contract is
  the durable half — the analogue of what `tests/sloth_corpus.rs` did for
  upstream compatibility — because it turns the next asymmetry into a named
  test failure rather than a census someone has to think to run.

  **Shipped 2026-08-08**, and it earned its keep on the first run: five
  constructs, sixteen fixtures, and a **third** divergence nobody had looked
  for. `metadata.annotations` is reported as dropped on `openslo/v1` and
  dropped in silence on `openslo/v1alpha` and on the CRD route — the mirror
  image of the display-name case, from the same shared `Metadata` envelope.
  It is filed as a LOW bug and pinned by
  `known_gap_object_annotations_are_noted_on_v1_only` rather than fixed here:
  this slice was scoped to the display name, and a message-quality gap that
  reaches no generated rule does not justify widening it. Also recorded, not
  removed: the sloth CRD is the only dialect with no period field, so
  `--period` moves its rules and slides off the three documents that pin their
  own.
- **PR 3 (agent-doable): release prep and the cut**, under the standing release
  delegation and its checklist.

**Done when**, every clause checkable by build, test, CI or the registry:

1. `tests/sloth_corpus.rs` records `Accepted` with
   `lint_codes: &["SLO_PLUGIN_CHAIN_DROPPED"]` for both
   `contrib-denominator-corrected.yaml` and `slo-plugin-k8s-getting-started.yml`,
   and `cargo test --all-features --test sloth_corpus` is green: corpus
   refusals fall 4 → 2, and the two that remain are the SLI-plugin pair.
2. A committed test asserts, at the binary level, that a CRD document carrying
   a plugin chain generates byte-identical rules to the same document with the
   chain removed — the claim PR 1 rests on, run rather than assumed.
3. `tests/dialect_parity.rs` fails by name when one dialect's disposition for a
   covered construct is changed and the others are left alone; the perturbation
   and the failing run are both quoted in PR 2's body.
4. The OpenSLO `v1` route emits the `metadata.displayName` note: the same
   document that printed none prints it, and deleting the note reddens the new
   test.
5. `cargo test --all-features` is green with every existing byte-identity and
   OpenSLO test file **unchanged** except the two corpus rows in clause 1, so
   no document that generates rules today generates different ones — the
   additive-only clause of [docs/SEMVER.md](docs/SEMVER.md).
6. `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` all read 1.8.0,
   `## Current state:` here reads v1.8.0 with a history row to match,
   `tests/roadmap_truth.rs` is green, all five required contexts are green on
   the prep commit, and crates.io `newest_version` reads 1.8.0 afterwards.

**Decisions this milestone takes, both overridable defaults (USER-ONLY to
change, agent-doable as written).**

- **D1.8-1: unify toward accept-and-lint, not toward fail-closed.** The
  symmetric alternative — making the native route refuse a plugin chain the way
  the CRD does — would turn three documents that generate rules today into hard
  errors, which the 1.x additive-only guarantee forbids. So within 1.x there is
  exactly one legal direction, and the real choice is whether to take it now or
  hold uniform fail-closed for a hypothetical 2.0. Default: take it now; the
  construct is already dropped on the native route and the lint already says so.
- **D1.8-2: the SLI-plugin refusals stay.** They are the two remaining corpus
  refusals and this milestone does not touch them, for the reasons above.
  Default: leave both fail-closed and let the corpus record 18/20 accepted
  rather than chase 20/20 by weakening a gate.

## Later / candidates (unscheduled)

Ranked with defaults in
[docs/design/POST_1_0_EXPANSION.md](docs/design/POST_1_0_EXPANSION.md)
(D1-D6 approved 2026-08-01; OpenSLO export left this list, became the v1.2.0
milestone, and shipped — see the history table). Listed here until a decision
schedules them:

- Additional lint rules beyond v1.3.0, both evaluated during v1.3.0's grounding
  pass and held rather than dropped. **Both were re-tested at the source on
  2026-08-07 rather than inherited, and both stay held — now for sharper
  reasons than "no spec exhibits it yet":**
  - **window-coverage** (the minimum factor across enabled alert conditions is
    above 1, so a sustained burn rate between 1 and that minimum exhausts the
    whole budget without any alert firing). It fired on zero of the 19 real
    specs the v1.3.0 pass ran over, so the re-test asked where a real
    custom-window configuration could come from at all. Upstream the answer is
    `slok/sloth@main`'s `examples/windows/` — `7d.yaml` and `custom-30d.yaml`,
    both `kind: AlertWindows` documents, a global catalogue rather than the
    per-SLO `alerting.windows` override slokit takes — and **neither exhibits
    the gap**. With factor = (errorBudgetPercent / 100) x (period /
    longWindow), `custom-30d.yaml`'s four conditions come out 14.4 / 4.8 / 3.0
    / **1.0** and `7d.yaml`'s 13.44 / 3.5 / 1.4 / **0.98**, so both close the
    budget by construction, exactly as the SRE defaults do (14.4 / 6 / 3 / 1).
    The rule would fire on nothing that exists. Clears when a real spec or
    window set whose minimum factor is above 1 turns up — a user report, or an
    upstream catalogue that is not one of those two.
    **Re-tested again 2026-08-08 by the v1.7.0 scoping census, at the source
    rather than by re-reading this bullet: both catalogues were re-fetched and
    both sets of factors recomputed from the documents themselves (13.44 / 3.5 /
    1.4 / 0.98 and 14.4 / 4.8 / 3.0 / 1.0). The hold stands, and for the first
    time it has a route out.** What starved this rule is that slokit could not
    *accept* a window catalogue at all, so the only custom-window configurations
    it could ever see were per-SLO `alerting.windows` overrides someone had
    already hand-written into a slokit spec. v1.7.0 made `kind: AlertWindows`
    importable (shipped as PR #47), which turns a user-supplied catalogue into
    an input this rule can be grounded on. Still held today, because teaching
    slokit a document kind does not conjure a document that exhibits the gap.
  - **objective-precision** (whether an objective's digits exceed what the
    period can measure) needs event volume, and the re-test makes that
    structural rather than circumstantial: no field of `SloSpec`
    (`src/spec/mod.rs`) carries traffic, event rate or sample count, and no
    supported input dialect has one either, so no corpus sweep can ever ground
    it. It needs a user report or a traffic-annotated grounding pass instead.
    **Re-tested again 2026-08-08 by the v1.7.0 scoping census, and the structural
    claim is now stated with its search rather than asserted:** `SloSpec` carries
    exactly `name`, `objective`, `description`, `labels`, `sli`, `alerting` and
    `period` and nothing else, and a case-insensitive sweep of `src/` for
    `traffic|volume|event_rate|sample_count|requests_per|rps|throughput` returns
    hits in one file only, `src/bin/slokit.rs:337-340` and `:701-703`, `:752-756`
    and `:818-820` — the `--traffic` flag on `calc` and `simulate`, an
    operator-supplied rate at call time that no spec dialect carries. **That
    sharpens the disposition rather than repeating it: this was never a lint
    candidate.** `lint` sees only a spec, and no spec has the input. If the check
    is ever built it belongs to `calc`, where the traffic number already arrives.
    Held, and re-filed against the right command.
- Carrying `examples/infraportal/` from SLO-definitions-as-code to live status,
  which is blocked on the InfraPortal services exposing `/metrics` at all (that
  work lives in the microservices repo, not here). **Deliberately NOT re-tested
  on 2026-08-07**: its source of truth is that other repo, which the scoping
  run was not scoped to read, so this bullet is an inherited claim rather than
  a checked one. Re-test it before either scheduling or re-deferring it.
  **Still NOT re-tested on 2026-08-08, and the reason is now named precisely
  rather than left as "was not scoped to read".** The v1.7.0 scoping ran as one
  lane of a parallel wave, where the partition across repos *is* the safety
  property; the microservices repo belongs to a different lane and this worker
  may not read it. That is a boundary on the READ, not evidence about the claim,
  so the claim stays unchecked rather than quietly re-confirmed. **Clearing
  condition (2026-08-08): one `curl` against any deployed InfraPortal service's
  `/metrics` returning a Prometheus exposition body that contains
  `http_request_duration_seconds_bucket`** — runnable by a sequential
  `/autodev` run, by the portfolio lane, or by the maintainer, and settling the
  bullet either way in a single command.
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
| 1.3.0 | 2026-08-05 | Lint rules grounded in real-world specs: `Spec::from_yaml_stream` (multi-document input, the enabler) plus the `SLI_FALLBACK_ASYMMETRY` rule (grounded on sloth's `home-wifi.yml`); the same grounding pass wild-validated the shipped rule set for the first time (`THRESHOLD_UNREACHABLE` on `victoria-metrics.yml`, `ALL_ALERTS_DISABLED` on `no-alerts.yml`) |
| 1.4.0 | 2026-08-07 | Per-severity dashboard burn panels: one burn-rate timeseries panel per enabled alert condition (long and short lookbacks, threshold line at the condition's factor, disabled severities skipped), expression drift against the generator's recordings guarded by `tests/dashboard_drift.rs`; plus fail-closed `generate --format operator` naming and explicit least-privilege workflow token permissions |
| 1.5.0 | 2026-08-07 | OpenSLO `v1alpha` import: per-document `apiVersion` dispatch plus the v1alpha mapping in a new sibling `src/spec/openslo/v1alpha.rs` (per-objective `ratioMetrics`, `timeWindows[].{count, unit}`, document-level `thresholdMetric`), reusing the version-independent v1 machinery; both sloth OpenSLO examples committed as fixtures, byte-identity against a hand-written native twin asserted at the binary level, and the v1alpha → `export --format openslo` → re-import round trip proven |
| 1.6.0 | 2026-08-08 | sloth **Kubernetes CRD** input (`apiVersion: sloth.slok.dev/v1`, `kind: PrometheusServiceLevel`): auto-detected and importable through the new sibling `src/spec/sloth_crd.rs`, pinnable with `--input-format sloth-crd`, fail-closed on sloth's SLO plugin chains (**superseded in 1.8.0**: captured and reported by `SLO_PLUGIN_CHAIN_DROPPED` instead, once the refusal's stated reason was measured and found false) and on the native `page_alert`/`ticket_alert` spellings inside a CRD document (still a refusal); byte-identity against sloth's own native twins asserted at the binary level across the whole `generate` option space. Plus the dashboard/generator option fix (`dashboard --period` / `--no-period-scaling`, one shared window-resolving seam) that had every window-scoped panel rendering "No data" beside non-default rules since 1.0.0 |
| 1.7.0 | 2026-08-08 | sloth **corpus parity**: all 20 documents of `slok/sloth@8a3be4f`'s `examples/` tree committed under `tests/fixtures/sloth_corpus/` with their upstream sha256 and exact disposition pinned by `tests/sloth_corpus.rs`, so a compatibility change is a named test failure rather than a discovery in the next release; plus the three defects that corpus exposed — unquoted YAML scalars in `labels` and `annotations` now coerce at every level (which is what makes upstream's `victoria-metrics.yml` readable, with the JSON Schema's `labelMap` widened in the same commit), `kind: AlertWindows` catalogue input via `--alert-windows <path>` on `generate` and `dashboard` plus the additive `GenerateOptions::alert_windows` and the new `spec::alert_windows` module, and the `SLO_PLUGIN_CHAIN_DROPPED` lint that makes sloth's silently discarded SLO plugin chains visible on the native route without changing what any document generates |

The v1.7.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme: slokit
had claimed sloth `prometheus/v1` compatibility since 0.1.0 and widened it one
dialect at a time, but no run had ever put the **whole** upstream corpus to a
shipped binary at once, so every widening was grounded on whichever handful of
documents happened to fail. v1.7.0 did that, pinned every answer as a committed
contract, and closed the three places where the answer was wrong. The grounding
was a census run from source on 2026-08-08 against a binary built from
`ac0434f`: `slok/sloth@main`'s `examples/` holds 21 entries, 18 documents plus
three directories, and `examples/windows/` holds two more — 20 documents, of
which 13 were accepted (two of them **with content silently discarded**), 4
refused correctly, 1 rejected that should not have been
(`victoria-metrics.yml`), and 2 of an unsupported document kind
(`windows/7d.yaml`, `windows/custom-30d.yaml`). Defect 1: nine unquoted label
scalars across three scalar types made a document sloth itself generates rules
from unreadable, because slokit types every label map as
`BTreeMap<String, String>` where sloth's decode coerces. Defect 2: `kind:
AlertWindows` was unreadable, both catalogues dying inside the CRD importer on
`apiVersion`-only auto-detection, with the mapping
`factor = (errorBudgetPercent / 100) x (sloPeriod / longWindow)` being
arithmetic slokit already owned. Defect 3: a sloth SLO plugin chain was refused
by name in the CRD dialect and silently dropped on the native route — proven
by byte-identity, sha256
`0e66157f2ff7f8d43ea1ba20da9b7e4ba98d7c83ba534a1003f06fc282a71451` with and
without a seven-entry chain — and reported by a lint rule rather than refused,
because [docs/SEMVER.md](docs/SEMVER.md) line 58 promises that YAML which
parses under 1.a parses under 1.b. **One clause of the scope was corrected
while it was implemented, not after** (PR #46): the census recorded TWO native
documents carrying a plugin chain and there are **three** —
`victoria-metrics.yml` carries a four-entry chain as well, which the census
could not see because that document was refused at parse time over defect 1, so
its body was never modelled. Fixing an intake defect can enlarge a census
rather than just close a row, and the third document was found by the new
guard's NEGATIVE direction rather than by re-reading the file. Slices
(dependency-ordered): PR 1 the corpus contract plus defects 1 and 3 (`#46`),
PR 2 `kind: AlertWindows` catalogue input (`#47`), PR 3 this release prep and
the cut. Five of its six done-when clauses are mechanical and green in CI —
`tests/sloth_corpus.rs` covers all 20 documents with the fixture-hash and
zero-discovery assertions live, `victoria-metrics.yml` validates and all nine
previously-rejected label values reach the generated rules,
`SLO_PLUGIN_CHAIN_DROPPED` fires on exactly the three documents that carry a
chain and on none of the eight `examples/infraportal/slos/` specs, a 30-day
catalogue of sloth's own defaults generates byte-identical rules to no
catalogue at all with both upstream catalogues importing at the computed
factors, and `tests/generate.rs` / `tests/examples_infraportal.rs` /
`tests/openslo.rs` / `tests/sloth_crd*.rs` plus the insta snapshots pass
unmodified with no new dependency. The sixth — crates.io reporting
`newest_version` 1.7.0 — is the registry check above, which nothing in this
repo can assert on its own.

The v1.6.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme: teach the
spec loader to read `apiVersion: sloth.slok.dev/v1`, `kind:
PrometheusServiceLevel` documents alongside the native `version: prometheus/v1`
YAML — the same one-way-door shape that picked OpenSLO export for v1.2.0 and
OpenSLO v1alpha import for v1.5.0. The grounding was read from source on
2026-08-07 rather than inherited: `slok/sloth@main`'s `examples/` holds 21
entries and **five** are in this dialect (more than the two OpenSLO documents
that justified v1.5.0), a binary built from the v1.5.0 tree rejected them with
`spec error: document 1: missing field 'service'` on auto-detection *and* on
explicit `--input-format slokit`, `grep -rni 'slok\.dev|PrometheusServiceLevel'`
over the whole tree returned zero hits, and the field mapping was derived from
sloth's own `pkg/kubernetes/api/sloth/v1/types.go` rather than from the
examples. Fidelity contract: envelope fields with no home in a slokit `Spec`
(`metadata.name`, `.namespace`, `.labels`) ignored with an import note; sloth's
SLO plugin chains (`spec.sloPlugins`, `slos[].plugins`) fail closed naming the
field — **superseded in 1.8.0**, which captures both keys and reports
`SLO_PLUGIN_CHAIN_DROPPED` instead; the refusal's stated reason ("it would
rewrite the generated rules") was measured on the CRD route and is false of
slokit's generator, which has no plugin-chain stage. **One clause of the scope was corrected while it was implemented, not
after** (PR #40): the plan claimed no committed guard scans `tests/fixtures/`,
and `tests/schema.rs:531` `native_fixture_files()` does — what actually saves
the slice is that its `read_dir` is non-recursive and filters on
`p.is_file()`, so a `sloth_crd/` subdirectory is invisible to it. **And one
hazard was sharpened** (also PR #40): sloth's own
`plugin-k8s-getting-started.yml` writes `page_alert:`/`ticket_alert:` in a
`kind: PrometheusServiceLevel` document, where the CRD's Go tags say
`pageAlert`/`ticketAlert` — a *faithful* camelCase-only mapper does not error
on that, it imports it and silently drops both severities' routing labels, so
those two spellings are refused by name rather than merely documented. Slices
(dependency-ordered): PR 1 the mapper (`#40`), PR 2 `--input-format sloth-crd`
plus the end-to-end proof and docs (`#41`), PR 3 this release prep and the cut.
Five of its six done-when clauses are mechanical and green in CI — `validate`
exits 0 on all three committed CRD fixtures by auto-detection and with the
explicit format
(`tests/sloth_crd_cli.rs::every_committed_fixture_validates_by_auto_detection`
and `::every_committed_fixture_validates_with_the_dialect_pinned`), the
generated rules are byte-identical to sloth's native twins across the
`--period` / `--no-period-scaling` option space
(`::the_getting_started_fixture_generates_its_native_twins_bytes` and
`::the_home_wifi_fixture_generates_its_native_twins_bytes`, both routed through
`assert_cli_twins_agree` over `option_matrix()`, itself guarded against
collapse by `::the_option_matrix_is_not_decorative`), the fail-closed and
import-note directions are asserted on the message path,
`tests/openslo.rs` / `tests/generate.rs` / `tests/examples_infraportal.rs` and
the insta snapshots pass unmodified, and `cargo test --no-default-features
--lib` still passes with no new dependency. The sixth — crates.io reporting
`newest_version` 1.6.0 — is the registry check above, which nothing in this
repo can assert on its own.

The v1.5.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme: teach the
OpenSLO importer to read `apiVersion: openslo/v1alpha` alongside `openslo/v1`,
because the OpenSLO corpus the ecosystem actually publishes is in the older
dialect. The grounding was read from source on 2026-08-07 rather than
inherited: `slok/sloth@main`'s `examples/` holds exactly two OpenSLO documents
(`openslo-getting-started.yml`, `openslo-kubernetes-apiserver.yml`) and **both**
declare `openslo/v1alpha`, and a binary built from the v1.4.0 tree rejected both
with `spec error: openslo document 1: unsupported apiVersion 'openslo/v1alpha'
(expected openslo/v1)` — so the whole OpenSLO interop story, import since 0.10.0
and export since 1.2.0, could not read the reference corpus of the project
slokit is spec-compatible with. Fidelity contract: fail closed, the same D3 rule
the export follows. Slices (dependency-ordered): PR 1 the mapper (`#35`), PR 2
the end-to-end proof and docs (`#36`), PR 3 this release prep and the cut. Five
of its six done-when clauses are mechanical and green in CI — `validate` exits 0
on both fixtures
(`tests/openslo_v1alpha_cli.rs::validate_accepts_both_sloth_v1alpha_fixtures_by_auto_detection`
and its `_with_an_explicit_format` sibling), the generated rules are
byte-identical to the hand-written native twin
(`generating_from_the_sloth_getting_started_matches_its_native_twin_byte_for_byte`),
`tests/openslo.rs` / `tests/openslo_export.rs` / the insta snapshots pass with
no v1 behaviour change, the fail-closed direction is asserted on the message
path, and `cargo test --no-default-features --lib` still passes with no new
dependency. The sixth — crates.io reporting `newest_version` 1.5.0 — is the
registry check above, which nothing in this repo can assert on its own.

The v1.4.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme (D5,
approved 2026-08-01, scheduled 2026-08-07 after the deferral premise was
re-tested and its conclusion overturned in the open): per-severity dashboard
burn panels — for each enabled alert window of each SLO, a burn-rate
timeseries panel plotting the long and short lookback burn rates
(`slo:sli_error:ratio_rate<w>` divided by `slo:error_budget:ratio`, the
generator's own `GROUPING` idiom) with a threshold line at the window's
factor, titled by severity; disabled severities get no panel, mirroring alert
generation. Values are burn-rate multiples so the threshold lines are the
plain SRE-table factors (the raw-error-ratio alternative was the recorded
flagged default). Slices: PR 1 the panels plus the expression drift guard
(`#31`, shipped as the `src/dashboard/burn.rs` submodule per the ~400-line
ceiling), PR 2 this release prep and the cut, folding the QA fail-closed
fixes (`#29`) and the workflow-permissions hardening (`#28`) staged since
1.3.0. Its done-when was checkable and the first three clauses are asserted
by `cargo test` (`src/dashboard/burn.rs` unit tests both directions,
`tests/dashboard_drift.rs` reading both real artifacts, the byte-identity
snapshot suite unchanged); the fourth — crates.io reporting `newest_version`
1.4.0 — is the registry check above, which nothing in this repo can assert on
its own.

The v1.3.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme (D5,
approved 2026-08-01): lint rules grounded in real-world specs, each one
citing a spec where it fires. The grounding pass ran the released v1.2.0
linter over 19 real specs (the 8 committed `examples/infraportal/slos/`,
`tests/fixtures/sample.yaml`, and 10 upstream `slok/sloth@main` examples) and
scheduled two dependency-ordered slices: PR 1 multi-document spec input
(`#25`, the enabler — sloth's own `multifile.yml` failed to load at all, and
slokit's own `export` wrote exactly that stream shape), PR 2 the
`SLI_FALLBACK_ASYMMETRY` rule (`#26`, grounded on sloth's `home-wifi.yml`,
where both SLOs guard `error_query` but leave `total_query` bare), and PR 3
this release prep and the cut. Dispositioned rather than scheduled, from the
same pass: `PLUGIN_UNKNOWN_OPTION` was found already shipped (v0.9.0) rather
than a new candidate; a missing `alerting.name` was rejected because
generation falls back to the SLO name; sloth's two openslo/v1alpha examples
were noted as a possible future import widening, not a lint rule.
Window-coverage and objective-precision were evaluated and held rather than
scheduled — see `## Later / candidates` above. Its done-when was checkable
and all three clauses are now met: both new capabilities are asserted by
`cargo test` against committed fixtures derived from the cited real specs
(`tests/multidoc_input.rs`, `tests/lint_fallback_asymmetry.rs`), the 8
`examples/infraportal/slos/` specs plus `tests/fixtures/sample.yaml` stay
lint-clean, and crates.io reporting `newest_version` 1.3.0 is the registry
check above.

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

- **2026-08-08: the fourth consecutive milestone came from outside the candidate
  list, and the practice that keeps finding them is written down here so the
  next scoping pass inherits the method instead of rediscovering it.** v1.4.0
  came off the candidate list; v1.5.0, v1.6.0 and now v1.7.0 did not. All three
  came from the same move: **run the shipped binary over the upstream corpus
  first, then read this file.** The candidate list is a list of ideas someone
  had, and ideas do not go stale in a way anyone notices; a census is a
  measurement, and it goes stale the moment upstream commits. v1.7.0 is the
  clearest case yet — the census found a document sloth generates from that
  slokit cannot parse at all (`victoria-metrics.yml`), a whole document kind
  slokit rejects (`kind: AlertWindows`), and a construct refused in one dialect
  while silently dropped in another, and **not one of the three was on the
  candidate list or in any backlog**. So this milestone also turns the census
  itself into a committed guard, which is the part that stops the next four
  scoping passes from having to run it by hand: after PR 1, the corpus is a test
  that fails when upstream and slokit disagree, and a scoping pass reads a
  failing test instead of re-deriving a table. The candidate list is not
  retired — it holds two lint rules and a cross-repo item, all three re-tested
  above — but it has now failed to produce a theme four times running, and the
  next pass should read it *second*.

- **2026-08-07: sloth's own Kubernetes CRD became the v1.6.0 milestone, and it
  was never on the candidate list at all.** Like the v1.5.0 theme before it, it
  came from re-reading the upstream corpus rather than from this file: five of
  `slok/sloth@main`'s 21 `examples/` entries are `kind: PrometheusServiceLevel`
  documents that slokit rejects with `missing field 'service'`, a message that
  does not even name the dialect. The candidate list itself was re-tested in
  the same pass and produced no schedulable item — both held lint rules stayed
  held with stronger evidence than they were held on (see the arithmetic under
  `## Later / candidates`), and the `examples/infraportal/` item's blocker is
  in another repo. Two consecutive milestones now sourced from outside the
  candidate list is itself the signal: that list has stopped being where the
  next theme comes from, and a scoping pass should read the ecosystem before
  reading it. `docs/design/POST_1_0_EXPANSION.md`'s D5 ordering remains
  exhausted (lint rules shipped as v1.3.0, dashboard panels as v1.4.0).
- **2026-08-07: OpenSLO v1alpha import became the v1.5.0 milestone, promoted
  from a one-line aside rather than from the candidate list.** It was never a
  `## Later / candidates` bullet; it existed only inside the v1.3.0 milestone
  record above, as `sloth's two openslo/v1alpha examples were noted as a
  possible future import widening, not a lint rule`. The scoping pass after
  the v1.4.0 cut re-verified that aside at the source instead of citing it
  (both sloth examples fetched, both confirmed `openslo/v1alpha`, and the
  rejection reproduced against a locally built v1.4.0 binary) and scheduled
  it, because the two remaining lint-rule candidates are each held on input
  that has not arrived and the `examples/infraportal/` item is blocked outside
  this repo. `docs/design/POST_1_0_EXPANSION.md`'s D5 ordering (lint rules,
  then dashboard panels) is now **exhausted**: both shipped, as v1.3.0 and
  v1.4.0, so this theme is grounded from source rather than inherited from
  that document.
- **2026-08-07: per-severity dashboard burn panels left the candidate list
  and became the v1.4.0 milestone.** The deleted candidate bullet read
  verbatim: `- Dashboard enhancements, for example per-severity burn panels
  — deferred until any generated dashboard has live data to render.` Its
  deferral premise was re-tested rather than inherited and is dispositioned
  in the v1.4.0 section: the premise (no live data in this repo) still
  holds, but the conclusion drawn from it did not survive re-examination,
  and the scheduling is flagged as an overridable default.
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
