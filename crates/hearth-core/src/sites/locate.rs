//! Which site a folder belongs to, and so which PHP runs there: what `php` and `composer` use in a
//! terminal, like Herd. Reads only the registry, so it works without the app running.

use std::path::{Component, Path};

use crate::php::store;
use crate::sites::registry::{self, Site};
use crate::web::pools::effective_php;
use crate::HearthPaths;

/// The site whose folder contains `dir` (the deepest one when sites are nested).
pub fn site_for_dir(paths: &HearthPaths, dir: &Path) -> Option<Site> {
    // Both sides with symlinks resolved: the shell's folder and a site's stored path can name the
    // same place differently (macOS `/var` -> `/private/var`, a symlinked `~/Sites`).
    let real =
        |p: &Path| std::fs::canonicalize(p).or_else(|_| std::path::absolute(p)).unwrap_or_else(|_| p.to_path_buf());
    let dir = real(dir);
    registry::list(paths)
        .into_iter()
        .filter(|s| s.path.as_deref().is_some_and(|p| contains(&real(p), &dir)))
        // The deepest folder wins; for the same folder a link (a deliberate choice) beats parking.
        .max_by_key(|s| {
            let depth = s.path.as_deref().map_or(0, |p| p.components().count());
            (depth, s.kind == registry::SiteKind::Linked)
        })
}

/// The PHP minor for `dir`: its site's isolation when that version is installed, else the default.
pub fn php_for_dir(paths: &HearthPaths, dir: &Path) -> Option<String> {
    let state = store::load(paths);
    let installed: Vec<String> = store::installed(paths).into_iter().map(|i| i.minor).collect();
    let isolated = site_for_dir(paths, dir).and_then(|s| s.php);
    effective_php(isolated.as_deref(), &installed, state.default.as_deref()).0
}

/// The PHP minor of the site `name`: its isolation when installed, else the default.
pub fn php_for_site(paths: &HearthPaths, name: &str) -> Result<Option<String>, crate::CoreError> {
    let site = registry::list(paths)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| crate::CoreError::Php(format!("No site named {name}.test")))?;
    let state = store::load(paths);
    let installed: Vec<String> = store::installed(paths).into_iter().map(|i| i.minor).collect();
    Ok(effective_php(site.php.as_deref(), &installed, state.default.as_deref()).0)
}

/// Whether `inner` is `outer` or below it, comparing names without case on Windows.
fn contains(outer: &Path, inner: &Path) -> bool {
    let key = |c: Component| {
        let text = c.as_os_str().to_string_lossy().into_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let outer: Vec<String> = outer.components().map(key).collect();
    let inner: Vec<String> = inner.components().map(key).collect();
    inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HearthPaths, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path().join("hearth"));
        paths.ensure().unwrap();
        let projects = d.path().join("Projects");
        for name in ["shop", "blog"] {
            std::fs::create_dir_all(projects.join(name).join("public")).unwrap();
            std::fs::write(projects.join(name).join("public").join("index.php"), "<?php").unwrap();
            std::fs::create_dir_all(projects.join(name).join("app").join("Models")).unwrap();
        }
        (d, paths, projects)
    }

    /// Marks PHP versions as installed (only the registry entries the lookup reads).
    fn installed(paths: &HearthPaths, minors: &[&str], default: &str) {
        for minor in minors {
            std::fs::create_dir_all(store::install_dir(paths, minor)).unwrap();
            std::fs::write(store::install_dir(paths, minor).join(store::VERSION_FILE), format!("{minor}.0")).unwrap();
            store::register(paths, minor, &[]).unwrap();
        }
        store::set_default(paths, default).unwrap();
    }

    #[test]
    fn a_folder_inside_a_site_gets_that_site() {
        let (_d, paths, projects) = home();
        registry::park(&paths, &projects).unwrap();
        let deep = projects.join("shop").join("app").join("Models");
        assert_eq!(site_for_dir(&paths, &projects.join("shop")).map(|s| s.name).as_deref(), Some("shop"));
        assert_eq!(site_for_dir(&paths, &deep).map(|s| s.name).as_deref(), Some("shop"));
        assert_eq!(site_for_dir(&paths, &projects).map(|s| s.name), None, "the parked folder itself is no site");
    }

    #[test]
    fn a_linked_folder_is_found_and_wins_inside_a_parked_one() {
        let (_d, paths, projects) = home();
        registry::park(&paths, &projects).unwrap();
        registry::link(&paths, "api.shop", &projects.join("shop")).unwrap();
        let found = site_for_dir(&paths, &projects.join("shop").join("app")).map(|s| s.name);
        assert_eq!(found.as_deref(), Some("api.shop"), "a link is a deliberate choice over parking");
        let (_d2, other, elsewhere) = home();
        registry::link(&other, "blog", &elsewhere.join("blog")).unwrap();
        assert_eq!(
            site_for_dir(&other, &elsewhere.join("blog").join("public")).map(|s| s.name).as_deref(),
            Some("blog")
        );
    }

    #[test]
    fn the_php_is_the_isolation_else_the_default() {
        let (_d, paths, projects) = home();
        installed(&paths, &["8.3", "8.4"], "8.4");
        registry::park(&paths, &projects).unwrap();
        registry::isolate(&paths, "shop", Some("8.3")).unwrap();
        registry::isolate(&paths, "blog", Some("8.1")).unwrap();
        assert_eq!(php_for_dir(&paths, &projects.join("shop").join("app")).as_deref(), Some("8.3"));
        assert_eq!(php_for_dir(&paths, &projects.join("blog")).as_deref(), Some("8.4"), "8.1 is not installed");
        assert_eq!(php_for_dir(&paths, &projects).as_deref(), Some("8.4"), "no site: the default");
    }

    /// macOS reports the current folder with symlinks resolved (`/private/var/...` for `/var/...`,
    /// or a real path for a symlinked `~/Sites`), while sites keep the path they were added with.
    #[cfg(unix)]
    #[test]
    fn a_site_added_through_a_symlink_is_found_from_the_real_folder() {
        let (d, paths, projects) = home();
        let alias = d.path().join("Sites");
        std::os::unix::fs::symlink(&projects, &alias).unwrap();
        registry::park(&paths, &alias).unwrap();
        let real = std::fs::canonicalize(projects.join("shop").join("app")).unwrap();
        assert_eq!(site_for_dir(&paths, &real).map(|s| s.name).as_deref(), Some("shop"));
    }

    #[cfg(windows)]
    #[test]
    fn folder_names_match_without_case_on_windows() {
        let (_d, paths, projects) = home();
        registry::park(&paths, &projects).unwrap();
        let shouted = std::path::PathBuf::from(projects.join("SHOP").to_string_lossy().to_uppercase());
        assert_eq!(site_for_dir(&paths, &shouted).map(|s| s.name).as_deref(), Some("shop"));
    }
}
