# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-08-07, when v1.6.0
(sloth Kubernetes CRD input) was scoped.
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

## Current state: v1.5.0

slokit is a stable, published SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget, burn-rate, and
   forward-looking simulation math in services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `simulate`, `check`,
   `dashboard`, and `schema` commands behind feature flags (`cli`, `spec`,
   `check`, `dashboard`).

`Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` all say 1.5.0. Under the standing
release delegation the cut (tag `v1.5.0`, GitHub release, the publish it fires)
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
the tag-pinned JSON Schema URLs are immutable. 1.5.0 keeps all three: it widens
what the OpenSLO importer accepts (`openslo/v1alpha` alongside `openslo/v1`),
which is the "the spec format only grows" clause of the freeze — input that used
to error now parses, no earlier 1.x signature changes, `openslo/v1` imports are
unchanged, and generated Prometheus rule output for every existing spec is
byte-identical to 1.4.0. It adds no dependency, so the lean core is untouched.

MSRV is 1.82 and it **is** CI-enforced: the `MSRV 1.82` job in
`.github/workflows/ci.yml` builds the default, all-features, and lean-core
configurations on a pinned 1.82 toolchain against an MSRV-compatible
resolution. CI additionally runs `fmt, clippy, test` (with `-D warnings`, a
committed-lockfile check via `cargo metadata --locked`, and a lean-core build
and test), `Security audit` (`cargo audit --deny warnings`), `promtool check
generated rules` against a pinned Prometheus release, and `coverage`.

## Unreleased on main

- **Fixed: `slokit dashboard` ignored `generate`'s window-resolving options**,
  so a dashboard built beside `slokit generate --period 7d` (or
  `--no-period-scaling`) queried `slo:sli_error:ratio_rate<window>` series
  those rules never record and every window-scoped panel rendered "No data".
  `dashboard` now takes the same two flags, both commands resolve through one
  seam, and `tests/dashboard_drift.rs` runs its two-source check across the
  whole option space rather than only under defaults. Found by a QA pass on
  the dashboard/generator contract, not by a user report.
- **Added: sloth Kubernetes CRD input** (v1.6.0 PR 1). A file whose first
  document declares `apiVersion: sloth.slok.dev/v1`, `kind:
  PrometheusServiceLevel` is now auto-detected and imported by
  `spec::sloth_crd` instead of dying in the native parser with `missing field
  'service'`. Kubernetes object metadata is ignored with an import note, and
  sloth's SLO plugin chains plus the native `page_alert`/`ticket_alert`
  spellings fail closed by name. `--input-format sloth-crd` and the
  binary-level tests are PR 2.

## Next milestones

### v1.6.0: sloth Kubernetes CRD input

**Theme:** teach the spec loader to read `apiVersion: sloth.slok.dev/v1`,
`kind: PrometheusServiceLevel` documents alongside the native
`version: prometheus/v1` YAML. That CRD is how sloth is used inside
Kubernetes, and slokit — a tool that already *emits* a Kubernetes CRD from
`generate --format operator` — cannot read it. Same shape of one-way door
that picked OpenSLO export for v1.2.0 and OpenSLO v1alpha import for v1.5.0.

**Grounding, read from source on 2026-08-07, not inherited:**

- `slok/sloth@main`'s `examples/` holds 21 entries and **five** are in this
  dialect (`k8s-getting-started.yml`, `k8s-home-wifi.yml`, `k8s-multifile.yml`,
  `plugin-k8s-getting-started.yml`, `slo-plugin-k8s-getting-started.yml`) —
  more than the two OpenSLO documents that justified v1.5.0.
- A binary built from the v1.5.0 tree rejects them, and the failure is worse
  than the OpenSLO one was. `slokit validate -i k8s-getting-started.yml` exits
  1 with `spec error: document 1: missing field 'service'`, on auto-detection
  *and* on explicit `--input-format slokit`: the message names neither the
  dialect nor the fact that the input is a sloth format at all. The OpenSLO
  path at least said `unsupported apiVersion 'openslo/v1alpha'`.
- Nothing in the repo supports it in any form:
  `grep -rni 'slok\.dev|PrometheusServiceLevel'` over the whole tree (excluding
  `target/` and `.git/`) returns zero hits, and `InputFormat` in
  `src/bin/slokit.rs` has exactly two variants, `Slokit` and `Openslo`.
- The field mapping was derived from sloth's own type definitions
  (`pkg/kubernetes/api/sloth/v1/types.go`, 244 lines), not from the examples.
  The CRD is the **same model** as the native spec with camelCase JSON names
  (`errorQuery`, `totalQuery`, `errorRatioQuery`, `pageAlert`, `ticketAlert`)
  wrapped in `metadata` plus `spec`. It is a rename-and-unwrap layer, not a
  second model the way OpenSLO is — which is why this milestone is small.
- **The byte-identity clause below is already proven satisfiable**, which is
  why it is the done-when and not an aspiration. Mechanically unwrapping
  `k8s-getting-started.yml` and `k8s-home-wifi.yml` into native form (strip the
  envelope, rename the camelCase keys) and running the v1.5.0 `slokit generate`
  over each produced output byte-identical to the same command over sloth's own
  native twins `getting-started.yml` and `home-wifi.yml` (8729 bytes,
  sha256 `deb721d8df254359…`; 18765 bytes, sha256 `3411e923027e2354…`). sloth
  declares those pairings itself: each `k8s-*.yml` opens with "the same example
  as `<native>.yml` but using Sloth Kubernetes CRD".

**Fidelity contract:** the split the OpenSLO importer already uses, not a new
philosophy. Envelope fields with no home in a slokit `Spec` (`metadata.name`,
`metadata.namespace`, `metadata.labels`) are **ignored with an import note**,
mirroring `spec::openslo`'s treatment of `metadata.annotations`. Constructs
that would silently generate the WRONG rules — sloth's SLO plugin chains,
`spec.sloPlugins` and `slos[].plugins`, which slokit has no equivalent for
(its `sli.plugin` is a different mechanism and *does* map to `sli.plugin.{id,
options}`) — **fail closed with an error naming the field**, the D3 rule the
export follows.

**Why not the other candidates** (each re-tested at its source this pass, see
`## Later / candidates` below for the arithmetic): both remaining lint rules
are still held, window-coverage because the only real custom-window
configurations that exist upstream both close the budget by construction and
objective-precision because no field of any supported dialect carries event
volume; and the `examples/infraportal/` live-status item's blocker lives in
another repo that this scoping run was not scoped to read.

**Slices (dependency-ordered; nothing calendar-sized):**

1. **PR 1 — the mapper.** Per-document dispatch in `spec::from_yaml` grows a
   branch for the `sloth.slok.dev/` `apiVersion` prefix beside the existing
   `openslo/` routing, and the mapping lands in a **new sibling module
   `src/spec/sloth_crd.rs`** — not in `src/spec/openslo.rs` (1226 lines) or
   `src/spec/mod.rs` (1041), both already over the 1000-line hard threshold;
   the `src/spec/openslo/v1alpha.rs` and `src/dashboard/burn.rs` precedent.
   All three CRD fixtures committed under `tests/fixtures/sloth_crd/` together
   with the two native twins, plus the fail-closed direction.
   **Corrected 2026-08-08 while implementing this slice** (the scoping pass got
   the reason right and the fact wrong): a committed guard *does* scan
   `tests/fixtures/` — `tests/schema.rs:531` `native_fixture_files()` reads
   that directory and feeds `positive_specs()`, which
   `every_native_spec_validates_against_the_schema` and
   `schema_positives_also_pass_the_rust_validator` both iterate. What saves the
   slice is that the `read_dir` is **not recursive** and filters on
   `p.is_file()`, so a `sloth_crd/` subdirectory is invisible to it, exactly as
   `tests/fixtures/openslo/` already is. The obligation is therefore still
   unschedulable-free, but only while the fixtures stay in a subdirectory:
   dropping a CRD document at the top level of `tests/fixtures/` reddens
   `schema.rs` on the commit that adds the file (control run 2026-08-08, quoted
   in the PR body).
2. **PR 2 — end-to-end proof and docs.** `--input-format` gains its third
   value (`sloth-crd`, an overridable default name), binary-level
   `validate`/`generate` tests over the committed fixtures (PR 1 asserts the
   twin byte-identity at the library level across the generate option space;
   PR 2 does it through the CLI), and the docs surface: README's input-format
   section and the `src/spec/sloth_crd.rs` module docs carrying the field
   table.
3. **PR 3 — release prep and the cut.** `CHANGELOG.md` gains a dated
   `## [1.6.0]` section folding the `[Unreleased]` dashboard fix already on
   `main`, `Cargo.toml` plus `Cargo.lock` bump, this section retires into the
   history table, then the tag, the GitHub release and the registry check.

**v1.6.0 done-when (every clause checkable; none is an existence search):**

1. `slokit validate -i <fixture>` exits **0** on all three committed CRD
   fixtures, by auto-detection AND with the explicit `--input-format` value,
   asserted at the binary level. All three exit 1 today with
   `spec error: document 1: missing field 'service'`.
2. Rules generated from the `k8s-getting-started.yml` fixture are
   **byte-for-byte identical** to rules generated from the committed native
   twin `getting-started.yml`, and likewise for the `k8s-home-wifi.yml` /
   `home-wifi.yml` pair. An implementation that parses but mis-maps passes
   clause 1 and fails this one.
3. A document carrying `spec.sloPlugins` or `slos[].plugins` errors with a
   message naming that field, and a document carrying `metadata.name`,
   `metadata.namespace` and `metadata.labels` imports cleanly while reporting
   an import note naming each ignored field.
4. Existing behaviour is unchanged: `tests/openslo.rs`, `tests/generate.rs`,
   `tests/examples_infraportal.rs` and the insta snapshots pass **unmodified**
   (`git status --porcelain` over those paths empty on the shipping PR).
5. `cargo test --no-default-features --lib` passes and
   `git status --porcelain Cargo.toml Cargo.lock` is empty on PRs 1 and 2: the
   lean core is untouched and the dialect costs no new dependency.
6. crates.io reports `newest_version` **1.6.0**.

**Semver:** additive under [docs/SEMVER.md](docs/SEMVER.md)'s "the spec format
only grows" clause, the same argument v1.5.0 ran on. Input that used to error
now parses, no existing 1.x signature changes, native and OpenSLO imports are
untouched, and generated rule bytes for every existing spec stay identical —
clause 4 is what holds that.

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
  - **objective-precision** (whether an objective's digits exceed what the
    period can measure) needs event volume, and the re-test makes that
    structural rather than circumstantial: no field of `SloSpec`
    (`src/spec/mod.rs`) carries traffic, event rate or sample count, and no
    supported input dialect has one either, so no corpus sweep can ever ground
    it. It needs a user report or a traffic-annotated grounding pass instead.
- Carrying `examples/infraportal/` from SLO-definitions-as-code to live status,
  which is blocked on the InfraPortal services exposing `/metrics` at all (that
  work lives in the microservices repo, not here). **Deliberately NOT re-tested
  on 2026-08-07**: its source of truth is that other repo, which the scoping
  run was not scoped to read, so this bullet is an inherited claim rather than
  a checked one. Re-test it before either scheduling or re-deferring it.
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
