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
# burn-rate windows, alerts missing routing labels, a `vector(` no-data
# fallback on only one of the two event queries, ...). --strict fails CI.
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

# Export a spec as OpenSLO v1 (see "OpenSLO interop" below)
slokit export -i slos.yaml --format openslo > slos.openslo.yaml

# Read sloth's Kubernetes CRD instead of a native spec, on any -i command
# (auto-detected too; see "sloth Kubernetes CRD input" below)
slokit generate -i k8s-slos.yaml --input-format sloth-crd

# Use sloth's burn-rate window catalogue for the SLO period instead of the
# defaults (see "sloth alert-window catalogues" below)
slokit generate -i slos.yaml --period 7d --alert-windows windows/7d.yaml

# Print the spec JSON Schema (see "Editor integration" below)
slokit schema
```

Every command's `-i` accepts a **single spec file or a directory** of
`*.yaml`/`*.yml` specs. With a directory, `generate` merges all rules into one
document, `check` reports across every service, and `dashboard` emits a JSON
array of dashboards. A file may also be a **multi-document YAML stream**
(`---`-separated, sloth's `multifile.yml` layout or the stream `slokit export`
writes): every document is loaded, and documents behave exactly like separate
files in a directory. Embedders get the same via `Spec::from_yaml_stream`.

A full worked example (SLO specs for an 8-service platform plus the rules slokit
generates from them) lives in [`examples/infraportal/`](examples/infraportal/).

`check` exit codes: `0` healthy, `1` the `--fail-on` level was reached
(`breach` by default, or `warning`/`never`), `2` a runtime error. `--output
json` prints the statuses as a JSON array for piping into other tools.

`dashboard` emits Grafana dashboard JSON with a block per SLO (error budget
remaining, current burn rate, objective, the SLI error ratio over time, and one
burn-rate panel per enabled alert condition with a threshold line at that
condition's burn-rate factor, titled by severity), querying the same `slo:...`
metrics the generator produces. A severity whose alert is disabled gets no burn
panel, mirroring alert generation. It declares a `datasource` template
variable, so it imports into any Grafana with a Prometheus data source.

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

## OpenSLO interop

slokit reads [OpenSLO](https://openslo.com) `kind: SLO` documents in **both**
published API versions and writes them back out in the current one, so a spec
can move in either direction rather than only into slokit.

| direction | `openslo/v1` | `openslo/v1alpha` |
|---|---|---|
| import (`-i`, `--input-format openslo`) | yes | yes, since 1.5.0 |
| export (`slokit export --format openslo`) | yes, always | no: the export writes the current version |

**Importing.** Every `-i` accepts OpenSLO. A file whose first YAML document
sets a top-level `apiVersion: openslo/...` is detected automatically;
`--input-format openslo|slokit|sloth-crd` overrides the detection either way
(the third value is [sloth's Kubernetes CRD](#sloth-kubernetes-crd-input)). The
version is read **per document**, so one multi-document stream may mix the two,
and an `apiVersion` that is neither is still a hard error naming the value.

```sh
slokit generate -i openslo-slos.yaml            # auto-detected
slokit generate -i slos.yaml --input-format openslo
```

`openslo/v1alpha` is the version the [sloth](https://github.com/slok/sloth)
reference examples declare, so those documents import unchanged. It states the
same ideas with a different shape — the metric lives on each objective as
`ratioMetrics.{good,total}` rather than in one document-level `spec.indicator`,
and the period is `timeWindows[0].{count, unit}` rather than
`timeWindow[0].duration` — and slokit reports the same fidelity notes and
fail-closed errors for it as for v1.

**Exporting.** `slokit export` is the inverse: one `kind: SLO` document per
slokit SLO, written to stdout as a multi-document stream, or to a directory as
one `<service>.yaml` per spec.

```sh
slokit export -i slos.yaml --format openslo > slos.openslo.yaml
slokit export -i slos/ -o openslo/            # one file per service
```

`--format` takes only `openslo` today; the flag exists so a second format is
not a breaking change later.

The conversion is a **semantic** round trip, not a byte-for-byte one: the two
models are not isomorphic, so `slokit export | slokit validate` gives back an
equivalent spec with exactly three documented differences, each reported as a
`note:` on stderr (stdout stays a clean YAML stream you can pipe or redirect).

A v1alpha document that goes in comes back out as `openslo/v1`: the trip
upgrades the version rather than preserving it. What is preserved is the
meaning — `slokit generate` produces byte-identical rules before and after the
round trip, which
`tests/openslo_v1alpha_cli.rs::a_v1alpha_document_exports_as_openslo_v1_and_regenerates_the_same_rules`
asserts on both sloth fixtures.

| slokit construct | on export |
|---|---|
| service-level `labels` | merged into every SLO's `metadata.labels`; re-importing leaves them on the SLOs |
| per-SLO `alerting` | dropped: slokit derives MWMBR alerts from the objective, while OpenSLO models alerting as separate `kind: AlertPolicy` documents |
| `version` (the slokit dialect tag) | replaced by `apiVersion: openslo/v1`; re-importing restores the default |

Anything OpenSLO cannot represent is a **hard error naming the field**, never a
silent drop or best-effort YAML a downstream consumer would reject: an
`sli.plugin` (which has no query until the plugin expands it at generation
time), an SLI setting no variant or several, an empty service or SLO name, an
objective outside `(0, 100]`, and a latency SLI whose `threshold` or
`histogram_metric` cannot become an OpenSLO `thresholdMetric`.

Exported queries keep slokit's `{{.window}}` token verbatim. slokit re-imports
it unchanged; any other OpenSLO consumer has to substitute its own lookback.

The library half is `slokit::spec::openslo::to_yaml` (and `to_yaml_reported`,
which keeps the notes), the sibling of `from_yaml`.

## sloth Kubernetes CRD input

slokit reads sloth's Kubernetes custom resource — `apiVersion:
sloth.slok.dev/v1`, `kind: PrometheusServiceLevel` — the shape sloth is used
in inside a cluster, and the shape `slokit generate --format operator` already
*emits*. Five of the twenty-one documents in sloth's own `examples/` are in
this dialect.

```sh
slokit validate -i k8s-getting-started.yaml         # auto-detected
slokit generate -i k8s-getting-started.yaml --input-format sloth-crd
```

**Detection and pinning.** A file whose first YAML document sets a top-level
`apiVersion: sloth.slok.dev/...` is imported as a CRD without being asked;
`--input-format sloth-crd` pins it for every file read, and
`--input-format slokit|openslo` pins it off again. Pinning matters when
detection cannot help — a document with the envelope stripped, a file being
checked deliberately as another dialect — and the failure then names the
dialect it was read as (<code>sloth-crd document 1: missing field `spec`</code>)
rather than a bare field error.

**It is the native model, renamed and wrapped**, not a second model the way
OpenSLO is: `spec.slos[]` under an `apiVersion`/`kind`/`metadata` envelope with
camelCase JSON names (`errorQuery`, `totalQuery`, `errorRatioQuery`,
`pageAlert`, `ticketAlert`). So a CRD document and sloth's own native twin of
it generate **byte-identical** rules — asserted over both twin pairs, in both
output formats, across the whole `--period` / `--no-period-scaling` space, at
the library level in `tests/sloth_crd.rs` and through the real binary in
`tests/sloth_crd_cli.rs`. The full field table is in the
[`slokit::spec::sloth_crd` module docs](https://docs.rs/slokit/latest/slokit/spec/sloth_crd/).

**Ignored with a note** (stderr, so a redirected stdout stays a clean rule
stream): `metadata.name`, `metadata.namespace` and `metadata.labels`. The last
one matters most — those are Kubernetes *object* labels, not rule labels
(`spec.labels` is the rule labels), so honoring them would silently label every
generated rule. `status` is dropped silently: it is the controller's writeback,
never input.

**Fails closed, naming the field**, for anything that would generate the wrong
rules: sloth's SLO plugin chains (`spec.sloPlugins`, `slos[].plugins`), which
slokit has no equivalent for — its `sli.plugin` is a different mechanism and
*is* mapped — and the native snake_case `page_alert` / `ticket_alert` spellings
inside a CRD document, which would otherwise be ignored as unknown keys and
drop the page/ticket severity labels without a word.

The library half is `slokit::spec::sloth_crd` (`is_sloth_crd`, `from_yaml`,
`from_path`), returning the same `Import` / `ImportNote` pair as the OpenSLO
importer. There is no export direction: `generate --format operator` already
writes a Kubernetes resource, a `PrometheusRule`.

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

**`slokit dashboard` takes the same `--period` and `--no-period-scaling` flags,
and it must be given the same values.** The resolved lookback window is part of
the recorded series NAME (`slo:sli_error:ratio_rate5m` versus
`slo:sli_error:ratio_rate1m`), so a dashboard built with different window
options queries series the rules do not record and every burn-rate panel
renders "No data". Library callers pass the same [`GenerateOptions`] to
`dashboard::dashboard_value_with` (and its `_json` siblings) that they passed
to `generate::generate_rules_with`.

[`GenerateOptions`]: https://docs.rs/slokit/latest/slokit/generate/struct.GenerateOptions.html

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

Recording rules, the Grafana dashboard's SLI and burn-rate panels, and the
current-burn-rate metadata rule all follow the effective windows, so the
generated rule set stays self-consistent. `slokit lint` warns when custom windows leave an enabled
severity with no conditions (`NO_SEVERITY_WINDOWS`) or outgrow the SLO period
(`PERIOD_TOO_SHORT`).

### sloth alert-window catalogues (`kind: AlertWindows`)

Where `alerting.windows` sets the conditions for **one SLO**, sloth's
`kind: AlertWindows` document sets them for **one SLO period**, across every
SLO that uses it. slokit reads those catalogues:

```sh
# One catalogue file...
slokit generate -i slos.yaml --period 7d --alert-windows windows/7d.yaml

# ...or a directory of them, one per period
slokit generate -i slos.yaml --alert-windows windows/

# The dashboard takes the same flag, and must: its panels query the series
# those rules record, and the burn-rate window is part of the series name
slokit dashboard -i slos.yaml --period 7d --alert-windows windows/7d.yaml

# Read a catalogue to check it and see the factors it would apply
slokit validate -i windows/7d.yaml
# ok: windows/7d.yaml is a valid AlertWindows catalogue for 7d
#     (page 1h/5m x13.44, page 6h/30m x3.5, ticket 1d/2h x1.4, ticket 3d/6h x0.98)
```

A catalogue states each condition as the share of the error budget it may burn
over the long window; slokit states the same thing as a burn-rate multiplier,
and the two are the same number:

```text
factor = (errorBudgetPercent / 100) x (sloPeriod / longWindow)
```

So sloth's own defaults, written out as a 30-day catalogue, come out as
`14.4 / 6 / 3 / 1` — the table slokit already uses — and generate byte-identical
rules to passing no catalogue at all.

Four rules worth knowing:

- **Precedence.** An SLO's own `alerting.windows` wins over a catalogue, which
  wins over the default table.
- **Catalogue windows are used verbatim**, never period-scaled again: the
  catalogue already states the windows for its own period.
- **A period no catalogue covers is an error**, naming the period and what the
  set does cover, rather than a silent fall back to the defaults.
- **Catalogues arrive on `--alert-windows`, not `-i`.** Passing one to `-i` on
  a command that consumes specs is refused by name, because it carries no SLOs
  and would otherwise generate an empty rules document.

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
