//! Service Level Indicators: how the "error ratio" for an SLO is measured.

use crate::window::Window;

/// The template token replaced with each lookback window when rendering
/// queries. Matches `sloth`'s Go-template convention.
pub const WINDOW_TOKEN: &str = "{{.window}}";

/// How an SLO's error ratio is computed from Prometheus.
///
/// Covers `sloth`'s two SLI shapes (events-based and raw) plus a slokit
/// extension, [`Sli::Latency`], that generates the histogram bucket query for
/// the common "fraction of requests slower than a threshold" SLO.
///
/// The enum is `#[non_exhaustive]`: SLI shapes have grown before (`Latency`
/// arrived in 0.3) and may grow again, so matches need a wildcard arm.
/// Constructing the existing variants remains supported.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sli {
    /// Bad events divided by total events. Both queries should contain the
    /// [`WINDOW_TOKEN`].
    Events {
        /// Query counting failing events over the window.
        error_query: String,
        /// Query counting all events over the window.
        total_query: String,
    },
    /// A query that already yields an error ratio in `[0, 1]`. Should contain
    /// the [`WINDOW_TOKEN`].
    Raw {
        /// Query yielding the error ratio over the window.
        error_ratio_query: String,
    },
    /// A latency SLI built from a Prometheus histogram: the fraction of requests
    /// that took longer than `threshold` (the `le` bucket boundary). The error
    /// ratio query is generated, so it needs no [`WINDOW_TOKEN`].
    Latency {
        /// Histogram base metric name, without the `_bucket`/`_count` suffix,
        /// e.g. `http_request_duration_seconds`.
        histogram_metric: String,
        /// The `le` bucket boundary, kept as a string to match the label
        /// exactly, e.g. `"0.3"`.
        threshold: String,
        /// Optional label matchers, written without braces, e.g. `job="api"`.
        selector: Option<String>,
    },
}

impl Sli {
    /// Render the SLI's error-ratio PromQL expression for a given window,
    /// substituting [`WINDOW_TOKEN`] with the Prometheus duration.
    ///
    /// For [`Sli::Events`] this is `(error) / (total)`; for [`Sli::Raw`] it is
    /// the raw query as written.
    pub fn error_ratio_expr(&self, window: Window) -> String {
        match self {
            Sli::Events {
                error_query,
                total_query,
            } => {
                format!(
                    "({})\n/\n({})",
                    substitute_window(error_query, window),
                    substitute_window(total_query, window)
                )
            }
            Sli::Raw { error_ratio_query } => substitute_window(error_ratio_query, window),
            Sli::Latency {
                histogram_metric,
                threshold,
                selector,
            } => {
                let win = window.prometheus();
                let sel = selector.as_deref().map(str::trim).filter(|s| !s.is_empty());
                let bucket_labels = match sel {
                    Some(s) => format!("{{{s}, le=\"{threshold}\"}}"),
                    None => format!("{{le=\"{threshold}\"}}"),
                };
                let count_labels = match sel {
                    Some(s) => format!("{{{s}}}"),
                    None => String::new(),
                };
                format!(
                    "1 - (\n  sum(rate({histogram_metric}_bucket{bucket_labels}[{win}]))\n  /\n  sum(rate({histogram_metric}_count{count_labels}[{win}]))\n)"
                )
            }
        }
    }

    /// Every user-supplied query string this SLI carries, for validation. A
    /// [`Sli::Latency`] carries none (its query is generated).
    pub fn queries(&self) -> Vec<&str> {
        match self {
            Sli::Events {
                error_query,
                total_query,
            } => vec![error_query, total_query],
            Sli::Raw { error_ratio_query } => vec![error_ratio_query],
            Sli::Latency { .. } => vec![],
        }
    }
}

/// Replace the window template token (with or without surrounding spaces) by the
/// Prometheus duration string for `window`.
pub fn substitute_window(query: &str, window: Window) -> String {
    let replacement = window.prometheus();
    query
        .replace("{{ .window }}", &replacement)
        .replace(WINDOW_TOKEN, &replacement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_expr_divides_error_by_total() {
        let sli = Sli::Events {
            error_query: "sum(rate(errors[{{.window}}]))".to_string(),
            total_query: "sum(rate(total[{{.window}}]))".to_string(),
        };
        let expr = sli.error_ratio_expr(Window::minutes(5));
        assert_eq!(expr, "(sum(rate(errors[5m])))\n/\n(sum(rate(total[5m])))");
    }

    #[test]
    fn raw_expr_substitutes_window() {
        let sli = Sli::Raw {
            error_ratio_query: "my_ratio[{{ .window }}]".to_string(),
        };
        assert_eq!(sli.error_ratio_expr(Window::hours(1)), "my_ratio[1h]");
    }

    #[test]
    fn latency_expr_with_selector() {
        let sli = Sli::Latency {
            histogram_metric: "http_request_duration_seconds".to_string(),
            threshold: "0.3".to_string(),
            selector: Some("job=\"api\"".to_string()),
        };
        let expr = sli.error_ratio_expr(Window::minutes(5));
        assert_eq!(
            expr,
            "1 - (\n  sum(rate(http_request_duration_seconds_bucket{job=\"api\", le=\"0.3\"}[5m]))\n  /\n  sum(rate(http_request_duration_seconds_count{job=\"api\"}[5m]))\n)"
        );
    }

    #[test]
    fn latency_expr_without_selector() {
        let sli = Sli::Latency {
            histogram_metric: "rpc_latency_seconds".to_string(),
            threshold: "0.5".to_string(),
            selector: None,
        };
        let expr = sli.error_ratio_expr(Window::hours(1));
        assert_eq!(
            expr,
            "1 - (\n  sum(rate(rpc_latency_seconds_bucket{le=\"0.5\"}[1h]))\n  /\n  sum(rate(rpc_latency_seconds_count[1h]))\n)"
        );
    }

    #[test]
    fn latency_has_no_template_queries() {
        let sli = Sli::Latency {
            histogram_metric: "m".to_string(),
            threshold: "1".to_string(),
            selector: None,
        };
        assert!(sli.queries().is_empty());
    }
}
