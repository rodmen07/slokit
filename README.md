# slokit

[![CI](https://github.com/rodmen07/slokit/actions/workflows/ci.yml/badge.svg)](https://github.com/rodmen07/slokit/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/slokit.svg)](https://crates.io/crates/slokit)
[![docs.rs](https://img.shields.io/docsrs/slokit)](https://docs.rs/slokit)

An SLO and error-budget engine for Rust.

`slokit` does two things the existing tools (all Go or Python) do not do together:

1. **Library core** with no `serde`, YAML, or CLI dependencies, so error-budget
   and burn-rate math embeds directly inside your services (for example, an Axum
   handler that reports live budget status).
2. **A generator** that reads a [sloth](https://sloth.dev)-compatible YAML spec
   and emits Prometheus recording rules, metadata rules, and multi-window
   multi-burn-rate (MWMBR) page/ticket alerts as a single static binary.

It is **drop-in compatible** with the `sloth` `prometheus/v1` spec, so existing
specs work unchanged, and the generated metrics use the same `slo:...` names and
`sloth_*` labels, so your Grafana dashboards keep working.

Planned work through the 1.0 API freeze lives in [ROADMAP.md](ROADMAP.md).

## Install

```sh
cargo install slokit          # the CLI
cargo add slokit              # the library (add `--no-default-features` for the lean core)
```

## CLI

```sh
# Generate Prometheus rules from a spec
slokit generate -i slos.yaml -o rules.yaml

# Generate a Prometheus Operator PrometheusRule instead
slokit generate -i slos.yaml --format operator

# Validate a spec without generating
slokit validate -i slos.yaml

# Lint a spec for advisory issues (100% objective, period shorter than the
# burn-rate windows, alerts missing routing labels, ...). --strict fails CI.
slokit lint -i slos.yaml --strict

# Do the error-budget math from the terminal
slokit calc --objective 99.9 --period 30d --total 1000000 --bad 250

# "What if" planning: if the service sustains a 0.5% error rate, how fast does
# the budget burn and which page/ticket alerts fire? (--traffic adds event counts)
slokit simulate --objective 99.9 --error-rate 0.5 --traffic 100

# Check a live Prometheus and report current budget/burn (exits 1 if any SLO breaches)
slokit check -i slos.yaml --url http://localhost:9090 --window 1h

# Check machine-readably, failing the build on warnings too
slokit check -i slos/ --url http://localhost:9090 --output json --fail-on warning

# Generate a Grafana dashboard (JSON) from a spec
slokit dashboard -i slos.yaml -o dashboard.json

# Print the spec JSON Schema (see "Editor integration" below)
slokit schema
```

Every command's `-i` accepts a **single spec file or a directory** of
`*.yaml`/`*.yml` specs. With a directory, `generate` merges all rules into one
document, `check` reports across every service, and `dashboard` emits a JSON
array of dashboards.

`check` exit codes: `0` healthy, `1` the `--fail-on` level was reached
(`breach` by default, or `warning`/`never`), `2` a runtime error. `--output
json` prints the statuses as a JSON array for piping into other tools.

`dashboard` emits Grafana dashboard JSON with a block per SLO (error budget
remaining, current burn rate, objective, and the SLI error ratio over time),
querying the same `slo:...` metrics the generator produces. It declares a
`datasource` template variable, so it imports into any Grafana with a Prometheus
data source.

`check` evaluates each SLO's SLI directly against Prometheus (no deployed
recording rules required) and prints a status table:

```text
service 'myservice' against http://localhost:9090 (current window 1h)

STATUS  SLO                               CONSUMED  REMAINING      BURN
OK      requests-availability               12.30%     87.70%     0.50x
BREACH  requests-latency                   120.00%    -20.00%    15.00x
```

`calc` output:

```text
Objective:    99.9% over 30d
Error budget: 0.1000% of events
Total events: 1000000
Allowed bad:  1000.00
Observed bad: 250
Burn rate:    0.25x
Consumed:     25.0000%
Remaining:    75.0000%
Exhausted in: 89d 23h

Burn-rate alert thresholds (error ratio that fires each window):
  page   long=1h   short=5m   factor=14.4  threshold=1.4400%
  page   long=6h   short=30m  factor=6     threshold=0.6000%
  ticket long=1d   short=2h   factor=3     threshold=0.3000%
  ticket long=3d   short=6h   factor=1     threshold=0.1000%
```

## Spec format

`slokit` reads the `sloth` `prometheus/v1` spec, plus slokit extensions: an
optional per-SLO `period` (sloth only offers this as a global flag), a
`latency` SLI, SLI plugins via `sli.plugin`, and custom burn-rate windows via
`alerting.windows` (all described below).

```yaml
version: "prometheus/v1"
service: myservice
labels:
  owner: team-platform
slos:
  - name: requests-availability
    objective: 99.9
    period: 30d            # slokit extension; defaults to 30d
    sli:
      events:
        error_query: sum(rate(http_requests_total{code=~"5.."}[{{.window}}]))
        total_query: sum(rate(http_requests_total[{{.window}}]))
    alerting:
      name: MyServiceHighErrorRate
      page_alert:
        labels: { severity: page }
      ticket_alert:
        labels: { severity: ticket }
```

Each SLO has exactly one of four SLI shapes:

- `events` (`error_query` / `total_query`): bad events over total events.
- `raw` (`error_ratio_query`): a query that already yields an error ratio.
- `latency` (slokit extension): the fraction of requests slower than a
  histogram bucket threshold. slokit generates the bucket math so you do not
  hand-write it:

  ```yaml
  sli:
    latency:
      histogram_metric: http_request_duration_seconds  # base name, no _bucket/_count suffix
      threshold: "0.3"                                  # the `le` bucket boundary
      selector: job="myservice"                         # optional label matchers, no braces
  ```

  This generates, at every window:

  ```promql
  1 - (
    sum(rate(http_request_duration_seconds_bucket{job="myservice", le="0.3"}[{{.window}}]))
    /
    sum(rate(http_request_duration_seconds_count{job="myservice"}[{{.window}}]))
  )
  ```

- `plugin` (`id` / `options`): a reusable SLI template from the plugin
  registry, expanded to one of the shapes above before validation and
  generation (see [SLI plugins](#sli-plugins)).

The `events` and `raw` query strings must contain the `{{.window}}` template
token; `latency` is generated and needs none.

### SLI plugins

Instead of copy-pasting the same availability query with only the `job`
selector changed, reference a named SLI template by id plus options:

```yaml
sli:
  plugin:
    id: slokit/availability/http-requests-total
    options:
      selector: job="api"
      error_code_regex: "5..|429"
```

A plugin expands into an ordinary `events`/`raw` SLI before validation, so a
plugin spec and its hand-written equivalent generate byte-identical rules.
Unknown plugin ids and broken option values are hard validation errors; option
names a plugin does not declare are an advisory lint (`PLUGIN_UNKNOWN_OPTION`).

Built-in plugins (the `slokit/` id namespace):

| id | what it measures | options (all optional) |
|----|------------------|-------------------------|
| `slokit/availability/http-requests-total` | availability from an `http_requests_total`-style counter; responses matching the error-code regex are bad events | `metric` (default `http_requests_total`), `selector`, `error_code_regex` (default `5..`) |
| `slokit/availability/grpc-server-handled` | availability from a `grpc_server_handled_total`-style counter; a `grpc_code` outside the success allowlist regex is a bad event | `metric` (default `grpc_server_handled_total`), `selector`, `success_code_regex` (default `OK`) |

**sloth compatibility, honestly:** only the `sli.plugin: {id, options}` spec
shape is sloth-compatible. sloth SLI plugins are Go source files executed at
runtime; slokit will never load or execute them, and it does not mirror the
`sloth-common/...` plugin catalog or its ids (that would imply
option-for-option behavioral equivalence with Go code slokit does not run). A
spec written against sloth's plugin catalog therefore fails with a clear
"unknown SLI plugin" error rather than silently generating different rules.

Rust embedders can register their own plugins by implementing the
`slokit::spec::plugin::SliPlugin` trait on a `SliPluginRegistry` and passing
the registry through the `_with` entry points (`Spec::validate_with`,
`SloSpec::to_sli_with`, `Spec::lint_with`) and `GenerateOptions::plugins`; see
the `slokit::spec::plugin` module docs for a worked example. External plugin
definition files (YAML/WASM) are deliberately out of scope for 0.9.

## Editor integration (JSON Schema)

The spec format ships as a JSON Schema (draft 2020-12) at
[`schema/slokit-spec.schema.json`](schema/slokit-spec.schema.json), covering
the sloth-compatible shape and every slokit extension (`period`, the `latency`
SLI, `sli.plugin`, `alerting.windows`). Wiring it into your editor gives
autocomplete, hover docs, and inline validation while you type. The schema
encodes structural rules only; `slokit validate` stays authoritative for
cross-field semantics it cannot express (duplicate SLO names, `short` shorter
than `long` in custom windows, plugin id resolution, quote balance in latency
selectors, and so on).

Get the schema without cloning the repo:

```sh
slokit schema                                # print to stdout
slokit schema -o slokit-spec.schema.json     # write to a file
```

or reference it by raw GitHub URL (substitute a release tag for `main` to pin
a version):

```text
https://raw.githubusercontent.com/rodmen07/slokit/main/schema/slokit-spec.schema.json
https://raw.githubusercontent.com/rodmen07/slokit/<tag>/schema/slokit-spec.schema.json
```

**VS Code** (the YAML extension, powered by `yaml-language-server`), in
`settings.json`:

```json
{
  "yaml.schemas": {
    "https://raw.githubusercontent.com/rodmen07/slokit/main/schema/slokit-spec.schema.json": [
      "slos.yaml",
      "slos/*.yaml"
    ]
  }
}
```

**Any editor running `yaml-language-server`** (Neovim, Helix, Zed, ...): add a
modeline at the top of the spec file itself:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/rodmen07/slokit/main/schema/slokit-spec.schema.json
version: "prometheus/v1"
service: myservice
slos: [...]
```

## Library

The core has no serialization or CLI dependencies:

```rust
use slokit::{Objective, Slo, BurnRate, Window};

let slo = Slo::new(Objective::percent(99.9).unwrap(), Window::days(30));

// With a million events, 0.1% may fail: ~1,000 allowed failures.
let budget = slo.error_budget(1_000_000.0);
assert!((budget.allowed_bad_events() - 1_000.0).abs() < 1e-6);

// A sustained 1% error rate is a 10x burn against a 99.9% objective.
let burn = BurnRate::from_error_ratio(0.01, &slo);
assert!((burn.value() - 10.0).abs() < 1e-9);
```

Generation lives behind the default `spec` feature:

```rust
use slokit::spec::Spec;
use slokit::generate::generate_rules;

let spec = Spec::from_path("slos.yaml")?;
let ruleset = generate_rules(&spec)?;
println!("{}", ruleset.to_prometheus_yaml()?);
# Ok::<(), slokit::SlokitError>(())
```

### Feature flags

| Feature | Default | Pulls in | Enables |
|---------|---------|----------|---------|
| `cli`   | yes     | `clap`, `anyhow`, `spec`, `check`, `dashboard` | the `slokit` binary |
| `spec`  | yes     | `serde`, `serde_norway`  | spec parsing and rule generation |
| `check` | yes     | `reqwest`, `serde_json`  | live Prometheus querying (`PrometheusClient`, `check_spec`) |
| `dashboard` | yes | `serde_json`             | Grafana dashboard generation (`dashboard_json`) |

For the lean math-only core: `slokit = { version = "0.12", default-features = false }`.

## The MWMBR model

`slokit` implements the burn-rate alerting from the Google SRE Workbook. For a
30-day SLO period:

| Severity | Long window | Short window | Burn rate | Budget consumed |
|----------|-------------|--------------|-----------|-----------------|
| Page     | 1h          | 5m           | 14.4      | 2%              |
| Page     | 6h          | 30m          | 6         | 5%              |
| Ticket   | 1d          | 2h           | 3         | 10%             |
| Ticket   | 3d          | 6h           | 1         | 10%             |

### Period-aware windows

The table above is calibrated for a 30-day period. When an SLO uses a different
`period`, slokit scales every lookback window proportionally (rounded to whole
minutes, never below 1m) while keeping the burn-rate factors, so each condition
still fires after consuming the same fraction of the budget. A 90d SLO pages on
3h/15m and 18h/90m windows, and tickets on 3d/6h and 9d/18h.

Pass `--no-period-scaling` to `slokit generate` (or set
`GenerateOptions::period_aware = false`) to use the 30d table verbatim for
every SLO.

### Custom burn-rate windows

Per SLO, `alerting.windows` (a slokit extension) replaces the default table
entirely:

```yaml
alerting:
  labels: { team: platform }
  windows:
    - severity: page      # `page` or `ticket`
      long: 30m
      short: 5m
      factor: 10          # burn-rate multiplier that fires this condition
    - severity: ticket
      long: 12h
      short: 1h
      factor: 2
```

Recording rules, the Grafana dashboard's SLI panel, and the current-burn-rate
metadata rule all follow the effective windows, so the generated rule set stays
self-consistent. `slokit lint` warns when custom windows leave an enabled
severity with no conditions (`NO_SEVERITY_WINDOWS`) or outgrow the SLO period
(`PERIOD_TOO_SHORT`).

## Stability and MSRV

The minimum supported Rust version is **1.82** (declared in Cargo.toml and
enforced by a dedicated CI job). The semver contract for the 1.x line is
written down in [docs/SEMVER.md](docs/SEMVER.md): what the public API covers,
byte-stability of generated rules within a minor line, spec format and JSON
Schema growth rules (tag-pinned schema URLs never change), the MSRV bump
policy (minor version, announced in the CHANGELOG), and what is explicitly
not covered (message wording, `Debug` output, human-readable CLI text).

Most public enums and structs are `#[non_exhaustive]` so the API can grow
without breaking changes: use a wildcard arm when matching, and build values
with the provided constructors or `Default` instead of struct literals.
0.12.0 was the deliberate final breaking-change window before the 1.0.0
freeze.

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
