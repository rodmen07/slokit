# InfraPortal platform SLOs (a slokit dogfooding example)

SLO definitions, as code, for the [InfraPortal microservices
platform](https://rodmen07.github.io/infraportal) authored with slokit itself.
This is the worked example that proves slokit on a realistic multi-service
platform rather than a single toy spec.

## What is here

- `slos/`: one spec per service (8 services), each with two SLOs:
  - **availability** (an `events` SLI: the ratio of `5xx` responses to all
    responses, from the request-duration histogram's `_count`), and
  - **latency** (a `latency` SLI: the fraction of requests served under the
    service's threshold, generated from the same histogram's buckets).
- `rules.yaml`: the Prometheus recording + multi-window multi-burn-rate (MWMBR)
  page/ticket alert rules slokit generates from `slos/`. 16 SLOs across 8
  services; regenerate with:

  ```sh
  slokit generate -i examples/infraportal/slos/ -o examples/infraportal/rules.yaml
  ```

The set is validated and kept in sync by
[`tests/examples_infraportal.rs`](../../tests/examples_infraportal.rs): every
spec must validate, and `rules.yaml` must be byte-for-byte what slokit produces
today, so it can never silently drift from the generator.

## Service objectives

| Service | Tier | Availability | Latency |
|---|---|---|---|
| accounts, contacts, activities, opportunities | CRM | 99.9% | 99.5% < 300ms |
| automation, integrations | Platform | 99.5% | 99.0% < 500ms |
| reporting | Platform | 99.9% | 99.0% < 800ms |
| search | Platform | 99.9% | 99.5% < 400ms |

## The metric contract (an honest note)

These SLIs read `http_request_duration_seconds` (the standard Prometheus
request-duration histogram an Axum service exports with an OpenTelemetry or
`axum-prometheus` metrics layer), labelled by `job="<service>"` and `code`.

**As of this writing the platform services do not yet expose `/metrics`**, so
these are the platform's SLO *definitions*, ready to activate the moment the
services are instrumented. That is the normal order of operations: agree the
reliability targets first, then wire the metrics to measure them. Deploying
`rules.yaml` before the metrics exist is harmless (the recording rules simply
evaluate over empty vectors) but pointless until instrumentation lands.

To point these specs at metrics you already emit, edit the `error_query` /
`total_query` and the latency `histogram_metric` / `selector` in each spec and
regenerate.
