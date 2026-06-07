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

/// Read and parse every `*.yaml`/`*.yml` spec in a directory, sorted by path.
pub fn from_dir(dir: &Path) -> Result<Vec<Spec>> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| SlokitError::Spec(format!("reading dir {}: {e}", dir.display())))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    paths.sort();

    if paths.is_empty() {
        return Err(SlokitError::Spec(format!(
            "no .yaml/.yml spec files found in {}",
            dir.display()
        )));
    }
    paths.iter().map(|p| from_path(p)).collect()
}

/// Load one or many specs from a path: a single file yields one spec, a
/// directory yields every `*.yaml`/`*.yml` spec it contains.
pub fn load(path: &Path) -> Result<Vec<Spec>> {
    if path.is_dir() {
        from_dir(path)
    } else {
        Ok(vec![from_path(path)?])
    }
}
