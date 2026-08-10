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
//! **Two real gaps fell out of the original audit, both the options-blindness
//! class one layer down from the series names.**
//!
//! 1. `check`'s "current burn rate" window was whatever `--window` said
//!    (default `1h`, calibrated for a 30-day period), while the generator's
//!    `slo:current_burn_rate:ratio` uses the MWMBR base window SCALED to the
//!    SLO's period — `1h` versus `1m` on a 7d SLO, 60x apart. **CLOSED by
//!    v1.9.0 PR 1 (the window seam):** [`slokit::check::BurnWindow::Rules`]
//!    resolves the window per SLO through the generator's own seam, and the
//!    agreement is now held across the reachable option space by
//!    [`rules_window_check_and_generate_agree_on_the_burn_window`]. The
//!    `known_gap_` test that pinned the old behaviour is deleted, as its own
//!    failure message mandated.
//! 2. `check` resolved plugin SLIs against the built-in registry only:
//!    `CheckOptions` carried no registry, so an embedder whose spec
//!    `validate_with` and `generate_rules_with` both accept could not check
//!    it at all. **CLOSED by v1.9.0 PR 2:** [`slokit::check::CheckOptions`]
//!    gained `plugins`, `check_spec_with` validates against that same
//!    registry, and the flow-through is proven end to end on the wire by
//!    [`embedder_registry_reaches_check_end_to_end`], with
//!    [`check_validates_on_the_given_registry_not_the_builtins`] pinning that
//!    validation runs on the registry the caller gave. The `known_gap_` test
//!    that pinned the old behaviour is deleted, as its own failure message
//!    mandated.
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

use slokit::check::{
    check_spec, check_spec_with, BurnWindow, CheckOptions, PrometheusClient, SloStatus,
};
use slokit::generate::{generate_rules_with, GenerateOptions};
use slokit::spec::alert_windows::AlertWindowsSet;
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

/// Run a real `check_spec_with` against the spy and return what it reported
/// plus every query it sent.
fn run_check_with(spec: &Spec, opts: &CheckOptions) -> (Vec<SloStatus>, Vec<String>) {
    let spy = QuerySpy::spawn();
    let client = PrometheusClient::new(spy.url()).expect("client builds");
    let statuses =
        check_spec_with(&client, spec, opts).expect("check must succeed against the spy");
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

/// The window inside each `slo:current_burn_rate:ratio` rule's numerator, one
/// per SLO in document order, read off the same rendered YAML `slokit
/// generate` writes. This is the generator's half of the window agreement:
/// the string after `ratio_rate` in `slo:sli_error:ratio_rate<window>`.
fn burn_windows_from_rules(spec: &Spec, opts: &GenerateOptions) -> Vec<String> {
    let yaml = generate_rules_with(spec, opts)
        .expect("generation must succeed")
        .to_prometheus_yaml()
        .expect("rendering must succeed");
    let doc: serde_norway::Value = serde_norway::from_str(&yaml).expect("generator output is YAML");
    let mut out = Vec::new();
    for group in doc["groups"].as_sequence().into_iter().flatten() {
        for rule in group["rules"].as_sequence().into_iter().flatten() {
            if rule.get("record").and_then(|r| r.as_str()) != Some("slo:current_burn_rate:ratio") {
                continue;
            }
            let expr = rule["expr"].as_str().expect("expr is a string");
            let window = series_in(expr)
                .into_iter()
                .find_map(|s| {
                    s.strip_prefix("slo:sli_error:ratio_rate")
                        .map(str::to_string)
                })
                .expect("the burn-rate rule reads a window-scoped SLI recording");
            out.push(window);
        }
    }
    assert_eq!(
        out.len(),
        spec.slos.len(),
        "expected one slo:current_burn_rate:ratio rule per SLO"
    );
    out
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

/// The option cells a `--rules-window` user can reach, shared by the window
/// agreement test below. Each cell mutates a `CheckOptions` and a
/// `GenerateOptions` from the SAME inputs, which is the invocation-pairing the
/// CLI performs; the assertion that the cells are not degenerate (each one
/// changes the answer somewhere) lives in the test.
fn rules_window_matrix() -> Vec<(&'static str, Window, bool, Option<PathBuf>)> {
    let catalogue_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sloth_corpus/windows");
    vec![
        // (label, default_period, period_aware, alert_windows dir)
        ("default", Window::days(30), true, None),
        ("--period 7d", Window::days(7), true, None),
        ("--no-period-scaling", Window::days(30), false, None),
        (
            "--alert-windows tests/fixtures/sloth_corpus/windows",
            Window::days(30),
            true,
            Some(catalogue_dir),
        ),
    ]
}

/// **Done-when clause 1 of v1.9.0 (the window seam), read from BOTH real
/// artifacts.** Under [`BurnWindow::Rules`], the window `check` puts on the
/// wire (captured by the spy) and states in each `SloStatus::current_window`
/// is the SAME window string the emitted `slo:current_burn_rate:ratio`
/// numerator names, for a 30d and a 7d spec, across the option matrix a user
/// can reach: default, `--period 7d`, `--no-period-scaling`, and
/// `--alert-windows` with a committed catalogue.
///
/// The guard runs across the option space rather than at the default point,
/// because both sides share `resolve_mwmbr` only if `check` actually routes
/// through it — a check-local re-derivation would agree under defaults and
/// drift under exactly these options (the drift class PR #38 removed from
/// `dashboard`).
#[test]
fn rules_window_check_and_generate_agree_on_the_burn_window() {
    let specs = vec![
        (
            "<synthetic: no period, so the default decides>".to_string(),
            Spec::from_yaml(NO_PERIOD_SPEC).expect("synthetic spec parses"),
        ),
        (
            "<synthetic: period 7d>".to_string(),
            Spec::from_yaml(SEVEN_DAY_SPEC).expect("synthetic spec parses"),
        ),
        (
            "tests/fixtures/alert_windows/mixed-periods.yaml (7d + 30d SLOs)".to_string(),
            Spec::from_yaml_stream(
                &std::fs::read_to_string(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/alert_windows/mixed-periods.yaml"),
                )
                .expect("fixture reads"),
            )
            .expect("fixture parses")
            .remove(0),
        ),
    ];

    let mut agreements = 0usize;
    for (label, spec) in &specs {
        for (cell, default_period, period_aware, catalogue) in rules_window_matrix() {
            let alert_windows = match &catalogue {
                Some(dir) => AlertWindowsSet::load(dir).expect("committed catalogue loads"),
                None => AlertWindowsSet::new(),
            };

            let mut gen_opts = GenerateOptions::default();
            gen_opts.default_period = default_period;
            gen_opts.period_aware = period_aware;
            gen_opts.alert_windows = alert_windows.clone();
            let generated = burn_windows_from_rules(spec, &gen_opts);

            let mut check_opts = CheckOptions::default();
            check_opts.default_period = default_period;
            check_opts.period_aware = period_aware;
            check_opts.alert_windows = alert_windows;
            check_opts.burn_window = BurnWindow::Rules;
            let (statuses, queries) = run_check_with(spec, &check_opts);

            assert_eq!(statuses.len(), generated.len());
            for (i, (status, generated_window)) in statuses.iter().zip(&generated).enumerate() {
                // The stated side: what `check` reports per SLO.
                assert_eq!(
                    &status.current_window.prometheus(),
                    generated_window,
                    "`check --rules-window {cell}` on {label} states a different burn window \
                     than the emitted slo:current_burn_rate:ratio for SLO #{i}"
                );
                // The wire side: what `check` actually asked Prometheus.
                let sli: Sli = spec.slos[i].to_sli().expect("spec resolves");
                let expected_window =
                    Window::parse(generated_window).expect("generated window parses");
                assert_eq!(
                    queries[2 * i + 1],
                    sli.error_ratio_expr(expected_window),
                    "`check --rules-window {cell}` on {label} queried a different window than \
                     the emitted slo:current_burn_rate:ratio for SLO #{i}"
                );
                agreements += 1;
            }
        }
    }
    assert!(
        agreements >= 16,
        "only {agreements} window agreements examined; the corpus or matrix collapsed"
    );
}

/// The matrix above is not degenerate: every non-default cell observably
/// CHANGES the resolved window somewhere, and rules resolution observably
/// differs from the fixed `1h` default (done-when clause 2's second
/// direction). Exact strings are asserted so a silent fallback (a catalogue
/// that failed to load, a scaling flag that stopped reaching the resolver)
/// cannot turn the agreement test vacuous — the values are re-derived from
/// `MwmbrConfig::sre_default()` (shortest lookback 5m at 30d, scaled to 1m at
/// 7d) and the committed 7d catalogue (shortest `shortWindow: 5m`, applied
/// verbatim).
#[test]
fn rules_window_cells_each_change_the_resolved_window() {
    let seven_day = Spec::from_yaml(SEVEN_DAY_SPEC).expect("synthetic spec parses");
    let no_period = Spec::from_yaml(NO_PERIOD_SPEC).expect("synthetic spec parses");

    let resolved = |spec: &Spec, cell: &str| -> String {
        let (_, default_period, period_aware, catalogue) = rules_window_matrix()
            .into_iter()
            .find(|(label, ..)| *label == cell)
            .expect("cell exists");
        let mut opts = CheckOptions::default();
        opts.default_period = default_period;
        opts.period_aware = period_aware;
        opts.alert_windows = match catalogue {
            Some(dir) => AlertWindowsSet::load(&dir).expect("committed catalogue loads"),
            None => AlertWindowsSet::new(),
        };
        opts.burn_window = BurnWindow::Rules;
        let (statuses, _) = run_check_with(spec, &opts);
        statuses[0].current_window.prometheus()
    };

    // 7d SLO, default cell: the 30d base (5m) scaled to 7d is 1m — and it
    // observably differs from the fixed default 1h (clause 2).
    assert_eq!(resolved(&seven_day, "default"), "1m");
    assert_ne!(resolved(&seven_day, "default"), "1h");
    // --period 7d re-periods the no-period spec: 5m at 30d becomes 1m at 7d.
    assert_eq!(resolved(&no_period, "default"), "5m");
    assert_eq!(resolved(&no_period, "--period 7d"), "1m");
    // --no-period-scaling uses the 30d table verbatim on the 7d SLO.
    assert_eq!(resolved(&seven_day, "--no-period-scaling"), "5m");
    // The committed 7d catalogue's shortest lookback (5m, verbatim) differs
    // from the scaled table's 1m, so the catalogue arm is really exercised.
    assert_eq!(
        resolved(
            &seven_day,
            "--alert-windows tests/fixtures/sloth_corpus/windows"
        ),
        "5m"
    );
}

/// **Done-when clause 2, first direction: an explicit fixed window is used
/// verbatim on the wire and stated per SLO,** never silently re-resolved the
/// rules' way.
#[test]
fn an_explicit_fixed_window_is_used_verbatim_and_stated() {
    let spec = Spec::from_yaml(SEVEN_DAY_SPEC).expect("synthetic spec parses");
    let mut opts = CheckOptions::default();
    opts.burn_window = BurnWindow::Fixed(Window::minutes(5));
    let (statuses, queries) = run_check_with(&spec, &opts);

    assert_eq!(statuses[0].current_window, Window::minutes(5), "stated");
    let sli = spec.slos[0].to_sli().expect("spec resolves");
    assert_eq!(
        queries[1],
        sli.error_ratio_expr(Window::minutes(5)),
        "used verbatim on the wire"
    );
    assert_ne!(
        queries[1],
        sli.error_ratio_expr(Window::minutes(1)),
        "not silently re-resolved to the rules window (1m for a 7d SLO)"
    );
}

/// **Done-when clause 3: `CheckOptions::default()` reproduces the pre-seam
/// behavior exactly.** The old entry point and the new one produce identical
/// wire queries and an identical serialized report for every audit spec, and
/// the default is the documented `Fixed(1h)` over a 30d default period.
#[test]
fn default_options_reproduce_the_pre_seam_behavior() {
    let opts = CheckOptions::default();
    assert_eq!(opts.default_period, Window::days(30));
    assert_eq!(opts.burn_window, BurnWindow::Fixed(Window::hours(1)));
    assert!(opts.period_aware);

    for (label, spec) in audit_specs() {
        let (old_statuses, old_queries) = run_check(&spec, Window::days(30), Window::hours(1));
        let (new_statuses, new_queries) = run_check_with(&spec, &CheckOptions::default());
        assert_eq!(
            old_queries, new_queries,
            "default CheckOptions sent different wire queries than check_spec on {label}"
        );
        assert_eq!(
            serde_json::to_string(&old_statuses).expect("serializes"),
            serde_json::to_string(&new_statuses).expect("serializes"),
            "default CheckOptions reported differently than check_spec on {label}"
        );
    }
}

/// **D1.9-1, pinned:** the CLI default window stays `1h` across 1.x —
/// `check`'s burn rate feeds `--fail-on`, so re-windowing the default would
/// silently flip existing CI gates. Agreement with the generated rules is
/// opt-in via `--rules-window`.
#[test]
#[cfg(feature = "cli")]
fn the_cli_default_window_stays_1h() {
    assert_eq!(
        cli_flag_default("check", "--window"),
        "1h",
        "`slokit check --window` no longer defaults to 1h; that is a breaking change \
         under D1.9-1 and docs/SEMVER.md's CLI clause"
    );
}

/// `--rules-window` and an explicit `--window` cannot be combined: one flag
/// silently winning over the other would be a parsed-and-discarded flag (the
/// svccat `--filter` shape), so the CLI refuses the combination outright.
/// A default-valued `--window` does not count as given, so plain
/// `--rules-window` works.
#[test]
#[cfg(feature = "cli")]
fn cli_rules_window_conflicts_with_an_explicit_window() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.yaml");
    let out = Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(["check", "-i"])
        .arg(&sample)
        .args([
            "--url",
            "http://127.0.0.1:9",
            "--rules-window",
            "--window",
            "5m",
        ])
        .output()
        .expect("slokit runs");
    assert!(
        !out.status.success(),
        "--rules-window with an explicit --window must be an error, not one flag winning"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--rules-window") && stderr.contains("--window"),
        "the conflict error should name both flags, got: {stderr}"
    );
}

/// `--no-period-scaling` and `--alert-windows` only mean something under
/// `--rules-window` (the fixed-window mode never consults the burn-rate
/// table), so giving either without it is an error rather than a silently
/// discarded flag.
#[test]
#[cfg(feature = "cli")]
fn cli_scaling_and_catalogue_flags_require_rules_window() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.yaml");
    for extra in [
        vec!["--no-period-scaling".to_string()],
        vec![
            "--alert-windows".to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/sloth_corpus/windows")
                .display()
                .to_string(),
        ],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_slokit"))
            .args(["check", "-i"])
            .arg(&sample)
            .args(["--url", "http://127.0.0.1:9"])
            .args(&extra)
            .output()
            .expect("slokit runs");
        assert!(
            !out.status.success(),
            "{} without --rules-window must be an error, not a no-op",
            extra[0]
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("--rules-window"),
            "the error for {} should point at --rules-window, got: {stderr}",
            extra[0]
        );
    }
}

/// Under `--rules-window` both output formats state each SLO's own window:
/// the table grows a WINDOW column (and stops claiming one global window in
/// its header), and the JSON's `current_window` varies per SLO. The
/// mixed-periods fixture (a 7d and a 30d SLO in one spec) makes the per-SLO
/// difference visible in a single run: 1m and 5m.
#[test]
#[cfg(feature = "cli")]
fn cli_rules_window_states_each_slos_window_in_both_formats() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/alert_windows/mixed-periods.yaml");
    let spy = QuerySpy::spawn();

    let table = Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(["check", "-i"])
        .arg(&fixture)
        .args(["--url", &spy.url(), "--rules-window"])
        .output()
        .expect("slokit runs");
    assert!(
        table.status.success(),
        "check --rules-window failed: {}",
        String::from_utf8_lossy(&table.stderr)
    );
    let stdout = String::from_utf8_lossy(&table.stdout);
    assert!(
        stdout.contains("WINDOW"),
        "rules-window table should carry a WINDOW column, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("current window"),
        "rules-window table must not claim one global current window, got:\n{stdout}"
    );
    let weekly = stdout
        .lines()
        .find(|l| l.contains("weekly-availability"))
        .expect("weekly row present");
    let monthly = stdout
        .lines()
        .find(|l| l.contains("monthly-availability"))
        .expect("monthly row present");
    assert!(
        weekly.trim_end().ends_with("1m"),
        "7d SLO row states 1m: {weekly}"
    );
    assert!(
        monthly.trim_end().ends_with("5m"),
        "30d SLO row states 5m: {monthly}"
    );

    let json = Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(["check", "-i"])
        .arg(&fixture)
        .args(["--url", &spy.url(), "--rules-window", "--output", "json"])
        .output()
        .expect("slokit runs");
    assert!(json.status.success());
    let statuses: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("json output parses");
    let windows: Vec<&str> = statuses
        .as_array()
        .expect("array")
        .iter()
        .map(|s| {
            s["current_window"]
                .as_str()
                .expect("current_window is a string")
        })
        .collect();
    assert_eq!(windows, ["1m", "5m"], "JSON states each SLO's own window");
    drop(spy);
}

/// The default table keeps its pre-seam shape: one global window in the
/// header, no WINDOW column. Paired with the test above, this is the
/// on/off behavior difference of `--rules-window` at the CLI surface.
#[test]
#[cfg(feature = "cli")]
fn cli_default_table_keeps_the_global_window_header() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/alert_windows/mixed-periods.yaml");
    let spy = QuerySpy::spawn();
    let out = Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(["check", "-i"])
        .arg(&fixture)
        .args(["--url", &spy.url()])
        .output()
        .expect("slokit runs");
    assert!(
        out.status.success(),
        "default check failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("(current window 1h)"),
        "default table states the one global window, got:\n{stdout}"
    );
    assert!(
        !stdout.contains("WINDOW"),
        "default table must not grow a WINDOW column (byte-identity with 1.8.0), got:\n{stdout}"
    );
    drop(spy);
}

/// A spec whose one SLO resolves through an embedder-registered plugin — the
/// registry-only spec the closed MED bug (2026-08-08, closed by v1.9.0 PR 2)
/// was about.
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

/// The embedder's plugin behind [`PLUGIN_SPEC`]: expands to a raw SLI reading
/// `avg_over_time(<metric>[window])`, so its fingerprint on the wire is
/// unmistakably not a built-in's.
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

/// An [`SliPluginRegistry`] holding exactly [`StaticRatio`] — no built-ins, so
/// anything it resolves provably came from the embedder.
fn static_ratio_registry() -> SliPluginRegistry {
    let mut registry = SliPluginRegistry::empty();
    registry.register(Box::new(StaticRatio)).expect("registers");
    registry
}

/// The end-to-end embedder flow the closed MED bug asked for (milestone
/// done-when clause 4): a spec whose plugin lives only in the caller's
/// registry is checked through the public API, and the queries on the wire
/// are the plugin's own expansion. All three public entry points now accept
/// the same registry-only spec; `tests/plugin.rs` proves validate and
/// generate, this proves `check`.
#[test]
fn embedder_registry_reaches_check_end_to_end() {
    let spec = Spec::from_yaml(PLUGIN_SPEC).expect("spec parses");
    let registry = static_ratio_registry();

    // The two entry points that always took the registry still agree.
    spec.validate_with(&registry)
        .expect("validate_with accepts the embedder's plugin");
    let mut gen_opts = GenerateOptions::default();
    gen_opts.plugins = Arc::new(registry);
    generate_rules_with(&spec, &gen_opts)
        .expect("generate_rules_with accepts the embedder's plugin");

    // `check` is the third, and `CheckOptions::plugins` is its route.
    let spy = QuerySpy::spawn();
    let client = PrometheusClient::new(spy.url()).expect("client builds");
    let mut opts = CheckOptions::default();
    opts.plugins = gen_opts.plugins.clone();
    let statuses = check_spec_with(&client, &spec, &opts)
        .expect("check_spec_with resolves the embedder's plugin");
    assert_eq!(statuses.len(), 1, "one SLO, one status");

    // The wire proves the SLI came from the embedder's expansion: the period
    // query and the default 1h current-window query, both over its metric.
    let queries = spy.drain();
    assert_eq!(
        queries,
        vec![
            "avg_over_time(app:error_ratio[30d])".to_string(),
            "avg_over_time(app:error_ratio[1h])".to_string(),
        ],
        "check must query the plugin-expanded SLI, period then current window"
    );

    // The registry composes with rules-window resolution: same spec, same
    // registry, `BurnWindow::Rules` — the 30d SLO's current window becomes
    // the generator's 5m base window instead of the fixed 1h.
    opts.burn_window = BurnWindow::Rules;
    check_spec_with(&client, &spec, &opts)
        .expect("check_spec_with resolves the embedder's plugin under rules windows");
    let queries = spy.drain();
    assert_eq!(
        queries,
        vec![
            "avg_over_time(app:error_ratio[30d])".to_string(),
            "avg_over_time(app:error_ratio[5m])".to_string(),
        ],
        "rules-window resolution must apply to a plugin-expanded SLI too"
    );
}

/// The validation half of the close condition (the half-close hazard the bug
/// named): `check_spec_with` must validate against the registry the caller
/// gave, not the built-ins — in both directions. Without the caller's
/// registry the spec is rejected before a single query is sent, exactly as
/// 1.8.0 behaved; with a registry that lacks the plugin it is rejected too,
/// so validation demonstrably runs on the given registry rather than being
/// skipped.
#[test]
fn check_validates_on_the_given_registry_not_the_builtins() {
    let spec = Spec::from_yaml(PLUGIN_SPEC).expect("spec parses");
    let spy = QuerySpy::spawn();
    let client = PrometheusClient::new(spy.url()).expect("client builds");

    // Direction 1: the registry-less entry points keep 1.8.0's behavior —
    // built-ins only, so the embedder's plugin id is unknown.
    let err = check_spec(&client, &spec, Window::days(30), Window::hours(1))
        .expect_err("the built-in registry must reject the embedder's plugin")
        .to_string();
    assert!(
        err.contains("unknown SLI plugin 'acme/static-ratio'"),
        "expected the built-in registry to reject the embedder's plugin, got: {err}"
    );

    // Direction 2: validation demonstrably RUNS, on the GIVEN registry. The
    // spec's plugin is in the caller's registry (so plugin resolution alone
    // would sail through and start querying), but its objective is not a
    // percentage — only a validation pass can reject it, and only one run on
    // the caller's registry rejects it for the OBJECTIVE rather than for an
    // unknown plugin id, which is what the built-ins would say first.
    let broken = PLUGIN_SPEC.replace("objective: 99.9", "objective: 150");
    let broken = Spec::from_yaml(&broken).expect("broken spec still parses");
    let mut opts = CheckOptions::default();
    opts.plugins = Arc::new(static_ratio_registry());
    let err = check_spec_with(&client, &broken, &opts)
        .expect_err("validation must reject the out-of-range objective")
        .to_string();
    assert!(
        err.contains("not a percentage"),
        "expected the objective error (validation ran, on the given registry), got: {err}"
    );
    // Validation collects every error, so a run against the WRONG registry
    // would report the plugin as unknown alongside the objective. Its absence
    // is what proves the registry validated with was the caller's.
    assert!(
        !err.contains("unknown SLI plugin"),
        "the caller's registry knows the plugin, so only the objective may be reported, got: {err}"
    );

    assert!(
        spy.drain().is_empty(),
        "both rejections must happen before any query reaches Prometheus"
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
