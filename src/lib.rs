//! # slokit
//!
//! An SLO and error-budget engine for Rust.
//!
//! `slokit` does two things:
//!
//! 1. **Library core** (always available): compute error budgets, burn rates,
//!    and the multi-window multi-burn-rate (MWMBR) alert model from the Google
//!    SRE Workbook. This core has no `serde`, YAML, or CLI dependencies, so it
//!    embeds cleanly inside services (for example, an Axum handler that reports
//!    live budget status).
//!
//! 2. **Generator** (the `spec` feature, on by default via `cli`): parse a
//!    [`sloth`](https://sloth.dev)-compatible YAML spec and generate Prometheus
//!    recording rules, metadata rules, and MWMBR page/ticket alert rules.
//!
//! ## Library example
//!
//! ```
//! use slokit::{Objective, Slo, BurnRate, Window};
//!
//! let slo = Slo::new(Objective::percent(99.9).unwrap(), Window::days(30));
//!
//! // With a million events, 0.1% may fail: ~1,000 allowed failures.
//! let budget = slo.error_budget(1_000_000.0);
//! assert!((budget.allowed_bad_events() - 1_000.0).abs() < 1e-6);
//!
//! // Observing a 1% error rate is a 10x burn against a 99.9% objective.
//! let burn = BurnRate::from_error_ratio(0.01, &slo);
//! assert!((burn.value() - 10.0).abs() < 1e-9);
//! ```
//!
//! ## Generating Prometheus rules
//!
//! With the default features enabled:
//!
//! ```ignore
//! use slokit::spec::Spec;
//! use slokit::generate::generate_rules;
//!
//! let spec = Spec::from_yaml(yaml_str)?;
//! let ruleset = generate_rules(&spec)?;
//! println!("{}", ruleset.to_yaml()?);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod error;

mod budget;
mod burn_rate;
mod sli;
mod slo;
mod window;

pub use budget::ErrorBudget;
pub use burn_rate::{AlertWindow, BurnRate, MwmbrConfig, Severity};
pub use error::{Result, SlokitError};
pub use sli::{Sli, WINDOW_TOKEN};
pub use slo::{Objective, Slo};
pub use window::Window;

#[cfg(feature = "spec")]
#[cfg_attr(docsrs, doc(cfg(feature = "spec")))]
pub mod spec;

#[cfg(feature = "spec")]
#[cfg_attr(docsrs, doc(cfg(feature = "spec")))]
pub mod generate;

#[cfg(feature = "check")]
#[cfg_attr(docsrs, doc(cfg(feature = "check")))]
pub mod check;

#[cfg(feature = "dashboard")]
#[cfg_attr(docsrs, doc(cfg(feature = "dashboard")))]
pub mod dashboard;
