//! Dogfooding: the InfraPortal platform SLO example set.
//!
//! `examples/infraportal/` holds real SLO specs for the InfraPortal
//! microservices platform (the same services shown at
//! rodmen07.github.io/infraportal) plus the Prometheus rules slokit generates
//! from them. These tests keep that example honest:
//!
//! 1. every spec validates,
//! 2. the committed `rules.yaml` is exactly what slokit produces today, so it
//!    can never silently drift from the generator (the same byte-stability
//!    contract the internal snapshot tests hold, applied to the public
//!    example), and
//! 3. the set covers every service with an availability and a latency SLO.

use std::fs;
use std::path::{Path, PathBuf};

use slokit::generate::{generate_all, GenerateOptions};
use slokit::spec::{validate_all, Spec};
use slokit::{MwmbrConfig, Window};

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/infraportal")
}

fn load_specs() -> Vec<Spec> {
    let mut files: Vec<PathBuf> = fs::read_dir(example_dir().join("slos"))
        .expect("read examples/infraportal/slos")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("yaml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no example specs found");
    files
        .iter()
        .map(|p| {
            let yaml = fs::read_to_string(p).unwrap();
            Spec::from_yaml(&yaml).unwrap_or_else(|e| panic!("spec {} parses: {e}", p.display()))
        })
        .collect()
}

/// Mirror the CLI `generate -i <dir>` defaults exactly.
fn cli_options() -> GenerateOptions {
    let mut opts = GenerateOptions::default();
    opts.default_period = Window::parse("30d").unwrap();
    opts.mwmbr = MwmbrConfig::sre_default();
    opts.period_aware = true;
    opts
}

/// Normalise line endings and blank out the version stamp so the comparison
/// survives CRLF checkouts and version bumps (only the rule content matters).
fn normalise(s: &str) -> String {
    s.replace('\r', "")
        .lines()
        .map(|l| match l.find("sloth_version:") {
            Some(i) => format!("{}sloth_version: [VERSION]", &l[..i]),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[test]
fn every_platform_spec_validates() {
    validate_all(&load_specs()).expect("all InfraPortal example specs are valid");
}

#[test]
fn covers_every_platform_service_with_two_slos() {
    let specs = load_specs();
    assert_eq!(specs.len(), 8, "expected 8 platform services");
    for spec in &specs {
        assert_eq!(
            spec.slos.len(),
            2,
            "{} should define an availability and a latency SLO",
            spec.service
        );
    }
}

#[test]
fn committed_rules_match_regeneration() {
    let generated = generate_all(&load_specs(), &cli_options())
        .expect("generation succeeds")
        .to_prometheus_yaml()
        .expect("renders to prometheus yaml");
    let committed =
        fs::read_to_string(example_dir().join("rules.yaml")).expect("read committed rules.yaml");

    assert_eq!(
        normalise(&committed),
        normalise(&generated),
        "examples/infraportal/rules.yaml is stale; regenerate with \
         `slokit generate -i examples/infraportal/slos/ -o examples/infraportal/rules.yaml`"
    );
}
