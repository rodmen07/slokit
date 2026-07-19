# slokit semver policy

What slokit guarantees release to release, written down before the 1.0.0
freeze so "semver" is a checkable contract instead of a vibe. slokit follows
[SemVer 2.0.0](https://semver.org) as interpreted by the
[Cargo semver reference](https://doc.rust-lang.org/cargo/reference/semver.html);
this document pins down the parts that reference leaves to the crate.

## The public API surface

The covered surface is everything documented on
[docs.rs/slokit](https://docs.rs/slokit): the crate root re-exports
(`Slo`, `Objective`, `Window`, `BurnRate`, `ErrorBudget`, `MwmbrConfig`,
`AlertWindow`, `Severity`, `Sli`, `SlokitError`, `Result`, `WINDOW_TOKEN`)
and the public modules `error`, `spec` (with `spec::plugin` and
`spec::openslo`), `generate`, `check`, and `dashboard`. Feature flags
(`cli`, `spec`, `check`, `dashboard`) keep their meaning; existing items
never move to a different feature in 1.x, and new features default to off
unless a minor release says otherwise in the CHANGELOG.

The CLI's flags and subcommands are part of the contract in spirit: existing
invocations keep working across 1.x, while new flags and subcommands may be
added in minors.

## Guarantees in 1.x

- **Patch releases (1.x.y)**: bug fixes only. No API additions or removals,
  no behavior changes except where documented behavior and actual behavior
  disagreed, and no changes to generated rule bytes (see below).
- **Minor releases (1.x.0)**: additive only. New items, new enum variants and
  struct fields behind `#[non_exhaustive]`, new lint codes, new built-in
  plugins, new spec keys, and new CLI flags may appear. Code that compiles
  against 1.a keeps compiling against 1.b for b > a, provided it respects the
  `#[non_exhaustive]` contract below.
- **Major release (2.0.0)**: anything else, including removing or renaming
  items, changing signatures, and semantic redesigns (for example a third
  alert severity, since `Severity` is deliberately exhaustive).

## Generated-output stability

Generated Prometheus rules are consumed by diff-based GitOps pipelines, so
their bytes are part of the contract:

- Within a minor line (1.x.0 through 1.x.y), `slokit generate` output is
  byte-stable for the same input spec and options.
- A minor release may change generated output (new labels, formatting,
  ordering) only with a CHANGELOG entry describing the change.
- The repository's twin tests (plugin expansions and OpenSLO imports proven
  byte-identical to their hand-written equivalents) are the enforcement
  mechanism and stay in CI.

The same applies to `slokit dashboard` JSON and the machine-readable
`--output json` shapes of `lint` and `check`: JSON objects gain fields
additively in minors; existing fields keep their names and meanings.

## Spec format and JSON Schema

- The spec format only grows: YAML that parses and validates under 1.a also
  parses and validates under 1.b for b > a. New keys are optional with
  defaults preserving old behavior.
- `schema/slokit-spec.schema.json` tracks the format additively in 1.x.
- Tag-pinned schema URLs are immutable: the schema at
  `https://raw.githubusercontent.com/rodmen07/slokit/<tag>/schema/slokit-spec.schema.json`
  never changes for a published tag, so editors pinned to a tag never see the
  schema shift under them. The `main` URL floats with development.
- `slokit schema` prints the embedded schema byte-identical to the repo file
  at that release.

## MSRV policy

- The minimum supported Rust version is **1.82**, declared as `rust-version`
  in Cargo.toml and enforced by a dedicated CI job that builds the default,
  all-features, and no-default-features configurations on the pinned
  toolchain.
- An MSRV raise is a **minor** version change, never a patch, and is called
  out in the CHANGELOG under its own heading. slokit does not treat an MSRV
  raise as a major change (the common Rust ecosystem policy), but raises are
  conservative and only made for a concrete need.
- The contract covers slokit's own code in every feature combination, built
  against an MSRV-compatible dependency resolution. The committed Cargo.lock
  tracks current dependency releases, some of which need newer rustc; on
  1.82, resolve with cargo's rust-version-aware resolver
  (`CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo update`) and pin
  `idna_adapter` to 1.1.0 (the upstream-supported switch from the ICU4X
  unicode backend, MSRV 1.83+, to unicode-rs). The CI job runs exactly this
  recipe, so it is checked on every push.
- Dev-dependencies (test tooling) are not part of the MSRV contract.

## Extension points and `#[non_exhaustive]`

The API is shaped so the likely growth axes are non-breaking:

- Enums that report or classify (`SlokitError`, `Sli`, `LintLevel`,
  `StatusLevel`, `OptionKind`) are `#[non_exhaustive]`; always include a
  wildcard arm when matching. `Severity` is deliberately exhaustive: the
  page/ticket split is the MWMBR model itself.
- Structs users receive or configure (`Spec` and the spec DTOs,
  `GenerateOptions`, `MwmbrConfig`, `AlertWindow`, `Lint`, `SloStatus`,
  `Import`, `ImportNote`, `RuleGroup`, `RuleSet`, `OptionSpec`) are
  `#[non_exhaustive]` with public fields: read and mutate fields freely, but
  construct values through the provided constructors, `Default`, or the
  parser, never with struct literals.
- The `SliPlugin` trait is designed for downstream implementations. New
  trait methods in 1.x always ship with default bodies, and the trait stays
  object-safe (`Box<dyn SliPlugin>` keeps working).

## Explicitly not covered

These may change in any release without a semver signal:

- The exact wording of error messages, validation problem lines, lint
  messages, and import notes. Match on `SlokitError` variants and lint
  `code` values (those are stable), not on message text.
- `Debug` formatting of any type.
- The human-readable (non `--output json`) text and table output of the CLI,
  including exact column layout and coloring.
- The internal module structure behind the documented paths. Import items
  from the paths shown on docs.rs.
- Floating-point results beyond documented semantics: bit-for-bit equality of
  intermediate math is not promised, only the documented formulas.

## Pre-1.0 note

0.12.0 is the deliberate final breaking-change window: the
`#[non_exhaustive]` audit and constructor additions landed there so 1.0.0 can
freeze this surface unchanged. Between 0.12.0 and 1.0.0 only fixes and
documentation are planned.
