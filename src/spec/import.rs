//! The result type every spec importer returns.
//!
//! Moved here verbatim from [`super::openslo`] in v1.6.0 PR 1, when a second
//! dialect ([`super::sloth_crd`]) needed the same two types: an importer
//! reports converted [`Spec`]s plus lint-style notes, and that shape is not
//! OpenSLO-specific. Both types stay re-exported from
//! [`super::openslo`], so `slokit::spec::openslo::{Import, ImportNote}`
//! remains the path it has always been (1.x is API-frozen; this is a move
//! plus a re-export, not a rename).

use super::Spec;

/// The result of importing a foreign SLO document into slokit's model: the
/// converted specs plus lint-style notes about constructs that were dropped
/// or transformed.
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume,
/// and future report fields must not be breaking changes.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Import {
    /// The converted specs, in first-seen document order. Run
    /// [`validate_all`](super::validate_all) before generating rules from
    /// them. How documents map onto specs is the importer's contract: the
    /// OpenSLO importer merges `kind: SLO` documents that share a
    /// `spec.service`, the sloth CRD importer yields one spec per
    /// `PrometheusServiceLevel` document.
    pub specs: Vec<Spec>,
    /// Lint-style notes: source constructs that do not map one-to-one and
    /// were ignored or rewritten. An empty vec means a lossless import.
    pub notes: Vec<ImportNote>,
}

/// One advisory note produced during import (not an error: the document was
/// representable, but something was dropped or rewritten on the way in).
///
/// The struct is `#[non_exhaustive]`: it is an output type readers consume,
/// and future fields (for example a machine-readable code) must not be
/// breaking changes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportNote {
    /// Where the note applies, e.g. `slo 'requests-availability'`.
    pub location: String,
    /// What was ignored or rewritten, and why.
    pub message: String,
}

impl std::fmt::Display for ImportNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// The note every importer emits for a dropped object-envelope
/// `metadata.annotations`.
///
/// All three dialects that carry an object envelope — `openslo/v1`,
/// `openslo/v1alpha` and the sloth Kubernetes CRD — accept the key and drop
/// it, and nothing it holds reaches the generated rules on any route. The only
/// thing that could ever differ between them is whether they SAY so, and in
/// what words. `openslo/v1` has said it since 0.10.0; the other two dropped it
/// in silence until 1.10.0.
///
/// It lives here, in the module all three importers already share for
/// [`Import`] and [`ImportNote`], rather than as a literal at each emission
/// site: three literals can be reworded one at a time, which is exactly what
/// "both routes note the drop, in different words" looked like from a distance
/// before 1.8.0 closed the same gap for `metadata.displayName`.
///
/// `tests/dialect_parity.rs` records this same string as the disposition of
/// all three `object-annotations` rows and asserts it against the shipped
/// binary, so a reword here that does not reach the contract fails there.
pub(super) const OBJECT_ANNOTATIONS_NOTE: &str = "metadata.annotations do not map and were ignored";
