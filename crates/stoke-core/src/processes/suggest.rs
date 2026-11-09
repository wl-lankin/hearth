//! Processes a project probably wants, read from its files. Only offered: nothing is added unasked.

use std::path::Path;

use serde_json::Value;

use crate::processes::store::Proc;

fn json(dir: &Path, name: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(name)).ok()?).ok()
}

fn has_dev_script(dir: &Path, name: &str) -> bool {
    json(dir, name).is_some_and(|v| v.pointer("/scripts/dev").is_some())
}

/// Whether composer.lock names `package` (also among the dev packages).
fn locked(lock: &Option<Value>, package: &str) -> bool {
    let Some(lock) = lock else { return false };
    ["packages", "packages-dev"].iter().any(|key| {
        lock.get(key)
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(|p| p.get("name").and_then(Value::as_str) == Some(package)))
    })
}

/// The suggestions for the project in `dir`, in the order the page shows them.
pub fn for_dir(dir: &Path) -> Vec<Proc> {
    let mut out = Vec::new();
    let mut add = |name: &str, command: &str| {
        out.push(Proc { name: name.into(), command: command.into(), autostart: false });
    };
    if has_dev_script(dir, "package.json") {
        add("vite", "npm run dev");
    }
    if has_dev_script(dir, "composer.json") {
        add("dev", "composer run dev");
    }
    if dir.join("artisan").is_file() {
        // queue:listen (not queue:work) picks up code changes without a restart.
        add("queue", "php artisan queue:listen --tries=1");
        add("schedule", "php artisan schedule:work");
        let lock = json(dir, "composer.lock");
        for (package, name, command) in [
            ("laravel/pail", "pail", "php artisan pail"),
            ("laravel/reverb", "reverb", "php artisan reverb:start"),
            ("laravel/horizon", "horizon", "php artisan horizon"),
        ] {
            if locked(&lock, package) {
                add(name, command);
            }
        }
    }
    out
}
