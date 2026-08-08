# Changelog

All notable changes to slokit are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
From 1.0.0, slokit follows the semver guarantees documented in
[docs/SEMVER.md](docs/SEMVER.md): no breaking changes in 1.x.

## [Unreleased]

Toward **v1.7.0, sloth corpus parity**: the whole of `slok/sloth`'s `examples/`
tree is now committed as a pinned contract, and two of the three defects that
census found are fixed.

### Added

- **The sloth upstream corpus is a committed contract.** All 20 documents of
  `slok/sloth@8a3be4f`'s `examples/` tree live under
  `tests/fixtures/sloth_corpus/`, each with its upstream sha256 and the exact
  disposition slokit gives it (accepted, or refused for this stated reason).
  `tests/sloth_corpus.rs` re-hashes every file and re-runs the binary over
  every one, so a compatibility change is a named test failure rather than a
  discovery in the next release. The hashes are what make the contract
  un-green-able by editing a fixture.
- **`SLO_PLUGIN_CHAIN_DROPPED` lint.** sloth's SLO plugin chains
  (`slo_plugins:` at spec level, `plugins:` on an SLO) rewrite the rules sloth
  generates; slokit has no equivalent and silently ignored them, while the
  Kubernetes CRD dialect refused the same construct by name — the one
  construct slokit treated differently depending on which spelling carried it.
  `slokit lint` now reports it on the native route, naming the key and the
  entries, so the drop is visible without changing what any document generates
  (refusing it is not available under the 1.x promise in
  [docs/SEMVER.md](docs/SEMVER.md)). Three upstream documents carry one.

### Fixed

- **Unquoted YAML scalars in `labels` and `annotations` are accepted**, coerced
  to their canonical string (`true`, `0.2`, `90`) exactly as sloth's own decode
  does, at every level that has those maps. Previously any unquoted scalar was
  a hard parse error reading `invalid type: boolean 'true', expected a string`,
  which named neither the field nor the fix. Upstream's own
  `examples/victoria-metrics.yml` — a document sloth generates rules from — was
  unreadable to slokit for exactly this reason, over nine values across three
  scalar types. The JSON Schema's `labelMap` is widened to match, so the two
  descriptions of the format still agree. Non-scalar values (a nested map or
  list) remain a parse error, now worded for labels rather than for plugin
  options.

## [1.6.0] - 2026-08-08

sloth **Kubernetes CRD** input: `apiVersion: sloth.slok.dev/v1`,
`kind: PrometheusServiceLevel` documents are now auto-detected and imported,
and the dialect can be pinned with `--input-format sloth-crd`. That CRD is how
sloth is used inside Kubernetes and it is the largest dialect in sloth's own
`examples/` (five of twenty-one entries); slokit already *emitted* one from
`generate --format operator` without being able to read one, and every slokit
up to 1.5.0 dropped such a document into the native parser, which exited 1 with
`spec error: document 1: missing field 'service'` — naming neither the dialect
nor the fact that the input was a sloth format at all. Everything here is
additive per [docs/SEMVER.md](docs/SEMVER.md) under the "the spec format only
grows" clause: input that used to error now parses, no existing 1.x signature
changes, native and OpenSLO imports are untouched, and generated Prometheus
rule output for every existing spec is byte-identical to 1.5.0. No new
dependency — `serde_norway` already parses these documents — so the lean core
(`--no-default-features`) is untouched.

This release also carries the dashboard/generator option fix below, found by a
QA pass rather than by a user report: every window-scoped panel of a dashboard
built beside `slokit generate --period 7d` or `--no-period-scaling` rendered
"No data" against a real Prometheus, on every release from 1.0.0 to 1.5.0.

### Fixed

- **`slokit dashboard` no longer ignores how the rules were generated.** The
  dashboard resolves each SLO's burn-rate windows to name the
  `slo:sli_error:ratio_rate<window>` series its panels query, but it hardcoded
  the 30d default period and always scaled — so a dashboard built beside
  `slokit generate --period 7d` queried seven window-scoped series those rules
  never record (`5m`, `30m`, `1h`, `2h`, `6h`, `1d`, `3d` against a recorded
  `1m`/`7m`/`14m`/`28m`/`84m`/`336m`/`7d`), and `--no-period-scaling` produced
  the same disagreement in the opposite direction. Every burn-rate panel and
  the SLI timeseries panel rendered "No data" against a real Prometheus, with
  nothing in either command's output saying so. `slokit dashboard` now accepts
  `--period` and `--no-period-scaling` to match `slokit generate`, and both
  commands resolve their windows through one shared seam.

### Added

- **sloth Kubernetes CRD input** (v1.6.0 slice 1): `apiVersion:
  sloth.slok.dev/v1`, `kind: PrometheusServiceLevel` documents — the shape
  sloth is used in inside Kubernetes, and five of the twenty-one documents in
  sloth's own `examples/`. slokit already *emitted* a Kubernetes custom
  resource (`generate --format operator`) without being able to read one;
  before this, such a document fell through to the native parser and exited 1
  with `spec error: document 1: missing field 'service'`, naming neither the
  dialect nor the problem. A file whose first document declares that
  `apiVersion` is now auto-detected and imported, single document or stream,
  one spec per `PrometheusServiceLevel`. New public module
  `slokit::spec::sloth_crd` (`is_sloth_crd`, `from_yaml`, `from_path`),
  returning the same `Import` / `ImportNote` pair as the OpenSLO importer —
  additive per [docs/SEMVER.md](docs/SEMVER.md), and no new dependency, so the
  lean core is untouched. Kubernetes object metadata (`metadata.name`,
  `.namespace`, `.labels`) is ignored with an import note rather than silently
  honored, because object labels are not rule labels. Fail-closed by name:
  sloth's SLO plugin chains (`spec.sloPlugins`, `slos[].plugins`), and the
  *native* `alerting.page_alert` / `alerting.ticket_alert` spellings inside a
  CRD document, which would otherwise drop the page/ticket routing labels in
  silence (sloth's own `examples/plugin-k8s-getting-started.yml` has exactly
  that bug).
- **`--input-format sloth-crd`** (v1.6.0 slice 2), the third value of the
  existing flag: pins the dialect for every file read instead of relying on
  detection, and pins it *off* in the other direction, so a document can be
  checked deliberately as another dialect. Pinned onto a document that is not a
  CRD, the failure names the dialect it was read as (`sloth-crd document 1:
  missing field \`spec\``) rather than only the field. The dialect's end-to-end
  behaviour is now asserted through the installed binary in
  `tests/sloth_crd_cli.rs`: `validate` exits 0 on all three committed fixtures
  on both routes, and `generate` renders bytes identical to sloth's own native
  twins over both twin pairs, in both output formats, across the whole
  `--period` / `--no-period-scaling` option space.
- `dashboard::dashboard_value_with`, `dashboard::dashboard_json_with`, and
  `dashboard::dashboards_json_with`, taking the `GenerateOptions` the rules
  were generated with. The existing `dashboard_value` / `dashboard_json` /
  `dashboards_json` are unchanged and equal to the `_with` forms under
  `GenerateOptions::default()`, which `tests/dashboard_drift.rs` asserts
  directly — additive per [docs/SEMVER.md](docs/SEMVER.md).
- `slokit dashboard --period <window>` and `slokit dashboard
  --no-period-scaling`, mirroring `slokit generate`.

## [1.5.0] - 2026-08-07

OpenSLO **v1alpha** import: the OpenSLO importer now reads
`apiVersion: openslo/v1alpha` alongside the `openslo/v1` it has read since
0.10.0, so the reference corpus of the project slokit is spec-compatible with
can be imported at all — both of sloth's committed OpenSLO examples declare
v1alpha, and every slokit up to 1.4.0 rejected them with `unsupported
apiVersion 'openslo/v1alpha' (expected openslo/v1)`. Everything here is
additive per [docs/SEMVER.md](docs/SEMVER.md): input that used to error now
parses, no existing signature changes, `openslo/v1` imports are unchanged, and
generated Prometheus rule output for every existing spec is byte-identical to
1.4.0. No new dependency — `serde_norway` already parses these documents, so
the lean core (`--no-default-features`) is untouched.

### Added

- **`openslo/v1alpha` import** (v1.5.0 slice 1): `Spec::from_yaml` /
  `from_yaml_stream` and every `-i` path now dispatch on each document's own
  `apiVersion`, so a stream may mix dialects. The v1alpha mapping lives in a
  new sibling module (`src/spec/openslo/v1alpha.rs`) rather than growing the
  already-oversized `src/spec/openslo.rs`, and it reuses the version-independent
  v1 machinery instead of re-implementing it: `{{.window}}` rewriting, the
  `(total) - (good)` error-query derivation, the histogram/threshold latency
  convention and multi-objective SLO naming are all shared code. The shape
  differences it owns are the per-objective metric
  (`spec.objectives[].ratioMetrics.{good,total}` with flat
  `source`/`queryType`/`query`, where v1 carries one document-level
  `spec.indicator`), the period as `spec.timeWindows[].{count, unit}` (where v1
  uses `spec.timeWindow[].duration`), the document-level
  `spec.indicator.thresholdMetric`, and `target` with no `targetPercent`.
- **Fidelity stays fail-closed**, the rule the v1 importer and the OpenSLO
  export already follow: any v1alpha construct with no slokit representation is
  an error naming the offending field (`spec.objectives[0].target is required
  (openslo/v1alpha has no targetPercent)`,
  `...ratioMetrics.total.source is missing (expected prometheus)`), never a
  silent drop; representable-but-lossy constructs emit an `ImportNote`.
- **Both sloth OpenSLO examples are committed as fixtures**
  (`tests/fixtures/openslo/v1alpha/getting-started.yaml` and
  `kubernetes-apiserver.yaml`), the second exercising multi-document input, two
  documents sharing a service, and a two-objective SLO. A hand-written native
  twin of the first is committed beside it, and
  `tests/openslo_v1alpha_cli.rs` asserts at the binary level that `slokit
  generate` over the imported document is **byte-identical** to `generate` over
  that twin — so a mapper that parses but mis-maps fails even though `validate`
  exits 0. The same suite proves the import → `export --format openslo` →
  re-import round trip (deliberately asymmetric: v1alpha in, `openslo/v1` out,
  because the export always writes the current version).

### Changed

- **README and module docs name both dialects.** The OpenSLO interop section
  gains a direction/version table, states that v1alpha is what sloth's examples
  declare, and states the upgrade-on-round-trip behaviour;
  `src/spec/openslo.rs`'s docs say the same where a library user looks, and
  `tests/openslo.rs` records which dialect it owns.

## [1.4.0] - 2026-08-07

Per-severity dashboard burn panels: the dashboard now plots, for each enabled
alert condition of each SLO, the exact burn-rate quantities that condition
compares, with the condition's factor as the threshold line. Everything here
is additive per [docs/SEMVER.md](docs/SEMVER.md); generated Prometheus rule
output is byte-identical to 1.3.0. The release also carries two fail-closed
CLI fixes from the 2026-08-06 QA review of `generate --format operator` and
explicit least-privilege CI token permissions.

### Added

- **Per-severity dashboard burn panels** (v1.4.0 slice 1): `slokit dashboard`
  now emits one burn-rate timeseries panel per enabled alert condition of each
  SLO, plotting the long- and short-window burn rates that condition compares
  (the recorded `slo:sli_error:ratio_rate<window>` series divided by
  `slo:error_budget:ratio`, the generator's own grouping idiom) with a
  threshold line at the condition's burn-rate factor, titled by severity. A
  severity whose alert is disabled gets no panel, mirroring alert generation.
  Values are burn-rate multiples, so the threshold lines are the plain
  SRE-table factors. A new drift-guard suite (`tests/dashboard_drift.rs`)
  reads both the emitted dashboard and the generator's rendered recording
  rules and fails if any dashboard expression references a `slo:` series the
  generator does not record for that spec. Generated Prometheus rule output is
  unchanged.

### Fixed

- **`generate --format operator` no longer emits colliding `metadata.name`
  resources.** Every emitted `PrometheusRule` was named
  `--name`-or-the-spec's-service per spec, so with several specs loaded (one
  `-i` directory, or since 1.3.0 one multi-document file) `--name X` stamped X
  onto every resource, and two specs legally sharing a service (validation
  rejects duplicate service/SLO pairs, not duplicate services) collided on the
  default too. `kubectl apply` of such a stream keeps only the last document,
  silently dropping every other spec's rules. Both routes now fail closed
  before anything is rendered, the same posture `export --output` already has
  for file names. `--name` remains valid with exactly one spec.
- **`--name` with `--format prometheus` is rejected instead of silently
  ignored.** Only the operator arm ever read the flag; the merged Prometheus
  rules document has no `metadata.name` to set, so the flag was a shipped
  no-op on the default format.

### Security

- **Explicit least-privilege `GITHUB_TOKEN` permissions on every workflow.**
  `ci.yml` granted an explicit `contents: read` on only one of five jobs and
  `publish.yml` on none, so the rest ran on the repository-default token
  scope (measurably including `Packages: read`). Both workflows now declare
  a workflow-level `permissions: contents: read` block, and
  `tests/workflow_permissions.rs` pins the contract: every workflow file
  must declare a workflow-level block and no grant anywhere may exceed
  `contents: read`.

## [1.3.0] - 2026-08-05

### Added

- **Multi-document spec input** (v1.3.0 slice 1): `Spec::from_yaml_stream`
  parses a `---`-separated YAML stream into one spec per document (empty
  documents skipped, errors naming the failing document by 1-based position),
  and every CLI command's `-i` now accepts such a file — sloth's own
  `examples/multifile.yml` layout, which previously failed to load at all
  ("deserializing from YAML containing more than one document is not
  supported"), and the very stream shape `slokit export` writes, which the
  tool could emit but not re-read as native specs. `Spec::from_yaml` keeps
  its exactly-one-document contract unchanged.
- **`SLI_FALLBACK_ASYMMETRY` lint rule** (v1.3.0 slice 2): warns when exactly
  one of an events SLI's `error_query`/`total_query` contains a `vector(`
  no-data fallback (textual, case-insensitive). Grounded on sloth's own
  `examples/home-wifi.yml`, where both SLOs guard `error_query` with
  `OR on() vector(0)` and leave `total_query` bare, so a scrape gap empties
  the ratio and every burn-rate alert silently stops evaluating — exactly
  the no-data failure the one-sided guard shows the author meant to handle.

## [1.2.0] - 2026-08-01

OpenSLO v1 **export**: the inverse of the v0.10.0 import, so a spec can leave
slokit as easily as it enters. Everything here is additive per
[docs/SEMVER.md](docs/SEMVER.md); no 1.0.0 or 1.1.0 signature changed, and the
lean core (`--no-default-features`) is untouched.

### Added

- OpenSLO v1 **export**, the inverse of the v0.10.0 import, so conversion is no
  longer a one-way door: `slokit::spec::openslo::to_yaml(&Spec) -> Result<String>`
  serializes a spec as `apiVersion: openslo/v1`, `kind: SLO` documents (one per
  slokit SLO, joined as a multi-document stream), and
  `openslo::to_yaml_reported` returns the same YAML plus `ExportNote`s, the
  export-side twin of `ImportNote`. Additive per
  [docs/SEMVER.md](docs/SEMVER.md); the lean core is untouched.
- The full inverse mapping table, the fail-closed error set, and the fidelity
  contract are documented in the new `spec::openslo::export` module. The
  contract is a semantic round trip, not byte identity: exporting and
  re-importing yields the source spec with exactly three documented
  transformations applied (service-level labels move onto each SLO, alerting
  metadata is dropped because OpenSLO models alerting as separate
  `kind: AlertPolicy` documents, and the slokit dialect tag returns to its
  default), each reported as a note rather than dropped silently.
- Constructs OpenSLO cannot represent fail closed with an error naming the
  field rather than emitting best-effort YAML: plugin SLIs (which have no query
  until the registered plugin expands them), an SLI with none or more than one
  variant set, an empty service or SLO name, a spec with no SLOs, an out-of-range
  objective, and a latency threshold or histogram metric that cannot be written
  as an OpenSLO `thresholdMetric`.

- `slokit export`, the CLI half: `slokit export -i <file|dir> [--format openslo]
  [-o <dir>]` writes the OpenSLO documents to stdout as one multi-document
  stream, or to a directory as one `<service>.yaml` per spec (created if
  absent). `--format` accepts only `openslo` today; the flag exists so a second
  format is not a breaking change. Export notes go to **stderr**, so stdout
  stays a stream that pipes and redirects cleanly, the same split the import
  path already uses.
- Two `--output` guards, both rejecting the whole batch before anything is
  written: a `service` that is not a usable file name (it is only checked
  non-empty by validation, so it can hold a path separator, `..`, or a drive
  letter) and two specs sharing a service, which would otherwise write to the
  same file and silently lose one.
- README: an **OpenSLO interop** section documenting both directions (the
  import's auto-detection and `--input-format`, and the new export), the three
  round-trip transformations, and the fail-closed error set. The import had
  shipped in v0.10.0 with no README coverage at all.

Shipped as three slices: PR 1 (the library half), PR 2 (the `slokit export`
subcommand), and this release prep with the cut that follows it.

## [1.1.0] - 2026-07-26

The first minor of the 1.x line: it publishes the work that accumulated on
`main` after the 1.0.0 freeze. Every change is additive per
[docs/SEMVER.md](docs/SEMVER.md), so no 1.0.0 code needs editing to upgrade.

### Added

- `slokit simulate`: a forward-looking "what if" subcommand. Given an objective,
  period, and a sustained error rate, it reports the resulting burn rate, the
  projected time to budget exhaustion (from a full or partly-spent budget), and
  which multi-window multi-burn-rate page/ticket conditions would fire at that
  rate. `--traffic <rps>` adds absolute allowed-bad and projected-bad event
  counts; `--output json` emits a machine-readable report. Where `calc` answers
  "given the bad events already observed, how much budget is left", `simulate`
  answers "if I sustain this rate from here, when do I run out and what pages".
- New public `slokit::simulate` module (`simulate`, `Simulation`,
  `WindowOutcome`) exposing the same steady-state math as a library API. Purely
  additive and dependency-free, built on the existing core; part of the
  always-available lean core (no feature flag).
- **`examples/infraportal/`**: a real dogfooding example set. SLO specs for the
  8-service InfraPortal platform (availability + latency per service, 16 SLOs)
  plus the Prometheus rules slokit generates from them, kept honest by
  `tests/examples_infraportal.rs` (every spec validates; the committed
  `rules.yaml` must match regeneration byte-for-byte, so the public example can
  never drift from the generator). Documented as SLO-definitions-as-code that
  activate once the services expose `/metrics`.

### Fixed

- `slokit simulate` now validates its numeric inputs at the CLI boundary the
  way `--objective` already did. Previously `--error-rate 150` (an impossible
  above-100% rate), `--error-rate=-5`, `--error-rate nan`, a negative or `NaN`
  `--traffic`, and a `NaN` `--remaining` were all accepted and produced
  nonsensical output at exit 0 ("Burn rate: NaNx", negative event counts, and a
  `null` burn rate in `--output json`). Each is now rejected with a clear error
  naming the flag and a non-zero exit. A finite out-of-range `--remaining` is
  still clamped to `0..=100`, as its help documents. Library behavior is
  unchanged: `slokit::simulate::simulate` keeps its frozen 1.x signature and its
  documented `[0, 1]` `error_ratio` precondition; this only tightens the CLI.

## [1.0.0] - 2026-07-19

The stable release. Identical in content to 0.12.0; this release turns the
0.12 freeze-prep surface into the contract:

- The public API is frozen per [docs/SEMVER.md](docs/SEMVER.md): 1.x changes
  are additive only (non_exhaustive types grow through constructors, the
  SliPlugin trait grows through default-bodied methods).
- Generated Prometheus rule output is byte-stable within a 1.x minor line,
  enforced by the twin snapshot tests.
- The spec JSON Schema URL contract holds: tag-pinned raw URLs are immutable.
- MSRV is 1.82, enforced in CI; raises happen only in a minor release with a
  changelog announcement.

## [0.12.0] - 2026-07-19

1.0 freeze prep: the public API is finalized for the 1.0.0 freeze. This is
the deliberate **last breaking-change window** before 1.0; the (small)
breaking changes below exist precisely so that post-1.0 growth will not be
breaking.

### Changed (breaking, the last planned window before 1.0)

- **`#[non_exhaustive]` audit across the public API.** Enums that classify or
  report (`SlokitError`, `Sli`, `LintLevel`, `StatusLevel`, `OptionKind`) and
  structs that are configured or consumed (`Spec`, `SloSpec`, `SliSpec`,
  `EventsSli`, `RawSli`, `LatencySli`, `PluginSli`, `Alerting`, `AlertMeta`,
  `AlertWindowSpec`, `AlertWindow`, `MwmbrConfig`, `GenerateOptions`,
  `RuleGroup`, `RuleSet`, `Lint`, `SloStatus`, `Import`, `ImportNote`,
  `OptionSpec`) are now `#[non_exhaustive]`. Downstream impact: matches on
  these enums need a wildcard arm, and struct-literal construction (including
  `..Default::default()` functional update) no longer compiles outside the
  crate. Fields stay public for reading and mutation. `Severity` and `Slo`
  stay deliberately exhaustive: the page/ticket split and the
  objective-over-period pair are the model itself, and changing either should
  be loudly breaking.
- **`OptionSpec` is now built with a `const` builder** instead of a struct
  literal: `OptionSpec::new(name, kind, help)` plus `.required()` and
  `.with_default(value)`, usable in the `const` option tables plugin authors
  write. The built-in plugins and docs use it.

### Added

- Constructors for everything users build now that literals are closed:
  `Spec::new`, `SloSpec::new`, `SliSpec::events`/`raw`/`latency`/`plugin`,
  `EventsSli::new`, `RawSli::new`, `LatencySli::new`, `PluginSli::new`,
  `AlertWindowSpec::new`, `AlertWindow::new` (const), `MwmbrConfig::new`.
  `GenerateOptions`, `Alerting`, `AlertMeta`, and `SliSpec` keep `Default`;
  mutate the public fields after construction for optional settings.
- **[docs/SEMVER.md](docs/SEMVER.md)**: the written 1.x semver contract.
  Public API surface, patch/minor guarantees, byte-stability of generated
  rules within a minor line, spec format and JSON Schema growth rules with
  the tag-pinned URL contract, the MSRV bump policy (minor version, CHANGELOG
  announcement), `#[non_exhaustive]` and `SliPlugin` extension-point rules,
  and the explicit non-guarantees (message wording, `Debug` output,
  human-readable CLI text). Linked from the README's new "Stability and
  MSRV" section.
- **MSRV 1.82 CI job**: builds default, all-features, and no-default-features
  configurations on the pinned 1.82 toolchain, against an MSRV-compatible
  dependency resolution (rust-version-aware fallback resolver plus the
  upstream-supported `idna_adapter` 1.1.0 pin; see docs/SEMVER.md).
  Build-only and without dev-dependencies (`cargo hack --no-dev-deps`) on
  purpose, so test tooling stays out of the MSRV contract.
  `rust-version = "1.82"` was already declared; it is now enforced.
- `#![deny(missing_docs)]` and `#![deny(rustdoc::broken_intra_doc_links)]` at
  the crate root (upgraded from `warn`; the surface was already fully
  documented, this locks it for 1.x).

## [0.11.0] - 2026-07-19

Spec JSON Schema: editor autocomplete and validation for the sloth-compatible
spec format, completing the interop tranche.

### Added

- **`schema/slokit-spec.schema.json`** (draft 2020-12): the spec shape
  including every slokit extension (per-SLO `period`, the `latency` SLI,
  `sli.plugin`, `alerting.windows`). The schema encodes structural rules:
  required fields, the `page`/`ticket` severity enum, the exclusive
  events/raw/latency/plugin SLI choice, Prometheus duration and metric-name
  patterns, objective bounds in the open interval (0, 100), and the window
  token in hand-written queries (exactly the two spellings generation
  substitutes, `{{.window}}` and `{{ .window }}`). Cross-field semantics remain
  owned by `slokit validate` (and the schema description says so): duplicate
  SLO names, `short` shorter than `long` in custom windows, zero-total
  durations such as `0s`, latency-selector quote/comma checks, and plugin id
  resolution against the registry. Unknown properties stay allowed, matching
  the parser's sloth-forward-compatibility.
- **`slokit schema` subcommand**: prints the embedded schema verbatim to
  stdout (or `-o <file>`), so editors and tooling can consume it without the
  repository. Library consumers get the same string as
  `slokit::spec::SCHEMA_JSON` (behind the existing `spec` feature).
- Schema tests (`tests/schema.rs`): every native fixture spec validates
  against the schema, including the slokit-native twins of the OpenSLO
  goldens and the plugin worked example plus its hand-written twin; the
  schema-accepted samples also pass `slokit validate`; negative cases the
  schema and the tool both reject (unknown window severity, multiple SLIs
  set, malformed durations, out-of-range objectives, empty label names,
  missing `{{.window}}` tokens, non-scalar plugin options, and more); OpenSLO
  documents are rejected as non-native input; and pins that the embedded
  string matches the repo file byte for byte and that `slokit schema` prints
  exactly it. The `jsonschema` crate is a dev-dependency only; runtime
  dependencies and the lean core are unchanged.
- README: an "Editor integration (JSON Schema)" section covering the VS Code
  `yaml.schemas` mapping, the `yaml-language-server` modeline, the `slokit
  schema` subcommand, and the raw GitHub URL pattern (pin a tag to pin a
  schema version).

## [0.10.0] - 2026-07-19

OpenSLO import: the input funnel widens beyond sloth-compatible specs.
`apiVersion: openslo/v1` `kind: SLO` documents (single or multi-document YAML
streams) now import into the internal spec model, so validate, lint, generate,
check, and dashboard all work on OpenSLO input unchanged.

### Added

- **`slokit::spec::openslo` module** (behind the existing `spec` feature):
  `from_yaml` / `from_path` convert OpenSLO v1 documents into slokit `Spec`s,
  returning an `Import` with the converted specs plus lint-style `ImportNote`s
  for constructs that were dropped or rewritten. `is_openslo` provides cheap
  format detection. The mapping (documented in full on the module):
  - `metadata.name`/`labels`, `spec.description`, `spec.service` (documents
    sharing a service in one stream merge into one spec, in document order);
  - `spec.timeWindow[0]` rolling `duration` becomes the per-SLO `period`;
  - `objectives[i].target` (unit fraction) or `targetPercent` becomes the
    objective percent; multi-objective documents produce one SLO per
    objective, suffixed with the objective `displayName` (slugified) or its
    1-based index;
  - `ratioMetric` maps to the `events` SLI (`bad`/`total` directly;
    `good`/`total` derives the error query as `(total) - (good)` with a note)
    and `ratioMetric.raw` maps to the `raw` SLI (`rawType: failure` as
    written, `rawType: success` inverted as `1 - (query)`);
  - `thresholdMetric` maps to the `latency` SLI when the query is a bare
    histogram base metric (optional `{...}` selector) and the objective op is
    `lte`/`lt`, with the objective `value` as the `le` threshold;
  - `spec.indicatorRef` resolves against `kind: SLI` documents in the same
    input.
- **Window convention for imported queries**: queries already carrying
  `{{.window}}` are kept as written; otherwise every fixed range selector
  whose content is a plain duration (`[5m]`, `[1h30m]`) is rewritten to
  `[{{.window}}]` and an import note lists the rewritten literals. Subquery
  ranges (`[1h:5m]`) and brackets inside string literals are untouched. A
  query with neither the token nor a rewritable range selector is an error.
- **Clear errors for unrepresentable documents**, each naming the OpenSLO
  path: unsupported `apiVersion`, calendar-aligned time windows (`calendar`,
  `isRolling: false`) and calendar duration units (`M`/`Q`/`Y`),
  `budgetingMethod` other than `Occurrences`, non-Prometheus metric sources
  and `metricSourceRef` references, threshold objectives with `op: gt`/`gte`,
  threshold queries that are not a bare histogram base metric, and
  unresolvable `indicatorRef`s. Ignored-but-representable constructs
  (`alertPolicies`, `metadata.annotations`, `timeSliceTarget`/`Window`,
  ratio-SLI `op`/`value`, non-SLO/SLI kinds, multi-value labels) produce
  notes instead.
- **CLI `--input-format {slokit|openslo}`** on `generate`, `validate`,
  `lint`, `check`, and `dashboard`. When omitted the format defaults to
  slokit, except that detection is unambiguous when a file's first YAML
  document sets a top-level `apiVersion: openslo/...`; that file is then
  imported as OpenSLO. Directory inputs respect the flag for every file (and
  auto-detect per file when it is omitted, so directories may mix formats).
  Import notes print to stderr.
- Fixtures under `tests/fixtures/openslo/` (simple ratio, multi-objective
  thresholds, latency, an unrepresentable calendar window, and a
  multi-document stream) with mapping tests, a golden snapshot of rules
  generated from imported OpenSLO, byte-identical equivalence against a
  hand-written slokit twin spec, promtool validation of OpenSLO-imported
  output, and round-trip validate/lint coverage.

### Changed

- Imported OpenSLO SLOs carry default (empty) alerting metadata, so `lint`
  reports `NO_ALERT_LABELS` for them until routing labels are added; this is
  the documented, intended surface for "OpenSLO alertPolicies do not map".

## [0.9.0] - 2026-07-19

SLI plugins: reusable, named SLI templates referenced from specs via the
sloth-compatible `sli.plugin` key and expanded to the existing core SLI shapes
before validation, so all downstream checks, generation, and promtool coverage
apply to plugin output unchanged. Design: `docs/design/SLI_PLUGINS.md`.

### Added

- **`sli.plugin` spec surface** (sloth-compatible shape): a fourth SLI shape,
  `sli.plugin: {id, options}`, mutually exclusive with `events`, `raw`, and
  `latency`. `options` values deserialize from any YAML scalar (string,
  number, bool) and are coerced to strings, so `threshold: 0.5` and
  `threshold: "0.5"` are equivalent; non-scalar values are a parse error.
  Only the spec shape is sloth-compatible: slokit resolves ids against its own
  registry and never loads or executes sloth's Go plugin files, so
  `sloth-common/...` ids fail validation with a clear unknown-plugin-id error
  rather than silently generating different rules.
- **Built-in plugins** (the `slokit/` id namespace):
  - `slokit/availability/http-requests-total` - availability from an
    `http_requests_total`-style counter; options `metric` (default
    `http_requests_total`), `selector`, and `error_code_regex` (default `5..`).
  - `slokit/availability/grpc-server-handled` - availability from a
    `grpc_server_handled_total`-style counter where a `grpc_code` outside the
    `success_code_regex` allowlist (default `OK`) is a bad event; options
    `metric`, `selector`, `success_code_regex`.
- **`SliPlugin` trait and `SliPluginRegistry`** (`slokit::spec::plugin`,
  behind the existing `spec` feature; the lean core is untouched): plugins
  declare typed options (`OptionSpec` with `OptionKind`
  String/Number/Bool/Duration, required flags, and defaults), and the registry
  enforces the contract before expansion (unknown id, missing required option,
  and kind failures are hard errors; defaults are applied). `register` refuses
  duplicate ids, so built-ins cannot be shadowed. Registry-loaded external
  plugin files are out of scope for 0.9; the API accommodates a future loader
  (a loader just registers plugins).
- **Registry-aware `_with` siblings** for embedders with custom plugins:
  `SloSpec::to_sli_with`, `Spec::validate_with` / `spec::validate_with` /
  `spec::validate_all_with`, and `Spec::lint_with` / `spec::lint_with`. The
  existing entry points keep their signatures and resolve against the built-in
  registry.
- New validation errors (per the 0.8.0 "output impossible or broken"
  philosophy, all reported through the usual aggregated validation lines):
  empty `sli.plugin.id`, unknown plugin id, missing required options, option
  values failing their declared kind, selector-shaped option values failing
  the 0.8.0 selector checks, metric-name options outside the Prometheus
  charset, and regex options that would break out of their quoted matcher.
  A plugin whose expansion forgets `{{.window}}` is caught by the existing
  post-expansion window-token check.
- New lint `PLUGIN_UNKNOWN_OPTION` (warning): an option name the plugin does
  not declare. Generation succeeds (undeclared names are ignored), so this is
  advisory, catching typos without rejecting forward-compatible specs.
- New error variant `SlokitError::Plugin` for registry-level failures
  (duplicate id, unknown id, broken options).

### Changed

- `GenerateOptions` gained the `plugins` field
  (`Arc<slokit::spec::plugin::SliPluginRegistry>`, default: the built-in
  registry), used by `generate_rules_with` and `generate_all` to resolve and
  validate `sli.plugin` SLIs. Breaking for struct literals; use
  `..Default::default()` (same mitigation as `period_aware` in 0.7.0).
- The "sets multiple SLIs" and "has no ... SLI" validation messages now name
  `plugin` alongside `events`, `raw`, and `latency`.

## [0.8.0] - 2026-07-19

Spec hardening: a validation gap audit, with real gaps split into hard errors
(where the old behavior generated broken or misleading Prometheus rules) and
new advisory lints (where the output loads but is probably not intended); plus
external validation of generated output with promtool.

### Added

- **promtool integration**: the test suite now validates generated rule files
  with `promtool check rules` (the sample fixture, merged multi-spec directory
  output, and a spec covering both custom `alerting.windows` and period-scaled
  default windows). The tests skip with a clear message when promtool is not
  on PATH; setting `SLOKIT_REQUIRE_PROMTOOL=1` turns absence into a failure.
  A new CI job downloads a pinned Prometheus release (v3.5.0), puts promtool
  on PATH, and runs these tests with that variable set on every push and PR.
- **Cross-spec validation**: `spec::validate_all` validates a set of specs
  together, prefixing each finding with its service, and rejects a service/SLO
  pair that appears in more than one spec (merged output would repeat
  rule-group names, which Prometheus refuses to load). `generate_all` and the
  CLI `validate`, `lint`, and `dashboard` commands run it automatically.
- New validation errors for specs whose output was already broken or
  misleading:
  - empty label/annotation names anywhere in the spec (rejected by Prometheus
    under every name-validation scheme);
  - whitespace-only `alerting.name` (the alert would effectively have no name);
  - latency `histogram_metric` outside the Prometheus metric-name charset
    (it is embedded unquoted, so the generated PromQL would not parse);
  - latency `selector` containing braces, a leading/trailing comma, or an
    unbalanced double quote (broken PromQL);
  - latency `threshold` with surrounding whitespace (embedded verbatim in the
    `le="..."` matcher, it could never match a real bucket label).
- New lints:
  - `SPEC_VERSION` - `version` is not `prometheus/v1`; slokit ignores the
    field and generates prometheus/v1 rules regardless.
  - `LABEL_NAME_CHARS` - a label/annotation name is outside the legacy
    `[a-zA-Z_][a-zA-Z0-9_]*` charset; Prometheus releases before 3.0 (and
    legacy name validation) reject rules that use it.
  - `RESERVED_LABEL` - a user label uses the reserved `sloth_` prefix, so the
    generated identity labels may overwrite it.
  - `THRESHOLD_UNREACHABLE` - a burn-rate condition's threshold
    (factor x error budget) is >= 1, an error ratio the SLI can never reach,
    so the condition can never fire.
  - `DUPLICATE_ALERT_WINDOW` - `alerting.windows` repeats an identical
    severity/long/short condition (compared after parsing, so `30m` and
    `1800s` count as duplicates).

### Changed

- `generate_all` now fails on duplicate service/SLO pairs across specs;
  previously it silently produced a rules file Prometheus would reject.
- Latency `threshold` values with surrounding whitespace (e.g. `" 0.3 "`) are
  now validation errors; they previously validated but generated a matcher
  that could never match.
- CLI `validate` and `lint` report invalid multi-spec input as one combined
  validation error with `service '...'` prefixes instead of stopping at the
  first invalid spec.

## [0.7.0] - 2026-07-18

Configurable alerting: the burn-rate window table is no longer fixed.

### Added

- **Custom burn-rate windows** (slokit spec extension): per-SLO
  `alerting.windows` replaces the default MWMBR table with explicit
  `severity`/`long`/`short`/`factor` conditions. Validation rejects unknown
  severities, non-positive factors, unparseable durations, and `short >= long`.
- **Period-aware default windows**: SLOs with a non-30d `period` now get the
  SRE Workbook table scaled proportionally to their period (rounded to whole
  minutes, 1m floor), so each condition still fires after consuming the same
  budget fraction. A 90d SLO pages on 3h/15m instead of 30d-calibrated windows.
  Library API: `MwmbrConfig::scaled` and `MwmbrConfig::sre_default_for_period`.
- `slokit generate --no-period-scaling` and `GenerateOptions::period_aware`
  opt out of scaling and use the 30d table verbatim.
- New lint `NO_SEVERITY_WINDOWS`: custom windows that leave an enabled severity
  with no conditions would silently drop that alert.

### Changed

- **Generated output changes for SLOs with a non-30d `period`** (behavioral
  change): recording and alert windows are now scaled to the period. Output for
  30d-period SLOs is byte-identical to 0.6.x. Use `--no-period-scaling` to keep
  the old behavior.
- The SLO-period recording, the `slo:current_burn_rate:ratio` metadata rule,
  and the dashboard SLI panel now derive their base window from the effective
  window set (still 5m for the default table) instead of hardcoding 5m.
- `slokit calc` scales the printed threshold table to `--period`.
- Lint `PERIOD_TOO_SHORT` now evaluates the SLO's effective windows (custom or
  period-scaled), so it no longer fires for short-period SLOs that scaling
  already handles.
- `GenerateOptions` gained the `period_aware` field (breaking for struct
  literals; use `..Default::default()`).

## [0.6.8] - 2026-06-27

### Changed

- Prometheus parsing now returns an explicit diagnostic when
  `data.resultType` is missing from a successful query payload.

### Added

- Regression coverage for missing `data.resultType` response handling.

## [0.6.7] - 2026-06-27

### Added

- Parser regression coverage for unsupported Prometheus `resultType` values,
  locking in actionable error messaging for unexpected API response shapes.

## [0.6.6] - 2026-06-27

### Added

- Regression coverage for HTTP diagnostics formatting that verifies newline
  compaction and truncation behavior for long non-success response bodies.

## [0.6.5] - 2026-06-27

### Added

- Integration coverage for Prometheus HTTP non-success responses with empty
  bodies, confirming diagnostics still include the HTTP status line.

## [0.6.4] - 2026-06-27

### Changed

- Prometheus `status: error` responses now include `errorType` in query
  diagnostics when available, improving operator-facing failure context.

### Added

- Parser regression coverage for `errorType` + `error` propagation in live
  query response handling.

## [0.6.3] - 2026-06-27

### Changed

- Prometheus HTTP non-success responses in `check` now include both status and
  a trimmed response-body snippet in query errors for faster diagnosis.

### Added

- Integration coverage that validates HTTP status + response body propagation in
  live check errors.

## [0.6.2] - 2026-06-27

### Added

- Integration coverage for live HTTP check paths that now explicitly rejects
  non-finite Prometheus values (`NaN`, `+Inf`) before budget/burn calculations.

## [0.6.1] - 2026-06-27

### Fixed

- Hardened live Prometheus checking to reject non-finite sample values (`NaN`,
  `+Inf`, `-Inf`) instead of allowing misleading status computations.
- Status-level evaluation now treats non-finite budget/burn inputs as
  non-healthy.

### Added

- Integration coverage for bearer-token authentication in Prometheus client
  HTTP requests.
- Regression tests for non-finite sample parsing and status classification
  behavior.

### Changed

- Formatting-only cleanup to satisfy strict `cargo fmt --check` CI enforcement.

## [0.6.0] - 2026-06-07

### Added

- `slokit lint` command and `Spec::lint` / `slokit::spec::lint` API: advisory
  checks that complement `validate`. Where `validate` reports errors that make
  generation wrong or impossible, `lint` reports legal-but-questionable
  configuration:
  - `OBJECTIVE_100` - objective of 100% leaves no error budget, so burn-rate
    alerts can never fire.
  - `OBJECTIVE_LOW` - objective below 50% is implausibly low.
  - `PERIOD_TOO_SHORT` - SLO period is not longer than the longest burn-rate
    window (3d in the default MWMBR model), so long-window alerts are meaningless.
  - `NO_ALERT_LABELS` - a page/ticket alert has no labels (e.g. `severity`), so
    Alertmanager routing may not match it.
  - `ALL_ALERTS_DISABLED` - both alerts are disabled; no burn-rate alerts will be
    generated for the SLO.
  - `NO_DESCRIPTION` (info) - the SLO has no description.
- `slokit lint --strict` exits non-zero when any warning-level finding is present
  (CI gate); `--output json` emits the findings as a JSON array.

## [0.5.0]

- Multi-spec (directory) loading and richer `check` output.

## [0.4.0]

- Grafana dashboard generation (`slokit dashboard`).

## [0.3.0]

- Latency SLI helpers (histogram-bucket based latency SLOs).

## [0.2.0]

- Live `check` command querying a Prometheus HTTP API.

## [0.1.0]

- Initial release: error-budget and burn-rate core, sloth-compatible spec
  parsing, and Prometheus MWMBR rule generation.
