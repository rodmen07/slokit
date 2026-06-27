# Changelog

All notable changes to slokit are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
slokit is pre-1.0; minor versions may include additive changes and, where noted,
small breaking changes.

## [Unreleased]

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
