//! The provenance seam (v1.10.0 PR 1): every route records which dialect
//! produced the [`Spec`] it returns, and recording it changes nothing else.
//!
//! slokit reads four input dialects and converts all four into the same
//! `Spec`. Until this seam existed, the conversion was where the document's
//! own vocabulary stopped: a message could only answer in native slokit
//! spelling, to a reader who may never have written a native spec. The fix is
//! two additive fields — [`Spec::dialect`] and [`Spec::api_version`] — and the
//! whole risk of adding them is that they are provenance masquerading as spec
//! data. So this file asserts the seam in both directions:
//!
//! - **It is set**, on every one of the four routes, with the `apiVersion` the
//!   document actually declared (or none, for the native format, which has no
//!   such key).
//! - **It is inert**: `Serialize` emits neither field, so a spec parsed from a
//!   document carrying an `apiVersion` serializes byte-identically to the same
//!   document without one, and no YAML key appeared in the spec format.
//!
//! The one visible consequence is deliberate and is asserted rather than
//! discovered: `Spec` derives `PartialEq`, so provenance is part of equality.
//!
//! The message this seam pays for is asserted at the binary level in
//! `tests/sloth_crd_cli.rs::the_spec_level_chain_finding_names_the_spelling_of_the_dialect_that_carried_it`;
//! what this file owns is the seam itself.

#![cfg(feature = "spec")]

use slokit::spec::{openslo, sloth_crd, SourceDialect, Spec};

const NATIVE_CHAINED: &str = include_str!("fixtures/sloth_corpus/slo-plugin-getting-started.yml");
const CRD_CHAINED: &str = include_str!("fixtures/sloth_corpus/slo-plugin-k8s-getting-started.yml");

/// A minimal valid native spec, used where the point is the shape of the
/// struct rather than the content of a document.
const NATIVE_MINIMAL: &str = "\
service: api
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(errors[{{.window}}]))
        total_query: sum(rate(total[{{.window}}]))
";

/// The same document with a top-level `apiVersion` a Kubernetes reader would
/// recognise and slokit does not. It still parses: the native parser does not
/// deny unknown fields, and refusing it would break docs/SEMVER.md's clause
/// that YAML validating under 1.a validates under 1.b.
const NATIVE_WITH_FOREIGN_API_VERSION: &str = "\
apiVersion: apps/v1
service: api
slos:
  - name: availability
    objective: 99.9
    sli:
      events:
        error_query: sum(rate(errors[{{.window}}]))
        total_query: sum(rate(total[{{.window}}]))
";

/// An `openslo/v1` SLO document for `service`, in the shape that dialect's
/// importer accepts (`indicator.spec.ratioMetric`, `timeWindow`).
fn openslo_v1_slo(service: &str, name: &str) -> String {
    format!(
        "\
apiVersion: openslo/v1
kind: SLO
metadata:
  name: {name}
spec:
  service: {service}
  budgetingMethod: Occurrences
  timeWindow:
    - duration: 30d
      isRolling: true
  indicator:
    metadata:
      name: {name}-sli
    spec:
      ratioMetric:
        counter: true
        good:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(good[{{{{.window}}}}]))
        total:
          metricSource:
            type: Prometheus
            spec:
              query: sum(rate(total[{{{{.window}}}}]))
  objectives:
    - target: 0.999
"
    )
}

/// The same SLO in `openslo/v1alpha`, whose schema is genuinely different
/// (`objectives[].ratioMetrics`, `timeWindows`) — which is why the two
/// versions have separate importers, and why provenance can distinguish them.
fn openslo_v1alpha_slo(service: &str, name: &str) -> String {
    format!(
        "\
apiVersion: openslo/v1alpha
kind: SLO
metadata:
  name: {name}
spec:
  service: {service}
  budgetingMethod: Occurrences
  objectives:
    - ratioMetrics:
        good:
          source: prometheus
          queryType: promql
          query: sum(rate(good[{{{{.window}}}}]))
        total:
          source: prometheus
          queryType: promql
          query: sum(rate(total[{{{{.window}}}}]))
      target: 0.999
  timeWindows:
    - count: 30
      unit: Day
"
    )
}

// ---------------------------------------------------------------------------
// The seam is set, on every route
// ---------------------------------------------------------------------------

/// **Done-when clause 1, the "it exists and every importer sets it" half.**
///
/// One table over all four routes rather than four tests, because the claim is
/// comparative: what makes the seam worth having is that the routes DISAGREE
/// about their answer. Four separate tests would each pass while the four
/// answers were the same value.
#[test]
fn every_route_records_the_dialect_and_api_version_of_the_document_it_read() {
    let native = Spec::from_yaml(NATIVE_MINIMAL).expect("native spec parses");
    assert_eq!(native.dialect, SourceDialect::Native);
    assert_eq!(
        native.api_version, None,
        "the native format has no apiVersion key; it spells its format version `version:`"
    );

    let v1 =
        openslo::from_yaml(&openslo_v1_slo("api", "availability")).expect("openslo/v1 imports");
    assert_eq!(v1.specs.len(), 1);
    assert_eq!(v1.specs[0].dialect, SourceDialect::OpenSloV1);
    assert_eq!(v1.specs[0].api_version.as_deref(), Some("openslo/v1"));

    let v1alpha = openslo::from_yaml(&openslo_v1alpha_slo("api", "availability"))
        .expect("openslo/v1alpha imports");
    assert_eq!(v1alpha.specs.len(), 1);
    assert_eq!(v1alpha.specs[0].dialect, SourceDialect::OpenSloV1Alpha);
    assert_eq!(
        v1alpha.specs[0].api_version.as_deref(),
        Some("openslo/v1alpha")
    );

    let crd = sloth_crd::from_yaml(CRD_CHAINED).expect("sloth CRD imports");
    assert_eq!(crd.specs.len(), 1);
    assert_eq!(crd.specs[0].dialect, SourceDialect::SlothCrd);
    assert_eq!(
        crd.specs[0].api_version.as_deref(),
        Some("sloth.slok.dev/v1")
    );

    // The comparative half: four routes, four different answers. A seam that
    // returned one constant would satisfy every assertion above.
    let dialects = [
        native.dialect,
        v1.specs[0].dialect,
        v1alpha.specs[0].dialect,
        crd.specs[0].dialect,
    ];
    for (i, a) in dialects.iter().enumerate() {
        for b in &dialects[i + 1..] {
            assert_ne!(a, b, "two routes report the same dialect: {dialects:?}");
        }
    }
}

/// A native document carrying a foreign `apiVersion` keeps its own dialect and
/// captures the string verbatim.
///
/// This is the input v1.10.0 PR 3 turns into an `UNKNOWN_API_VERSION` finding;
/// PR 1 owes only that the value survives the parse, unmodified and
/// un-normalised, because a message that quotes it must quote what the author
/// wrote.
#[test]
fn a_native_document_with_a_foreign_api_version_captures_it_verbatim() {
    let spec = Spec::from_yaml(NATIVE_WITH_FOREIGN_API_VERSION)
        .expect("a native spec with an unknown top-level key still parses");
    assert_eq!(spec.dialect, SourceDialect::Native);
    assert_eq!(spec.api_version.as_deref(), Some("apps/v1"));
    spec.validate().expect("and it still validates");
}

/// Provenance on merged OpenSLO specs is FIRST-DOCUMENT-WINS, stated as a
/// decision rather than left as an accident.
///
/// SLO documents sharing a `spec.service` merge into ONE spec regardless of
/// which OpenSLO version each declared, and one spec can hold only one
/// dialect. The rule is therefore that the document which CREATED the spec
/// names it. Asserted in the order that can fail: the v1alpha document comes
/// second, so a "last wins" or "whichever ran last" implementation reports
/// `OpenSloV1Alpha` here.
#[test]
fn a_mixed_version_stream_records_the_dialect_that_created_the_spec() {
    let stream = format!(
        "{}---\n{}",
        openslo_v1_slo("api", "availability"),
        openslo_v1alpha_slo("api", "latency")
    );
    let import = openslo::from_yaml(&stream).expect("a mixed stream imports");
    assert_eq!(
        import.specs.len(),
        1,
        "both documents name the same service, so they merge"
    );
    assert_eq!(import.specs[0].slos.len(), 2, "and both SLOs are kept");
    assert_eq!(import.specs[0].dialect, SourceDialect::OpenSloV1);
    assert_eq!(import.specs[0].api_version.as_deref(), Some("openslo/v1"));
}

// ---------------------------------------------------------------------------
// The seam is inert
// ---------------------------------------------------------------------------

/// **Done-when clause 1, the "additive, no YAML key appeared" half.**
///
/// Serializing a `Spec` must emit neither field, so the spec format gains no
/// key and `schema/slokit-spec.schema.json` needs no edit. Proven by the pair
/// that can actually differ: the same document with and without an
/// `apiVersion` line. An assertion that the output merely lacks the substring
/// `apiVersion` would pass on an empty string, so the two renderings are
/// compared to each other AND to the literal bytes expected.
#[test]
fn serializing_a_spec_emits_neither_provenance_field() {
    let plain = Spec::from_yaml(NATIVE_MINIMAL).expect("native spec parses");
    let foreign =
        Spec::from_yaml(NATIVE_WITH_FOREIGN_API_VERSION).expect("and so does the annotated one");
    assert_ne!(
        plain, foreign,
        "the two inputs differ, so a byte-identical rendering is a real claim"
    );

    let plain_yaml = serde_norway::to_string(&plain).expect("a spec serializes");
    let foreign_yaml = serde_norway::to_string(&foreign).expect("a spec serializes");
    assert_eq!(
        plain_yaml, foreign_yaml,
        "the captured apiVersion reached the serialized spec"
    );

    // The bytes themselves, so "identical" cannot become "identically wrong":
    // this is today's output, unchanged by the seam.
    assert_eq!(
        plain_yaml,
        "\
version: prometheus/v1
service: api
slos:
- name: availability
  objective: 99.9
  sli:
    events:
      error_query: sum(rate(errors[{{.window}}]))
      total_query: sum(rate(total[{{.window}}]))
  alerting:
    page_alert:
      disable: false
    ticket_alert:
      disable: false
"
    );
}

/// The one visible consequence of D1.10-1, asserted rather than assumed.
///
/// `Spec` derives `PartialEq`, so two specs whose every other field agrees now
/// compare UNEQUAL when they came from different dialects. No whole-`Spec`
/// equality assertion existed in the suite when the field was added (the
/// existing `assert_eq!`s compare fields), so nothing broke — but embedders
/// have the derive too, and the effect is stated here so it is a documented
/// property with a test rather than a surprise.
#[test]
fn provenance_participates_in_spec_equality() {
    let native = Spec::from_yaml(NATIVE_CHAINED).expect("native chained spec parses");
    let crd = sloth_crd::from_yaml(CRD_CHAINED).expect("its CRD twin imports");
    let crd = &crd.specs[0];

    // The twins agree on everything the generator reads — the property
    // `tests/sloth_crd.rs` asserts by byte-identity of the generated rules.
    assert_eq!(native.service, crd.service);
    assert_eq!(native.labels, crd.labels);
    assert_eq!(native.slos, crd.slos);
    assert_eq!(native.slo_plugins, crd.slo_plugins);

    // And they are nevertheless unequal, because they remember where they
    // came from. That is the whole point of the seam.
    assert_ne!(&native, crd);

    let mut relabelled = native.clone();
    relabelled.dialect = SourceDialect::SlothCrd;
    relabelled.api_version = crd.api_version.clone();
    assert_eq!(
        &relabelled, crd,
        "provenance is the ONLY difference between the twins"
    );
}
