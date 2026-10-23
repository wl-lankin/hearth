use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hearth_core::{ipc, CoreError, HearthPaths};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "hearth", version, about = "Hearth: a local PHP development environment")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Shows whether the Hearth core is running.
    Status {
        /// Print the raw status as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = match HearthPaths::from_env() {
        Ok(paths) => paths,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match cli.command {
        Command::Status { json } => status(&paths, json).await,
    }
}

async fn status(paths: &HearthPaths, as_json: bool) -> ExitCode {
    match ipc::call(paths, "core.status", Value::Null).await {
        Ok(status) if as_json => {
            println!("{status}");
            ExitCode::SUCCESS
        }
        Ok(status) => {
            println!(
                "Hearth core is running (v{}, pid {}, up {}s)",
                status["version"].as_str().unwrap_or("?"),
                status["pid"],
                status["uptimeSecs"],
            );
            ExitCode::SUCCESS
        }
        Err(CoreError::NotRunning) if as_json => {
            println!("{}", json!({ "running": false }));
            ExitCode::FAILURE
        }
        Err(CoreError::NotRunning) => {
            eprintln!("Hearth core is not running. Start the Hearth app.");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
