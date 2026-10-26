use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use hearth_core::php::manifest::Os;
use hearth_core::php::store::{self, install_dir};
use hearth_core::sites::{locate, registry};
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
    #[command(name = "php:install", alias = "php:update")]
    PhpInstall { minor: String },
    /// Removes a PHP version.
    #[command(name = "php:uninstall")]
    PhpUninstall { minor: String },
    /// Makes a PHP version the one behind `php`, `composer` and `laravel`.
    #[command(name = "php:use", alias = "use")]
    PhpUse { minor: String },
    /// Switches an extension of a PHP version on or off, e.g. `hearth php:ext 8.4 xdebug on`.
    #[command(name = "php:ext")]
    PhpExt {
        minor: String,
        name: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// Runs the PHP of the project you are in (or `--site=<name>`'s) with Hearth's php.ini.
    #[command(trailing_var_arg = true, allow_hyphen_values = true, disable_help_flag = true)]
    Php { args: Vec<String> },
    /// Runs Composer with the project's PHP (or `--site=<name>`'s).
    #[command(trailing_var_arg = true, allow_hyphen_values = true, disable_help_flag = true)]
    Composer { args: Vec<String> },
    /// Runs the Laravel installer with the project's PHP.
    #[command(trailing_var_arg = true, allow_hyphen_values = true, disable_help_flag = true)]
    Laravel { args: Vec<String> },
    /// Opens `php artisan tinker` in a site (default: the current folder's).
    Tinker {
        #[arg(long)]
        site: Option<String>,
    },
    /// Serves every subfolder of a folder (default: the current one) as <name>.test.
    Park { dir: Option<PathBuf> },
    /// Stops serving the subfolders of a parked folder.
    #[command(alias = "forget")]
    Unpark { dir: Option<PathBuf> },
    /// Lists the parked folders.
    #[command(alias = "paths")]
    Parked {
        #[arg(long)]
        json: bool,
    },
    /// Serves a folder (the current one) as <name>.test; the name defaults to the folder name.
    Link {
        name: Option<String>,
        /// Serve it over HTTPS right away.
        #[arg(long)]
        secure: bool,
        /// Run it on this PHP version, e.g. 8.3.
        #[arg(long)]
        isolate: Option<String>,
    },
    /// Stops serving a linked site.
    Unlink { name: Option<String> },
    /// Lists the sites.
    #[command(alias = "sites")]
    Links {
        #[arg(long)]
        json: bool,
    },
    /// Forwards <name>.test to a local server, e.g. `hearth proxy app http://localhost:5173`.
    Proxy {
        name: String,
        upstream: String,
        #[arg(long)]
        secure: bool,
    },
    /// Removes a proxy.
    Unproxy { name: String },
    /// Lists the proxies.
    Proxies {
        #[arg(long)]
        json: bool,
    },
    /// Lists the sites that run on their own PHP version.
    Isolated {
        #[arg(long)]
        json: bool,
    },
    /// Shows the site of the current folder.
    Which {
        #[arg(long)]
        json: bool,
    },
    /// Prints the PHP program a site (default: the current folder's) runs.
    #[command(name = "which-php")]
    WhichPhp { site: Option<String> },
    /// Shows a site's details: the site of a folder (default: the current one).
    #[command(name = "site:info", alias = "site-information")]
    SiteInfo {
        path: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Restarts the web server and the PHP pools.
    Restart { service: Option<String> },
    /// Starts the web server and the PHP pools again after `stop`.
    Start { service: Option<String> },
    /// Stops the web server and the PHP pools until `start` (or the next app start).
    Stop { service: Option<String> },
    /// Prints the top-level domain of the sites (always `test`).
    Tld,
    /// Anything else, e.g. a Herd command for a feature Hearth does not have yet.
    #[command(external_subcommand)]
    Other(Vec<String>),
    /// Prints the address the sites are served on.
    Loopback,
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
    Unsecure {
        name: Option<String>,
        /// Every secured site.
        #[arg(long)]
        all: bool,
    },
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
    /// Shows what Herd's parked folders, links, proxies, PHP versions and HTTPS choices would add;
    /// imports them with --apply. Herd itself is never changed.
    #[command(name = "import:herd")]
    ImportHerd {
        /// Herd's Valet folder (default: Herd's usual place).
        #[arg(long)]
        from: Option<PathBuf>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = as_hearth(std::env::args_os().collect());
    // `php`, `composer` and `laravel` pass every argument to PHP as typed, so they skip the parser
    // (which would, for one, eat a leading `--`).
    if let Some(command) = args.get(1).and_then(|a| a.to_str()).filter(|c| matches!(*c, "php" | "composer" | "laravel"))
    {
        let rest: Vec<String> = args[2..].iter().map(|a| a.to_string_lossy().into_owned()).collect();
        return match HearthPaths::from_env() {
            Ok(paths) => run_named(&paths, command, &rest),
            Err(e) => fail(&e.to_string()),
        };
    }
    let cli = Cli::parse_from(args);
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
        // Handled before parsing (see above); listed in the enum for `hearth --help`.
        Command::Php { args } => run_named(&paths, "php", &args),
        Command::Composer { args } => run_named(&paths, "composer", &args),
        Command::Laravel { args } => run_named(&paths, "laravel", &args),
        Command::Tinker { site } => tinker(&paths, site),
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
        Command::Link { name, secure, isolate } => match site_name(name) {
            Ok(name) => {
                let dir = folder(None);
                let mut steps = vec![("sites.link", json!({ "name": name, "dir": dir }))];
                if let Some(minor) = &isolate {
                    steps.push(("sites.isolate", json!({ "name": name, "minor": minor })));
                }
                if secure {
                    steps.push(("sites.secure", json!({ "name": name })));
                }
                for (method, params) in steps {
                    if let Err(e) = ipc::call(&paths, method, params).await {
                        return fail(&message(e));
                    }
                }
                println!("{name} is now served at {}", site_url(&sites(&paths).await, &name));
                ExitCode::SUCCESS
            }
            Err(code) => code,
        },
        Command::Parked { json } => match ipc::call(&paths, "sites.list", Value::Null).await {
            Ok(list) if json => {
                println!("{}", list["parked"]);
                ExitCode::SUCCESS
            }
            Ok(list) => {
                for dir in list["parked"].as_array().into_iter().flatten() {
                    println!("{}", dir.as_str().unwrap_or_default());
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail(&message(e)),
        },
        Command::Proxies { json } => {
            list_sites(
                &paths,
                json,
                |s| s["kind"] == "proxy",
                |s| json!({ "name": s["name"], "upstream": s["upstream"], "secured": s["secured"] }),
            )
            .await
        }
        Command::Isolated { json } => {
            list_sites(&paths, json, |s| s["php"].is_string(), |s| json!({ "name": s["name"], "php": s["php"] })).await
        }
        Command::Which { json } => site_info(&paths, None, json).await,
        Command::SiteInfo { path, json } => site_info(&paths, path, json).await,
        Command::WhichPhp { site } => which_php(&paths, site),
        Command::Restart { .. } => {
            print_call(&paths, "web.restart", Value::Null, false, |_| println!("The web server and PHP restart")).await
        }
        Command::Start { .. } => {
            print_call(&paths, "web.start", Value::Null, false, |_| println!("The web server and PHP start")).await
        }
        Command::Stop { .. } => {
            print_call(&paths, "web.stop", Value::Null, false, |_| println!("The web server and PHP are stopped")).await
        }
        Command::Tld => {
            println!("test");
            ExitCode::SUCCESS
        }
        Command::Other(args) => fail(&missing(args.first().map(String::as_str).unwrap_or_default())),
        Command::Loopback => {
            println!("127.0.0.1");
            ExitCode::SUCCESS
        }
        Command::Unlink { name } => match site_name(name) {
            Ok(name) => {
                print_call(&paths, "sites.unlink", json!({ "name": name }), false, |_| println!("{name} is unlinked"))
                    .await
            }
            Err(code) => code,
        },
        Command::Links { json } => print_call(&paths, "sites.list", Value::Null, json, print_sites).await,
        Command::Proxy { name, upstream, secure } => {
            let name = domain(&name);
            if let Err(e) = ipc::call(&paths, "sites.proxy", json!({ "name": name, "upstream": upstream })).await {
                return fail(&message(e));
            }
            if secure {
                if let Err(e) = ipc::call(&paths, "sites.secure", json!({ "name": name })).await {
                    return fail(&message(e));
                }
            }
            println!("{} now forwards to {upstream}", site_url(&sites(&paths).await, &name));
            ExitCode::SUCCESS
        }
        Command::Unproxy { name } => {
            let name = domain(&name);
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
        Command::Unsecure { all: true, .. } => {
            let list = sites(&paths).await;
            let secured: Vec<String> = list["sites"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["secured"] == true)
                .filter_map(|s| s["name"].as_str().map(str::to_string))
                .collect();
            for name in &secured {
                if let Err(e) = ipc::call(&paths, "sites.unsecure", json!({ "name": name })).await {
                    return fail(&message(e));
                }
            }
            println!("{} sites are served over plain HTTP again", secured.len());
            ExitCode::SUCCESS
        }
        Command::Unsecure { name, .. } => match site_name(name) {
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
        Command::Trust => prompting(&paths, "tls.trust", "This computer now trusts Hearth's certificates").await,
        Command::DnsSetup => prompting(&paths, "dns.setup", "<name>.test now resolves on this computer").await,
        Command::ImportHerd { from, apply, json } => {
            let mut params = json!({ "apply": apply });
            if let Some(from) = from {
                params["from"] = json!(folder(Some(from)));
            }
            print_call(&paths, "herd.import", params, json, |v| print_import(v, apply)).await
        }
    }
}

fn print_import(value: &Value, applied: bool) {
    let plan = &value["plan"];
    let list = |key: &str| plan[key].as_array().cloned().unwrap_or_default();
    let pair =
        |v: &Value| (v[0].as_str().unwrap_or_default().to_string(), v[1].as_str().unwrap_or_default().to_string());
    println!("From {}", value["from"].as_str().unwrap_or_default());
    for dir in list("park") {
        println!("park   {}", dir.as_str().unwrap_or_default());
    }
    for (name, dir) in list("link").iter().map(pair) {
        println!("link   {name}.test -> {dir}");
    }
    for (name, upstream) in list("proxy").iter().map(pair) {
        println!("proxy  {name}.test -> {upstream}");
    }
    for (name, minor) in list("isolate").iter().map(pair) {
        println!("php    {name}.test -> PHP {minor}");
    }
    for name in list("secure") {
        println!("https  {}.test", name.as_str().unwrap_or_default());
    }
    for skip in list("skipped") {
        println!(
            "skip   {}: {}",
            skip["name"].as_str().unwrap_or_default(),
            skip["reason"].as_str().unwrap_or_default()
        );
    }
    for fail in value["failed"].as_array().into_iter().flatten() {
        println!(
            "failed {}: {}",
            fail["name"].as_str().unwrap_or_default(),
            fail["reason"].as_str().unwrap_or_default()
        );
    }
    let nothing = ["park", "link", "proxy", "isolate", "secure"].iter().all(|k| list(k).is_empty());
    if nothing {
        println!("Nothing new to import.");
    } else if applied {
        println!("Imported.");
    } else {
        println!("Run `hearth import:herd --apply` to import.");
    }
}

/// Calls a method that shows the user a system prompt, waiting until they have answered it.
async fn prompting(paths: &HearthPaths, method: &str, done: &str) -> ExitCode {
    println!("Confirm the prompt Windows or macOS shows now.");
    match ipc::call_waiting(paths, method, Value::Null, Duration::from_secs(600)).await {
        Ok(_) => {
            println!("{done}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&message(e)),
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

/// What to say for a command Hearth does not have: Herd's command groups name the feature that is
/// still to come, so an old script fails with a reason instead of doing nothing.
fn missing(command: &str) -> String {
    let group = command.split(':').next().unwrap_or_default();
    let feature = match group {
        "mail" => "mail catcher",
        "services" | "start-service" => "services (databases, Redis, ...)",
        "db" => "database tools",
        "debug" | "coverage" => "one-click Xdebug",
        "log" | "logs" | "insights" => "log and insights viewer",
        "processes" => "process manager",
        "workspace" | "workspaces" => "workspaces",
        "node" => "Node version manager",
        "mcp" => "MCP server",
        "share" | "fetch-share-url" => "site sharing",
        "studio" => return "Hearth has no Studio, and none is planned".into(),
        _ => return format!("Hearth has no `{command}` command (see `hearth --help`)"),
    };
    format!("Hearth has no {feature} yet")
}

/// A site name from a Herd-style domain argument: `Shop.test` and `shop` are both `shop`.
fn domain(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".test").map(str::to_string).unwrap_or(lower)
}

/// Lists the sites matching `keep`, each as `shape`; as JSON or one per line.
async fn list_sites(
    paths: &HearthPaths,
    as_json: bool,
    keep: impl Fn(&Value) -> bool,
    shape: impl Fn(&Value) -> Value,
) -> ExitCode {
    match ipc::call(paths, "sites.list", Value::Null).await {
        Ok(list) => {
            let rows: Vec<Value> =
                list["sites"].as_array().into_iter().flatten().filter(|s| keep(s)).map(&shape).collect();
            if as_json {
                println!("{}", Value::from(rows));
            } else {
                for row in rows {
                    let rest: Vec<String> = row
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(k, _)| *k != "name")
                        .map(|(_, v)| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                        .collect();
                    println!("{}.test  {}", row["name"].as_str().unwrap_or_default(), rest.join("  "));
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(&message(e)),
    }
}

/// The site of `path` (default: the current folder) with its address and PHP.
async fn site_info(paths: &HearthPaths, path: Option<PathBuf>, as_json: bool) -> ExitCode {
    let dir = folder(path);
    let Some(site) = locate::site_for_dir(paths, &dir) else {
        return fail(&format!("{} is not in a site", dir.display()));
    };
    // The version the site is set to, else the one it gets (the default).
    let php = site.php.clone().or_else(|| locate::php_for_site(paths, &site.name).ok().flatten());
    let url = site_url(&sites(paths).await, &site.name);
    let mut info = serde_json::to_value(&site).unwrap_or(Value::Null);
    info["url"] = json!(url);
    info["php"] = json!(php);
    if as_json {
        println!("{info}");
    } else {
        let folder = site.path.as_deref().map(|p| p.display().to_string()).unwrap_or_default();
        let php = php.map(|m| format!("PHP {m}")).unwrap_or_else(|| "no PHP installed".into());
        println!("{}.test  {url}  {folder}  {php}", site.name);
    }
    ExitCode::SUCCESS
}

/// The PHP program a site (or the current folder) runs.
fn which_php(paths: &HearthPaths, site: Option<String>) -> ExitCode {
    let minor = match site {
        Some(name) => match locate::php_for_site(paths, &domain(&name)) {
            Ok(minor) => minor,
            Err(e) => return fail(&e.to_string()),
        },
        None => locate::php_for_dir(paths, &folder(None)),
    };
    let Some(minor) = minor else {
        return fail("No PHP version is installed. Install one with: hearth php:install 8.4");
    };
    let program = match Os::current() {
        Os::Windows => install_dir(paths, &minor).join("php.exe"),
        Os::Macos => install_dir(paths, &minor).join("bin").join("php"),
    };
    println!("{}", program.display());
    ExitCode::SUCCESS
}

/// The given site name, or the one the current folder would get.
fn site_name(name: Option<String>) -> Result<String, ExitCode> {
    if let Some(name) = name {
        return Ok(domain(&name));
    }
    let cwd = folder(None);
    let base = cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    registry::name_from_folder(&base)
        .ok_or_else(|| fail("This folder's name cannot be a site name; give one, e.g. `hearth link shop`"))
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
/// The arguments as `hearth` would get them. The CLI is also installed as `php`, `composer`,
/// `laravel` (run that with the project's PHP) and `herd` (Herd's commands are Hearth's).
fn as_hearth(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let name = args
        .first()
        .and_then(|a| std::path::Path::new(a).file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    match name.as_str() {
        "php" | "composer" | "laravel" => args.insert(1, name.into()),
        _ => {}
    }
    args
}

/// `php`, `composer` or `laravel` with the PHP of the project (or of a leading `--site=<name>`).
fn run_named(paths: &HearthPaths, command: &str, args: &[String]) -> ExitCode {
    let (site, args) = split_site(args);
    let script = match command {
        "composer" => Some(paths.bin.join("composer.phar")),
        "laravel" => Some(
            paths.data.join("composer").join("vendor").join("laravel").join("installer").join("bin").join("laravel"),
        ),
        _ => None,
    };
    run_php(paths, site.as_deref(), script, args, None)
}

/// A leading `--site=<name>` or `--site <name>` (Herd's way to pick a site's PHP), and the rest.
fn split_site(args: &[String]) -> (Option<String>, &[String]) {
    match args {
        [first, rest @ ..] if first.starts_with("--site=") => (Some(first["--site=".len()..].to_string()), rest),
        [first, name, rest @ ..] if first == "--site" => (Some(name.clone()), rest),
        _ => (None, args),
    }
}

/// `php artisan tinker` in a site's folder, with that site's PHP.
fn tinker(paths: &HearthPaths, site: Option<String>) -> ExitCode {
    let found = match &site {
        Some(name) => registry::list(paths).into_iter().find(|s| s.name == name.trim_end_matches(".test")),
        None => std::env::current_dir().ok().and_then(|cwd| locate::site_for_dir(paths, &cwd)),
    };
    let Some(folder) = found.as_ref().and_then(|s| s.path.clone()) else {
        return fail("Run tinker inside a site's folder, or name one with --site");
    };
    let name = found.map(|s| s.name);
    run_php(paths, name.as_deref(), Some(folder.join("artisan")), &["tinker".to_string()], Some(&folder))
}

fn run_php(
    paths: &HearthPaths,
    site: Option<&str>,
    script: Option<std::path::PathBuf>,
    args: &[String],
    cwd: Option<&std::path::Path>,
) -> ExitCode {
    let minor = match site {
        Some(name) => {
            let name = name.trim_end_matches(".test");
            match locate::php_for_site(paths, name) {
                Ok(minor) => minor,
                Err(e) => return fail(&e.to_string()),
            }
        }
        None => std::env::current_dir().ok().and_then(|cwd| locate::php_for_dir(paths, &cwd)),
    };
    let Some(minor) = minor.filter(|m| install_dir(paths, m).join(store::VERSION_FILE).is_file()) else {
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
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.args(args);
    // On unix the CLI becomes PHP: signals (Ctrl+C) and the exit status are PHP's own.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = command.exec();
        return fail(&format!("Could not start PHP {minor}: {e}"));
    }
    // On Windows Ctrl+C reaches every program in the console: this one lets PHP decide and
    // waits for it, so the prompt only returns once PHP has finished.
    #[cfg(windows)]
    // SAFETY: removes no handler; only makes this process ignore Ctrl+C from now on.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1);
    }
    #[allow(unreachable_code)]
    match command.status() {
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
