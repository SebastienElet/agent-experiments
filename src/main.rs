mod cli_output;
mod eval_cli;
use clap::Parser;
#[derive(Parser)]
#[command(version, about = "Bounded agent experiments")]
struct Cli {
    #[command(flatten)]
    args: eval_cli::EvalArgs,
}
fn main() -> std::process::ExitCode {
    eval_cli::run(Cli::parse().args)
}
