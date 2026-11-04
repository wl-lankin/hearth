//! Which site an action is about: the `site` argument (a name, a `.test` host or a path inside a
//! site), else the folder the server was started in. A path picks the deepest site holding it.

use std::path::{Path, PathBuf};

use hearth_core::sites::registry::{self, Site};
use hearth_core::workspaces::registry::within;
use hearth_core::HearthPaths;

/// `p` resolved, without Windows' `\\?\` prefix (sites are stored that way too after linking).
fn real(p: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let text = resolved.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_string())
}

/// The site `arg` names, or the one `start` is in.
pub fn resolve(paths: &HearthPaths, arg: Option<&str>, start: &Path) -> Result<Site, String> {
    let sites = registry::list(paths);
    let wanted = arg.map(str::trim).filter(|a| !a.is_empty());
    let not_found = |what: &str| format!("No Hearth site matches [{what}]. Call `site list`, then pass the site name.");
    let as_path = match wanted {
        Some(a) if !(a.contains('/') || a.contains('\\') || Path::new(a).is_dir()) => {
            let name = a.to_lowercase();
            let name = name.strip_suffix(".test").unwrap_or(&name).to_string();
            return sites.into_iter().find(|s| s.name == name).ok_or_else(|| not_found(a));
        }
        Some(a) => PathBuf::from(a),
        None => start.to_path_buf(),
    };
    let here = real(&as_path);
    sites
        .into_iter()
        .filter(|s| s.path.as_deref().is_some_and(|p| within(&here, &real(p))))
        .max_by_key(|s| s.path.as_ref().map_or(0, |p| real(p).as_os_str().len()))
        .ok_or_else(|| not_found(&as_path.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use hearth_core::sites::registry;
    use hearth_core::HearthPaths;

    use super::resolve;

    fn project(dir: &Path) {
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
    }

    #[test]
    fn names_hosts_and_paths_find_the_deepest_site() {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path().join("hearth"));
        paths.ensure().unwrap();
        let shop = d.path().join("shop");
        let api = shop.join("packages").join("api");
        project(&shop);
        project(&api);
        registry::link(&paths, "shop", &shop).unwrap();
        registry::link(&paths, "api.shop", &api).unwrap();
        let name = |arg: Option<&str>, start: &Path| resolve(&paths, arg, start).map(|s| s.name);
        assert_eq!(name(Some("Shop"), d.path()).unwrap(), "shop");
        assert_eq!(name(Some("api.shop.test"), d.path()).unwrap(), "api.shop");
        assert_eq!(name(Some(&api.join("src").to_string_lossy()), d.path()).unwrap(), "api.shop");
        assert_eq!(name(None, &shop.join("app")).unwrap(), "shop");
        assert_eq!(name(None, &api).unwrap(), "api.shop");
        assert_eq!(
            name(Some("nope"), d.path()).unwrap_err(),
            "No Hearth site matches [nope]. Call `site list`, then pass the site name."
        );
        let outside = d.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        assert!(name(None, &outside).unwrap_err().starts_with("No Hearth site matches ["));
    }
}
