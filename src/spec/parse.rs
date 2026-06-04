//! YAML loading for [`Spec`](super::Spec).

use std::path::Path;

use crate::error::{Result, SlokitError};

use super::Spec;

/// Parse a [`Spec`] from a YAML string.
pub fn from_yaml(yaml: &str) -> Result<Spec> {
    serde_norway::from_str(yaml).map_err(|e| SlokitError::Spec(e.to_string()))
}

/// Read and parse a [`Spec`] from a YAML file on disk.
pub fn from_path(path: &Path) -> Result<Spec> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| SlokitError::Spec(format!("reading {}: {e}", path.display())))?;
    from_yaml(&contents)
}
