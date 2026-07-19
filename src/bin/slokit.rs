//! `slokit` command-line interface.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use slokit::check::{check_spec, PrometheusClient, SloStatus, StatusLevel};
use slokit::dashboard::dashboards_json;
use slokit::generate::{generate_all, generate_rules_with, GenerateOptions};
use slokit::spec::{openslo, validate_all, Lint, LintLevel, Spec, SCHEMA_JSON};
use slokit::{BurnRate, MwmbrConfig, Objective, Slo, Window};

/// Every spec file under `input`: the file itself, or each `*.yaml`/`*.yml`
/// in the directory, sorted by path.
fn spec_files(input: &Path) -> Result<Vec<PathBuf>> {
    if !input.is_dir() {
        return Ok(vec![input.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(input)
        .with_context(|| format!("reading dir {}", input.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|x| x.to_str()),
                    Some("yaml") | Some("yml")
                )
        })
        .collect();
    files.sort();
    if files.is_empty() {
        anyhow::bail!("no .yaml/.yml spec files found in {}", input.display());
    }
    Ok(files)
}

/// Load one spec (file) or many (directory of `*.yaml`/`*.yml`), honoring the
/// input format flag. Without an explicit format, each file is auto-detected:
/// a first document with a top-level `apiVersion: openslo/...` is imported as
/// OpenSLO, anything else parses as a native slokit spec. OpenSLO import
/// notes are printed to stderr.
fn load_specs(input: &InputArgs) -> Result<Vec<Spec>> {
    let mut specs = Vec::new();
    for file in spec_files(&input.input)? {
        let contents = std::fs::read_to_string(&file)
            .with_context(|| format!("reading {}", file.display()))?;
        let use_openslo = match input.input_format {
            Some(InputFormat::Openslo) => true,
            Some(InputFormat::Slokit) => false,
            None => openslo::is_openslo(&contents),
        };
        if use_openslo {
            let import = openslo::from_yaml(&contents)
                .with_context(|| format!("importing OpenSLO specs from {}", file.display()))?;
            for note in &import.notes {
                eprintln!("note: {}: {note}", file.display());
            }
            specs.extend(import.specs);
        } else {
            specs.push(
                Spec::from_yaml(&contents)
                    .with_context(|| format!("loading spec from {}", file.display()))?,
            );
        }
    }
    Ok(specs)
}

#[derive(Parser)]
#[command(
    name = "slokit",
    version,
    about = "SLO and error-budget engine: generate Prometheus burn-rate alerts and do error-budget math"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate Prometheus rules from a sloth-compatible SLO spec.
    Generate(GenerateArgs),
    /// Validate an SLO spec without generating rules.
    Validate(ValidateArgs),
    /// Report advisory lint findings for an SLO spec (legal but questionable config).
    Lint(LintArgs),
    /// Compute error budget and burn-rate thresholds from the command line.
    Calc(CalcArgs),
    /// Query a live Prometheus and report current budget and burn rate.
    Check(CheckArgs),
    /// Generate a Grafana dashboard (JSON) from a spec.
    Dashboard(DashboardArgs),
    /// Print the JSON Schema for the spec format (editor/tooling integration).
    Schema(SchemaArgs),
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// Plain Prometheus `rules.yaml`.
    Prometheus,
    /// Prometheus Operator `PrometheusRule` custom resource.
    Operator,
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    /// Human-readable status table.
    Table,
    /// Machine-readable JSON array of statuses.
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum InputFormat {
    /// Native slokit spec (sloth-compatible `prometheus/v1` YAML).
    Slokit,
    /// OpenSLO v1 `kind: SLO` documents (single or multi-document YAML).
    Openslo,
}

/// Input options shared by every command that reads specs.
#[derive(Args)]
struct InputArgs {
    /// Input spec file or directory of specs (YAML).
    #[arg(short, long)]
    input: PathBuf,
    /// Input spec format. Defaults to slokit, except that detection is
    /// unambiguous when a file's first YAML document sets a top-level
    /// `apiVersion: openslo/...`: that file is then imported as OpenSLO.
    /// Pass the flag to override auto-detection either way.
    #[arg(long, value_enum)]
    input_format: Option<InputFormat>,
}

#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum FailOn {
    /// Exit non-zero only when an SLO is breaching (default).
    Breach,
    /// Exit non-zero when any SLO is warning or breaching.
    Warning,
    /// Never exit non-zero from status (only on errors).
    Never,
}

#[derive(Args)]
struct GenerateArgs {
    #[command(flatten)]
    input: InputArgs,
    /// Output file. Defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Prometheus)]
    format: Format,
    /// Default SLO period for SLOs that do not set their own.
    #[arg(long, default_value = "30d")]
    period: String,
    /// metadata.name for the operator format. Defaults to the spec's service.
    #[arg(long)]
    name: Option<String>,
    /// Use the 30d-calibrated burn-rate windows verbatim instead of scaling
    /// them to each SLO's period.
    #[arg(long)]
    no_period_scaling: bool,
}

#[derive(Args)]
struct ValidateArgs {
    #[command(flatten)]
    input: InputArgs,
}

#[derive(Args)]
struct LintArgs {
    #[command(flatten)]
    input: InputArgs,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    output: OutputFormat,
    /// Exit non-zero when any warning-level finding is present.
    #[arg(long)]
    strict: bool,
}

#[derive(Args)]
struct DashboardArgs {
    #[command(flatten)]
    input: InputArgs,
    /// Output file. Defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct SchemaArgs {
    /// Output file. Defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
struct CheckArgs {
    #[command(flatten)]
    input: InputArgs,
    /// Prometheus base URL, e.g. http://localhost:9090.
    #[arg(short, long)]
    url: String,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    output: OutputFormat,
    /// Exit non-zero when a status reaches this level.
    #[arg(long, value_enum, default_value_t = FailOn::Breach)]
    fail_on: FailOn,
    /// Short window for the "current" burn rate.
    #[arg(long, default_value = "1h")]
    window: String,
    /// Default SLO period for SLOs that do not set their own.
    #[arg(long, default_value = "30d")]
    period: String,
    /// Bearer token sent with each request.
    #[arg(long)]
    bearer_token: Option<String>,
    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

#[derive(Args)]
struct CalcArgs {
    /// Objective as a percentage, e.g. 99.9.
    #[arg(long)]
    objective: f64,
    /// SLO period, e.g. 30d.
    #[arg(long, default_value = "30d")]
    period: String,
    /// Total events over the period (enables budget event counts).
    #[arg(long)]
    total: Option<f64>,
    /// Observed bad events so far (enables consumption and exhaustion).
    #[arg(long)]
    bad: Option<f64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate(args) => run_generate(args),
        Command::Validate(args) => run_validate(args),
        Command::Lint(args) => run_lint(args),
        Command::Calc(args) => run_calc(args),
        Command::Check(args) => run_check(args),
        Command::Dashboard(args) => run_dashboard(args),
        Command::Schema(args) => run_schema(args),
    }
}

fn write_output(rendered: String, output: Option<PathBuf>, what: &str) -> Result<()> {
    match output {
        Some(path) => {
            std::fs::write(&path, rendered)
                .with_context(|| format!("writing {what} to {}", path.display()))?;
            eprintln!("wrote {what} to {}", path.display());
        }
        None => std::io::stdout().write_all(rendered.as_bytes())?,
    }
    Ok(())
}

fn run_dashboard(args: DashboardArgs) -> Result<()> {
    let specs = load_specs(&args.input)?;
    validate_all(&specs)?;
    write_output(dashboards_json(&specs)?, args.output, "dashboard")
}

fn run_schema(args: SchemaArgs) -> Result<()> {
    // Verbatim, so the output is byte-identical to the in-repo schema file.
    write_output(SCHEMA_JSON.to_string(), args.output, "schema")
}

fn run_generate(args: GenerateArgs) -> Result<()> {
    let specs = load_specs(&args.input)?;
    // The CLI always resolves `sli.plugin` against the default (built-in)
    // registry.
    let mut opts = GenerateOptions::default();
    opts.default_period = Window::parse(&args.period)?;
    opts.mwmbr = MwmbrConfig::sre_default();
    opts.period_aware = !args.no_period_scaling;

    let rendered = match args.format {
        // All specs merge into one rules document.
        Format::Prometheus => generate_all(&specs, &opts)?.to_prometheus_yaml()?,
        // One PrometheusRule resource per spec, joined as a multi-document YAML.
        Format::Operator => {
            let mut docs = Vec::with_capacity(specs.len());
            for spec in &specs {
                let name = args.name.clone().unwrap_or_else(|| spec.service.clone());
                docs.push(generate_rules_with(spec, &opts)?.to_operator_yaml(&name, &spec.labels)?);
            }
            docs.join("---\n")
        }
    };

    write_output(rendered, args.output, "rules")
}

fn run_validate(args: ValidateArgs) -> Result<()> {
    let specs = load_specs(&args.input)?;
    // Per-spec validation plus cross-spec checks (duplicate service/SLO pairs
    // would collide when the specs' rules are merged into one file).
    validate_all(&specs)?;
    for spec in &specs {
        println!(
            "ok: '{}' is valid ({} SLO{})",
            spec.service,
            spec.slos.len(),
            if spec.slos.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

fn run_lint(args: LintArgs) -> Result<()> {
    let specs = load_specs(&args.input)?;

    // Surface structural errors first; advisory findings only make sense for
    // specs that are otherwise valid (individually and merged).
    validate_all(&specs)?;

    let findings: Vec<(String, Lint)> = specs
        .iter()
        .flat_map(|spec| spec.lint().into_iter().map(|l| (spec.service.clone(), l)))
        .collect();

    match args.output {
        OutputFormat::Json => {
            let arr: Vec<_> = findings
                .iter()
                .map(|(service, l)| {
                    serde_json::json!({
                        "service": service,
                        "level": l.level.label(),
                        "code": l.code,
                        "location": l.location,
                        "message": l.message,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
        OutputFormat::Table => print_lint_table(&findings),
    }

    let has_warning = findings.iter().any(|(_, l)| l.level == LintLevel::Warning);
    if args.strict && has_warning {
        std::process::exit(1);
    }
    Ok(())
}

fn print_lint_table(findings: &[(String, Lint)]) {
    if findings.is_empty() {
        println!("no lint findings");
        return;
    }
    println!(
        "{:<5} {:<14} {:<28} {:<20} MESSAGE",
        "LEVEL", "SERVICE", "LOCATION", "CODE"
    );
    for (service, l) in findings {
        println!(
            "{:<5} {:<14} {:<28} {:<20} {}",
            l.level.label(),
            service,
            l.location,
            l.code,
            l.message
        );
    }
}

fn run_calc(args: CalcArgs) -> Result<()> {
    let objective = Objective::percent(args.objective)?;
    let period = Window::parse(&args.period)?;
    let slo = Slo::new(objective, period);

    println!("Objective:    {}% over {}", objective.as_percent(), period);
    println!(
        "Error budget: {} of events",
        ratio_str(slo.error_budget_ratio())
    );

    if let Some(total) = args.total {
        let budget = slo.error_budget(total);
        println!("Total events: {total}");
        println!("Allowed bad:  {:.2}", budget.allowed_bad_events());

        if let Some(bad) = args.bad {
            let observed_ratio = if total > 0.0 { bad / total } else { 0.0 };
            let burn = BurnRate::from_error_ratio(observed_ratio, &slo);
            println!("Observed bad: {bad}");
            println!("Burn rate:    {:.2}x", burn.value());
            println!("Consumed:     {}", ratio_str(budget.consumed_ratio(bad)));
            println!("Remaining:    {}", ratio_str(budget.remaining_ratio(bad)));
            match budget.time_to_exhaustion(bad, burn, period) {
                Some(d) => println!("Exhausted in: {}", humanize(d)),
                None => println!("Exhausted in: never (no budget burn)"),
            }
        }
    }

    println!("\nBurn-rate alert thresholds (error ratio that fires each window):");
    let budget_ratio = slo.error_budget_ratio();
    for w in MwmbrConfig::sre_default_for_period(period).windows {
        println!(
            "  {:<6} long={:<4} short={:<4} factor={:<5} threshold={}",
            w.severity.label(),
            w.long.to_string(),
            w.short.to_string(),
            w.factor,
            ratio_str(w.factor * budget_ratio),
        );
    }
    Ok(())
}

fn run_check(args: CheckArgs) -> Result<()> {
    // Distinguish exit codes: runtime errors exit 2, a fail-on hit exits 1.
    let result = (|| -> Result<bool> {
        let specs = load_specs(&args.input)?;
        let default_period = Window::parse(&args.period)?;
        let current_window = Window::parse(&args.window)?;

        let mut client =
            PrometheusClient::with_timeout(&args.url, Duration::from_secs(args.timeout))?;
        if let Some(token) = &args.bearer_token {
            client = client.with_bearer_token(token.clone());
        }

        let mut statuses = Vec::new();
        for spec in &specs {
            statuses.extend(check_spec(&client, spec, default_period, current_window)?);
        }

        match args.output {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&statuses)?),
            OutputFormat::Table => print_status_table(&statuses, &args.url, current_window),
        }

        Ok(fail_threshold_hit(&statuses, args.fail_on))
    })();

    match result {
        Ok(true) => std::process::exit(1),
        Ok(false) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(2);
        }
    }
}

fn fail_threshold_hit(statuses: &[SloStatus], fail_on: FailOn) -> bool {
    statuses.iter().any(|s| match fail_on {
        FailOn::Never => false,
        FailOn::Warning => matches!(s.level, StatusLevel::Warning | StatusLevel::Breaching),
        FailOn::Breach => s.level == StatusLevel::Breaching,
    })
}

fn print_status_table(statuses: &[SloStatus], url: &str, current_window: Window) {
    println!("checked against {url} (current window {current_window})\n");
    println!(
        "{:<7} {:<14} {:<28} {:>9} {:>10} {:>9}",
        "STATUS", "SERVICE", "SLO", "CONSUMED", "REMAINING", "BURN"
    );
    for s in statuses {
        println!(
            "{:<7} {:<14} {:<28} {:>9} {:>10} {:>9}",
            s.level.label(),
            s.service,
            s.name,
            opt_pct(s.budget_consumed_ratio),
            opt_pct(s.budget_remaining_ratio),
            opt_burn(s.current_burn_rate),
        );
    }
}

fn opt_pct(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{:.2}%", x * 100.0),
        None => "n/a".to_string(),
    }
}

fn opt_burn(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{x:.2}x"),
        None => "n/a".to_string(),
    }
}

/// Render a ratio as a percentage with a sensible number of digits.
fn ratio_str(r: f64) -> String {
    if r.is_infinite() {
        return "inf".to_string();
    }
    format!("{:.4}%", r * 100.0)
}

/// Render a duration as an approximate, human-friendly string.
fn humanize(d: Duration) -> String {
    let secs = d.as_secs();
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}
