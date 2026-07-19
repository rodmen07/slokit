//! Reusable, named SLI templates: the [`SliPlugin`] trait and its registry.
//!
//! A plugin expands declared, typed options into one of the existing core
//! [`Sli`] shapes. Expansion happens during spec resolution (before
//! validation), so plugin-provided queries flow through the exact same window
//! substitution, validation, generation, and promtool machinery as
//! hand-written ones. Specs reference a plugin with the sloth-compatible
//! shape:
//!
//! ```yaml
//! sli:
//!   plugin:
//!     id: slokit/availability/http-requests-total
//!     options:
//!       selector: job="api"
//! ```
//!
//! slokit's built-in plugins live under the `slokit/` id namespace (see
//! [`SliPluginRegistry::with_builtins`]). slokit never loads or executes
//! sloth's Go plugin files, so `sloth-common/...` ids fail resolution with a
//! clear unknown-plugin-id error instead of silently generating different
//! rules.
//!
//! # Registering a custom plugin
//!
//! ```
//! use std::collections::BTreeMap;
//!
//! use slokit::spec::plugin::{OptionKind, OptionSpec, SliPlugin, SliPluginRegistry};
//! use slokit::{Result, Sli};
//!
//! struct StaticRatio;
//!
//! impl SliPlugin for StaticRatio {
//!     fn id(&self) -> &str {
//!         "acme/static-ratio"
//!     }
//!     fn description(&self) -> &str {
//!         "error ratio from a pre-recorded ratio metric"
//!     }
//!     fn options(&self) -> &[OptionSpec] {
//!         const OPTIONS: &[OptionSpec] = &[OptionSpec::new(
//!             "metric",
//!             OptionKind::String,
//!             "name of the recorded error-ratio metric",
//!         )
//!         .required()];
//!         OPTIONS
//!     }
//!     fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli> {
//!         Ok(Sli::Raw {
//!             error_ratio_query: format!("avg_over_time({}[{{{{.window}}}}])", options["metric"]),
//!         })
//!     }
//! }
//!
//! let mut registry = SliPluginRegistry::empty();
//! registry.register(Box::new(StaticRatio)).unwrap();
//! let options = BTreeMap::from([("metric".to_string(), "app:error_ratio".to_string())]);
//! assert!(matches!(
//!     registry.resolve("acme/static-ratio", &options).unwrap(),
//!     Sli::Raw { .. }
//! ));
//! ```
//!
//! A spec that references an embedder-registered plugin fails plain
//! [`Spec::validate`](super::Spec::validate) with an unknown-plugin-id error;
//! pass the custom registry via the `_with` entry points
//! ([`Spec::validate_with`](super::Spec::validate_with),
//! [`SloSpec::to_sli_with`](super::SloSpec::to_sli_with), and
//! [`GenerateOptions::plugins`](crate::generate::GenerateOptions)).

use std::collections::BTreeMap;
use std::fmt;

use crate::error::{Result, SlokitError};
use crate::sli::{Sli, WINDOW_TOKEN};
use crate::window::Window;

use super::validate::{is_metric_name, quotes_balanced};

/// The kind of value an option accepts. Values arrive as strings
/// (sloth-compatible); the kind controls how they are checked.
///
/// The enum is `#[non_exhaustive]`: new kinds (for example an integer or
/// choice kind) may be added, so matches need a wildcard arm. Constructing
/// the existing variants remains supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptionKind {
    /// Any string, passed through (selector fragments, regexes, metric names).
    String,
    /// Must parse as a finite f64.
    Number,
    /// Must be "true" or "false".
    Bool,
    /// Must parse via [`Window::parse`] (e.g. "5m", "1h").
    Duration,
}

/// Declaration of one option a plugin accepts.
///
/// The struct is `#[non_exhaustive]` so declarations can gain fields (for
/// example a deprecation note) without breaking plugin authors. Build one
/// with the `const` builder, which works in the `const` tables plugins
/// typically use:
///
/// ```
/// use slokit::spec::plugin::{OptionKind, OptionSpec};
///
/// const OPTIONS: &[OptionSpec] = &[
///     OptionSpec::new("metric", OptionKind::String, "counter metric name")
///         .with_default("http_requests_total"),
///     OptionSpec::new("selector", OptionKind::String, "label matchers").required(),
/// ];
/// assert!(OPTIONS[1].required);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OptionSpec {
    /// Option name as written in the spec.
    pub name: &'static str,
    /// How the value is checked before expansion.
    pub kind: OptionKind,
    /// Whether the spec must provide it.
    pub required: bool,
    /// Default applied when an optional option is absent.
    pub default: Option<&'static str>,
    /// One-line description for docs and error messages.
    pub help: &'static str,
}

impl OptionSpec {
    /// Declare an optional option with no default: its name, the kind checked
    /// before expansion, and a one-line help string.
    pub const fn new(name: &'static str, kind: OptionKind, help: &'static str) -> Self {
        Self {
            name,
            kind,
            required: false,
            default: None,
            help,
        }
    }

    /// Mark the option required (the spec must provide it).
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Give the option a default applied when the spec omits it.
    pub const fn with_default(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }
}

/// A reusable, named SLI template: expands declared options into a core
/// [`Sli`].
pub trait SliPlugin: Send + Sync {
    /// Stable identifier referenced by `sli.plugin.id`.
    fn id(&self) -> &str;
    /// One-line description for docs and future listing commands.
    fn description(&self) -> &str;
    /// The options this plugin accepts; the registry validates against these
    /// before calling [`expand`](SliPlugin::expand).
    fn options(&self) -> &[OptionSpec];
    /// Build the SLI. Called with defaults applied and declared kinds already
    /// checked; `expand` only needs plugin-specific semantic checks.
    fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli>;
}

/// Holds [`SliPlugin`]s by id. [`Default`] is
/// [`with_builtins`](SliPluginRegistry::with_builtins).
///
/// [`register`](SliPluginRegistry::register) refuses duplicate ids (no silent
/// shadowing, including of built-ins), so an embedder cannot accidentally
/// redefine what a built-in id generates for specs shared across teams.
pub struct SliPluginRegistry {
    plugins: BTreeMap<String, Box<dyn SliPlugin>>,
}

impl fmt::Debug for SliPluginRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliPluginRegistry")
            .field("ids", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Default for SliPluginRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl SliPluginRegistry {
    /// A registry with no plugins at all.
    pub fn empty() -> Self {
        Self {
            plugins: BTreeMap::new(),
        }
    }

    /// A registry preloaded with slokit's built-in plugins:
    ///
    /// - `slokit/availability/http-requests-total`
    /// - `slokit/availability/grpc-server-handled`
    pub fn with_builtins() -> Self {
        let mut registry = Self::empty();
        registry
            .register(Box::new(HttpRequestsAvailability))
            .expect("built-in plugin ids are unique");
        registry
            .register(Box::new(GrpcServerHandledAvailability))
            .expect("built-in plugin ids are unique");
        registry
    }

    /// Add a plugin. Errors on a duplicate id (no silent shadowing, including
    /// of built-ins).
    pub fn register(&mut self, plugin: Box<dyn SliPlugin>) -> Result<()> {
        let id = plugin.id().to_string();
        if id.trim().is_empty() {
            return Err(SlokitError::Plugin(
                "plugin id must not be empty".to_string(),
            ));
        }
        if self.plugins.contains_key(&id) {
            return Err(SlokitError::Plugin(format!(
                "duplicate SLI plugin id '{id}' (already registered; ids cannot be shadowed)"
            )));
        }
        self.plugins.insert(id, plugin);
        Ok(())
    }

    /// Look up a plugin by id.
    pub fn get(&self, id: &str) -> Option<&dyn SliPlugin> {
        self.plugins.get(id).map(|p| p.as_ref())
    }

    /// Registered ids, sorted (stable output for docs and errors).
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(String::as_str)
    }

    /// Full resolution: look up `id`, apply declared defaults, check required
    /// options and kinds, then expand. This is what spec resolution
    /// ([`SloSpec::to_sli_with`](super::SloSpec::to_sli_with)) calls.
    ///
    /// Option names the plugin does not declare are not an error here (the
    /// extra key may be a forward-compatible spec shared across slokit
    /// versions); they are surfaced by the `PLUGIN_UNKNOWN_OPTION` lint.
    pub fn resolve(&self, id: &str, options: &BTreeMap<String, String>) -> Result<Sli> {
        let plugin = self.get(id).ok_or_else(|| {
            SlokitError::Plugin(format!(
                "unknown SLI plugin '{id}' (not in the plugin registry)"
            ))
        })?;

        let mut effective = options.clone();
        for spec in plugin.options() {
            match effective.get(spec.name) {
                Some(value) => check_kind(id, spec, value)?,
                None => {
                    if let Some(default) = spec.default {
                        effective.insert(spec.name.to_string(), default.to_string());
                    } else if spec.required {
                        return Err(SlokitError::Plugin(format!(
                            "plugin '{id}': missing required option `{}` ({})",
                            spec.name, spec.help
                        )));
                    }
                }
            }
        }

        plugin.expand(&effective)
    }
}

/// Check one provided option value against its declared [`OptionKind`].
fn check_kind(id: &str, spec: &OptionSpec, value: &str) -> Result<()> {
    match spec.kind {
        OptionKind::String => Ok(()),
        OptionKind::Number => match value.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(()),
            _ => Err(SlokitError::Plugin(format!(
                "plugin '{id}': option `{}` must be a finite number, got '{value}'",
                spec.name
            ))),
        },
        OptionKind::Bool => match value {
            "true" | "false" => Ok(()),
            _ => Err(SlokitError::Plugin(format!(
                "plugin '{id}': option `{}` must be \"true\" or \"false\", got '{value}'",
                spec.name
            ))),
        },
        OptionKind::Duration => match Window::parse(value) {
            Ok(_) => Ok(()),
            Err(e) => Err(SlokitError::Plugin(format!(
                "plugin '{id}': option `{}`: {e}",
                spec.name
            ))),
        },
    }
}

/// Reject a metric-name option outside the classic Prometheus charset. It is
/// embedded unquoted, so anything else is broken PromQL (same check as the
/// latency SLI's `histogram_metric`).
fn check_metric_option(id: &str, name: &str, value: &str) -> Result<()> {
    if !is_metric_name(value) {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` '{value}' is not a valid Prometheus metric name ([a-zA-Z_:][a-zA-Z0-9_:]*); it is embedded unquoted in the generated query"
        )));
    }
    Ok(())
}

/// Trim and validate a selector-shaped option value (label matchers written
/// without braces), returning `None` when absent or empty. Same checks and
/// error style as the latency SLI's `selector`.
fn check_selector_option<'a>(
    id: &str,
    name: &str,
    value: Option<&'a str>,
) -> Result<Option<&'a str>> {
    let Some(sel) = value.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if sel.contains('{') || sel.contains('}') {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` must not contain braces; write only the matchers, e.g. `job=\"api\"`"
        )));
    }
    if sel.starts_with(',') || sel.ends_with(',') {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` must not start or end with a comma"
        )));
    }
    if !quotes_balanced(sel) {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` has an unbalanced double quote"
        )));
    }
    Ok(Some(sel))
}

/// Validate an option value embedded inside a double-quoted matcher (like the
/// regex in `code=~"..."`): a double quote or a trailing unescaped backslash
/// would break out of the quoted string and generate PromQL that cannot load.
fn check_quoted_option(id: &str, name: &str, value: &str) -> Result<()> {
    if value.contains('"') {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` must not contain a double quote; the value is embedded inside a quoted label matcher"
        )));
    }
    let trailing_backslashes = value.chars().rev().take_while(|c| *c == '\\').count();
    if trailing_backslashes % 2 == 1 {
        return Err(SlokitError::Plugin(format!(
            "plugin '{id}': `{name}` must not end with an unescaped backslash; it would escape the closing quote of the label matcher"
        )));
    }
    Ok(())
}

/// Shared expansion for the built-in counter-availability plugins: bad and
/// total event rates from one counter, where the bad-event selector appends
/// `<code_matcher>"<regex>"` to the user's matchers.
fn expand_counter_availability(
    id: &str,
    options: &BTreeMap<String, String>,
    default_metric: &str,
    regex_option: &str,
    default_regex: &str,
    code_matcher: &str,
) -> Result<Sli> {
    let metric = options
        .get("metric")
        .map(String::as_str)
        .unwrap_or(default_metric);
    check_metric_option(id, "metric", metric)?;
    let selector =
        check_selector_option(id, "selector", options.get("selector").map(String::as_str))?;
    let regex = options
        .get(regex_option)
        .map(String::as_str)
        .unwrap_or(default_regex);
    check_quoted_option(id, regex_option, regex)?;

    let error_matchers = match selector {
        Some(sel) => format!("{sel}, {code_matcher}\"{regex}\""),
        None => format!("{code_matcher}\"{regex}\""),
    };
    let error_query = format!("sum(rate({metric}{{{error_matchers}}}[{WINDOW_TOKEN}]))");
    let total_query = match selector {
        Some(sel) => format!("sum(rate({metric}{{{sel}}}[{WINDOW_TOKEN}]))"),
        None => format!("sum(rate({metric}[{WINDOW_TOKEN}]))"),
    };
    Ok(Sli::Events {
        error_query,
        total_query,
    })
}

const HTTP_ID: &str = "slokit/availability/http-requests-total";

const HTTP_OPTIONS: &[OptionSpec] = &[
    OptionSpec::new("metric", OptionKind::String, "counter metric name")
        .with_default("http_requests_total"),
    OptionSpec::new(
        "selector",
        OptionKind::String,
        "label matchers without braces, e.g. job=\"api\"",
    ),
    OptionSpec::new(
        "error_code_regex",
        OptionKind::String,
        "regex for the `code` label identifying bad events",
    )
    .with_default("5.."),
];

/// Built-in `slokit/availability/http-requests-total`: availability from an
/// `http_requests_total`-style counter, where responses matching an
/// error-code regex are the bad events.
struct HttpRequestsAvailability;

impl SliPlugin for HttpRequestsAvailability {
    fn id(&self) -> &str {
        HTTP_ID
    }

    fn description(&self) -> &str {
        "availability from an http_requests_total-style counter, where responses matching an error-code regex are the bad events"
    }

    fn options(&self) -> &[OptionSpec] {
        HTTP_OPTIONS
    }

    fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli> {
        expand_counter_availability(
            HTTP_ID,
            options,
            "http_requests_total",
            "error_code_regex",
            "5..",
            "code=~",
        )
    }
}

const GRPC_ID: &str = "slokit/availability/grpc-server-handled";

const GRPC_OPTIONS: &[OptionSpec] = &[
    OptionSpec::new("metric", OptionKind::String, "counter metric name")
        .with_default("grpc_server_handled_total"),
    OptionSpec::new(
        "selector",
        OptionKind::String,
        "label matchers without braces, e.g. job=\"rpc\"",
    ),
    OptionSpec::new(
        "success_code_regex",
        OptionKind::String,
        "regex of `grpc_code` values counted as successes; any other code is a bad event",
    )
    .with_default("OK"),
];

/// Built-in `slokit/availability/grpc-server-handled`: availability from a
/// `grpc_server_handled_total`-style counter, where responses whose
/// `grpc_code` falls outside a success-code allowlist regex are the bad
/// events.
struct GrpcServerHandledAvailability;

impl SliPlugin for GrpcServerHandledAvailability {
    fn id(&self) -> &str {
        GRPC_ID
    }

    fn description(&self) -> &str {
        "availability from a grpc_server_handled_total-style counter, where responses with a grpc_code outside the success-code regex are the bad events"
    }

    fn options(&self) -> &[OptionSpec] {
        GRPC_OPTIONS
    }

    fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli> {
        expand_counter_availability(
            GRPC_ID,
            options,
            "grpc_server_handled_total",
            "success_code_regex",
            "OK",
            "grpc_code!~",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    struct Toy;

    const TOY_OPTIONS: &[OptionSpec] = &[
        OptionSpec {
            name: "metric",
            kind: OptionKind::String,
            required: true,
            default: None,
            help: "recorded ratio metric",
        },
        OptionSpec {
            name: "threshold",
            kind: OptionKind::Number,
            required: false,
            default: Some("0.5"),
            help: "a number",
        },
        OptionSpec {
            name: "enabled",
            kind: OptionKind::Bool,
            required: false,
            default: None,
            help: "a bool",
        },
        OptionSpec {
            name: "lookback",
            kind: OptionKind::Duration,
            required: false,
            default: None,
            help: "a duration",
        },
    ];

    impl SliPlugin for Toy {
        fn id(&self) -> &str {
            "test/toy"
        }
        fn description(&self) -> &str {
            "toy plugin for registry tests"
        }
        fn options(&self) -> &[OptionSpec] {
            TOY_OPTIONS
        }
        fn expand(&self, options: &BTreeMap<String, String>) -> Result<Sli> {
            // Echo the effective options so tests can observe defaults.
            Ok(Sli::Raw {
                error_ratio_query: format!(
                    "{}:{}[{WINDOW_TOKEN}]",
                    options["metric"], options["threshold"]
                ),
            })
        }
    }

    fn toy_registry() -> SliPluginRegistry {
        let mut r = SliPluginRegistry::empty();
        r.register(Box::new(Toy)).unwrap();
        r
    }

    #[test]
    fn builtins_are_registered_and_sorted() {
        let registry = SliPluginRegistry::with_builtins();
        let ids: Vec<&str> = registry.ids().collect();
        assert_eq!(
            ids,
            vec![
                "slokit/availability/grpc-server-handled",
                "slokit/availability/http-requests-total",
            ]
        );
        assert!(registry.get(HTTP_ID).is_some());
        assert!(registry.get("nope").is_none());
    }

    #[test]
    fn default_is_with_builtins() {
        let registry = SliPluginRegistry::default();
        assert_eq!(registry.ids().count(), 2);
    }

    #[test]
    fn empty_registry_has_no_ids() {
        assert_eq!(SliPluginRegistry::empty().ids().count(), 0);
    }

    #[test]
    fn duplicate_registration_is_an_error() {
        let mut registry = SliPluginRegistry::with_builtins();
        struct Impostor;
        impl SliPlugin for Impostor {
            fn id(&self) -> &str {
                HTTP_ID
            }
            fn description(&self) -> &str {
                "shadow attempt"
            }
            fn options(&self) -> &[OptionSpec] {
                &[]
            }
            fn expand(&self, _: &BTreeMap<String, String>) -> Result<Sli> {
                unreachable!()
            }
        }
        let err = registry.register(Box::new(Impostor)).unwrap_err();
        assert!(err.to_string().contains("duplicate SLI plugin id"));
    }

    #[test]
    fn debug_prints_the_ids() {
        let dbg = format!("{:?}", SliPluginRegistry::with_builtins());
        assert!(
            dbg.contains("slokit/availability/http-requests-total"),
            "{dbg}"
        );
    }

    #[test]
    fn resolve_unknown_id_is_an_error() {
        let err = SliPluginRegistry::with_builtins()
            .resolve("slokit/availabilty/http", &BTreeMap::new())
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown SLI plugin 'slokit/availabilty/http'"),
            "{msg}"
        );
        assert!(msg.contains("not in the plugin registry"), "{msg}");
    }

    #[test]
    fn resolve_missing_required_option_is_an_error() {
        let err = toy_registry()
            .resolve("test/toy", &BTreeMap::new())
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing required option `metric`"), "{msg}");
        assert!(msg.contains("recorded ratio metric"), "{msg}");
    }

    #[test]
    fn resolve_applies_defaults() {
        let sli = toy_registry()
            .resolve("test/toy", &opts(&[("metric", "m")]))
            .unwrap();
        assert_eq!(
            sli,
            Sli::Raw {
                error_ratio_query: "m:0.5[{{.window}}]".to_string()
            }
        );
    }

    #[test]
    fn resolve_checks_declared_kinds() {
        let registry = toy_registry();
        let cases = [
            (("threshold", "abc"), "must be a finite number"),
            (("threshold", "NaN"), "must be a finite number"),
            (("enabled", "yes"), "must be \"true\" or \"false\""),
            (("lookback", "5x"), "invalid duration"),
        ];
        for ((name, value), needle) in cases {
            let err = registry
                .resolve("test/toy", &opts(&[("metric", "m"), (name, value)]))
                .unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains(needle), "option {name}={value}: {msg}");
            assert!(msg.contains("test/toy"), "option {name}={value}: {msg}");
        }
    }

    #[test]
    fn resolve_accepts_sound_typed_values() {
        let registry = toy_registry();
        let sli = registry
            .resolve(
                "test/toy",
                &opts(&[
                    ("metric", "m"),
                    ("threshold", "0.25"),
                    ("enabled", "true"),
                    ("lookback", "1h30m"),
                ]),
            )
            .unwrap();
        assert!(matches!(sli, Sli::Raw { .. }));
    }

    #[test]
    fn resolve_ignores_undeclared_option_names() {
        // Undeclared names are a lint, not an error: generation still works.
        let sli = toy_registry()
            .resolve("test/toy", &opts(&[("metric", "m"), ("mystery", "x")]))
            .unwrap();
        assert!(matches!(sli, Sli::Raw { .. }));
    }

    #[test]
    fn http_expansion_matches_the_worked_example() {
        let registry = SliPluginRegistry::with_builtins();
        let sli = registry
            .resolve(HTTP_ID, &opts(&[("selector", "job=\"api\"")]))
            .unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query:
                    "sum(rate(http_requests_total{job=\"api\", code=~\"5..\"}[{{.window}}]))"
                        .to_string(),
                total_query: "sum(rate(http_requests_total{job=\"api\"}[{{.window}}]))".to_string(),
            }
        );
    }

    #[test]
    fn http_expansion_without_selector() {
        let sli = SliPluginRegistry::with_builtins()
            .resolve(HTTP_ID, &BTreeMap::new())
            .unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query: "sum(rate(http_requests_total{code=~\"5..\"}[{{.window}}]))"
                    .to_string(),
                total_query: "sum(rate(http_requests_total[{{.window}}]))".to_string(),
            }
        );
    }

    #[test]
    fn http_expansion_with_custom_metric_and_regex() {
        let sli = SliPluginRegistry::with_builtins()
            .resolve(
                HTTP_ID,
                &opts(&[
                    ("metric", "nginx_http_requests_total"),
                    ("error_code_regex", "5..|429"),
                ]),
            )
            .unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query: "sum(rate(nginx_http_requests_total{code=~\"5..|429\"}[{{.window}}]))"
                    .to_string(),
                total_query: "sum(rate(nginx_http_requests_total[{{.window}}]))".to_string(),
            }
        );
    }

    #[test]
    fn grpc_expansion_defaults_to_ok_allowlist() {
        let sli = SliPluginRegistry::with_builtins()
            .resolve(GRPC_ID, &opts(&[("selector", "job=\"rpc\"")]))
            .unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query:
                    "sum(rate(grpc_server_handled_total{job=\"rpc\", grpc_code!~\"OK\"}[{{.window}}]))"
                        .to_string(),
                total_query: "sum(rate(grpc_server_handled_total{job=\"rpc\"}[{{.window}}]))"
                    .to_string(),
            }
        );
    }

    #[test]
    fn grpc_expansion_with_custom_allowlist() {
        let sli = SliPluginRegistry::with_builtins()
            .resolve(GRPC_ID, &opts(&[("success_code_regex", "OK|NotFound")]))
            .unwrap();
        assert_eq!(
            sli,
            Sli::Events {
                error_query:
                    "sum(rate(grpc_server_handled_total{grpc_code!~\"OK|NotFound\"}[{{.window}}]))"
                        .to_string(),
                total_query: "sum(rate(grpc_server_handled_total[{{.window}}]))".to_string(),
            }
        );
    }

    #[test]
    fn broken_selector_options_are_errors() {
        let registry = SliPluginRegistry::with_builtins();
        let cases = [
            ("{job=\"x\"}", "must not contain braces"),
            ("job=\"x\",", "must not start or end with a comma"),
            (",job=\"x\"", "must not start or end with a comma"),
            ("job=\"x", "unbalanced double quote"),
        ];
        for (selector, needle) in cases {
            let err = registry
                .resolve(HTTP_ID, &opts(&[("selector", selector)]))
                .unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains(needle), "selector {selector}: {msg}");
        }
    }

    #[test]
    fn empty_selector_option_is_treated_as_absent() {
        let sli = SliPluginRegistry::with_builtins()
            .resolve(HTTP_ID, &opts(&[("selector", "   ")]))
            .unwrap();
        assert_eq!(
            sli.queries()[1],
            "sum(rate(http_requests_total[{{.window}}]))"
        );
    }

    #[test]
    fn invalid_metric_option_is_an_error() {
        let err = SliPluginRegistry::with_builtins()
            .resolve(HTTP_ID, &opts(&[("metric", "http requests total")]))
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("not a valid Prometheus metric name"));
    }

    #[test]
    fn regex_options_that_break_the_quoted_matcher_are_errors() {
        let registry = SliPluginRegistry::with_builtins();
        let err = registry
            .resolve(HTTP_ID, &opts(&[("error_code_regex", "5\"..")]))
            .unwrap_err();
        assert!(err.to_string().contains("must not contain a double quote"));
        let err = registry
            .resolve(HTTP_ID, &opts(&[("error_code_regex", "5..\\")]))
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("must not end with an unescaped backslash"));
        // An even number of trailing backslashes is a legal escaped backslash.
        assert!(registry
            .resolve(HTTP_ID, &opts(&[("error_code_regex", "5..\\\\")]))
            .is_ok());
    }

    #[test]
    fn builtin_descriptions_and_options_are_exposed() {
        let registry = SliPluginRegistry::with_builtins();
        let http = registry.get(HTTP_ID).unwrap();
        assert!(http.description().contains("http_requests_total"));
        let names: Vec<&str> = http.options().iter().map(|o| o.name).collect();
        assert_eq!(names, vec!["metric", "selector", "error_code_regex"]);
        let grpc = registry.get(GRPC_ID).unwrap();
        let names: Vec<&str> = grpc.options().iter().map(|o| o.name).collect();
        assert_eq!(names, vec!["metric", "selector", "success_code_regex"]);
    }
}
