//! YAML loading for [`Spec`](super::Spec).

use std::path::Path;

use serde::Deserialize;
use serde_norway::{Deserializer as YamlDeserializer, Value};

use crate::error::{Result, SlokitError};

use super::Spec;

/// Parse a [`Spec`] from a YAML string.
pub fn from_yaml(yaml: &str) -> Result<Spec> {
    serde_norway::from_str(yaml).map_err(|e| SlokitError::Spec(e.to_string()))
}

/// Parse every document of a YAML stream as a [`Spec`], in stream order.
///
/// A plain single-document file yields one spec, so this is a superset of
/// [`from_yaml`]; the reason both exist is that a stream is the natural shape
/// for "many specs in one file" (sloth's `examples/multifile.yml`, or the
/// stream `slokit export` writes to stdout), while [`from_yaml`] keeps the
/// exactly-one contract embedders may rely on. Empty documents (stray `---`
/// separators, comment-only documents) are skipped, matching the OpenSLO
/// importer. Errors name the failing document by its 1-based position in the
/// stream; a stream with no non-empty documents is an error.
pub fn from_yaml_stream(yaml: &str) -> Result<Vec<Spec>> {
    let mut specs = Vec::new();
    for (idx, de) in YamlDeserializer::from_str(yaml).enumerate() {
        let n = idx + 1;
        let value =
            Value::deserialize(de).map_err(|e| SlokitError::Spec(format!("document {n}: {e}")))?;
        if value.is_null() {
            continue;
        }
        let spec: Spec = serde_norway::from_value(value)
            .map_err(|e| SlokitError::Spec(format!("document {n}: {e}")))?;
        specs.push(spec);
    }
    if specs.is_empty() {
        return Err(SlokitError::Spec(
            "input contains no YAML documents".to_string(),
        ));
    }
    Ok(specs)
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
