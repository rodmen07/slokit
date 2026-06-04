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

# Do the error-budget math from the terminal
slokit calc --objective 99.9 --period 30d --total 1000000 --bad 250
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

`slokit` reads the `sloth` `prometheus/v1` spec, plus one extension: an optional
per-SLO `period` (sloth only offers this as a global flag).

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
| `cli`   | yes     | `clap`, `anyhow`, `spec` | the `slokit` binary |
| `spec`  | yes     | `serde`, `serde_norway`  | spec parsing and rule generation |

For the lean math-only core: `slokit = { version = "0.1", default-features = false }`.

## The MWMBR model

`slokit` implements the burn-rate alerting from the Google SRE Workbook. For a
30-day SLO period:

| Severity | Long window | Short window | Burn rate | Budget consumed |
|----------|-------------|--------------|-----------|-----------------|
| Page     | 1h          | 5m           | 14.4      | 2%              |
| Page     | 6h          | 30m          | 6         | 5%              |
| Ticket   | 1d          | 2h           | 3         | 10%             |
| Ticket   | 3d          | 6h           | 1         | 10%             |

## License

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
