# Changelog

All notable changes to slokit are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
From 1.0.0, slokit follows the semver guarantees documented in
[docs/SEMVER.md](docs/SEMVER.md): no breaking changes in 1.x.

## [Unreleased]

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
