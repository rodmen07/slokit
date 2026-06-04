//! Error type for the `slokit` crate.

/// Errors produced by the core library.
///
/// I/O and CLI-level concerns are handled by the binary (via `anyhow`); this
/// enum stays focused on domain and parsing failures so library consumers get
/// precise, matchable variants.
#[derive(Debug, thiserror::Error)]
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
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, SlokitError>;
