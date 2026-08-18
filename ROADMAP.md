# slokit Roadmap

Canonical planning document for slokit. Last updated 2026-08-10, by the v1.9.0
(check parity with the generated rules) release prep; the milestone was scoped
on 2026-08-09 (PR #59) and built as two slices, the window seam (PR #60) and
the registry seam (PR #61). Backward-looking detail lives in
[CHANGELOG.md](CHANGELOG.md); this file covers where the crate is going.

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
that does fail is `cargo metadata --locked`, which runs in BOTH
`.github/workflows/ci.yml` and `.github/workflows/publish.yml` — the release
path repairs a stale lockfile just as silently as a PR would, except that
there the repaired file is packaged into the `.crate` and shipped.
`tests/release_lockfile.rs` holds every release-path cargo step to `--locked`.

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

## Current state: v1.9.0

slokit is a stable, published SLO and error-budget engine with two pillars:

1. A dependency-light core math library (thiserror only, builds with
   `--no-default-features`) for embedding error-budget, burn-rate, and
   forward-looking simulation math in services.
2. A sloth `prometheus/v1`-compatible Prometheus rule generator, plus a CLI
   with `generate`, `validate`, `lint`, `calc`, `simulate`, `check`,
   `dashboard`, and `schema` commands behind feature flags (`cli`, `spec`,
   `check`, `dashboard`).

`Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` all say 1.9.0. Under the standing
release delegation the cut (tag `v1.9.0`, GitHub release, the publish it fires)
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
the tag-pinned JSON Schema URLs are immutable. 1.9.0 keeps all three, and it
touched the read side only. Additive library surface: `check::CheckOptions`
(`#[non_exhaustive]`, its `Default` reproducing 1.8.0 behavior),
`check::BurnWindow`, `check_spec_with` / `check_slo_with`, and
`CheckOptions::plugins`; `check_spec` and `check_slo` keep their signatures
and delegate, so no existing call site changes. The CLI gains `--rules-window`
plus `--no-period-scaling` and `--alert-windows` on `check`, and changes no
default: `--window` still defaults to `1h` at every period (D1.9-2's
alternative, flipping the default to rules-agreement, is recorded as a 2.0
candidate precisely because `check`'s burn rate feeds `--fail-on` and
re-windowing it silently would flip existing CI gates), and a default
invocation's wire queries and output are byte-identical to 1.8.0. Generated
rule output is untouched by construction rather than by promise: the only
generator-side edit in the whole milestone is `generate::recording::base_window`
taking `&MwmbrConfig` instead of `&SloContext<'_>` so that `check` can call the
generator's own resolution rather than re-derive it — the seam D1.9-2 required
— and the spec format, `schema/`, and every runtime dependency are unchanged
(the sole `Cargo.toml` delta since 1.8.0 is a dev-dependency bump, jsonschema
0.48 → 0.49, out of the Dependabot queue). The durable artifact is
`tests/check_generate_agreement.rs`, which reads BOTH real artifacts — the wire
queries `check` actually sends, captured by the `QuerySpy` loopback, and the
window inside the emitted `slo:current_burn_rate:ratio` — and fails by name
when the two name different windows, across the default / `--period 7d` /
`--no-period-scaling` / `--alert-windows` option space for 30d and 7d SLOs. It
adds no dependency, so the lean core is untouched.

MSRV is 1.82 and it **is** CI-enforced: the `MSRV 1.82` job in
`.github/workflows/ci.yml` builds the default, all-features, and lean-core
configurations on a pinned 1.82 toolchain against an MSRV-compatible
resolution. CI additionally runs `fmt, clippy, test` (with `-D warnings`, a
committed-lockfile check via `cargo metadata --locked`, and a lean-core build
and test), `Security audit` (`cargo audit --deny warnings`), `promtool check
generated rules` against a pinned Prometheus release, and `coverage`.

## Unreleased on main

Merged since v1.9.0 and not yet released. Kept in step with
[CHANGELOG.md](CHANGELOG.md)'s `[Unreleased]` section by
`tests/roadmap_truth.rs::roadmap_declares_unreleased_work_exactly_when_the_changelog_has_some`.

- **QA: the dashboard's default time range follows the resolved period.** The
  emitted `time.from` was the literal `now-30d` whatever the SLO period, the
  LOW bug the 2026-08-07 dashboard-drift pass filed and deliberately kept out
  of PR #38. It now follows the longest period any SLO in the spec resolves
  to (byte-identical at the 30d default), pinned by four tests in
  `tests/dashboard.rs`.

- **QA: the published examples get their first compilation.** The docs.rs
  landing example was fenced `ignore` and had rotted into calling a method
  `RuleSet` has never had, and the README pointed library consumers at a
  pre-1.0 version of a crate at 1.9.0. Both are fixed, and
  `tests/doc_examples.rs` (4 guards) now holds every README Rust example
  byte-equal to the visible body of a compiled doctest under `src/`, refuses
  an `ignore` fence anywhere in `src/`, and checks the README's dependency
  pin against `CARGO_PKG_VERSION`. No library or CLI behavior changed.

- **v1.10.0 PR 1: the provenance seam, and the spelling it fixes.**
  `spec::SourceDialect` plus the additive `Spec::dialect` / `Spec::api_version`
  fields, set by every importer and captured by the native parser; the
  spec-level `SLO_PLUGIN_CHAIN_DROPPED` finding now names `sloPlugins` on a
  sloth CRD and keeps `slo_plugins` natively. `tests/source_dialect.rs`
  (5 tests) owns the seam, `tests/sloth_crd_cli.rs` the message. Done-when
  clauses 1 and 2 hold; clause 3 is half done (one of the two `known_gap_`
  tests deleted, PR 2 owes the other).

- **v1.10.0 PR 2: the note the two silent routes never sent.**
  `metadata.annotations` is now reported as dropped on `openslo/v1alpha` and
  on the sloth CRD route, not only on `openslo/v1`, from one
  `OBJECT_ANNOTATIONS_NOTE` constant in `src/spec/import.rs` that all three
  emission sites read. The two routes were silent for different reasons: the
  v1alpha path shares v1's envelope and had parsed the key since 1.5.0
  without mentioning it, while the CRD's `ObjectMeta` had no such field, so
  serde discarded the key before any code saw it. In
  `tests/dialect_parity.rs` the `object-annotations` construct's three rows
  are now `NotedAndIgnored` and its verdict is `Uniform`;
  `known_gap_object_annotations_are_noted_on_v1_only` is deleted and four
  tests replace it. Done-when clauses 3 and 4 hold — `grep -rc "fn known_gap_"
  tests/ src/` is 0. No generated rule changes.

- **v1.10.0 PR 3: the dialect the dispatch could not name.** An `apiVersion`
  naming no format slokit reads is now REPORTED on both surfaces that can see
  it: the new `UNKNOWN_API_VERSION` lint code for a document that parses
  natively anyway, and a dialect-naming parse error for one that does not.
  Both print the accepted groups beside the value, composed in
  `src/spec/import.rs::KNOWN_API_GROUPS` from `openslo::API_GROUP` and
  `sloth_crd::API_GROUP` — the same constants the auto-detector routes on, so
  the set a message calls accepted cannot drift from the set the dispatch
  uses. Reported, never refused (D1.10-3): `tests/unknown_api_version.rs`
  (12 tests) pins the acceptance beside the report — the committed example
  with `apiVersion: apps/v1` prepended still validates and still generates
  byte-identical rules — and reads the accepted set back out of the shipped
  message to hold it against `is_openslo` / `is_sloth_crd`. **Done-when clause
  5 holds, both halves.** The `lint --strict` consequence D1.10-4 names is
  stated in the CHANGELOG and has its own test. `tests/sloth_crd_cli.rs`'s
  pinned-native assertion was updated in the same commit: the pre-1.6.0
  `missing field `service`` message it deliberately preserved is now preceded
  by the dialect the document declared.

## Next milestones

### v1.10.0: the spec remembers which dialect it came from

**Theme.** slokit reads four input dialects, and the moment a document becomes
a `Spec` it forgets which one produced it. Everything downstream therefore
answers in native slokit vocabulary, to a reader who may never have written
any: a sloth Kubernetes CRD's plugin-chain warning tells them to remove
`slo_plugins` from a file whose key is `sloPlugins`, `metadata.annotations` is
reported as dropped on one of the three object-envelope routes and dropped in
silence on the other two, and a document in an unrecognised dialect is answered
with whichever native field the parser happened to miss first. Those are three
separate LOW bugs in the backlog with one root: the import layer knows the
dialect and nothing carries that knowledge past the import boundary. This
milestone gives it a seam — the same move v1.9.0 made for window resolution —
and then spends it on the three messages.

It is a read-side, message-quality minor: no generated rule changes, the spec
format gains no key, and the public API grows only additively.

**Grounding, measured at a fresh binary on 2026-08-11 rather than inherited
from the bug texts.** Freshness proven before any conclusion was drawn, because
this crate has produced a stale-`target/` false negative before: appending a
garbage token made `cargo build --all-features` fail at `src/lib.rs:84`
(`expected one of ! or ::`), so the binary under test is the tree under test.

- **The spelling.** `slokit lint -i tests/fixtures/sloth_corpus/slo-plugin-k8s-getting-started.yml`
  prints ``WARN myservice spec SLO_PLUGIN_CHAIN_DROPPED `slo_plugins` is a
  sloth SLO plugin chain`` while line 17 of that document reads `sloPlugins:`.
  The SLO-level finding says `` `plugins` ``, which the CRD spells `plugins`
  too, so exactly one of the two spellings is wrong. Both are string literals
  at `src/spec/lint.rs:192` and `:197`, and `lint` reads a `Spec`, which has no
  field to read the dialect from.
- **The silence.** Three documents differing in nothing but dialect
  (`tests/fixtures/dialect_parity/object-annotations.*`, committed by v1.8.0
  PR 2): the `openslo/v1` run prints `note: … metadata.annotations do not map
  and were ignored`, the `openslo/v1alpha` and `sloth-crd` runs print no such
  note, and all three exit 0 as valid.
- **The dispatch.** `apiVersion: openslo/v2` errors with `unsupported
  apiVersion 'openslo/v2' (expected openslo/v1 or openslo/v1alpha)` and
  `sloth.slok.dev/v2` with its sibling message — both good. A Kubernetes
  `apps/v1` Deployment pointed at slokit by mistake errors with `spec error:
  document 1: missing field 'service'`. The dispatch is precise where a prefix
  matches and mute where none does.
- **The constraint the fix must respect, measured rather than assumed.** A
  *valid native spec* with `apiVersion: apps/v1` prepended — the committed
  `examples/infraportal/slos/accounts-service.yaml` — still reports `ok:
  'accounts-service' is valid (2 SLOs)` and generates byte-identical rules
  (18126 bytes with and without the key). So an unrecognised `apiVersion`
  cannot become a refusal in 1.x without breaking
  [docs/SEMVER.md](docs/SEMVER.md)'s "YAML that parses and validates under 1.a
  also parses and validates under 1.b". That is the same wall that made the
  plugin chain a lint rule in v1.7.0 instead of a refusal, hit a second time
  from a different direction.
- **Two stale claims in the same family**, found by reading the code the fix
  touches rather than by grep: `src/spec/mod.rs:96-97` and
  `src/spec/lint.rs:188-189` both still say the CRD dialect "refuses
  `spec.sloPlugins` outright". v1.8.0 PR 1 (PR #52) removed that refusal, and
  the probe above shows the CRD route linting the chain and exiting 0. PR 1
  corrects both comments in the files it is already editing.

**Decisions, each an overridable default (say "approved" to take all five):**

- **D1.10-1 — provenance is two additive fields on `Spec`, mirroring the
  `slo_plugins` precedent in that same struct.** A new
  `#[non_exhaustive] pub enum SourceDialect { Native, OpenSloV1,
  OpenSloV1Alpha, SlothCrd }` whose `Default` is `Native`, carried as
  `#[serde(skip)] pub dialect: SourceDialect` (set by whichever importer
  produced the spec, never read from or written to YAML), plus
  `#[serde(rename = "apiVersion", default, skip_serializing)] pub api_version:
  Option<String>` capturing the document's own top-level `apiVersion`
  verbatim when it had one. `slo_plugins` is already exactly this shape —
  captured to make a drop visible, never applied, never serialized back — so
  the spec format gains no key, `schema/slokit-spec.schema.json` needs no
  edit, and the "spec format only grows" clause is untouched. Named
  `SourceDialect` rather than `Dialect` because `src/bin/slokit.rs:48` already
  has a CLI-level `Dialect` with a different variant set (it counts
  `AlertWindows`, a document kind rather than a spec dialect).
  *Alternative recorded:* thread the dialect through `lint`'s call sites only
  — rejected because `lint` takes a `Spec` and the library API is the surface
  embedders use, so a CLI-local fix would leave every embedder on the wrong
  spelling.
  *Consequence PR 1 owes an answer to, not a free change:* `Spec` derives
  `PartialEq`, so a new field changes what equality means.
  **CORRECTED 2026-08-12 by PR 1, which ran the suite instead of trusting this
  paragraph:** the scoping pass wrote "no whole-`Spec` equality assertion
  exists today", having found only field-wise `assert_eq!`s
  (`tests/multi_spec.rs:50`, `tests/multidoc_input.rs:31-34`,
  `tests/sloth_crd.rs:305`). That was **false — there are two**, and both went
  red on the field's first build: `tests/openslo_export.rs`'s
  `assert_round_trip` (`assert_eq!(got, &want)`, reached by 5 tests) and
  `tests/sloth_crd.rs`'s twin reconstruction (`assert_eq!(imported, native)`).
  Neither was a defect in the seam: a round-tripped spec really did come from
  an OpenSLO document, and an imported CRD really is not its native twin any
  more. Both now assert the provenance difference explicitly and compare the
  rest, so the change is documented in the guards rather than absorbed by
  them. The lesson for PRs 2-4 is the shape, not the count: a grep for
  `assert_eq!(spec` cannot find an equality assertion written over two
  variables named something else.
- **D1.10-2 — messages become dialect-aware only where the dialect changes
  what the reader must type.** A finding that names a KEY gets the reader's own
  spelling; prose, codes, locations and the JSON `--output json` field names
  stay identical across dialects, because they are the machine-readable surface
  docs/SEMVER.md freezes. Concretely this milestone re-spells exactly one
  message, `SLO_PLUGIN_CHAIN_DROPPED` at spec level.
- **D1.10-3 — an unrecognised `apiVersion` is reported, never refused, while
  the document still parses natively.** Measured above: refusing it would break
  the 1.x parse-compatibility clause on documents that generate byte-identical
  rules today. Refusal is recorded as a 2.0 candidate. Where the native parse
  ALSO fails, the error names the `apiVersion` and the dialects slokit accepts
  instead of the first missing native field — that rewords an error on a
  document that already exited 1, so it is not an acceptance change.
- **D1.10-4 — the report channel for that case is a new lint code,
  `UNKNOWN_API_VERSION`, and the `lint --strict` consequence is stated rather
  than hidden.** A new code means a document that was warning-free can now fail
  `lint --strict`; v1.7.0 set that precedent with `SLO_PLUGIN_CHAIN_DROPPED`
  and the CHANGELOG says so under Added. *Alternative rejected:* a stderr
  `note:` — the note channel is importer-only (`Import` / `ImportNote`,
  `src/spec/import.rs:21` and `:42`), and a natively parsed document has no
  importer to carry one.
- **D1.10-5 — single-theme minor.** The dashboard `time.from` follow-up, the
  README feature-flag table, and the README ```sh guard all stay out, the same
  discipline D4 set for v1.2.0 and D1.9-3 for v1.9.0.

**Slices, dependency-ordered (no calendar sizing).** Four rather than three
because each is one reviewable claim, and PRs 2 and 3 both depend on PR 1's
fields existing:

1. **PR 1 — the provenance seam, and the spelling it fixes.** `SourceDialect`
   plus the two `Spec` fields; every importer sets the dialect and the native
   parser captures `apiVersion`; `plugin_chain_lints` takes the spec-level
   spelling from the dialect instead of the literal at `src/spec/lint.rs:192`;
   `known_gap_the_crd_lint_finding_names_the_native_spelling`
   (`tests/sloth_crd_cli.rs`) deleted in the same commit, as its own message
   mandates; the two stale "refuses `spec.sloPlugins`" comments corrected.
2. **PR 2 — the note the two silent routes never sent.**
   `metadata.annotations` reported on `openslo/v1alpha` and on the sloth CRD
   route from the same string constant the `v1` route already uses;
   `known_gap_object_annotations_are_noted_on_v1_only` deleted; the two
   `SilentlyIgnored` rows in `tests/dialect_parity.rs` become
   `NotedAndIgnored` and the construct's verdict becomes `Uniform`. This slice
   needs no new machinery and is the cheapest of the four.
3. **PR 3 — the dialect the dispatch could not name.** The
   `UNKNOWN_API_VERSION` lint over `Spec::api_version`, and the dialect-naming
   parse error for the case where no native parse succeeds either. The
   byte-identity guard of done-when 5 lands here, in the same commit as the
   lint, because it is what proves the rule reports without refusing.
4. **PR 4 — release prep and the cut**, the `roadmap_truth`-enforced shape,
   then tag, release, and the registry read, under the standing delegation.

**Done-when (every clause settled by a build, a test, a CI run, or a registry
read — no existence searches):**

1. `spec::SourceDialect` and both `Spec` fields exist and are additive:
   `cargo test --all-features` green, the lean core still builds and tests
   (`.github/workflows/ci.yml:56` and `:59`), the `MSRV 1.82` job green on all
   three feature configurations, and a native spec round-tripped through
   `Serialize` is byte-identical to today's output, so no YAML key appeared.
2. One test asserts BOTH spellings together: a sloth CRD document's spec-level
   `SLO_PLUGIN_CHAIN_DROPPED` finding names `sloPlugins` while the same
   construct in a native document still names `slo_plugins` — with a negative
   control that forces the dialect to `Native` on the CRD route and observes
   exactly that test go red, perturbation count-asserted and the RED line
   quoted.
3. Both characterisation tests are DELETED in the commits that close them, as
   each one's own failure message instructs, and
   `grep -rc "fn known_gap_" tests/ src/` goes from 2 to 0.
4. `object-annotations` in `tests/dialect_parity.rs` records `NotedAndIgnored`
   on all three rows with verdict `Uniform`, and the three notes come from one
   shared constant, so a reworded note cannot diverge silently — the shape
   v1.8.0 PR 2 used for `metadata.displayName`.
5. The compatibility constraint becomes a test rather than a promise:
   `examples/infraportal/slos/accounts-service.yaml` with `apiVersion: apps/v1`
   prepended still exits 0 from `validate` and generates rules byte-identical
   to the same document without the key (18126 bytes today), while `lint` on
   that document reports `UNKNOWN_API_VERSION`; and a document carrying an
   unrecognised `apiVersion` that also fails the native parse errors naming
   both the `apiVersion` and the accepted dialects, not a missing native field.
6. Registry read, never inferred: crates.io reports `newest_version` 1.10.0
   after the PR 4 cut.

### v1.11.0: the export meets its ecosystem

Scheduled 2026-08-18 by the scoping pass after the v1.10.0 feature slices
merged. Sequenced AFTER the v1.10.0 cut (PR 4 above): nothing here starts
until crates.io reports 1.10.0.

**Theme.** `slokit export --format openslo` has exactly one reader today:
slokit's own importer. The round-trip suite proves self-agreement and nothing
else — exporter and importer share one reading of the OpenSLO spec, so a
shared misreading is structurally invisible to every test in this repo. The
first external reader this export has ever met, run for this scoping pass,
refused 2 of the 12 documents the repo's own committed specs export to. This
milestone fixes what that reader found, then makes the reader a CI gate (the
promtool pattern, applied to the crate's other output format), and pins the
OpenSLO organisation's own example corpus as a committed contract the way
v1.7.0 pinned sloth's.

**Grounding, measured at a fresh binary on 2026-08-18 rather than assumed.**
Freshness proven first, because this crate has produced a stale-`target/`
false negative before: appending a garbage token made `cargo build
--all-features` fail at `src/lib.rs:83` (`expected one of ! or ::`), so the
binary probed is the tree probed.

- **The verdict.** oslo v0.13.0 — OpenSLO's official validator, the
  `windows-amd64` release binary, its sha256 checked against the published
  `oslo-0.13.0.sha256` before first run — over every document
  `slokit export --format openslo` produces from the repo's committed specs
  (8 `examples/infraportal/` services plus 4 native fixtures): 12 documents,
  **10 `Valid!`, 2 refused**, both with `'spec.timeWindow': length must be
  between 1 and 1`.
- **The trigger.** The two refusals are the exports of exactly the two
  period-less committed specs (`tests/fixtures/sample.yaml` and
  `tests/fixtures/multifile.yaml`, zero `period:` keys between them). A spec
  with `period:` exports a `timeWindow` and passes; a spec without one exports
  **no `timeWindow` at all**, and OpenSLO requires exactly one entry.
- **The omission is deliberate and pinned green.** The mapping table at
  `src/spec/openslo/export.rs:23` documents `spec.timeWindow[0]` as "(omitted
  when unset)", and a test at `export.rs:837` asserts the absence —
  the invalid output has a green test holding it in place.
- **The closure that hid it.** The importer accepts a missing time window by
  design (`src/spec/openslo/v1alpha.rs:58`: a missing `spec.timeWindows`
  means "the generation-time default period applies"), so export→import
  round-trips exit 0 while the ecosystem's validator exits 1 on the same
  bytes. No test in this repo could have caught this, structurally: both
  halves of the loop move together.
- **No workaround at the CLI.** `export` takes exactly `--input`,
  `--input-format`, `--format` and `--output`. There is no `--period`; a user
  whose spec omits the key cannot supply it at export time.
- **The corpus this pass read for the first time.** The OpenSLO
  organisation's own examples tree (`OpenSLO/OpenSLO@e74b589`, 4 documents —
  the whole corpus; the repo is spec prose, not an example library): slokit
  refuses all 4, with precise messages — 3 at the `budgetingMethod` gate
  (`Timeslices` twice, `RatioTimeslices` once), 1 at
  `metricSource.type 'Any'`. Read past the first error, every one of the 4
  also carries `metricSource.type: Any` (a documentation placeholder no
  Prometheus generator can map) and one uses a calendar-aligned
  `isRolling: false` window. So the corpus grounds dispositions and
  messages, not new capability — the method gate is only the first of three
  walls those documents hit. See the new candidate bullet below.
- **The other census directions were re-run and offer no theme.** Upstream
  `slok/sloth@main` is still at `8a3be4f` (read from the commits API
  2026-08-18), so the sloth corpus contract has nothing new to say. Both held
  candidate-list rules were re-tested at the shipped binary: the two
  committed catalogues still report minimum factors at or below 1
  (7d `13.44/3.5/1.4/0.98`, custom-30d `14.4/4.8/3/1`), and
  `src/spec/mod.rs` still has zero traffic-family fields, so both holds
  stand — the fifth consecutive scoping pass in which the candidate list
  produced no schedulable item.

**Decisions, each an overridable default:**

- **D1.11-1 (the fix): a period-less spec exports its resolved period.**
  `spec.timeWindow[0]` becomes `{duration: 30d, isRolling: true}` through
  `SloSpec::resolve_period` (`src/spec/mod.rs:790`) — the seam the generator,
  `lint`, and (since PR #71) the dashboard already resolve through — so the
  document the export emits is the document the generator means. Output stays
  byte-identical for every spec that carries `period:` (10 of the 12 exports
  unchanged). The `export.rs:837` absence assertion inverts; that is a
  deliberate, CHANGELOG-stated behaviour change to the export of a degenerate
  input, not a refactor. The alternative — refusing period-less exports — is
  rejected as a 1.x regression: those exports exit 0 today.
- **D1.11-2 (the reader): CI runs the official validator over every export
  the repo can produce.** A job pinning oslo v0.13.0 `linux-amd64` by release
  tag AND sha256 (the same pattern that pins promtool in
  `.github/workflows/ci.yml`), exporting every committed native spec and
  validating each emitted document. A new gate is not trusted until it has
  been observed FAILING: the job must go red against the unfixed export (or
  an equivalent perturbation) before it counts, with the red run recorded in
  the PR body.
- **D1.11-3 (the contract): the OpenSLO org corpus becomes a committed
  test.** `tests/openslo_corpus.rs` on the `tests/sloth_corpus.rs` pattern:
  all 4 documents committed under `tests/fixtures/openslo_corpus/`, upstream
  commit and per-file sha256 pinned, each with its recorded disposition and
  refusal message, reconciled against the fixture tree in both directions —
  so the next scoping pass reads a failing test when upstream moves instead
  of re-running this census by hand.
- **D1.11-4 (what this milestone is not):** no `Timeslices` support, no
  calendar windows, no non-Prometheus metric sources. All three are what the
  corpus documents would need next, and all three stay held until an
  importable document that needs them exists (candidate bullet below).

**Slices (dependency-ordered):**

1. **PR 1 — the fix.** Resolved-period `timeWindow` emission, a regression
   test that fails without it (the export of `tests/fixtures/sample.yaml`
   carries exactly one `timeWindow` and its duration is `30d`), the
   `export.rs:837` assertion inverted in the same commit, CHANGELOG entry.
2. **PR 2 — the reader.** The pinned-oslo CI job, green on PR 1's output,
   observed red on the pre-PR-1 defect as its negative control.
3. **PR 3 — the contract.** `tests/openslo_corpus.rs` plus its fixtures,
   dispositions quoted from this section's census.
4. **PR 4 — release prep and the cut**, the `roadmap_truth`-enforced shape,
   then tag, release, and the registry read, under the standing delegation.

**Done-when (every clause settled by a build, a test, a CI run, or a
registry read — no existence searches):**

1. The period-less regression test is red on the pre-PR-1 tree and green
   after, both runs quoted in PR 1's body.
2. `oslo validate --file` exits 0 in CI for every document
   `slokit export --format openslo` produces from every committed spec, on
   the sha256-pinned binary — and the job has been observed red on an
   invalid export before being trusted, recorded in PR 2's body.
3. `tests/openslo_corpus.rs` goes red when one fixture byte or one recorded
   disposition is perturbed, both controls run and quoted in PR 3's body.
4. Registry read, never inferred: crates.io reports `newest_version` 1.11.0
   after the PR 4 cut.

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
    **Re-tested 2026-08-09 by the v1.9.0 scoping pass at the shipped binary
    rather than by re-reading this bullet:** `slokit validate -i` over both
    committed catalogues reports the factors itself now — 7d
    `13.44 / 3.5 / 1.4 / 0.98`, custom-30d `14.4 / 4.8 / 3 / 1` — minimum at
    or below 1 in both, so both still close the budget by construction, and
    upstream `slok/sloth@main` is still at `8a3be4f` (read from the contents
    API), so no new catalogue exists to test. The hold stands.
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
    **Re-tested 2026-08-09 by the v1.9.0 scoping pass:** the same
    case-insensitive sweep now hits two files — `src/bin/slokit.rs` (the
    `--traffic` flag on `calc` and `simulate`, unchanged) and `src/budget.rs`,
    whose single hit is the phrase "known event volume" in a doc comment
    present since the initial commit, an operator-supplied number at call
    time, not a spec field. `src/spec/mod.rs` still has zero hits, so the
    structural claim holds and the hold stands.
- **Timeslices-family budgeting methods (`Timeslices` and `RatioTimeslices`),
  first evaluated 2026-08-18 by the v1.11.0 scoping census and HELD, under the
  same doctrine as the two lint rules above: capability held on input that has
  not arrived.** The OpenSLO organisation's own 4-document example corpus
  (`OpenSLO/OpenSLO@e74b589`) is the only place slokit has ever seen the
  methods: 3 of the 4 documents carry them, and slokit refuses each with
  `spec.budgetingMethod '<method>' is not representable; slokit models the
  Occurrences method only` — the parse surface already exists
  (`src/spec/openslo.rs:395,461-462` read `budgetingMethod`,
  `timeSliceTarget`, `timeSliceWindow`; `:495` is the gate). What holds it:
  read past that first error, **every** timeslices document in the corpus also
  carries `metricSource.type: Any` (a documentation placeholder no Prometheus
  generator can map) and one uses a calendar-aligned `isRolling: false`
  window, so building the method would make zero currently-refused documents
  importable — the method gate is only the first of three walls. Clears when
  an importable timeslices document exists: a user report, or an upstream
  corpus document whose metric source is `Prometheus` and whose window is
  rolling. Re-test at the source rather than inheriting this bullet.
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
| 1.8.0 | 2026-08-08 | import **dialect parity**: the sloth Kubernetes CRD route captures `spec.sloPlugins` and `slos[].plugins` into the fields the native parser has filled since 1.7.0, so a plugin chain is reported by `SLO_PLUGIN_CHAIN_DROPPED` instead of refused (corpus 16 accepted / 4 refused → 18 / 2, the byte-identity premise proven on the CRD route across the whole option space), and the OpenSLO `v1` route reports a dropped `metadata.displayName` in the same words `v1alpha` has used since 1.5.0; the durable half is `tests/dialect_parity.rs`, a committed contract running the shipped binary over one matched document per dialect for every construct more than one importer can receive (16 fixtures), whose first run found a third divergence (`metadata.annotations`, noted on `v1` only — filed as a LOW bug and pinned by `known_gap_object_annotations_are_noted_on_v1_only` rather than fixed) |
| 1.9.0 | 2026-08-10 | **check parity with the generated rules**: `slokit check` stops resolving its own answers and starts resolving them the way `generate` does. `check --rules-window` computes each SLO's current burn rate over the generator's own per-SLO window — through `generate::recording::base_window`, shared rather than re-derived — closing a gap where the two numbers published under the name "current burn rate" (the CLI's BURN column and the recorded `slo:current_burn_rate:ratio` the Grafana panel reads) disagreed 12x at the default period and 60x on a 7d SLO with nothing saying so; `--no-period-scaling` and `--alert-windows` carry the same meaning they have on `generate`, an explicit `--window` stays authoritative and still defaults to `1h`, and both output formats state the window each number was computed over. And an embedder's `SliPluginRegistry` finally reaches `check` (`check::CheckOptions::plugins`), with `check_spec_with`'s internal validation moved onto that same registry, so a spec `validate_with` and `generate_rules_with` both accept can no longer be un-checkable. Additive throughout: `check_spec` / `check_slo` keep their signatures, the options struct is `#[non_exhaustive]` with a `Default` reproducing 1.8.0, and a default invocation is byte-identical on the wire. The durable half is `tests/check_generate_agreement.rs`, which reads BOTH real artifacts — the wire queries `check` sends, and the window inside the emitted recording rule — and fails by name when they differ, across the default / `--period 7d` / `--no-period-scaling` / `--alert-windows` space for 30d and 7d SLOs; both `known_gap_` characterisation tests are deleted in the commits that closed them, as their own failure messages mandated |

The v1.9.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme: `check`
was the one read-side surface still resolving its own answers instead of
resolving them the way the generator does. The check-vs-generate QA audit
(PR #44, 2026-08-08) had proved the series names cannot drift — `check`
queries raw SLI expressions and never names a recorded series — but found two
gaps one layer down and pinned both with `known_gap_` tests rather than
leaving them as prose: the burn window disagreement above, and `check` being
the one public entry point a custom SLI-plugin registry could not reach. This
milestone was those two MED bugs promoted to a single theme. Its grounding was
re-probed at the binary on 2026-08-09 rather than inherited from the bug text
(a fresh 59s compile of `af9c7c8`, not a cached `Finished`), which is what
established that the disagreement is 12x at the default period and not only
the 60x the 7d case in the bug report showed. Three decisions, each an
overridable default and none overridden: D1.9-1 the CLI default window stays
`1h` and agreement is opt-in, because `check`'s burn rate feeds `--fail-on`
and re-windowing the default would silently flip existing CI gates (flipping
it is recorded as a 2.0 candidate); D1.9-2 resolution goes through the
generator's own seam rather than a check-local re-derivation, and the registry
lands in the same options struct rather than growing a second `_with` family,
because a second resolver is exactly the drift PR #38 removed from
`dashboard`; D1.9-3 single-theme minor, so the dashboard `time.from` follow-up
stayed its own backlog item. Slices, dependency-ordered: PR 1 the window seam
(`#60`), PR 2 the registry seam (`#61`), PR 3 this release prep and the cut.
Five of its six done-when clauses are mechanical and green in CI — the
both-artifacts window agreement across the option matrix for 30d and 7d specs,
`--window` authoritative in both directions (given, used verbatim; absent under
rules-window mode, observably not `1h` on a 7d SLO), default behavior
byte-identical, the registry-only plugin checked end to end against the
`QuerySpy` with validation running on the caller's registry, and both
`known_gap_` tests deleted in the commits that closed them with the two MED
bugs closed against those commits. The sixth — crates.io reporting
`newest_version` 1.9.0 — is the registry check in the current-state section
above, which nothing in this repo can assert on its own.

The v1.8.0 milestone as it was scoped, for the record, since the section
itself is gone from `## Next milestones` now that it shipped. Theme: slokit
reads four input dialects — native since 0.1.0, OpenSLO `v1` in 0.10.0,
OpenSLO `v1alpha` in 1.5.0, the sloth Kubernetes CRD in 1.6.0 — each verified
against sloth or a native twin when it was added, and nothing had ever checked
that they agree with **each other**. The v1.7.0 corpus census, re-run to scope
this milestone (the census needed no refresh and that was measured, not
assumed: `slok/sloth@main` was still `8a3be4f`, the commit
`tests/sloth_corpus.rs` pins, and the corpus suite was 8/8 green), found two
constructs where slokit's answer depended on which dialect the document
arrived in. A sloth SLO plugin chain was a hard error in the CRD dialect and a
warning in the native one, and the CRD refusal's stated reason — "it would
rewrite the generated rules" — was measured against slokit's own generator and
found **false**: slokit has no plugin-chain stage, and generating with and
without a chain is byte-identical. `metadata.displayName` was noted on
`v1alpha` import and dropped in silence on `v1`, from a shared envelope that
parses it either way. Two decisions, both taken as overridable defaults:
D1.8-1 unify toward accept-and-lint, because the symmetric alternative (making
the native route refuse) would turn documents that generate rules today into
hard errors, which the 1.x additive-only guarantee forbids; D1.8-2 the two
SLI-plugin refusals stay fail-closed, the corpus records 18/20 rather than
chasing 20/20 by weakening a gate. Slices (dependency-ordered): PR 1 the CRD
plugin-chain capture (`#52`), PR 2 the parity contract plus the `v1`
display-name note (`#53`), PR 3 this release prep and the cut. Five of its six
done-when clauses are mechanical and green in CI — the two corpus rows read
`Accepted` with `SLO_PLUGIN_CHAIN_DROPPED` and refusals fell 4 → 2
(`tests/sloth_corpus.rs`), the CRD byte-identity claim is run rather than
assumed (`tests/sloth_crd_cli.rs::a_crd_plugin_chain_changes_no_generated_byte`),
`tests/dialect_parity.rs` fails by name when one dialect's disposition changes
(the perturbation and failing run quoted in PR #53's body), the `v1` route
emits the display-name note with both on- and off-state tests, and
`cargo test --all-features` is green with every pre-existing byte-identity and
OpenSLO test file unchanged except the two corpus rows. The sixth —
crates.io reporting `newest_version` 1.8.0 — is the registry check above,
which nothing in this repo can assert on its own. The contract's first run
also **found more than the scoping recorded**: `metadata.annotations` diverges
the other way (noted on `v1`, silent on `v1alpha` and the CRD route), filed as
a LOW bug with its close condition rather than fixed in a slice scoped to the
display name.

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

- **2026-08-18: the census turned around and read the OUTPUT side, and the
  fifth consecutive milestone came from outside the candidate list.** Every
  prior scoping census ran the shipped binary over upstream INPUT documents;
  the v1.11.0 pass also handed slokit's own output to the ecosystem's reader
  (oslo v0.13.0, OpenSLO's official validator) and found a defect no test in
  this repo could see: the export and the importer share one reading of the
  OpenSLO spec, so the round-trip suite is self-agreement, and the two
  period-less committed specs have exported `timeWindow`-less — invalid —
  OpenSLO since v1.2.0 with a green test pinning the omission
  (`src/spec/openslo/export.rs:837`). The INPUT-side census was also widened
  to a corpus never previously read, the OpenSLO organisation's own examples
  (4 documents, all refused, dispositions recorded in the v1.11.0 section),
  which grounded a held candidate rather than a theme. The sloth corpus is
  unchanged upstream (`8a3be4f`) and the candidate list again produced no
  schedulable item, so the method stands: census first — now in both
  directions — candidate list second.
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
