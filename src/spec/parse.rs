//! YAML loading for [`Spec`](super::Spec).

use std::path::Path;

use serde::Deserialize;
use serde_norway::{Deserializer as YamlDeserializer, Value};

use crate::error::{Result, SlokitError};

use super::import::{accepted_api_groups, is_known_api_group};
use super::Spec;

/// Rephrase a failed native parse when the document declared an `apiVersion`
/// naming no dialect slokit reads.
///
/// The failure this exists for looked, until 1.10.0, exactly like a native
/// document with a typo: pointing `slokit validate` at a Kubernetes manifest,
/// or at an SLO document written for some other tool, reported
/// ``missing field `service` `` — a required field named as absent from a
/// document that is in fact well-formed in a format slokit was never asked to
/// read. The reader is told about a field and learns nothing about the format
/// mismatch that actually happened, which is the one thing they could act on.
///
/// The accepted set comes from [`super::import::KNOWN_API_GROUPS`], the same
/// constant the auto-detector's prefixes are composed from, and it is PRINTED
/// rather than merely tested: a mis-derived set is invisible on a tree where
/// nothing is wrong, and this message is one of only two places it is ever
/// observed.
///
/// The underlying native error is kept, not replaced. It is still the answer
/// for the reader who really did mean to write a native spec and prepended a
/// stray key, and dropping it would trade one blind message for another.
///
/// This reaches only documents that ALREADY failed, so it rewords an error and
/// changes nothing about what parses (`ROADMAP.md` D1.10-3).
fn explain(context: Option<&str>, api_version: Option<&str>, native_error: &str) -> SlokitError {
    let prefix = match context {
        Some(c) => format!("{c}: "),
        None => String::new(),
    };
    match api_version {
        // No reader at all: an unrecognised group.
        Some(v) if !is_known_api_group(v) => SlokitError::Spec(format!(
            "{prefix}apiVersion '{v}' names no format slokit reads (accepted: {}), \
             and the document is not a native slokit spec either \
             (the native format declares no apiVersion): {native_error}",
            accepted_api_groups()
        )),
        // A reader exists, but this is not it. The bug's own repro: pointing
        // the NATIVE parser at a sloth `PrometheusServiceLevel` (by pinning
        // `--input-format slokit`, or by calling `Spec::from_yaml` on one from
        // an embedder) reported ``missing field `service` `` about a document
        // slokit imports perfectly well through its own route. Naming a
        // dialect it does read is a different answer from naming none, so the
        // two cases get different messages rather than one that fits neither.
        Some(v) => SlokitError::Spec(format!(
            "{prefix}apiVersion '{v}' names a dialect slokit imports rather than the native spec \
             format, and this document was read as a native slokit spec \
             (the native format declares no apiVersion): {native_error}"
        )),
        // No `apiVersion` at all: an ordinary native document with an ordinary
        // native mistake. Untouched, which is the point — this reword must not
        // reach the reader who really did mean to write a native spec.
        None => SlokitError::Spec(format!("{prefix}{native_error}")),
    }
}

/// The document's own top-level `apiVersion`, if it declared one as a string.
///
/// Read off the already-deserialized [`Value`] so the stream path does not
/// parse the document twice, and shared with [`from_yaml`]'s error path so the
/// two routes cannot disagree about what the document declared.
fn declared_api_version(value: &Value) -> Option<&str> {
    value.get("apiVersion").and_then(Value::as_str)
}

/// Parse a [`Spec`] from a YAML string.
pub fn from_yaml(yaml: &str) -> Result<Spec> {
    serde_norway::from_str(yaml).map_err(|e| {
        // Only on the error path: re-reading the document as a `Value` costs
        // nothing when the parse succeeded, and doing it up front would change
        // which error a non-mapping document reports.
        let value = serde_norway::from_str::<Value>(yaml).ok();
        explain(
            None,
            value.as_ref().and_then(declared_api_version),
            &e.to_string(),
        )
    })
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
        let api_version = declared_api_version(&value).map(str::to_owned);
        let spec: Spec = serde_norway::from_value(value).map_err(|e| {
            explain(
                Some(&format!("document {n}")),
                api_version.as_deref(),
                &e.to_string(),
            )
        })?;
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
