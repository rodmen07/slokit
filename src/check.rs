//! Live checking against a Prometheus HTTP API.
//!
//! Given a spec, [`check_spec`] evaluates each SLO's SLI directly against a
//! running Prometheus (no deployed recording rules required) and reports the
//! current error budget and burn rate. This is the runtime companion to the
//! offline rule [generator](crate::generate).

use std::time::Duration;

use crate::burn_rate::BurnRate;
use crate::error::{Result, SlokitError};
use crate::spec::{SloSpec, Spec};
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
            return Err(SlokitError::Query(format!("HTTP {}", resp.status())));
        }
        let body: serde_json::Value = resp.json().map_err(|e| SlokitError::Query(e.to_string()))?;
        parse_query_value(&body)
    }
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
        return Err(SlokitError::Query(msg.to_string()));
    }
    let data = body
        .get("data")
        .ok_or_else(|| SlokitError::Query("response missing `data`".into()))?;
    let result_type = data
        .get("resultType")
        .and_then(|s| s.as_str())
        .unwrap_or("");
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
        Some(s) => s
            .parse::<f64>()
            .map(Some)
            .map_err(|_| SlokitError::Query(format!("could not parse sample value '{s}'"))),
        None => Ok(None),
    }
}

/// How an SLO is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A point-in-time status report for a single SLO.
#[derive(Debug, Clone)]
pub struct SloStatus {
    /// SLO name.
    pub name: String,
    /// Objective as a percentage.
    pub objective_percent: f64,
    /// SLO period.
    pub period: Window,
    /// The short window used for the "current" burn rate.
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

/// Check a single SLO against a live Prometheus.
pub fn check_slo(
    client: &PrometheusClient,
    slo_spec: &SloSpec,
    default_period: Window,
    current_window: Window,
) -> Result<SloStatus> {
    let slo = slo_spec.to_slo(default_period)?;
    let sli = slo_spec.to_sli()?;
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
    spec.validate()?;
    spec.slos
        .iter()
        .map(|slo| check_slo(client, slo, default_period, current_window))
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
    }
}
