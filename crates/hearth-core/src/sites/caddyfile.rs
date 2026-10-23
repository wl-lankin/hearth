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
    /// PHP minor -> FastCGI ports of its running pool.
    pub php_upstreams: BTreeMap<String, Vec<u16>>,
    pub log: PathBuf,
}

/// A path as one quoted Caddyfile token: forward slashes (Caddy accepts them on Windows and they
/// avoid backslash escapes), `"` escaped; braces are literal inside quotes.
fn quoted(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/").replace('"', "\\\"");
    format!("\"{text}\"")
}

pub fn render(ctx: &CaddyContext) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        // Loopback only: local sites are never reachable from the network.
        "{{\n\tadmin off\n\tauto_https off\n\tdefault_bind 127.0.0.1 [::1]\n\thttp_port {}\n\tlog {{\n\t\toutput file {}\n\t}}\n}}\n",
        ctx.http_port,
        quoted(&ctx.log)
    );
    for route in &ctx.routes {
        out.push('\n');
        let address = format!("http://{}.test:{}", route.name, ctx.http_port);
        match &route.target {
            Route::Php { docroot, minor } => {
                let Some(ports) = ctx.php_upstreams.get(minor).filter(|p| !p.is_empty()) else {
                    let _ = writeln!(out, "# {}: PHP {minor} is not running", route.name);
                    continue;
                };
                let upstreams: Vec<String> = ports.iter().map(|p| format!("127.0.0.1:{p}")).collect();
                let _ = write!(
                    out,
                    "{address} {{\n\troot * {}\n\tphp_fastcgi {} {{\n\t\tlb_policy least_conn\n\t}}\n\tfile_server\n\tencode gzip\n}}\n",
                    quoted(docroot),
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
