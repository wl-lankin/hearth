//! What the move would do, item by item, from an inventory of Herd. Writes nothing.

use serde::Serialize;

use super::scan::{IniKind, Inventory};
use crate::herd::import::{self, ImportPlan};
use crate::StokePaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Area {
    Sites,
    Php,
    Https,
    Redis,
    Mail,
    MariaDb,
    Node,
    Path,
    Mcp,
    Autostart,
    Hosts,
}

/// One line of the dry run: what it is, what happens to it, or why it stays behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub area: Area,
    pub what: String,
    pub action: String,
    pub moved: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MovePlan {
    pub items: Vec<Item>,
    /// The sites part, as the existing importer carries it out.
    pub sites: ImportPlan,
}

/// The PHP extensions Stoke's builds can switch on.
pub const STOKE_EXTENSIONS: &[&str] = &["redis", "xdebug"];

fn moved(area: Area, what: impl Into<String>, action: impl Into<String>) -> Item {
    Item { area, what: what.into(), action: action.into(), moved: true, reason: None }
}

fn stays(area: Area, what: impl Into<String>, reason: impl Into<String>) -> Item {
    Item { area, what: what.into(), action: "not moved".into(), moved: false, reason: Some(reason.into()) }
}

/// The plan for moving `inv` into this Stoke home, where `stoke_php` are the minors it has.
pub fn plan(paths: &StokePaths, inv: &Inventory, stoke_php: &[String]) -> MovePlan {
    let sites = import::plan(paths, &inv.sites);
    let mut items = Vec::new();
    use Area::*;

    for dir in &sites.park {
        items.push(moved(Sites, format!("Parked folder {}", dir.display()), "parked in Stoke"));
    }
    for (name, dir) in &sites.link {
        items.push(moved(Sites, format!("{name}.test"), format!("linked to {}", dir.display())));
    }
    for (name, upstream) in &sites.proxy {
        items.push(moved(Sites, format!("{name}.test"), format!("proxy to {upstream}")));
    }
    for (name, minor) in &sites.isolate {
        items.push(moved(Sites, format!("{name}.test PHP"), format!("PHP {minor}")));
    }
    for name in &sites.secure {
        items.push(moved(Sites, format!("{name}.test HTTPS"), "HTTPS"));
    }
    for skipped in &sites.skipped {
        items.push(stays(Sites, &skipped.name, &skipped.reason));
    }

    for minor in inv.php_versions.iter().filter(|v| !stoke_php.contains(v)) {
        items.push(moved(Php, format!("PHP {minor}"), format!("install PHP {minor} from Stoke's builds")));
    }
    if let Some(default) = &inv.php_default {
        items.push(moved(Php, "PHP default version", format!("default PHP {default}")));
    }
    for (minor, lines) in &inv.php_ini {
        for line in lines {
            let what = format!(
                "{} (PHP {minor})",
                match &line.kind {
                    IniKind::Extension(name) => name.clone(),
                    _ => line.key.clone(),
                }
            );
            items.push(match &line.kind {
                IniKind::Setting => moved(Php, what, format!("{} = {} in Stoke's php.ini", line.key, line.value)),
                IniKind::Extension(name) if STOKE_EXTENSIONS.contains(&name.as_str()) => {
                    moved(Php, what, "turned on in Stoke")
                }
                IniKind::Extension(name) if name == "herd" => moved(Php, what, "Stoke's dumps instead"),
                IniKind::Extension(name) => stays(Php, what, format!("Stoke's PHP builds have no {name} extension")),
                IniKind::HerdPath => match line.key.as_str() {
                    "auto_prepend_file" => moved(Php, what, "Stoke's dump loader instead"),
                    "curl.cainfo" | "openssl.cafile" => moved(Php, what, "Stoke's CA bundle instead"),
                    "extension_dir" => moved(Php, what, "Stoke's own extension folder"),
                    _ => stays(Php, what, "It points into Herd's folders"),
                },
            });
        }
    }

    if inv.ca.is_some() {
        items.push(moved(Https, "Herd's certificate authority", "used by Stoke, so browsers keep trusting your sites"));
    }
    items.push(moved(Https, "Web ports", "Stoke serves on 80 and 443"));

    if let Some(redis) = &inv.redis {
        let data = if redis.dump.is_some() { "with Herd's data" } else { "(Herd had no saved data)" };
        items.push(moved(Redis, format!("Redis on {}", redis.port), format!("Stoke's Redis on {} {data}", redis.port)));
    }
    if let Some(port) = inv.smtp_port {
        items.push(moved(Mail, format!("Mail catcher on {port}"), format!("Stoke catches mail on {port}")));
    }
    if let Some(version) = &inv.mariadb {
        items.push(stays(
            MariaDb,
            format!("MariaDB {version}"),
            "It is not running; your other databases are not touched",
        ));
    }

    for version in &inv.node_versions {
        items.push(moved(Node, format!("Node {version}"), "installed in Stoke"));
    }
    if let Some(default) = &inv.node_default {
        items.push(moved(Node, "Node default version", format!("default {default}")));
    }

    for entry in &inv.user_path {
        items.push(moved(Path, entry.clone(), "removed from your PATH; Stoke's bin instead"));
    }
    for entry in &inv.machine_path {
        items.push(moved(Path, entry.clone(), "removed from the machine PATH"));
    }
    for (name, _) in &inv.machine_vars {
        items.push(moved(Path, name.clone(), "removed (Herd's Node manager)"));
    }
    if inv.mcp.is_some() {
        items.push(moved(Mcp, "MCP entry herd", "points at Stoke's MCP server"));
    }
    if inv.autostart.is_some() {
        items.push(moved(Autostart, "Herd starting with Windows", "switched off"));
    }
    for host in &inv.other_hosts {
        items.push(stays(Hosts, host.clone(), "Not a site (workspace or Herd's own)"));
    }

    MovePlan { items, sites }
}
