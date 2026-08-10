//! Live checking against a Prometheus HTTP API.
//!
//! Given a spec, [`check_spec`] evaluates each SLO's SLI directly against a
//! running Prometheus (no deployed recording rules required) and reports the
//! current error budget and burn rate. This is the runtime companion to the
//! offline rule [generator](crate::generate).

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::burn_rate::{BurnRate, MwmbrConfig};
use crate::error::{Result, SlokitError};
use crate::generate::GenerateOptions;
use crate::spec::alert_windows::AlertWindowsSet;
use crate::spec::plugin::SliPluginRegistry;
use crate::spec::{SloSpec, Spec, DEFAULT_PERIOD};
use crate::window::Window;

/// A minimal blocking client for the Prometheus instant-query API.
pub struct PrometheusClient {
    base_url: String,
    bearer_token: Option<String>,
    http: reqwest::blocking::Client,
}

impl PrometheusClient {
    /// Build a client with a default 30-second timeout.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    /// Build a client with an explicit request timeout.
    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| SlokitError::Query(e.to_string()))?;
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Ok(Self {
            base_url,
            bearer_token: None,
            http,
        })
    }

    /// Attach a bearer token sent with every request.
    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Run an instant query and return the first sample value, or `None` when
    /// the query returns an empty result.
    pub fn query_scalar(&self, promql: &str) -> Result<Option<f64>> {
        let url = format!("{}/api/v1/query", self.base_url);
        let mut req = self.http.get(&url).query(&[("query", promql)]);
        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().map_err(|e| SlokitError::Query(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(SlokitError::Query(format_http_error(status, &body)));
        }
        let body: serde_json::Value = resp.json().map_err(|e| SlokitError::Query(e.to_string()))?;
        parse_query_value(&body)
    }
}

fn format_http_error(status: reqwest::StatusCode, body: &str) -> String {
    let compact = body.replace(['\n', '\r'], " ");
    let compact = compact.trim();
    if compact.is_empty() {
        return format!("HTTP {}", status);
    }

    let mut snippet: String = compact.chars().take(180).collect();
    if compact.chars().count() > 180 {
        snippet.push_str("...");
    }

    format!("HTTP {}: {}", status, snippet)
}

/// Extract the first sample value from a Prometheus instant-query response,
/// returning `None` for an empty (but successful) result.
fn parse_query_value(body: &serde_json::Value) -> Result<Option<f64>> {
    let status = body.get("status").and_then(|s| s.as_str()).unwrap_or("");
    if status != "success" {
        let msg = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown error");
        let err_type = body.get("errorType").and_then(|e| e.as_str()).unwrap_or("");
        let full = if err_type.is_empty() {
            msg.to_string()
        } else {
            format!("{err_type}: {msg}")
        };
        return Err(SlokitError::Query(full));
    }
    let data = body
        .get("data")
        .ok_or_else(|| SlokitError::Query("response missing `data`".into()))?;
    let result_type = data
        .get("resultType")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if result_type.is_empty() {
        return Err(SlokitError::Query(
            "response missing `data.resultType`".into(),
        ));
    }
    let value_str = match result_type {
        "scalar" => data
            .get("result")
            .and_then(|r| r.get(1))
            .and_then(|v| v.as_str()),
        "vector" => data
            .get("result")
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .and_then(|s| s.get("value"))
            .and_then(|v| v.get(1))
            .and_then(|v| v.as_str()),
        other => {
            return Err(SlokitError::Query(format!(
                "unexpected resultType '{other}' (expected scalar or vector)"
            )))
        }
    };
    match value_str {
        Some(s) => {
            let value = s
                .parse::<f64>()
                .map_err(|_| SlokitError::Query(format!("could not parse sample value '{s}'")))?;
            if !value.is_finite() {
                return Err(SlokitError::Query(format!("non-finite sample value '{s}'")));
            }
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

/// How an SLO is doing right now.
///
/// The enum is `#[non_exhaustive]`: new levels (for example an explicit
/// no-data state) may be added, so matches need a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum StatusLevel {
    /// Comfortably within budget.
    Ok,
    /// Budget running low, or burning faster than sustainable.
    Warning,
    /// Budget for the period is exhausted.
    Breaching,
}

impl StatusLevel {
    /// A short uppercase label for display.
    pub fn label(&self) -> &'static str {
        match self {
            StatusLevel::Ok => "OK",
            StatusLevel::Warning => "WARN",
            StatusLevel::Breaching => "BREACH",
        }
    }
}

/// Decide a status from the period budget remaining and the current burn rate.
///
/// Breaching when no budget remains; warning when under 10% remains or the
/// current burn rate exceeds 1.0 (faster than the budget can sustain).
fn level_for(remaining: Option<f64>, burn: Option<f64>) -> StatusLevel {
    // Non-finite values should never be considered healthy.
    let non_finite =
        remaining.is_some_and(|r| !r.is_finite()) || burn.is_some_and(|b| !b.is_finite());
    if non_finite {
        return StatusLevel::Warning;
    }
    if remaining.is_some_and(|r| r <= 0.0) {
        return StatusLevel::Breaching;
    }
    let low_budget = remaining.is_some_and(|r| r < 0.10);
    let fast_burn = burn.is_some_and(|b| b > 1.0);
    if low_budget || fast_burn {
        StatusLevel::Warning
    } else {
        StatusLevel::Ok
    }
}

/// Serialize a [`Window`] as its Prometheus duration string (e.g. `30d`).
fn ser_window<S: serde::Serializer>(w: &Window, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&w.prometheus())
}

/// A point-in-time status report for a single SLO.
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume
/// (produced by [`check_slo`]/[`check_spec`]), and the report has a history
/// of gaining fields, so new ones must not be breaking changes.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct SloStatus {
    /// The service this SLO belongs to.
    pub service: String,
    /// SLO name.
    pub name: String,
    /// Objective as a percentage.
    pub objective_percent: f64,
    /// SLO period.
    #[serde(serialize_with = "ser_window")]
    pub period: Window,
    /// The short window used for the "current" burn rate.
    #[serde(serialize_with = "ser_window")]
    pub current_window: Window,
    /// Average error ratio over the whole period, if data was returned.
    pub period_error_ratio: Option<f64>,
    /// Error ratio over the current window, if data was returned.
    pub current_error_ratio: Option<f64>,
    /// Current burn rate (current error ratio over the budget ratio).
    pub current_burn_rate: Option<f64>,
    /// Fraction of the period budget consumed.
    pub budget_consumed_ratio: Option<f64>,
    /// Fraction of the period budget remaining (negative when overspent).
    pub budget_remaining_ratio: Option<f64>,
    /// Overall status.
    pub level: StatusLevel,
}

/// The default fixed window for the "current" burn rate: `1h`, the CLI's
/// long-standing `--window` default. Kept across 1.x because `check`'s burn
/// rate feeds `--fail-on`, so re-windowing the default would silently flip
/// existing CI gates (decision D1.9-1 in `ROADMAP.md`).
pub const DEFAULT_CURRENT_WINDOW: Window = Window::hours(1);

/// How [`check_slo_with`] chooses the window behind each SLO's "current"
/// burn rate.
///
/// The enum is `#[non_exhaustive]`: further resolution modes may be added
/// without a breaking change, so matches need a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BurnWindow {
    /// One fixed window for every SLO — the CLI's `--window`, and the only
    /// behavior that existed before 1.9. The default is
    /// [`DEFAULT_CURRENT_WINDOW`] (`1h`).
    Fixed(Window),
    /// Per SLO, the window the generated rules compute
    /// `slo:current_burn_rate:ratio` over: the shortest lookback of the
    /// burn-rate config resolved exactly as [`generate`](crate::generate)
    /// resolves it — per-SLO `alerting.windows` first, then a matching
    /// `kind: AlertWindows` catalogue, then the config table scaled to the
    /// SLO's period, then the table verbatim.
    Rules,
}

/// Options controlling a check run.
///
/// The struct is `#[non_exhaustive]`, mirroring
/// [`GenerateOptions`](crate::generate::GenerateOptions): options are
/// expected to keep growing. Start from
/// [`CheckOptions::default`] — which reproduces the behavior of
/// [`check_spec`] / [`check_slo`] exactly — and set the fields you need:
///
/// ```
/// use slokit::check::{BurnWindow, CheckOptions};
///
/// let mut opts = CheckOptions::default();
/// opts.burn_window = BurnWindow::Rules;
/// assert_eq!(opts.burn_window, BurnWindow::Rules);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckOptions {
    /// Period used for SLOs that do not set their own `period`.
    pub default_period: Window,
    /// How the window behind each SLO's "current" burn rate is chosen.
    pub burn_window: BurnWindow,
    /// The burn-rate config [`BurnWindow::Rules`] resolves against,
    /// calibrated for the standard 30-day period. Per-SLO `alerting.windows`
    /// in the spec override it entirely. Ignored under
    /// [`BurnWindow::Fixed`].
    pub mwmbr: MwmbrConfig,
    /// Scale `mwmbr`'s lookback windows to each SLO's resolved period (on by
    /// default), exactly as `generate` does. Ignored under
    /// [`BurnWindow::Fixed`].
    pub period_aware: bool,
    /// sloth `kind: AlertWindows` catalogues, keyed by SLO period (empty by
    /// default), with the same precedence they have in `generate`. Ignored
    /// under [`BurnWindow::Fixed`].
    pub alert_windows: AlertWindowsSet,
    /// The registry used to resolve `sli.plugin` SLIs (defaults to slokit's
    /// built-in plugins), mirroring
    /// [`GenerateOptions::plugins`](crate::generate::GenerateOptions).
    /// Validation inside [`check_spec_with`] runs against this same registry,
    /// so a spec that [`Spec::validate_with`] and
    /// [`generate_rules_with`](crate::generate::generate_rules_with) accept
    /// is checkable too. Wrapped in an [`Arc`] because `CheckOptions` is
    /// `Clone` and a boxed-trait registry is not.
    pub plugins: Arc<SliPluginRegistry>,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            default_period: DEFAULT_PERIOD,
            burn_window: BurnWindow::Fixed(DEFAULT_CURRENT_WINDOW),
            mwmbr: MwmbrConfig::sre_default(),
            period_aware: true,
            alert_windows: AlertWindowsSet::new(),
            plugins: Arc::new(SliPluginRegistry::with_builtins()),
        }
    }
}

/// The window `opts` chooses for one SLO's "current" burn rate. `period` is
/// the SLO's already-resolved period.
///
/// The [`BurnWindow::Rules`] arm goes through the generator's own resolution
/// seam (`generate::resolve_mwmbr`, then the same shortest-lookback rule the
/// `slo:current_burn_rate:ratio` recording uses), never a check-local
/// re-derivation — a second resolver is exactly the drift class PR #38
/// removed from `dashboard`.
fn current_window_for(slo_spec: &SloSpec, period: Window, opts: &CheckOptions) -> Result<Window> {
    match opts.burn_window {
        BurnWindow::Fixed(window) => Ok(window),
        BurnWindow::Rules => {
            let gen_opts = GenerateOptions {
                default_period: opts.default_period,
                mwmbr: opts.mwmbr.clone(),
                period_aware: opts.period_aware,
                alert_windows: opts.alert_windows.clone(),
                plugins: opts.plugins.clone(),
                ..GenerateOptions::default()
            };
            let mwmbr = crate::generate::resolve_mwmbr(slo_spec.custom_mwmbr()?, period, &gen_opts);
            Ok(crate::generate::base_window(&mwmbr))
        }
    }
}

/// Check a single SLO against a live Prometheus.
pub fn check_slo(
    client: &PrometheusClient,
    service: &str,
    slo_spec: &SloSpec,
    default_period: Window,
    current_window: Window,
) -> Result<SloStatus> {
    let opts = CheckOptions {
        default_period,
        burn_window: BurnWindow::Fixed(current_window),
        ..CheckOptions::default()
    };
    check_slo_with(client, service, slo_spec, &opts)
}

/// Check a single SLO against a live Prometheus, with explicit options.
pub fn check_slo_with(
    client: &PrometheusClient,
    service: &str,
    slo_spec: &SloSpec,
    opts: &CheckOptions,
) -> Result<SloStatus> {
    let slo = slo_spec.to_slo(opts.default_period)?;
    let current_window = current_window_for(slo_spec, slo.period, opts)?;
    let sli = slo_spec.to_sli_with(&opts.plugins)?;
    let budget_ratio = slo.error_budget_ratio();

    let period_error_ratio = client.query_scalar(&sli.error_ratio_expr(slo.period))?;
    let current_error_ratio = client.query_scalar(&sli.error_ratio_expr(current_window))?;

    let current_burn_rate =
        current_error_ratio.map(|r| BurnRate::from_error_ratio(r, &slo).value());
    let budget_consumed_ratio = period_error_ratio.map(|r| {
        if budget_ratio > 0.0 {
            r / budget_ratio
        } else {
            f64::INFINITY
        }
    });
    let budget_remaining_ratio = budget_consumed_ratio.map(|c| 1.0 - c);
    let level = level_for(budget_remaining_ratio, current_burn_rate);

    Ok(SloStatus {
        service: service.to_string(),
        name: slo_spec.name.clone(),
        objective_percent: slo.objective.as_percent(),
        period: slo.period,
        current_window,
        period_error_ratio,
        current_error_ratio,
        current_burn_rate,
        budget_consumed_ratio,
        budget_remaining_ratio,
        level,
    })
}

/// Check every SLO in a spec against a live Prometheus.
///
/// The spec is validated first; the first query failure aborts the run.
pub fn check_spec(
    client: &PrometheusClient,
    spec: &Spec,
    default_period: Window,
    current_window: Window,
) -> Result<Vec<SloStatus>> {
    let opts = CheckOptions {
        default_period,
        burn_window: BurnWindow::Fixed(current_window),
        ..CheckOptions::default()
    };
    check_spec_with(client, spec, &opts)
}

/// Check every SLO in a spec against a live Prometheus, with explicit
/// options.
///
/// The spec is validated first — against [`CheckOptions::plugins`], the same
/// registry the SLIs are then resolved with, so acceptance and resolution
/// cannot disagree; the first query failure aborts the run.
pub fn check_spec_with(
    client: &PrometheusClient,
    spec: &Spec,
    opts: &CheckOptions,
) -> Result<Vec<SloStatus>> {
    spec.validate_with(&opts.plugins)?;
    spec.slos
        .iter()
        .map(|slo| check_slo_with(client, &spec.service, slo, opts))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vector_response() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"0.0123"]}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_query_value(&body).unwrap(), Some(0.0123));
    }

    #[test]
    fn parses_scalar_response() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"scalar","result":[1719000000,"42"]}}"#,
        )
        .unwrap();
        assert_eq!(parse_query_value(&body).unwrap(), Some(42.0));
    }

    #[test]
    fn empty_vector_is_none() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"vector","result":[]}}"#,
        )
        .unwrap();
        assert_eq!(parse_query_value(&body).unwrap(), None);
    }

    #[test]
    fn error_status_is_propagated() {
        let body: serde_json::Value =
            serde_json::from_str(r#"{"status":"error","error":"bad query"}"#).unwrap();
        let err = parse_query_value(&body).unwrap_err();
        assert!(err.to_string().contains("bad query"));
    }

    #[test]
    fn error_status_includes_error_type_when_present() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"error","errorType":"bad_data","error":"parse failure"}"#,
        )
        .unwrap();
        let err = parse_query_value(&body).unwrap_err().to_string();
        assert!(err.contains("bad_data: parse failure"));
    }

    #[test]
    fn non_finite_scalar_is_rejected() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"scalar","result":[1719000000,"NaN"]}}"#,
        )
        .unwrap();
        let err = parse_query_value(&body).unwrap_err();
        assert!(err.to_string().contains("non-finite sample value"));
    }

    #[test]
    fn unexpected_result_type_is_reported() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"matrix","result":[]}}"#,
        )
        .unwrap();
        let err = parse_query_value(&body).unwrap_err().to_string();
        assert!(err.contains("unexpected resultType 'matrix'"));
    }

    #[test]
    fn missing_result_type_is_reported() {
        let body: serde_json::Value =
            serde_json::from_str(r#"{"status":"success","data":{"result":[]}}"#).unwrap();
        let err = parse_query_value(&body).unwrap_err().to_string();
        assert!(err.contains("response missing `data.resultType`"));
    }

    #[test]
    fn http_error_format_compacts_newlines() {
        let formatted = format_http_error(
            reqwest::StatusCode::BAD_GATEWAY,
            "upstream\nfailed\rwith timeout",
        );
        assert!(formatted.contains("HTTP 502 Bad Gateway"));
        assert!(formatted.contains("upstream failed with timeout"));
    }

    #[test]
    fn http_error_format_truncates_long_body_with_ellipsis() {
        let long_body = "x".repeat(220);
        let formatted = format_http_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, &long_body);
        assert!(formatted.starts_with("HTTP 503 Service Unavailable: "));
        assert!(formatted.ends_with("..."));
    }

    #[test]
    fn non_finite_vector_is_rejected() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"+Inf"]}]}}"#,
        )
        .unwrap();
        let err = parse_query_value(&body).unwrap_err();
        assert!(err.to_string().contains("non-finite sample value"));
    }

    #[test]
    fn slostatus_serializes_to_json() {
        let status = SloStatus {
            service: "svc".to_string(),
            name: "slo".to_string(),
            objective_percent: 99.9,
            period: Window::days(30),
            current_window: Window::hours(1),
            period_error_ratio: Some(0.0005),
            current_error_ratio: Some(0.001),
            current_burn_rate: Some(1.0),
            budget_consumed_ratio: Some(0.5),
            budget_remaining_ratio: Some(0.5),
            level: StatusLevel::Ok,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"service\":\"svc\""));
        assert!(json.contains("\"period\":\"30d\"")); // Window serialized as a string
        assert!(json.contains("\"level\":\"ok\"")); // rename_all = lowercase
    }

    #[test]
    fn status_levels_follow_budget_and_burn() {
        // Exhausted budget breaches regardless of burn.
        assert_eq!(level_for(Some(0.0), Some(0.1)), StatusLevel::Breaching);
        assert_eq!(level_for(Some(-0.2), None), StatusLevel::Breaching);
        // Low budget warns.
        assert_eq!(level_for(Some(0.05), Some(0.1)), StatusLevel::Warning);
        // Fast burn warns even with budget left.
        assert_eq!(level_for(Some(0.8), Some(2.0)), StatusLevel::Warning);
        // Healthy.
        assert_eq!(level_for(Some(0.8), Some(0.3)), StatusLevel::Ok);
        // Non-finite values are never healthy.
        assert_eq!(level_for(Some(f64::NAN), Some(0.3)), StatusLevel::Warning);
        assert_eq!(
            level_for(Some(0.8), Some(f64::INFINITY)),
            StatusLevel::Warning
        );
    }

    #[test]
    fn http_error_formatter_includes_status_and_body_snippet() {
        let msg = format_http_error(
            reqwest::StatusCode::BAD_GATEWAY,
            "{\"error\":\"upstream timeout\"}",
        );
        assert!(msg.contains("HTTP 502 Bad Gateway"));
        assert!(msg.contains("upstream timeout"));
    }

    #[test]
    fn http_error_formatter_handles_empty_body() {
        let msg = format_http_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, "   \n");
        assert_eq!(msg, "HTTP 500 Internal Server Error");
    }

    #[test]
    fn default_options_are_fixed_1h_over_30d() {
        let opts = CheckOptions::default();
        assert_eq!(opts.default_period, Window::days(30));
        assert_eq!(opts.burn_window, BurnWindow::Fixed(Window::hours(1)));
        assert!(opts.period_aware);
        assert!(opts.alert_windows.periods().is_empty());
        // The default registry is the built-ins, id for id — the behavior
        // `check_spec` / `check_slo` always had.
        let default_ids: Vec<&str> = opts.plugins.ids().collect();
        let builtin_ids: Vec<String> = SliPluginRegistry::with_builtins()
            .ids()
            .map(str::to_string)
            .collect();
        assert_eq!(default_ids, builtin_ids);
        assert!(!default_ids.is_empty(), "built-ins must not be empty");
    }

    #[test]
    fn rules_window_resolution_follows_the_generator() {
        let spec = Spec::from_yaml(
            r#"
service: svc
slos:
  - name: weekly
    objective: 99.9
    period: 7d
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#,
        )
        .expect("spec parses");
        let slo_spec = &spec.slos[0];
        let period = slo_spec.to_slo(DEFAULT_PERIOD).expect("resolves").period;

        // Scaled to the 7d period, the 30d base window 5m becomes 1m.
        let mut opts = CheckOptions {
            burn_window: BurnWindow::Rules,
            ..CheckOptions::default()
        };
        assert_eq!(
            current_window_for(slo_spec, period, &opts).expect("resolves"),
            Window::minutes(1)
        );

        // With scaling off the 30d table applies verbatim.
        opts.period_aware = false;
        assert_eq!(
            current_window_for(slo_spec, period, &opts).expect("resolves"),
            Window::minutes(5)
        );

        // A fixed window ignores the burn-rate table entirely.
        opts.burn_window = BurnWindow::Fixed(Window::hours(6));
        assert_eq!(
            current_window_for(slo_spec, period, &opts).expect("resolves"),
            Window::hours(6)
        );
    }
}
