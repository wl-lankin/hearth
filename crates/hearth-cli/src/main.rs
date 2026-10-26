use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use hearth_core::php::manifest::Os;
use hearth_core::php::store::{self, install_dir};
use hearth_core::sites::registry;
use hearth_core::{ipc, CoreError, HearthPaths};
use serde_json::{json, Value};
use std::path::PathBuf;

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
    /// Serves every subfolder of a folder (default: the current one) as <name>.test.
    Park { dir: Option<PathBuf> },
    /// Stops serving the subfolders of a parked folder.
    Unpark { dir: Option<PathBuf> },
    /// Serves a folder (the current one) as <name>.test; the name defaults to the folder name.
    Link { name: Option<String> },
    /// Stops serving a linked site.
    Unlink { name: Option<String> },
    /// Lists the sites.
    #[command(alias = "sites")]
    Links {
        #[arg(long)]
        json: bool,
    },
    /// Forwards <name>.test to a local server, e.g. `hearth proxy app http://localhost:5173`.
    Proxy { name: String, upstream: String },
    /// Removes a proxy.
    Unproxy { name: String },
    /// Runs a site (default: the current folder's) on a PHP version.
    Isolate {
        minor: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Runs a site on the default PHP version again.
    Unisolate {
        #[arg(long)]
        site: Option<String>,
    },
    /// Opens a site (default: the current folder's) in the browser.
    Open { name: Option<String> },
    /// Serves a site (default: the current folder's) over HTTPS with Hearth's certificate.
    Secure { name: Option<String> },
    /// Serves a site (default: the current folder's) over plain HTTP again.
    Unsecure { name: Option<String> },
    /// Lists the sites served over HTTPS.
    Secured {
        #[arg(long)]
        json: bool,
    },
    /// Makes this computer trust Hearth's certificates (once; the system asks to confirm).
    Trust,
    /// Makes <name>.test resolve on this computer (once; the system asks for admin rights).
    #[command(name = "dns:setup")]
    DnsSetup,
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
        Command::Park { dir } => {
            let dir = folder(dir);
            let done = format!("Every folder in {} is now a site", dir.display());
            print_call(&paths, "sites.park", json!({ "dir": dir }), false, |_| println!("{done}")).await
        }
        Command::Unpark { dir } => {
            let dir = folder(dir);
            let done = format!("{} is no longer parked", dir.display());
            print_call(&paths, "sites.unpark", json!({ "dir": dir }), false, |_| println!("{done}")).await
        }
        Command::Link { name } => match site_name(name) {
            Ok(name) => {
                let dir = folder(None);
                match ipc::call(&paths, "sites.link", json!({ "name": name, "dir": dir })).await {
                    Ok(_) => {
                        let port = http_port(&paths).await;
                        println!("{name} is now served at http://{name}.test:{port}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => fail(&message(e)),
                }
            }
            Err(code) => code,
        },
        Command::Unlink { name } => match site_name(name) {
            Ok(name) => {
                print_call(&paths, "sites.unlink", json!({ "name": name }), false, |_| println!("{name} is unlinked"))
                    .await
            }
            Err(code) => code,
        },
        Command::Links { json } => print_call(&paths, "sites.list", Value::Null, json, print_sites).await,
        Command::Proxy { name, upstream } => {
            let done = format!("{name}.test now forwards to {upstream}");
            print_call(&paths, "sites.proxy", json!({ "name": name, "upstream": upstream }), false, |_| {
                println!("{done}")
            })
            .await
        }
        Command::Unproxy { name } => {
            print_call(&paths, "sites.unproxy", json!({ "name": name }), false, |_| println!("{name}.test is removed"))
                .await
        }
        Command::Isolate { minor, site } => match site_name(site) {
            Ok(site) => {
                let params = json!({ "name": site, "minor": minor });
                print_call(&paths, "sites.isolate", params, false, |_| println!("{site} now runs on PHP {minor}")).await
            }
            Err(code) => code,
        },
        Command::Unisolate { site } => match site_name(site) {
            Ok(site) => {
                let params = json!({ "name": site, "minor": null });
                print_call(&paths, "sites.isolate", params, false, |_| println!("{site} now runs on the default PHP"))
                    .await
            }
            Err(code) => code,
        },
        Command::Open { name } => match site_name(name) {
            Ok(name) => {
                let url = site_url(&sites(&paths).await, &name);
                open_url(&url)
            }
            Err(code) => code,
        },
        Command::Secure { name } => match site_name(name) {
            Ok(name) => match ipc::call(&paths, "sites.secure", json!({ "name": name })).await {
                Ok(_) => {
                    println!("{name} is now served at {}", site_url(&sites(&paths).await, &name));
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&message(e)),
            },
            Err(code) => code,
        },
        Command::Unsecure { name } => match site_name(name) {
            Ok(name) => match ipc::call(&paths, "sites.unsecure", json!({ "name": name })).await {
                Ok(_) => {
                    println!("{name} is now served at {}", site_url(&sites(&paths).await, &name));
                    ExitCode::SUCCESS
                }
                Err(e) => fail(&message(e)),
            },
            Err(code) => code,
        },
        Command::Secured { json } => match ipc::call(&paths, "sites.list", Value::Null).await {
            Ok(list) => {
                let names: Vec<&str> = list["sites"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["secured"] == true)
                    .filter_map(|s| s["name"].as_str())
                    .collect();
                if json {
                    println!("{}", Value::from(names));
                } else {
                    for name in names {
                        println!("{}", site_url(&list, name));
                    }
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail(&message(e)),
        },
        Command::Trust => {
            print_call(&paths, "tls.trust", Value::Null, false, |_| {
                println!("This computer now trusts Hearth's certificates")
            })
            .await
        }
        Command::DnsSetup => {
            print_call(&paths, "dns.setup", Value::Null, false, |_| {
                println!("<name>.test now resolves on this computer")
            })
            .await
        }
    }
}

/// The sites list, or an empty one when the core does not answer.
async fn sites(paths: &HearthPaths) -> Value {
    ipc::call(paths, "sites.list", Value::Null).await.unwrap_or(Value::Null)
}

/// A site's address: HTTPS when it is secured.
fn site_url(list: &Value, name: &str) -> String {
    let secured = list["sites"].as_array().into_iter().flatten().any(|s| s["name"] == name && s["secured"] == true);
    if secured {
        format!("https://{name}.test:{}", list["httpsPort"].as_u64().unwrap_or(8443))
    } else {
        format!("http://{name}.test:{}", list["httpPort"].as_u64().unwrap_or(8080))
    }
}

/// The given folder, or the current one, as an absolute path.
fn folder(dir: Option<PathBuf>) -> PathBuf {
    let dir = dir.unwrap_or_else(|| PathBuf::from("."));
    std::path::absolute(&dir).unwrap_or(dir)
}

/// The given site name, or the one the current folder would get.
fn site_name(name: Option<String>) -> Result<String, ExitCode> {
    if let Some(name) = name {
        return Ok(name);
    }
    let cwd = folder(None);
    let base = cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    registry::name_from_folder(&base)
        .ok_or_else(|| fail("This folder's name cannot be a site name; give one, e.g. `hearth link shop`"))
}

async fn http_port(paths: &HearthPaths) -> u64 {
    ipc::call(paths, "sites.list", Value::Null).await.ok().and_then(|v| v["httpPort"].as_u64()).unwrap_or(8080)
}

fn print_sites(value: &Value) {
    for site in value["sites"].as_array().into_iter().flatten() {
        let target = site["path"].as_str().or(site["upstream"].as_str()).unwrap_or_default();
        let php = site["php"].as_str().map(|m| format!("  PHP {m}")).unwrap_or_default();
        let problem = site["problem"].as_str().map(|p| format!("  ({p})")).unwrap_or_default();
        println!(
            "{}.test  {}  {target}{php}{problem}",
            site["name"].as_str().unwrap_or_default(),
            site["kind"].as_str().unwrap_or_default()
        );
    }
}

fn open_url(url: &str) -> ExitCode {
    #[cfg(windows)]
    let status = std::process::Command::new("cmd").args(["/C", "start", "", url]).status();
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();
    match status {
        Ok(s) if s.success() => {
            println!("Opened {url}");
            ExitCode::SUCCESS
        }
        _ => fail(&format!("Could not open {url}")),
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

/// The exit code to hand on: 0 to 255 pass through; a crash (negative on Windows, e.g. 0xC0000005),
/// an out-of-range code or a signal (no code) is a failure, never a success.
fn exit_code(code: Option<i32>) -> u8 {
    match code {
        Some(c @ 0..=255) => c as u8,
        _ => 1,
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
    // PHPRC lets PHP processes this one starts find the same php.ini (see php/shims.rs).
    command.arg("-c").arg(&ini).env("PHPRC", ini.parent().unwrap_or(&ini));
    if let Some(script) = script {
        command.arg(script);
    }
    match command.args(args).status() {
        Ok(status) => ExitCode::from(exit_code(status.code())),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_pass_through_and_crashes_are_failures() {
        assert_eq!(exit_code(Some(0)), 0);
        assert_eq!(exit_code(Some(255)), 255);
        // 0xC0000005 (access violation) as Windows reports it.
        assert_eq!(exit_code(Some(-1073741819)), 1);
        assert_eq!(exit_code(Some(256)), 1);
        // Killed by a signal on Unix.
        assert_eq!(exit_code(None), 1);
    }
}
