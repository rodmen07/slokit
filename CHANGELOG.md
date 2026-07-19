# Changelog

All notable changes to slokit are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
slokit is pre-1.0; minor versions may include additive changes and, where noted,
small breaking changes.

## [Unreleased]

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
