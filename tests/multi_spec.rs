//! Directory (multi-spec) loading, merged generation, and multi-dashboard output.

use std::fs;
use std::path::PathBuf;

use slokit::dashboard::dashboards_json;
use slokit::generate::{generate_all, GenerateOptions};
use slokit::spec::Spec;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("slokit-test-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

const SPEC_A: &str = r#"
service: alpha
slos:
  - name: avail
    objective: 99.9
    sli:
      raw:
        error_ratio_query: a[{{.window}}]
"#;

const SPEC_B: &str = r#"
service: beta
slos:
  - name: avail
    objective: 99.0
    sli:
      raw:
        error_ratio_query: b[{{.window}}]
"#;

#[test]
fn loads_and_merges_directory_of_specs() {
    let dir = temp_dir("dir");
    fs::write(dir.join("a.yaml"), SPEC_A).unwrap();
    fs::write(dir.join("b.yml"), SPEC_B).unwrap();
    fs::write(dir.join("README.md"), "ignored").unwrap();

    let specs = Spec::from_dir(&dir).unwrap();
    assert_eq!(specs.len(), 2);
    // Sorted by path: a.yaml before b.yml.
    assert_eq!(specs[0].service, "alpha");
    assert_eq!(specs[1].service, "beta");

    // generate_all merges: each spec yields 3 groups -> 6 total.
    let rs = generate_all(&specs, &GenerateOptions::default()).unwrap();
    assert_eq!(rs.groups.len(), 6);

    // Multiple specs render a JSON array of dashboards.
    let json = dashboards_json(&specs).unwrap();
    assert!(json.trim_start().starts_with('['));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn single_spec_dashboard_is_an_object() {
    let specs = vec![Spec::from_yaml(SPEC_A).unwrap()];
    let json = dashboards_json(&specs).unwrap();
    assert!(json.trim_start().starts_with('{'));
}

#[test]
fn empty_directory_is_an_error() {
    let dir = temp_dir("empty");
    assert!(Spec::from_dir(&dir).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_service_slo_pair_across_specs_fails_merged_generation() {
    // Two specs with the same service and SLO name would repeat rule-group
    // names in the merged output, which Prometheus refuses to load.
    let specs = vec![
        Spec::from_yaml(SPEC_A).unwrap(),
        Spec::from_yaml(SPEC_A).unwrap(),
    ];
    let err = generate_all(&specs, &GenerateOptions::default()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("duplicate service/SLO pair across specs"),
        "{msg}"
    );
    assert!(msg.contains("service 'alpha'"), "{msg}");
}
