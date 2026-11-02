use agent_experiments::eval::{compare, evidence, live::Codex, repository, runner, shim, validate};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Args)]
pub struct EvalArgs {
    #[arg(long, global = true, default_value = ".")]
    repository: PathBuf,
    #[command(subcommand)]
    command: EvalCommand,
}

#[derive(Subcommand)]
enum EvalCommand {
    ValidateEvals,
    ValidateEvidence {
        reports: Vec<PathBuf>,
    },
    FixtureSmoke,
    Preflight,
    Run(RunArgs),
    RepositoryRun(RepositoryArgs),
    RepositoryCompare {
        baseline: PathBuf,
        candidate: PathBuf,
    },
    Compare {
        baseline: PathBuf,
        candidate: PathBuf,
    },
    #[command(hide = true)]
    Shim {
        tool: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Args)]
struct RepositoryArgs {
    #[arg(long)]
    case: PathBuf,
    #[arg(long)]
    instructions: PathBuf,
    #[arg(long)]
    report: PathBuf,
    #[arg(long, value_parser = model_id)]
    model: String,
    #[arg(long, default_value = "low", value_parser = ["low", "medium", "high"])]
    reasoning_effort: String,
    #[arg(long, default_value = "600", value_parser = clap::value_parser!(u16).range(1..=600))]
    timeout_seconds: u16,
    #[arg(long)]
    tool_path: String,
    #[arg(long)]
    network: bool,
}

#[derive(Args)]
struct RunArgs {
    #[arg(long, value_parser = model_id)]
    model: String,
    #[arg(long, value_delimiter = ',', required = true)]
    only: Vec<String>,
    #[arg(long, default_value = "1", value_parser = clap::value_parser!(u8).range(1..=10))]
    runs: u8,
    #[arg(long)]
    report: PathBuf,
    #[arg(long, default_value = "120", value_parser = clap::value_parser!(u16).range(1..=600))]
    timeout_seconds: u16,
    #[arg(long, default_value = "low", value_parser = ["low", "medium", "high"])]
    reasoning_effort: String,
    #[arg(long, value_parser = variant_path)]
    variant_file: Option<String>,
}

fn model_id(value: &str) -> Result<String, String> {
    if value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        Ok(value.to_owned())
    } else {
        Err("Model must be an explicit model identifier".to_owned())
    }
}

fn variant_path(value: &str) -> Result<String, String> {
    let valid = value
        .strip_prefix("variants/")
        .and_then(|name| name.strip_suffix(".md"))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if valid {
        Ok(value.to_owned())
    } else {
        Err("Variant must be a synthetic Markdown file under variants/".to_owned())
    }
}

pub fn run(args: EvalArgs) -> ExitCode {
    match dispatch(args) {
        Ok(code) => code,
        Err(error) => {
            let _ = crate::cli_output::write_error(format_args!("eval: {error}"));
            ExitCode::from(1)
        }
    }
}

fn dispatch(args: EvalArgs) -> Result<ExitCode, String> {
    match args.command {
        EvalCommand::ValidateEvals => {
            crate::cli_output::write_output(&validate::validate_evaluations(&args.repository)?)
                .map_err(|error| error.to_string())?;
        }
        EvalCommand::ValidateEvidence { reports } => {
            if reports.is_empty() {
                crate::cli_output::write_output(&validate::validate_evidence(&args.repository)?)
                    .map_err(|error| error.to_string())?;
            } else {
                for report in &reports {
                    evidence::read_report(report)?;
                }
                crate::cli_output::write_output(&format!(
                    "{} selected reports valid",
                    reports.len()
                ))
                .map_err(|error| error.to_string())?;
            }
        }
        EvalCommand::Preflight => {
            let codex = Codex::discover()?;
            crate::cli_output::write_output(&format!(
                "Codex isolated preflight passed: {}",
                codex.version
            ))
            .map_err(|error| error.to_string())?;
        }
        EvalCommand::FixtureSmoke => print_json(&runner::run_smoke(&args.repository)?)?,
        EvalCommand::Run(options) => return run_and_publish(&args.repository, options),
        EvalCommand::RepositoryRun(options) => {
            let status = repository::run(&repository::Options {
                case: options.case,
                instructions: options.instructions,
                report: options.report,
                model: options.model,
                effort: options.reasoning_effort,
                timeout_seconds: u64::from(options.timeout_seconds),
                tool_path: options.tool_path,
                network: options.network,
            })?;
            print_json(&status)?;
            return Ok(if status == agent_experiments::eval::report::Status::Pass {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            });
        }
        EvalCommand::RepositoryCompare {
            baseline,
            candidate,
        } => print_json(&repository::compare(&baseline, &candidate)?)?,
        EvalCommand::Compare {
            baseline,
            candidate,
        } => {
            print_json(&compare::compare(
                &evidence::read_report(&baseline)?,
                &evidence::read_report(&candidate)?,
            )?)?;
        }
        EvalCommand::Shim { tool, args } => {
            let code = shim::invoke_shim(&tool, &args)?;
            return Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_and_publish(repository: &Path, args: RunArgs) -> Result<ExitCode, String> {
    evidence::assert_new_report(&args.report)?;
    let options = runner::SeriesOptions {
        model: args.model,
        only: args.only,
        runs: u64::from(args.runs),
        timeout_seconds: u64::from(args.timeout_seconds),
        reasoning_effort: args.reasoning_effort,
        variant: args.variant_file,
    };
    let report = runner::run_live(repository, &options)?;
    evidence::publish_report(&args.report, &report)?;
    crate::cli_output::write_output(&format!("New report: {}", args.report.display()))
        .map_err(|error| error.to_string())?;
    let passed = report.cases.iter().all(|case| {
        case.runs
            .iter()
            .all(|run| run.status == agent_experiments::eval::report::Status::Pass)
    });
    Ok(if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    crate::cli_output::write_output(
        &serde_json::to_string_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}
