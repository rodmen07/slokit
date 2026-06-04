//! `slokit` command-line interface.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use slokit::check::{check_spec, PrometheusClient, SloStatus, StatusLevel};
use slokit::generate::{generate_rules_with, GenerateOptions};
use slokit::spec::Spec;
use slokit::{BurnRate, MwmbrConfig, Objective, Slo, Window};

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
    /// Compute error budget and burn-rate thresholds from the command line.
    Calc(CalcArgs),
    /// Query a live Prometheus and report current budget and burn rate.
    Check(CheckArgs),
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// Plain Prometheus `rules.yaml`.
    Prometheus,
    /// Prometheus Operator `PrometheusRule` custom resource.
    Operator,
}

#[derive(Args)]
struct GenerateArgs {
    /// Input spec file (YAML).
    #[arg(short, long)]
    input: PathBuf,
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
}

#[derive(Args)]
struct ValidateArgs {
    /// Input spec file (YAML).
    #[arg(short, long)]
    input: PathBuf,
}

#[derive(Args)]
struct CheckArgs {
    /// Input spec file (YAML).
    #[arg(short, long)]
    input: PathBuf,
    /// Prometheus base URL, e.g. http://localhost:9090.
    #[arg(short, long)]
    url: String,
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
        Command::Calc(args) => run_calc(args),
        Command::Check(args) => run_check(args),
    }
}

fn run_generate(args: GenerateArgs) -> Result<()> {
    let spec = Spec::from_path(&args.input)
        .with_context(|| format!("loading spec from {}", args.input.display()))?;

    let opts = GenerateOptions {
        default_period: Window::parse(&args.period)?,
        mwmbr: MwmbrConfig::sre_default(),
    };
    let ruleset = generate_rules_with(&spec, &opts)?;

    let rendered = match args.format {
        Format::Prometheus => ruleset.to_prometheus_yaml()?,
        Format::Operator => {
            let name = args.name.as_deref().unwrap_or(&spec.service);
            ruleset.to_operator_yaml(name, &spec.labels)?
        }
    };

    match args.output {
        Some(path) => {
            std::fs::write(&path, rendered)
                .with_context(|| format!("writing rules to {}", path.display()))?;
            eprintln!("wrote rules to {}", path.display());
        }
        None => {
            std::io::stdout().write_all(rendered.as_bytes())?;
        }
    }
    Ok(())
}

fn run_validate(args: ValidateArgs) -> Result<()> {
    let spec = Spec::from_path(&args.input)
        .with_context(|| format!("loading spec from {}", args.input.display()))?;
    spec.validate()?;
    let slo_count = spec.slos.len();
    println!(
        "ok: '{}' is valid ({slo_count} SLO{})",
        args.input.display(),
        if slo_count == 1 { "" } else { "s" }
    );
    Ok(())
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
    for w in MwmbrConfig::sre_default().windows {
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
    let spec = Spec::from_path(&args.input)
        .with_context(|| format!("loading spec from {}", args.input.display()))?;
    let default_period = Window::parse(&args.period)?;
    let current_window = Window::parse(&args.window)?;

    let mut client = PrometheusClient::with_timeout(&args.url, Duration::from_secs(args.timeout))?;
    if let Some(token) = args.bearer_token {
        client = client.with_bearer_token(token);
    }

    let statuses = check_spec(&client, &spec, default_period, current_window)?;

    println!(
        "service '{}' against {} (current window {})\n",
        spec.service, args.url, current_window
    );
    println!(
        "{:<7} {:<32} {:>9} {:>10} {:>9}",
        "STATUS", "SLO", "CONSUMED", "REMAINING", "BURN"
    );
    let mut breaching = false;
    for s in &statuses {
        breaching |= s.level == StatusLevel::Breaching;
        print_status_row(s);
    }

    if breaching {
        std::process::exit(1);
    }
    Ok(())
}

fn print_status_row(s: &SloStatus) {
    println!(
        "{:<7} {:<32} {:>9} {:>10} {:>9}",
        s.level.label(),
        s.name,
        opt_pct(s.budget_consumed_ratio),
        opt_pct(s.budget_remaining_ratio),
        opt_burn(s.current_burn_rate),
    );
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
