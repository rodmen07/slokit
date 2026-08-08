//! Does `slokit check` drift from `slokit generate` the way `slokit dashboard`
//! did? This suite is the audit, and it answers the question with the wire.
//!
//! PR #38 found that `dashboard` re-derived the generator's window resolution
//! from the spec alone, so it silently disagreed with `generate` about all
//! seven window-scoped `slo:` series as soon as a non-default option was in
//! play. `check` also has to talk about the same SLOs against the same
//! Prometheus, so the same class was plausible here and had never been looked
//! at. Asserting either way without looking would be exactly the inherited
//! absence claim that keeps biting this repo.
//!
//! **The finding: `check` cannot drift on series NAMES, because it never names
//! a recorded series at all.** It queries the raw SLI expression
//! (`Sli::error_ratio_expr`) directly, so it works against a Prometheus with no
//! slokit recording rules deployed — that is the documented point of the
//! command. [`check_references_none_of_the_series_the_generator_records`] pins
//! that as a two-source fact rather than a grep: it captures every PromQL
//! `check` actually puts on the wire and intersects it with the `record:` keys
//! the generator emits for the same spec under the same options. Empty
//! intersection is the contract.
//!
//! **Two real gaps did fall out, and both are the options-blindness class one
//! layer down from the series names.** Each has a `known_gap_` test below that
//! states the current behaviour so a fix has to update it deliberately:
//!
//! 1. `check`'s "current burn rate" window is whatever `--window` says
//!    (default `1h`, calibrated for a 30-day period), while the generator's
//!    `slo:current_burn_rate:ratio` uses the MWMBR base window SCALED to the
//!    SLO's period. For a 7d SLO those are `1h` and `1m` — the same named
//!    quantity computed over windows 60x apart, one following the period and
//!    one not.
//! 2. `check` resolves plugin SLIs against the built-in registry only. There
//!    is no `check_spec_with`, so an embedder whose spec `validate_with` and
//!    `generate_rules_with` both accept cannot check it at all.
//!
//! What is NOT re-tested here: the period. `check` and `generate` do agree on
//! the resolved SLO period across the option matrix, and
//! [`check_and_generate_resolve_the_same_period`] proves it from both sides.

#![cfg(feature = "check")]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use slokit::check::{check_spec, PrometheusClient, SloStatus};
use slokit::generate::{generate_rules_with, GenerateOptions};
use slokit::spec::plugin::{OptionKind, OptionSpec, SliPlugin, SliPluginRegistry};
use slokit::spec::Spec;
use slokit::{Result, Sli, Window};

/// A successful instant-query response with no samples, so every status comes
/// back `None` and no query fails for a reason unrelated to this suite.
const EMPTY_VECTOR: &str = r#"{"status":"success","data":{"resultType":"vector","result":[]}}"#;

/// A spec that sets no `period`, so the caller's default decides it.
const NO_PERIOD_SPEC: &str = r#"
service: checkaudit
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(errors_total[{{.window}}]))
        total_query: sum(rate(requests_total[{{.window}}]))
"#;

/// A spec whose own `period` is non-default, which is a different resolution
/// path from inheriting the caller's default.
const SEVEN_DAY_SPEC: &str = r#"
service: checkaudit
slos:
  - name: latency
    objective: 99.5
    period: 7d
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#;

// ---------------------------------------------------------------------------
// A Prometheus that records what it was asked.
// ---------------------------------------------------------------------------

/// A loopback listener that answers every instant query with [`EMPTY_VECTOR`]
/// and forwards the decoded `query` parameter of each request down a channel.
struct QuerySpy {
    port: u16,
    rx: mpsc::Receiver<String>,
}

impl QuerySpy {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                // The client sends a bodyless GET, so the headers terminator is
                // the whole request; loop until it arrives so a long PromQL
                // expression split across reads is never truncated.
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            raw.extend_from_slice(&buf[..n]);
                            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let request = String::from_utf8_lossy(&raw).into_owned();
                if tx.send(request).is_err() {
                    break;
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    EMPTY_VECTOR.len(),
                    EMPTY_VECTOR
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { port, rx }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Every query received so far, in arrival order. The spy sends to the
    /// channel BEFORE it writes the response, and the client is blocking, so
    /// once `check_spec` has returned every query it made is already queued.
    fn drain(&self) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(request) = self.rx.try_recv() {
            out.push(query_param(&request));
        }
        out
    }
}

/// Pull `query=` out of the request line and percent-decode it.
fn query_param(request: &str) -> String {
    let line = request.lines().next().unwrap_or_default();
    let target = line
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("no request target in {line:?}"));
    let qs = target
        .split_once('?')
        .unwrap_or_else(|| panic!("no query string in {target:?}"))
        .1;
    for pair in qs.split('&') {
        if let Some(value) = pair.strip_prefix("query=") {
            return percent_decode(value);
        }
    }
    panic!("no `query` parameter in {qs:?}");
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).expect("ascii hex");
                out.push(u8::from_str_radix(hex, 16).expect("valid percent escape"));
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("decoded query is utf-8")
}

/// Run a real `check_spec` against the spy and return what it reported plus
/// every query it sent.
fn run_check(
    spec: &Spec,
    default_period: Window,
    current_window: Window,
) -> (Vec<SloStatus>, Vec<String>) {
    let spy = QuerySpy::spawn();
    let client = PrometheusClient::new(spy.url()).expect("client builds");
    let statuses = check_spec(&client, spec, default_period, current_window)
        .expect("check must succeed against the spy");
    let queries = spy.drain();
    assert!(
        !queries.is_empty(),
        "the spy captured nothing, so this run proves nothing"
    );
    (statuses, queries)
}

// ---------------------------------------------------------------------------
// Corpus and matrices.
// ---------------------------------------------------------------------------

/// Every committed spec, glob-discovered with a zero-match hard failure so an
/// emptied directory cannot pass vacuously, plus the two synthetic specs whose
/// periods are not 30d.
fn audit_specs() -> Vec<(String, Spec)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![
        root.join("tests/fixtures/sample.yaml"),
        root.join("tests/fixtures/multifile.yaml"),
    ];
    let examples = root.join("examples/infraportal/slos");
    let mut example_files: Vec<PathBuf> = std::fs::read_dir(&examples)
        .unwrap_or_else(|e| panic!("reading {}: {e}", examples.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    assert!(
        !example_files.is_empty(),
        "no example specs discovered under {}",
        examples.display()
    );
    example_files.sort();
    files.extend(example_files);

    let mut specs = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        for spec in Spec::from_yaml_stream(&text)
            .unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()))
        {
            specs.push((path.display().to_string(), spec));
        }
    }
    specs.push((
        "<synthetic: no period, so the default decides>".to_string(),
        Spec::from_yaml(NO_PERIOD_SPEC).expect("synthetic spec parses"),
    ));
    specs.push((
        "<synthetic: period 7d>".to_string(),
        Spec::from_yaml(SEVEN_DAY_SPEC).expect("synthetic spec parses"),
    ));
    assert!(specs.len() > 2, "corpus collapsed to the synthetic specs");
    specs
}

/// The `check` invocations a user can reach from the CLI, paired with the
/// `generate` invocation that resolves the same period. `period_aware` has no
/// counterpart on `check` at all, which is the subject of the first known gap
/// below.
fn check_matrix() -> Vec<(&'static str, Window, Window)> {
    vec![
        ("check", Window::days(30), Window::hours(1)),
        ("check --period 7d", Window::days(7), Window::hours(1)),
        ("check --window 5m", Window::days(30), Window::minutes(5)),
        (
            "check --period 90d --window 6h",
            Window::days(90),
            Window::hours(6),
        ),
    ]
}

/// Every series name the generator records for `spec` under `opts`, read from
/// the rendered Prometheus rules YAML (`record:` keys) — the same bytes
/// `slokit generate` writes.
fn recorded_series(spec: &Spec, opts: &GenerateOptions) -> BTreeSet<String> {
    let yaml = generate_rules_with(spec, opts)
        .expect("generation must succeed for a committed spec")
        .to_prometheus_yaml()
        .expect("rendering must succeed");
    let doc: serde_norway::Value = serde_norway::from_str(&yaml).expect("generator output is YAML");
    let mut out = BTreeSet::new();
    for group in doc["groups"].as_sequence().into_iter().flatten() {
        for rule in group["rules"].as_sequence().into_iter().flatten() {
            if let Some(name) = rule.get("record").and_then(|r| r.as_str()) {
                out.insert(name.to_string());
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no recording rules parsed out of the generator's YAML"
    );
    out
}

/// Every `slo:`-prefixed metric name mentioned anywhere in `expr`.
fn series_in(expr: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = expr;
    while let Some(pos) = rest.find("slo:") {
        let tail = &rest[pos..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .unwrap_or(tail.len());
        out.insert(tail[..end].to_string());
        rest = &tail[end..];
    }
    out
}

/// The recording rule the generator emits for `record_name`, as its expression.
fn recorded_expr(spec: &Spec, opts: &GenerateOptions, record_name: &str) -> String {
    let yaml = generate_rules_with(spec, opts)
        .expect("generation must succeed")
        .to_prometheus_yaml()
        .expect("rendering must succeed");
    let doc: serde_norway::Value = serde_norway::from_str(&yaml).expect("generator output is YAML");
    for group in doc["groups"].as_sequence().into_iter().flatten() {
        for rule in group["rules"].as_sequence().into_iter().flatten() {
            if rule.get("record").and_then(|r| r.as_str()) == Some(record_name) {
                return rule["expr"].as_str().expect("expr is a string").to_string();
            }
        }
    }
    panic!("the generator recorded no rule named {record_name}");
}

// ---------------------------------------------------------------------------
// The audit.
// ---------------------------------------------------------------------------

/// The answer to the question this suite was written for.
///
/// The PR #38 drift class needs `check` and `generate` to both NAME a series
/// whose name encodes a resolved window. `check` names none: every query it
/// sends is the SLI's own expression, so the intersection with the generator's
/// `record:` keys is empty for every spec under every invocation. That is what
/// makes `check` usable against a Prometheus with no slokit rules deployed, and
/// it is why no amount of option skew can make the two disagree about a name.
#[test]
fn check_references_none_of_the_series_the_generator_records() {
    let specs = audit_specs();
    let mut checked = 0usize;
    for (label, spec) in &specs {
        for (invocation, default_period, current_window) in check_matrix() {
            let (_, queries) = run_check(spec, default_period, current_window);

            let mut opts = GenerateOptions::default();
            opts.default_period = default_period;
            let recorded = recorded_series(spec, &opts);

            for query in &queries {
                let referenced = series_in(query);
                let overlap: Vec<_> = referenced.intersection(&recorded).cloned().collect();
                assert!(
                    overlap.is_empty(),
                    "`slokit {invocation}` on {label} queries recorded series {overlap:?}, \
                     so its names can now drift from `slokit generate`; this suite's premise \
                     (check reads raw SLI queries only) no longer holds"
                );
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 100,
        "only {checked} queries examined; the corpus or matrix collapsed"
    );
}

/// `check` re-derives its queries from the spec, and this states exactly what
/// it derives: two queries per SLO, the SLI expression at the resolved period
/// and at the current window, in that order. Both sides are real — the left is
/// captured off the wire, the right is computed from the public library API.
#[test]
fn every_query_check_sends_is_the_slis_own_expression() {
    for (label, spec) in audit_specs() {
        for (invocation, default_period, current_window) in check_matrix() {
            let (_, queries) = run_check(&spec, default_period, current_window);

            let mut expected = Vec::new();
            for slo_spec in &spec.slos {
                let period = slo_spec
                    .to_slo(default_period)
                    .expect("committed spec resolves")
                    .period;
                let sli: Sli = slo_spec.to_sli().expect("committed spec resolves");
                expected.push(sli.error_ratio_expr(period));
                expected.push(sli.error_ratio_expr(current_window));
            }

            assert_eq!(
                queries, expected,
                "`slokit {invocation}` on {label} sent queries that are not the SLI expressions \
                 at the resolved period and the current window"
            );
        }
    }
}

/// The one resolution `check` and `generate` genuinely share, proven from both
/// sides: `check`'s reported period equals the window the generator puts in the
/// name of the period-scoped SLI recording (the `sum_over_time(...)` rule).
#[test]
fn check_and_generate_resolve_the_same_period() {
    for (label, spec) in audit_specs() {
        for (invocation, default_period, current_window) in check_matrix() {
            let (statuses, _) = run_check(&spec, default_period, current_window);

            let mut opts = GenerateOptions::default();
            opts.default_period = default_period;
            let yaml = generate_rules_with(&spec, &opts)
                .expect("generation must succeed")
                .to_prometheus_yaml()
                .expect("rendering must succeed");
            let doc: serde_norway::Value =
                serde_norway::from_str(&yaml).expect("generator output is YAML");

            // The period recording is the only SLI rule whose expr averages an
            // already-recorded series over the period.
            let mut generated_periods = Vec::new();
            for group in doc["groups"].as_sequence().into_iter().flatten() {
                for rule in group["rules"].as_sequence().into_iter().flatten() {
                    let (Some(record), Some(expr)) = (
                        rule.get("record").and_then(|r| r.as_str()),
                        rule.get("expr").and_then(|e| e.as_str()),
                    ) else {
                        continue;
                    };
                    if expr.starts_with("sum_over_time(slo:sli_error:ratio_rate") {
                        generated_periods.push(
                            record
                                .strip_prefix("slo:sli_error:ratio_rate")
                                .expect("period recording is a ratio_rate series")
                                .to_string(),
                        );
                    }
                }
            }
            assert_eq!(
                generated_periods.len(),
                spec.slos.len(),
                "expected one period recording per SLO in {label}"
            );

            let checked_periods: Vec<String> =
                statuses.iter().map(|s| s.period.prometheus()).collect();
            assert_eq!(
                checked_periods, generated_periods,
                "`slokit {invocation}` on {label} resolved different SLO periods than \
                 `slokit generate` with the same --period"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Known gaps. Each test states today's behaviour; closing the gap must update
// it deliberately rather than letting a green suite hide the change.
// ---------------------------------------------------------------------------

/// **KNOWN GAP (filed as a MED bug on the crates backlog, 2026-08-08).**
/// `check`'s "current burn rate" is computed over `--window`, whose default
/// `1h` is the un-scaled 30-day-calibrated base window, while the generator's
/// `slo:current_burn_rate:ratio` uses the MWMBR base window scaled to the SLO's
/// own period. For a 7d SLO the generator says `1m` and `check` says `1h`, so
/// the CLI's BURN column and the Grafana panel of the same name are computed
/// over windows 60x apart and neither says so.
///
/// Both numbers are read from the shipped product: `1h` off `slokit check
/// --help`, `1m` out of the generator's own emitted expression. When `check`
/// learns to follow the period this test fails, which is the point.
#[test]
#[cfg(feature = "cli")]
fn known_gap_check_burn_window_ignores_the_generators_period_scaled_base_window() {
    let spec = Spec::from_yaml(SEVEN_DAY_SPEC).expect("synthetic spec parses");
    let opts = GenerateOptions::default();
    assert!(
        opts.period_aware,
        "the gap is stated against the default, which scales windows to the period"
    );

    // The generator's side: which series feeds slo:current_burn_rate:ratio.
    let expr = recorded_expr(&spec, &opts, "slo:current_burn_rate:ratio");
    let generator_window = series_in(&expr)
        .into_iter()
        .find_map(|s| {
            s.strip_prefix("slo:sli_error:ratio_rate")
                .map(str::to_string)
        })
        .expect("the burn-rate rule reads a window-scoped SLI recording");
    assert_eq!(
        generator_window, "1m",
        "the 30d base window (5m) scaled to a 7d period is 1m; if the scaling \
         changed, restate the gap rather than relaxing it"
    );

    // The CLI's side: the default `check` actually ships.
    let cli_default = cli_flag_default("check", "--window");
    assert_eq!(
        cli_default, "1h",
        "`slokit check --window` no longer defaults to 1h"
    );

    assert_ne!(
        generator_window, cli_default,
        "GAP CLOSED: `slokit check` and the generated slo:current_burn_rate:ratio now use \
         the same window for a 7d SLO. Delete this test and the backlog bug it cites."
    );

    // And the wire confirms `check` really uses the CLI window, not the base.
    let (_, queries) = run_check(&spec, Window::days(30), Window::hours(1));
    let sli = spec.slos[0].to_sli().expect("synthetic spec resolves");
    assert_eq!(
        queries[1],
        sli.error_ratio_expr(Window::hours(1)),
        "check's current-window query should be the SLI at the CLI window"
    );
    assert_ne!(
        queries[1],
        sli.error_ratio_expr(Window::minutes(1)),
        "check's current-window query should NOT be the generator's scaled base window"
    );
}

/// **KNOWN GAP (filed as a MED bug on the crates backlog, 2026-08-08).**
/// `SloSpec::to_sli_with`, `Spec::validate_with` and `GenerateOptions::plugins`
/// all let an embedder bring a custom SLI plugin registry. `check` has no such
/// entry point: `check_slo` calls `SloSpec::to_sli()`, which hardcodes the
/// built-in registry, and `check_spec` validates with the built-ins first, so a
/// spec that `validate_with` and `generate_rules_with` both accept cannot be
/// checked at all. `tests/plugin.rs` proves the flow-through for validate and
/// generate; this proves `check` is the one path it does not reach.
#[test]
fn known_gap_check_cannot_see_a_custom_plugin_registry() {
    const PLUGIN_SPEC: &str = r#"
service: checkaudit
slos:
  - name: plugged
    objective: 99.9
    sli:
      plugin:
        id: acme/static-ratio
        options:
          metric: app:error_ratio
"#;

    struct StaticRatio;
    impl SliPlugin for StaticRatio {
        fn id(&self) -> &str {
            "acme/static-ratio"
        }
        fn description(&self) -> &str {
            "an embedder's ratio metric"
        }
        fn options(&self) -> &[OptionSpec] {
            const OPTIONS: &[OptionSpec] = &[OptionSpec::new(
                "metric",
                OptionKind::String,
                "name of the recorded error-ratio metric",
            )
            .required()];
            OPTIONS
        }
        fn expand(&self, options: &std::collections::BTreeMap<String, String>) -> Result<Sli> {
            Ok(Sli::Raw {
                error_ratio_query: format!(
                    "avg_over_time({}[{}])",
                    options["metric"],
                    slokit::WINDOW_TOKEN
                ),
            })
        }
    }

    let spec = Spec::from_yaml(PLUGIN_SPEC).expect("spec parses");
    let mut registry = SliPluginRegistry::empty();
    registry.register(Box::new(StaticRatio)).expect("registers");

    // Two of the three public entry points take the registry.
    spec.validate_with(&registry)
        .expect("validate_with accepts the embedder's plugin");
    let mut opts = GenerateOptions::default();
    opts.plugins = Arc::new(registry);
    generate_rules_with(&spec, &opts).expect("generate_rules_with accepts the embedder's plugin");

    // `check` is the third, and it has no registry parameter to give.
    let spy = QuerySpy::spawn();
    let client = PrometheusClient::new(spy.url()).expect("client builds");
    let err = check_spec(&client, &spec, Window::days(30), Window::hours(1))
        .expect_err(
            "GAP CLOSED: check_spec now resolves a custom plugin registry. Delete this test \
             and the backlog bug it cites.",
        )
        .to_string();
    assert!(
        err.contains("unknown SLI plugin 'acme/static-ratio'"),
        "expected the built-in registry to reject the embedder's plugin, got: {err}"
    );
    assert!(
        spy.drain().is_empty(),
        "check should fail before it queries Prometheus"
    );
}

/// Read `[default: X]` for `flag` out of the real binary's help for `command`.
#[cfg(feature = "cli")]
fn cli_flag_default(command: &str, flag: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args([command, "--help"])
        .output()
        .expect("slokit runs");
    assert!(
        out.status.success(),
        "`slokit {command} --help` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let help = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = help.lines().collect();
    let start = lines
        .iter()
        .position(|l| is_flag_heading(l) && l.contains(flag))
        .unwrap_or_else(|| panic!("`slokit {command} --help` does not mention {flag}"));
    // clap wraps each option into a heading line plus an indented block, so the
    // default sits several lines below the flag itself; stop at the next flag.
    let marker = "[default: ";
    for line in lines.iter().skip(start + 1) {
        if is_flag_heading(line) {
            break;
        }
        if let Some(pos) = line.find(marker) {
            let rest = &line[pos + marker.len()..];
            let end = rest
                .find(']')
                .unwrap_or_else(|| panic!("unterminated default for {flag}: {line:?}"));
            return rest[..end].to_string();
        }
    }
    panic!("`slokit {command} --help` advertises no default for {flag}");
}

/// A clap option heading (`  -u, --url <URL>`), as opposed to a wrapped help
/// line or a `- value: description` possible-values entry.
#[cfg(feature = "cli")]
fn is_flag_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("- ")
}
