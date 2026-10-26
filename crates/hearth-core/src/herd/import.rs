//! What a Herd import would add to Hearth (`plan`, writes nothing) and doing it (`apply`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::herd::{HerdSetup, Skipped};
use crate::sites::registry;
use crate::HearthPaths;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportPlan {
    pub park: Vec<PathBuf>,
    pub link: Vec<(String, PathBuf)>,
    pub proxy: Vec<(String, String)>,
    pub isolate: Vec<(String, String)>,
    pub secure: Vec<String>,
    pub skipped: Vec<Skipped>,
}

impl ImportPlan {
    pub fn is_empty(&self) -> bool {
        self.park.is_empty()
            && self.link.is_empty()
            && self.proxy.is_empty()
            && self.isolate.is_empty()
            && self.secure.is_empty()
    }
}

/// What importing `herd` would change in this Hearth home.
pub fn plan(paths: &HearthPaths, herd: &HerdSetup) -> ImportPlan {
    let config = registry::load(paths);
    let mut plan = ImportPlan { skipped: herd.skipped.clone(), ..ImportPlan::default() };
    let skip = |plan: &mut ImportPlan, name: &str, reason: &str| {
        plan.skipped.push(Skipped { name: name.into(), reason: reason.into() });
    };
    let taken = |name: &str| config.links.contains_key(name) || config.proxies.contains_key(name);

    for dir in &herd.parked {
        let shown = dir.display().to_string();
        let absolute = std::path::absolute(dir).unwrap_or_else(|_| dir.clone());
        if config.parked.contains(&absolute) {
            skip(&mut plan, &shown, "already parked");
        } else if !dir.is_dir() {
            skip(&mut plan, &shown, "Folder missing");
        } else {
            plan.park.push(absolute);
        }
    }
    for (name, upstream) in &herd.proxies {
        if taken(name) {
            skip(&mut plan, name, "already in Hearth");
        } else if !registry::valid_upstream(upstream) {
            skip(&mut plan, name, "Not an address like http://localhost:5173");
        } else {
            plan.proxy.push((name.clone(), upstream.clone()));
        }
    }
    // A name Herd proxies is served as that proxy, whatever its link says.
    for (name, target) in herd.links.iter().filter(|(n, _)| !herd.proxies.contains_key(*n)) {
        if taken(name) {
            skip(&mut plan, name, "already in Hearth");
        } else if !target.is_dir() {
            skip(&mut plan, name, "Folder missing");
        } else {
            plan.link.push((name.clone(), target.clone()));
        }
    }

    // The sites Hearth will have afterwards, and which of them run PHP.
    let mut php_sites: BTreeSet<String> =
        registry::list(paths).into_iter().filter(|s| s.upstream.is_none()).map(|s| s.name).collect();
    php_sites.extend(plan.link.iter().map(|(n, _)| n.clone()));
    for dir in &plan.park {
        for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            if let Some(name) = registry::name_from_folder(&entry.file_name().to_string_lossy()) {
                php_sites.insert(name);
            }
        }
    }
    php_sites.retain(|n| !config.proxies.contains_key(n));
    let mut all_sites = php_sites.clone();
    all_sites.extend(config.proxies.keys().cloned());
    all_sites.extend(plan.proxy.iter().map(|(n, _)| n.clone()));

    for (name, minor) in &herd.isolated {
        if php_sites.contains(name) && config.isolated.get(name) != Some(minor) {
            plan.isolate.push((name.clone(), minor.clone()));
        }
    }
    for name in &herd.secured {
        if all_sites.contains(name) && !config.secured.contains(name) {
            plan.secure.push(name.clone());
        }
    }
    plan
}

/// Carries out `plan`; what could not be done is returned, the rest is kept.
pub fn apply(paths: &HearthPaths, plan: &ImportPlan) -> Vec<Skipped> {
    let mut failed = Vec::new();
    let mut check = |name: &str, result: Result<(), crate::CoreError>| {
        if let Err(e) = result {
            failed.push(Skipped { name: name.into(), reason: e.to_string() });
        }
    };
    for dir in &plan.park {
        check(&dir.display().to_string(), registry::park(paths, dir));
    }
    for (name, target) in &plan.link {
        check(name, registry::link(paths, name, target));
    }
    for (name, upstream) in &plan.proxy {
        check(name, registry::proxy(paths, name, upstream));
    }
    for (name, minor) in &plan.isolate {
        check(name, registry::isolate(paths, name, Some(minor)));
    }
    for name in &plan.secure {
        check(name, registry::secure(paths, name));
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn hearth() -> (tempfile::TempDir, HearthPaths, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path().join("hearth"));
        paths.ensure().unwrap();
        let projects = d.path().join("Projects");
        for name in ["shop", "blog"] {
            std::fs::create_dir_all(projects.join(name).join("public")).unwrap();
            std::fs::write(projects.join(name).join("public").join("index.php"), "<?php").unwrap();
        }
        (d, paths, projects)
    }

    fn herd(projects: &std::path::Path) -> HerdSetup {
        HerdSetup {
            parked: vec![projects.to_path_buf(), projects.join("gone")],
            links: BTreeMap::from([
                ("api.shop".to_string(), projects.join("shop")),
                ("admin.shop".to_string(), projects.join("shop")),
                ("old".to_string(), projects.join("deleted")),
            ]),
            proxies: BTreeMap::from([("vite.shop".to_string(), "http://localhost:5177".to_string())]),
            isolated: BTreeMap::from([
                ("api.shop".to_string(), "8.3".to_string()),
                ("blog".to_string(), "8.1".to_string()),
                ("nowhere".to_string(), "8.2".to_string()),
            ]),
            secured: ["api.shop", "vite.shop", "blog", "nowhere"].iter().map(|s| s.to_string()).collect(),
            skipped: vec![Skipped { name: "Weird_Name".into(), reason: "Not a name Hearth can serve".into() }],
        }
    }

    fn reasons(plan: &ImportPlan) -> Vec<(String, String)> {
        plan.skipped.iter().map(|s| (s.name.clone(), s.reason.clone())).collect()
    }

    #[test]
    fn a_plan_lists_everything_and_writes_nothing() {
        let (_d, paths, projects) = hearth();
        let plan = plan(&paths, &herd(&projects));
        assert_eq!(plan.park, vec![projects.clone()]);
        let links: Vec<&str> = plan.link.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(links, ["admin.shop", "api.shop"]);
        assert_eq!(plan.proxy, vec![("vite.shop".to_string(), "http://localhost:5177".to_string())]);
        // "blog" is a site through the parked folder; "nowhere" is no site at all.
        assert_eq!(plan.isolate, vec![("api.shop".into(), "8.3".into()), ("blog".into(), "8.1".into())]);
        assert_eq!(plan.secure, ["api.shop", "blog", "vite.shop"]);
        let skipped = reasons(&plan);
        assert!(skipped.contains(&("Weird_Name".into(), "Not a name Hearth can serve".into())));
        assert!(skipped.contains(&("old".into(), "Folder missing".into())));
        assert!(skipped.contains(&(projects.join("gone").display().to_string(), "Folder missing".into())));
        assert!(!paths.config.join("sites.json").exists(), "a plan writes nothing");
    }

    #[test]
    fn applying_brings_the_sites_over_and_a_second_run_adds_nothing() {
        let (_d, paths, projects) = hearth();
        let setup = herd(&projects);
        let failed = apply(&paths, &plan(&paths, &setup));
        assert!(failed.is_empty(), "{failed:?}");
        let sites = registry::list(&paths);
        let api = sites.iter().find(|s| s.name == "api.shop").unwrap();
        assert_eq!((api.php.as_deref(), api.secured), (Some("8.3"), true));
        let vite = sites.iter().find(|s| s.name == "vite.shop").unwrap();
        assert_eq!((vite.upstream.as_deref(), vite.secured), (Some("http://localhost:5177"), true));
        let blog = sites.iter().find(|s| s.name == "blog").unwrap();
        assert_eq!(blog.php.as_deref(), Some("8.1"));

        let again = plan(&paths, &setup);
        assert!(again.is_empty(), "{again:?}");
        let skipped = reasons(&again);
        assert!(skipped.contains(&("api.shop".into(), "already in Hearth".into())));
        assert!(skipped.contains(&(projects.display().to_string(), "already parked".into())));
    }

    #[test]
    fn a_hearth_site_of_the_same_name_is_kept() {
        let (_d, paths, projects) = hearth();
        registry::proxy(&paths, "api.shop", "http://localhost:3000").unwrap();
        let plan = plan(&paths, &herd(&projects));
        assert!(!plan.link.iter().any(|(n, _)| n == "api.shop"));
        assert!(!plan.isolate.iter().any(|(n, _)| n == "api.shop"), "a proxy has no PHP");
        apply(&paths, &plan);
        let api = registry::list(&paths).into_iter().find(|s| s.name == "api.shop").unwrap();
        assert_eq!(api.upstream.as_deref(), Some("http://localhost:3000"));
    }
}
