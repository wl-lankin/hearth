//! The Caddyfile for all sites. Hearth owns it completely: admin API off, no automatic HTTPS (M3
//! adds Hearth's own certificates), one block per site on the HTTP port.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// How one site is served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Php {
        docroot: PathBuf,
        minor: String,
    },
    Proxy {
        upstream: String,
    },
    /// Listed as a comment with the reason (folder missing, no index file, ...).
    Skip {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRoute {
    pub name: String,
    pub target: Route,
}

pub struct CaddyContext {
    pub http_port: u16,
    pub routes: Vec<SiteRoute>,
    /// PHP minor -> FastCGI addresses of its running pool ("127.0.0.1:19840", "unix//.../php-8.4.sock").
    pub php_upstreams: BTreeMap<String, Vec<String>>,
    pub log: PathBuf,
    /// Write Windows paths with forward slashes (Caddy accepts them there). On macOS a backslash is
    /// a legal file name character and stays.
    pub windows_paths: bool,
}

/// Paths that cannot be written into the Caddyfile safely and literally:
/// - `{$...}` is replaced with an environment variable while Caddy parses, before any escaping;
/// - a backslash kept as a name character (macOS) could escape the closing quote, letting the rest
///   become directives (Windows backslashes are separators and become `/`);
/// - line breaks and other control characters end the token.
pub fn servable(path: &Path, windows_paths: bool) -> Result<(), String> {
    let text = path.to_string_lossy();
    if text.contains("{$") {
        return Err("Caddy cannot serve a folder whose path contains {$".into());
    }
    if !windows_paths && text.contains('\\') {
        return Err("Caddy cannot serve a folder whose path contains a backslash".into());
    }
    if text.chars().any(char::is_control) {
        return Err("Caddy cannot serve a folder whose path contains a line break or control character".into());
    }
    Ok(())
}

/// A path as one quoted Caddyfile token: `"` escaped so it stays one token, `{` `}` escaped so Caddy
/// does not read them as placeholders at runtime.
fn quoted(path: &Path, windows_paths: bool) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if windows_paths {
        text = text.replace('\\', "/");
    }
    let text = text.replace('"', "\\\"").replace('{', "\\{").replace('}', "\\}");
    format!("\"{text}\"")
}

pub fn render(ctx: &CaddyContext) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        // Loopback only: local sites are never reachable from the network.
        "{{\n\tadmin off\n\tauto_https off\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port {}\n\tlog {{\n\t\toutput file {}\n\t}}\n}}\n",
        ctx.http_port,
        quoted(&ctx.log, ctx.windows_paths)
    );
    for route in &ctx.routes {
        out.push('\n');
        let address = format!("http://{}.test:{}", route.name, ctx.http_port);
        match &route.target {
            Route::Php { docroot, minor } => {
                let Some(upstreams) = ctx.php_upstreams.get(minor).filter(|p| !p.is_empty()) else {
                    let _ = writeln!(out, "# {}: PHP {minor} is not running", route.name);
                    continue;
                };
                let _ = write!(
                    out,
                    "{address} {{\n\troot * {}\n\tphp_fastcgi {} {{\n\t\tlb_policy least_conn\n\t\tlb_try_duration 2s\n\t}}\n\tfile_server\n\tencode gzip\n}}\n",
                    quoted(docroot, ctx.windows_paths),
                    upstreams.join(" ")
                );
            }
            Route::Proxy { upstream } => {
                let _ = write!(out, "{address} {{\n\treverse_proxy {upstream}\n}}\n");
            }
            Route::Skip { reason } => {
                let _ = writeln!(out, "# {}: {reason}", route.name);
            }
        }
    }
    out
}
