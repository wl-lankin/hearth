use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use hearth_core::php::manifest::Os;
use hearth_core::php::store::{self, install_dir};
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
    /// Lists the installed PHP versions.
    #[command(name = "php:list")]
    PhpList {
        #[arg(long)]
        json: bool,
    },
    /// Lists the PHP versions available for this computer.
    #[command(name = "php:available")]
    PhpAvailable {
        #[arg(long)]
        json: bool,
    },
    /// Installs or updates a PHP version, e.g. `hearth php:install 8.4`.
    #[command(name = "php:install")]
    PhpInstall { minor: String },
    /// Removes a PHP version.
    #[command(name = "php:uninstall")]
    PhpUninstall { minor: String },
    /// Makes a PHP version the one behind `php`, `composer` and `laravel`.
    #[command(name = "php:use")]
    PhpUse { minor: String },
    /// Switches an extension of a PHP version on or off, e.g. `hearth php:ext 8.4 xdebug on`.
    #[command(name = "php:ext")]
    PhpExt {
        minor: String,
        name: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Runs the default PHP with Hearth's php.ini.
    #[command(trailing_var_arg = true, allow_hyphen_values = true, disable_help_flag = true)]
    Php { args: Vec<String> },
    /// Runs Composer with the default PHP.
    #[command(trailing_var_arg = true, allow_hyphen_values = true, disable_help_flag = true)]
    Composer { args: Vec<String> },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = match HearthPaths::from_env() {
        Ok(paths) => paths,
        Err(e) => return fail(&e.to_string()),
    };
    match cli.command {
        Command::Status { json } => status(&paths, json).await,
        Command::PhpList { json } => print_call(&paths, "php.list", Value::Null, json, print_list).await,
        Command::PhpAvailable { json } => print_call(&paths, "php.available", Value::Null, json, print_available).await,
        Command::PhpInstall { minor } => install(&paths, &minor).await,
        Command::PhpUninstall { minor } => {
            print_call(&paths, "php.uninstall", json!({ "minor": minor }), false, |_| {
                println!("PHP {minor} is removed.")
            })
            .await
        }
        Command::PhpUse { minor } => {
            print_call(&paths, "php.use", json!({ "minor": minor }), false, |_| println!("php now runs PHP {minor}."))
                .await
        }
        Command::PhpExt { minor, name, state } => {
            let params = json!({ "minor": minor, "name": name, "on": state == "on" });
            print_call(&paths, "php.extension", params, false, |_| println!("{name} is {state} for PHP {minor}.")).await
        }
        Command::Php { args } => run_php(&paths, None, &args),
        Command::Composer { args } => run_php(&paths, Some(paths.bin.join("composer.phar")), &args),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

fn message(e: CoreError) -> String {
    match e {
        CoreError::NotRunning => "Hearth core is not running. Start the Hearth app.".into(),
        CoreError::Rpc { message, .. } => message,
        other => other.to_string(),
    }
}

/// Calls `method`; prints the raw result with `--json`, else through `human`.
async fn print_call(
    paths: &HearthPaths,
    method: &str,
    params: Value,
    as_json: bool,
    human: impl FnOnce(&Value),
) -> ExitCode {
    match ipc::call(paths, method, params).await {
        Ok(value) if as_json => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Ok(value) => {
            human(&value);
            ExitCode::SUCCESS
        }
        Err(e) => fail(&message(e)),
    }
}

fn print_list(value: &Value) {
    let default = value["default"].as_str();
    for php in value["installed"].as_array().into_iter().flatten() {
        let minor = php["minor"].as_str().unwrap_or_default();
        let extensions: Vec<String> = php["extensions"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, on)| format!("{name} {}", if on.as_bool() == Some(true) { "on" } else { "off" }))
            .collect();
        let tag = if default == Some(minor) { "default" } else { "" };
        println!("PHP {}  {tag:<7}  {}", php["version"].as_str().unwrap_or_default(), extensions.join(", "));
    }
}

fn print_available(value: &Value) {
    for b in value.as_array().into_iter().flatten() {
        let installed = match (b["installedVersion"].as_str(), b["updateAvailable"].as_bool()) {
            (Some(v), Some(true)) => format!("installed {v}, update available"),
            (Some(v), _) => format!("installed {v}"),
            (None, _) => String::new(),
        };
        let preview = if b["channel"] == "rc" { " (preview)" } else { "" };
        println!(
            "{:<5} {:<12} {installed}{preview}",
            b["minor"].as_str().unwrap_or_default(),
            b["version"].as_str().unwrap_or_default()
        );
    }
}

async fn install(paths: &HearthPaths, minor: &str) -> ExitCode {
    let id = match ipc::call(paths, "php.install", json!({ "minor": minor })).await {
        Ok(v) => v["jobId"].as_u64().unwrap_or_default(),
        Err(e) => return fail(&message(e)),
    };
    let mut last = String::new();
    loop {
        let job = match ipc::call(paths, "jobs.get", json!({ "id": id })).await {
            Ok(job) => job,
            Err(e) => return fail(&message(e)),
        };
        match job["state"].as_str() {
            Some("done") => {
                println!("PHP {minor} is installed.");
                return ExitCode::SUCCESS;
            }
            Some("failed") => return fail(job["error"].as_str().unwrap_or("The install failed")),
            _ => {
                let line = format!(
                    "{} ({:.0} %)",
                    job["message"].as_str().unwrap_or_default(),
                    job["progress"].as_f64().unwrap_or(0.0) * 100.0
                );
                if line != last {
                    println!("{line}");
                    last = line;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Runs the default PHP directly (no core needed), passing the exit code through.
fn run_php(paths: &HearthPaths, script: Option<std::path::PathBuf>, args: &[String]) -> ExitCode {
    let Some(minor) = store::load(paths).default.filter(|m| install_dir(paths, m).join(store::VERSION_FILE).is_file())
    else {
        return fail("No PHP version is installed. Install one with: hearth php:install 8.4");
    };
    let php = match Os::current() {
        Os::Windows => install_dir(paths, &minor).join("php.exe"),
        Os::Macos => install_dir(paths, &minor).join("bin").join("php"),
    };
    let ini = paths.config.join("php").join(&minor).join("php.ini");
    let mut command = std::process::Command::new(php);
    command.arg("-c").arg(ini);
    if let Some(script) = script {
        command.arg(script);
    }
    match command.args(args).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => fail(&format!("Could not start PHP {minor}: {e}")),
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
        Err(e) => fail(&message(e)),
    }
}
