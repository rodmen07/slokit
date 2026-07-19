//! Error type for the `slokit` crate.

/// Errors produced by the core library.
///
/// I/O and CLI-level concerns are handled by the binary (via `anyhow`); this
/// enum stays focused on domain and parsing failures so library consumers get
/// precise, matchable variants.
///
/// The enum is `#[non_exhaustive]`: new failure domains have been added before
/// (the `Plugin` variant arrived with the SLI plugin system) and more may
/// follow, so matches need a wildcard arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SlokitError {
    /// An objective was outside the open interval that makes it a valid SLO.
    #[error("invalid objective: {0}")]
    InvalidObjective(String),

    /// A duration string (e.g. `30d`, `1h`) could not be parsed.
    #[error("invalid duration: {0}")]
    InvalidDuration(String),

    /// A spec could not be deserialized or was structurally malformed.
    #[error("spec error: {0}")]
    Spec(String),

    /// A spec parsed but failed semantic validation. The message contains one
    /// line per problem found.
    #[error("validation failed:\n{0}")]
    Validation(String),

    /// A live Prometheus query failed (transport, HTTP status, or response
    /// shape). Only produced with the `check` feature.
    #[error("prometheus query failed: {0}")]
    Query(String),

    /// An SLI plugin registry operation failed: a duplicate or unknown plugin
    /// id, or option values that violate the plugin's declared contract. Only
    /// produced with the `spec` feature.
    #[error("plugin error: {0}")]
    Plugin(String),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, SlokitError>;
