//! Reading Herd's Valet folder: `config.json` (parked folders), `Sites` (linked sites as links),
//! and `Nginx/<name>.test.conf` (proxies, PHP isolations, HTTPS). Nothing in it is ever written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HerdSetup {
    pub parked: Vec<PathBuf>,
    pub links: BTreeMap<String, PathBuf>,
    pub proxies: BTreeMap<String, String>,
    pub isolated: BTreeMap<String, String>,
    pub secured: BTreeSet<String>,
    pub skipped: Vec<Skipped>,
}

/// Where Herd keeps its Valet folder on this computer.
pub fn default_valet_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support").join("Herd").join("config").join("valet")
    } else {
        home.join(".config").join("herd").join("config").join("valet")
    })
}

/// Reads Herd's sites from `valet`.
pub fn read(valet: &Path) -> Result<HerdSetup, CoreError> {
    let config = std::fs::read(valet.join("config.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .ok_or_else(|| CoreError::Php(format!("{} is not a Herd folder", valet.display())))?;
    let mut setup = HerdSetup::default();
    let sites_dir = valet.join("Sites");

    // Herd's own Sites folder is parked too; its entries arrive as links instead.
    let own = std::fs::canonicalize(&sites_dir).ok();
    for path in config["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str()) {
        let path = PathBuf::from(path);
        if own.is_some() && std::fs::canonicalize(&path).ok() == own {
            continue;
        }
        setup.parked.push(path);
    }

    for entry in std::fs::read_dir(&sites_dir).into_iter().flatten().filter_map(|e| e.ok()) {
        let Ok(target) = std::fs::read_link(entry.path()) else { continue };
        if let Some(name) = site_name(&entry.file_name().to_string_lossy(), &mut setup.skipped) {
            setup.links.insert(name, target);
        }
    }

    for entry in std::fs::read_dir(valet.join("Nginx")).into_iter().flatten().filter_map(|e| e.ok()) {
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some(raw) = file.strip_suffix(".test.conf") else { continue };
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        let Some(name) = site_name(raw, &mut setup.skipped) else { continue };
        read_conf(&name, &text, &mut setup);
    }
    Ok(setup)
}

/// A Herd site name as a Hearth one (lowercase), or a skip line when it cannot be one.
fn site_name(raw: &str, skipped: &mut Vec<Skipped>) -> Option<String> {
    let name = raw.to_lowercase();
    if crate::sites::registry::valid_name(&name) {
        return Some(name);
    }
    if !skipped.iter().any(|s| s.name == raw) {
        skipped.push(Skipped { name: raw.into(), reason: "Not a name Hearth can serve".into() });
    }
    None
}

/// What one site's nginx conf says: a proxy target, a PHP isolation, HTTPS.
fn read_conf(name: &str, text: &str, setup: &mut HerdSetup) {
    for line in text.lines().map(str::trim) {
        if let Some(version) = line.strip_prefix("# ISOLATED_PHP_VERSION=") {
            match php_minor(version.trim()) {
                Some(minor) => {
                    setup.isolated.insert(name.into(), minor);
                }
                None => setup.skipped.push(Skipped {
                    name: name.into(),
                    reason: format!("PHP version {} is not like 8.4", version.trim()),
                }),
            }
        } else if let Some(url) = line.strip_prefix("proxy_pass ") {
            let url = url.trim_end_matches(';').trim().trim_end_matches('/');
            setup.proxies.entry(name.into()).or_insert_with(|| url.into());
        } else if line.starts_with("listen ") && line.contains(":443") && line.contains("ssl") {
            setup.secured.insert(name.into());
        }
    }
}

/// `8.3` stays, Herd's older `83` becomes `8.3`; anything else is not a version.
fn php_minor(version: &str) -> Option<String> {
    if crate::php::store::is_minor(version) {
        return Some(version.into());
    }
    let digits: Vec<char> = version.chars().collect();
    match digits.as_slice() {
        [major, minor] if major.is_ascii_digit() && minor.is_ascii_digit() => Some(format!("{major}.{minor}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory link: a symlink where allowed, else a junction (needs no extra rights).
    fn link_dir(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(target, link).is_err() {
            let ok = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "could not create a junction");
        }
    }

    fn conf(valet: &Path, name: &str, body: &str) {
        std::fs::write(valet.join("Nginx").join(format!("{name}.test.conf")), body).unwrap();
    }

    const SECURE: &str = "server {\n    listen 127.0.0.1:443 ssl;\n    server_name x;\n}\n";

    /// A Valet folder with two parked paths (one of them Herd's own Sites), links, proxies and isolations.
    fn valet() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let valet = d.path().join("valet");
        let projects = d.path().join("Projects");
        let shop = projects.join("shop");
        std::fs::create_dir_all(shop.join("public")).unwrap();
        std::fs::create_dir_all(valet.join("Sites")).unwrap();
        std::fs::create_dir_all(valet.join("Nginx")).unwrap();
        let config = serde_json::json!({ "tld": "test", "paths": [valet.join("Sites"), projects] });
        std::fs::write(valet.join("config.json"), config.to_string()).unwrap();
        link_dir(&shop, &valet.join("shop"));
        std::fs::rename(valet.join("shop"), valet.join("Sites").join("shop")).unwrap();
        link_dir(&shop, &valet.join("Sites").join("admin.shop"));
        link_dir(&shop, &valet.join("Sites").join("Passify"));
        conf(&valet, "shop", &format!("# ISOLATED_PHP_VERSION=8.3\n{SECURE}"));
        conf(&valet, "admin.shop", &format!("# ISOLATED_PHP_VERSION=81\n{SECURE}"));
        conf(&valet, "Passify", &format!("# ISOLATED_PHP_VERSION=x\n{SECURE}"));
        conf(&valet, "vite.shop", "server {\n    listen 127.0.0.1:80;\n    location / {\n        proxy_pass http://localhost:5177/;\n    }\n}\n");
        (d, valet, projects)
    }

    #[test]
    fn parked_folders_leave_out_herds_own_sites_folder() {
        let (_d, valet, projects) = valet();
        assert_eq!(read(&valet).unwrap().parked, vec![projects]);
    }

    #[test]
    fn links_with_aliases_come_from_the_sites_folder_in_lowercase() {
        let (_d, valet, projects) = valet();
        let setup = read(&valet).unwrap();
        let names: Vec<&str> = setup.links.keys().map(String::as_str).collect();
        assert_eq!(names, ["admin.shop", "passify", "shop"]);
        let target = std::fs::canonicalize(&setup.links["admin.shop"]).unwrap();
        assert_eq!(target, std::fs::canonicalize(projects.join("shop")).unwrap());
    }

    #[test]
    fn confs_give_proxies_isolations_and_https() {
        let (_d, valet, _projects) = valet();
        let setup = read(&valet).unwrap();
        assert_eq!(setup.proxies.get("vite.shop").map(String::as_str), Some("http://localhost:5177"));
        assert_eq!(setup.isolated.get("shop").map(String::as_str), Some("8.3"));
        assert_eq!(setup.isolated.get("admin.shop").map(String::as_str), Some("8.1"));
        assert!(!setup.isolated.contains_key("passify"));
        assert!(setup
            .skipped
            .contains(&Skipped { name: "passify".into(), reason: "PHP version x is not like 8.4".into() }));
        let secured: Vec<&str> = setup.secured.iter().map(String::as_str).collect();
        assert_eq!(secured, ["admin.shop", "passify", "shop"]);
    }

    #[test]
    fn a_folder_without_config_json_is_not_herd() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path()).unwrap_err().to_string(), format!("{} is not a Herd folder", d.path().display()));
    }
}
