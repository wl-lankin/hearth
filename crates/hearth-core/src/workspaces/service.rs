//! Making, adopting, merging and deleting workspaces.

use std::path::{Path, PathBuf};

use crate::ipc::CoreState;
use crate::sites::registry as sites;
use crate::workspaces::database::{self, Mode, Prepared};
use crate::workspaces::registry::{self, within, Record};
use crate::workspaces::{env_file, git, names};
use crate::{CoreError, HearthPaths};

const TLD: &str = "test";

/// What a create asks for.
#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub site: String,
    /// The branch; a random `<adjective>-<animal>` when empty.
    pub name: Option<String>,
    pub database: Mode,
    /// HTTPS; the parent's setting when `None`.
    pub secure: Option<bool>,
    /// Run `artisan migrate` for an empty database.
    pub migrate: bool,
    /// `cli` or `app`.
    pub by: String,
}

/// A workspace and what the user should know about how it was made.
#[derive(Debug, Clone)]
pub struct Made {
    pub record: Record,
    pub notes: Vec<String>,
}

fn problem(message: impl Into<String>) -> CoreError {
    CoreError::Php(message.into())
}

/// `p` resolved (symlinks, 8.3 names, `/` from git) and without Windows' `\\?\` prefix.
fn real(p: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let text = resolved.to_string_lossy();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_string())
}

/// Where Hearth puts the worktrees it makes.
pub fn root(paths: &HearthPaths) -> PathBuf {
    paths.data.join("workspaces")
}

/// Whether `path` is one of Hearth's own worktree folders: exactly `<root>/<parent>/<label>`,
/// compared resolved (junctions, 8.3 names, `..`). Only these may be deleted by hand.
fn own_folder(paths: &HearthPaths, path: &Path) -> bool {
    let (path, root) = (real(path), real(&root(paths)));
    path.parent().and_then(Path::parent).is_some_and(|grand| real(grand) == root)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn url(paths: &HearthPaths, host: &str, secured: bool) -> String {
    let config = sites::load(paths);
    let (scheme, port, default) =
        if secured { ("https", config.https_port, 443) } else { ("http", config.http_port, 80) };
    let port = if port == default { String::new() } else { format!(":{port}") };
    format!("{scheme}://{host}.{TLD}{port}")
}

/// The parent site and its checkout: (site, repository root, served folder inside it).
fn parent_of(paths: &HearthPaths, name: &str) -> Result<(sites::Site, PathBuf, String), CoreError> {
    let site = sites::list(paths).into_iter().find(|s| s.name == name);
    let site = site.ok_or_else(|| problem(format!("No site named {name}.test")))?;
    let dir =
        site.path.clone().ok_or_else(|| problem(format!("{name}.test is a proxy: it has no folder to branch")))?;
    let top = git::toplevel(&dir)
        .ok_or_else(|| problem("Workspaces need a git repository. Run `git init` and commit first."))?;
    if !git::has_commits(&top) {
        return Err(problem("The repository has no commits yet. Commit first, then make a workspace."));
    }
    let (top, dir) = (real(&top), real(&dir));
    let app_dir = dir.strip_prefix(&top).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    Ok((site, top, app_dir))
}

/// Makes a workspace: a worktree of `options.site`'s repository on a new (or existing) branch.
pub fn create(core: &CoreState, options: CreateOptions) -> Result<Made, CoreError> {
    let paths = &core.paths;
    let (parent, repo, app_dir) = parent_of(paths, &options.site)?;
    let base = git::current_branch(&repo)
        .ok_or_else(|| problem("The parent has no branch checked out (a detached HEAD): check one out first"))?;
    let branch = match options.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None => names::random_branch(|b| git::branch_exists(&repo, b)),
    };
    let host = free_host(paths, &branch, &parent.name);
    let label = host.strip_suffix(&format!(".{}", parent.name)).unwrap_or(&host).to_string();
    let path = root(paths).join(&parent.name).join(&label);
    std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| CoreError::io(&path, e))?;
    let new_branch = !git::branch_exists(&repo, &branch);
    git::add(&repo, &path, &branch, &base)?;
    let record = Record {
        kind: "worktree".into(),
        name: branch.clone(),
        site: host,
        url: String::new(),
        path: real(&path),
        app_dir,
        parent_site: parent.name.clone(),
        parent_path: repo.clone(),
        branch,
        base_branch: base,
        base_commit: git::head(&repo).unwrap_or_default(),
        secured: false,
        php_version: parent.php.clone(),
        database_mode: Mode::Shared,
        database_name: None,
        database_driver: None,
        created_by: options.by.clone(),
        created_at: now(),
        owned: true,
    };
    provision(core, record, &parent, &options, new_branch)
}

/// Serves an existing linked worktree (made with `git worktree add`) as a workspace.
pub fn adopt(core: &CoreState, path: &Path, options: CreateOptions) -> Result<Made, CoreError> {
    let paths = &core.paths;
    let top = git::toplevel(path).ok_or_else(|| problem(format!("{} is not inside a git checkout", path.display())))?;
    let top = real(&top);
    if let Some(r) = registry::load(paths).into_iter().find(|r| real(&r.path) == top) {
        return Err(problem(format!("{} is already the workspace {}", top.display(), r.site)));
    }
    let repo = git::linked_worktree_parent(&top)
        .map(|p| real(&p))
        .ok_or_else(|| problem(format!("{} is not a git worktree", top.display())))?;
    // The parent site: the deepest site folder of the repository's own checkout. Worktrees inside
    // the repository (`.worktrees/`, agents' folders) are checkouts of their own, never parents.
    let parent = sites::list(paths)
        .into_iter()
        .filter(|s| {
            s.path.as_deref().is_some_and(|p| {
                let p = real(p);
                within(&p, &repo) && git::toplevel(&p).is_some_and(|top| real(&top) == repo)
            })
        })
        .max_by_key(|s| s.path.as_ref().map_or(0, |p| p.as_os_str().len()))
        .ok_or_else(|| problem(format!("No site serves the repository {}", repo.display())))?;
    let parent_dir = real(parent.path.as_deref().unwrap_or(&repo));
    let app_dir = parent_dir.strip_prefix(&repo).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let branch = git::current_branch(&top)
        .ok_or_else(|| problem("The worktree has no branch checked out (a detached HEAD): check one out first"))?;
    let base = git::current_branch(&repo).unwrap_or_else(|| "main".into());
    let base_commit = git::run(&repo, &["merge-base", &base, &branch]).unwrap_or_default();
    let host = free_host(paths, &branch, &parent.name);
    let record = Record {
        kind: "worktree".into(),
        name: branch.clone(),
        site: host,
        url: String::new(),
        path: top,
        app_dir,
        parent_site: parent.name.clone(),
        parent_path: repo,
        branch,
        base_branch: base,
        base_commit,
        secured: false,
        php_version: parent.php.clone(),
        database_mode: Mode::Shared,
        database_name: None,
        database_driver: None,
        created_by: options.by.clone(),
        created_at: now(),
        owned: false,
    };
    provision(core, record, &parent, &options, false)
}

/// A host for `branch` under `parent` that no site, workspace or Hearth folder uses yet.
fn free_host(paths: &HearthPaths, branch: &str, parent: &str) -> String {
    let taken_sites: Vec<String> = sites::list(paths).into_iter().map(|s| s.name).collect();
    let taken_ws: Vec<String> = registry::load(paths).into_iter().map(|r| r.site).collect();
    let folder = root(paths).join(parent);
    names::host(&names::label(branch), parent, |h| {
        let label = h.strip_suffix(&format!(".{parent}")).unwrap_or(h);
        taken_sites.iter().any(|s| s == h) || taken_ws.iter().any(|s| s == h) || folder.join(label).exists()
    })
}

/// What a half-made workspace has, so a failure can take it back.
#[derive(Default)]
struct Undo {
    linked: Option<String>,
    database: Option<DbUndo>,
}

/// A database `provision` made: dropped again when a later step fails.
struct DbUndo {
    /// The parent's .env (for the connection).
    env: String,
    name: String,
    driver: String,
}

/// Seeds, links, sets up `.env` and the database, then records the workspace. Any failure takes
/// back what was done: the worktree of a created workspace too, and its branch when the create
/// made it (`new_branch`).
fn provision(
    core: &CoreState,
    mut record: Record,
    parent: &sites::Site,
    options: &CreateOptions,
    new_branch: bool,
) -> Result<Made, CoreError> {
    let mut undo = Undo::default();
    match provision_steps(core, &mut record, parent, options, &mut undo) {
        Ok(notes) => {
            if core.web_sync {
                crate::web::service::schedule_sync(core);
            }
            Ok(Made { record, notes })
        }
        Err(e) => {
            let paths = &core.paths;
            if let Some(db) = undo.database {
                let parent_db = env_file::get(&db.env, "DB_DATABASE").unwrap_or_default();
                let _ = database::drop(paths, &db.env, "", &db.name, &db.driver, &parent_db);
            }
            let linked = undo.linked.is_some();
            if let Some(host) = undo.linked {
                let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
                let _ = sites::unlink(paths, &host);
            }
            if record.owned {
                let _ = git::remove(&record.parent_path, &record.path, own_folder(paths, &record.path));
                if new_branch {
                    let _ = git::run(&record.parent_path, &["branch", "-D", &record.branch]);
                }
            }
            let _ = registry::remove(paths, &record.site);
            // A sync between link and failure may serve the host: let the web server follow.
            if linked && core.web_sync {
                crate::web::service::schedule_sync(core);
            }
            Err(e)
        }
    }
}

fn provision_steps(
    core: &CoreState,
    record: &mut Record,
    parent: &sites::Site,
    options: &CreateOptions,
    undo: &mut Undo,
) -> Result<Vec<String>, CoreError> {
    let paths = &core.paths;
    let mut notes = Vec::new();
    let (app, parent_app) = (record.app_path(), record.parent_app_path());
    seed(&parent_app, &app, &mut notes)?;
    {
        let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
        sites::link(paths, &record.site, &app)?;
        undo.linked = Some(record.site.clone());
        if let Some(minor) = &record.php_version {
            sites::isolate(paths, &record.site, Some(minor))?;
        }
        record.secured = options.secure.unwrap_or(parent.secured);
        if record.secured {
            sites::secure(paths, &record.site)?;
        }
    }
    record.url = url(paths, &record.site, record.secured);
    // .env: the new address, and the database the mode asks for.
    let env_path = app.join(".env");
    let env = std::fs::read_to_string(&env_path).unwrap_or_default();
    let prepared: Prepared = database::prepare(paths, &app, &parent_app, &env, options.database, &record.branch);
    notes.extend(prepared.notes.iter().cloned());
    if let (Some(name), Some(driver)) = (&prepared.name, &prepared.driver) {
        if driver != "sqlite" {
            let parent_env = std::fs::read_to_string(parent_app.join(".env")).unwrap_or_default();
            undo.database = Some(DbUndo { env: parent_env, name: name.clone(), driver: driver.clone() });
        }
    }
    record.database_mode = prepared.mode;
    record.database_name = prepared.name.clone();
    record.database_driver = prepared.driver.clone();
    if env_path.is_file() {
        let values = env_values(&env, record, &prepared);
        let pairs: Vec<(&str, &str)> = values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        std::fs::write(&env_path, env_file::set(&env, &pairs)).map_err(|e| CoreError::io(&env_path, e))?;
    }
    registry::put(paths, record.clone())?;
    notes.extend(registry::mark(record)?);
    if record.database_mode == Mode::Empty && options.migrate && app.join("artisan").is_file() {
        if let Err(e) = migrate(paths, &app, &record.site) {
            notes.push(format!("`php artisan migrate` failed ({e}): run it in the workspace"));
        }
    }
    Ok(notes)
}

/// The `.env` lines that change: the address (only where the parent's address was) and the database.
fn env_values(env: &str, record: &Record, prepared: &Prepared) -> Vec<(String, String)> {
    let old = format!("{}.{TLD}", record.parent_site);
    let new = format!("{}.{TLD}", record.site);
    let mut values = Vec::new();
    if env_file::get(env, "APP_URL").is_some() {
        values.push(("APP_URL".to_string(), record.url.clone()));
    }
    if env_file::get(env, "SESSION_DOMAIN").as_deref() == Some(old.as_str()) {
        values.push(("SESSION_DOMAIN".to_string(), new.clone()));
    }
    if let Some(list) = env_file::get(env, "SANCTUM_STATEFUL_DOMAINS").filter(|l| !l.is_empty()) {
        let moved: Vec<String> = list
            .split(',')
            .map(str::trim)
            .map(|d| match d.strip_prefix(&old) {
                Some("") => new.clone(),
                Some(port) if port.starts_with(':') => format!("{new}{port}"),
                _ => d.to_string(),
            })
            .collect();
        values.push(("SANCTUM_STATEFUL_DOMAINS".to_string(), moved.join(",")));
    }
    values.extend(prepared.env.iter().cloned());
    values
}

/// Copies what git does not bring: `.env`, `.env.testing`, and the installed dependencies and built
/// assets when the lockfiles match (installing them again would take minutes).
fn seed(parent: &Path, app: &Path, notes: &mut Vec<String>) -> Result<(), CoreError> {
    std::fs::create_dir_all(app).map_err(|e| CoreError::io(app, e))?;
    for file in [".env", ".env.testing"] {
        let (from, to) = (parent.join(file), app.join(file));
        if from.exists() && !to.exists() {
            std::fs::copy(&from, &to).map_err(|e| CoreError::io(&from, e))?;
        }
    }
    let same = |file: &str| {
        let (a, b) = (std::fs::read(parent.join(file)), std::fs::read(app.join(file)));
        matches!((a, b), (Ok(a), Ok(b)) if a == b)
    };
    if parent.join("vendor").is_dir() && !app.join("vendor").exists() {
        if same("composer.lock") {
            copy_or_note(&parent.join("vendor"), &app.join("vendor"), "composer install", notes);
        } else {
            notes.push("composer.lock differs from the parent's: run `composer install` in the workspace".into());
        }
    }
    let locks = ["pnpm-lock.yaml", "yarn.lock", "bun.lock", "bun.lockb", "package-lock.json", "npm-shrinkwrap.json"];
    if parent.join("node_modules").is_dir() && !app.join("node_modules").exists() {
        match locks.iter().find(|l| parent.join(l).is_file()) {
            Some(lock) if same(lock) => {
                copy_or_note(&parent.join("node_modules"), &app.join("node_modules"), "npm install", notes)
            }
            _ => notes.push(
                "The JavaScript lockfile differs from the parent's: install the packages in the workspace".into(),
            ),
        }
    }
    let build = Path::new("public").join("build");
    if parent.join(&build).is_dir() && !app.join(&build).exists() && !app.join("public").join("hot").exists() {
        copy_or_note(&parent.join(&build), &app.join(&build), "npm run build", notes);
    }
    Ok(())
}

/// Copies a dependency folder; when that fails the partial copy goes and a note says what to run
/// (a half-copied `vendor` would look installed).
fn copy_or_note(from: &Path, to: &Path, instead: &str, notes: &mut Vec<String>) {
    if let Err(e) = copy_tree(from, to) {
        let _ = std::fs::remove_dir_all(to);
        let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        notes.push(format!("{name} could not be copied ({e}): run `{instead}` in the workspace"));
    }
}

/// Copies a folder. Links stay links to the same target (`node_modules/.bin`, pnpm's layout,
/// Composer path repositories): copying what they point to would copy the world.
fn copy_tree(from: &Path, to: &Path) -> Result<(), CoreError> {
    std::fs::create_dir_all(to).map_err(|e| CoreError::io(to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| CoreError::io(from, e))?.flatten() {
        let kind = entry.file_type().map_err(|e| CoreError::io(entry.path(), e))?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            copy_link(&entry.path(), &target)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| CoreError::io(entry.path(), e))?;
        }
    }
    Ok(())
}

/// Makes `to` a link to where `from` points. Windows: a symbolic link when allowed (developer
/// mode), else a junction for a folder, else a copy of a file.
fn copy_link(from: &Path, to: &Path) -> Result<(), CoreError> {
    let target = std::fs::read_link(from).map_err(|e| CoreError::io(from, e))?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, to).map_err(|e| CoreError::io(to, e))
    }
    #[cfg(windows)]
    {
        // A relative target is relative to the link's folder.
        let resolved = if target.is_absolute() { target.clone() } else { from.parent().unwrap_or(from).join(&target) };
        if resolved.is_dir() {
            if std::os::windows::fs::symlink_dir(&target, to).is_ok() {
                return Ok(());
            }
            let out =
                crate::workspaces::command("cmd").arg("/c").arg("mklink").arg("/J").arg(to).arg(&resolved).output();
            return match out {
                Ok(o) if o.status.success() => Ok(()),
                _ => Err(CoreError::Php(format!("{} could not be linked", to.display()))),
            };
        }
        if std::os::windows::fs::symlink_file(&target, to).is_ok() {
            return Ok(());
        }
        std::fs::copy(&resolved, to).map(|_| ()).map_err(|e| CoreError::io(to, e))
    }
}

/// `php artisan migrate --force` with the PHP the workspace's site runs on.
fn migrate(paths: &HearthPaths, app: &Path, site: &str) -> Result<(), CoreError> {
    let php = sites::list(paths).into_iter().find(|s| s.name == site).and_then(|s| s.php);
    let installed: Vec<String> = crate::php::store::installed(paths).into_iter().map(|i| i.minor).collect();
    let default = crate::php::store::load(paths).default;
    let (minor, note) = crate::web::pools::effective_php(php.as_deref(), &installed, default.as_deref());
    let minor = minor.ok_or_else(|| problem(note.unwrap_or_else(|| "no PHP is installed".into())))?;
    let dir = crate::php::store::install_dir(paths, &minor);
    let program = if cfg!(windows) { dir.join("php.exe") } else { dir.join("bin").join("php") };
    let ini = crate::php::ini::ini_dir(paths, &minor);
    let out = crate::workspaces::command(&program)
        .arg("-c")
        .arg(ini.join("php.ini"))
        .args(["artisan", "migrate", "--force", "--no-interaction"])
        .env("PHPRC", &ini)
        .current_dir(app)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| problem(format!("PHP {minor} could not be started: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    let tail: String = text.trim().chars().rev().take(400).collect::<String>().chars().rev().collect();
    Err(problem(tail))
}

/// Deletes a workspace: its host, its own database (unless kept) and its worktree (unless kept).
/// The branch stays in the repository.
pub fn delete(core: &CoreState, name: &str, keep_database: bool, keep_files: bool) -> Result<Vec<String>, CoreError> {
    let paths = &core.paths;
    let record = registry::find(paths, name)?;
    let mut notes = Vec::new();
    {
        let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = sites::unlink(paths, &record.site);
    }
    // Its dev processes go with its site.
    if let Err(e) = crate::processes::run::prune(core) {
        notes.push(format!("Its processes could not be cleaned up ({e})"));
    }
    let own_db = record.database_mode != Mode::Shared && record.database_driver.as_deref() != Some("sqlite");
    if own_db && !keep_database {
        // The database Hearth made, as recorded: what the .env names now may be another one.
        let env = std::fs::read_to_string(record.app_path().join(".env")).unwrap_or_default();
        let parent_env = std::fs::read_to_string(record.parent_app_path().join(".env")).unwrap_or_default();
        let name = record.database_name.clone().unwrap_or_default();
        let parent_db = env_file::get(&parent_env, "DB_DATABASE").unwrap_or_default();
        let driver = record.database_driver.clone().unwrap_or_default();
        match database::drop(paths, &env, &parent_env, &name, &driver, &parent_db) {
            Ok(()) => {
                if env_file::get(&env, "DB_DATABASE").is_some_and(|now| now != name) {
                    notes
                        .push(format!("Dropped {name}, the database Hearth made; the one its .env names now was kept"));
                }
            }
            Err(e) => notes.push(format!("Its database {name} could not be dropped ({e})")),
        }
    }
    if !keep_files && record.path.exists() {
        // Only Hearth's own folders may be deleted by hand when git cannot; an adopted worktree
        // elsewhere is left to git.
        let own = record.owned && own_folder(paths, &record.path);
        if let Err(e) = git::remove(&record.parent_path, &record.path, own) {
            notes.push(format!("The worktree could not be removed completely ({e})"));
        }
    }
    registry::remove(paths, &record.site)?;
    if core.web_sync {
        crate::web::service::schedule_sync(core);
    }
    Ok(notes)
}

/// Merges a workspace's branch into its base in the parent; `push` pushes the base, `delete` deletes
/// the workspace afterwards.
pub fn merge(core: &CoreState, name: &str, push: bool, delete_after: bool) -> Result<(String, Vec<String>), CoreError> {
    let record = registry::find(&core.paths, name)?;
    if !git::is_clean(&record.path) {
        return Err(problem(format!("The workspace {} has uncommitted changes: commit them first", record.name)));
    }
    git::merge(&record.parent_path, &record.branch, &record.base_branch)?;
    let mut notes = Vec::new();
    // The merge happened: a push that fails is a note, not a failure of the merge.
    if push {
        match git::push_origin(&record.parent_path, &record.base_branch) {
            Ok(()) => notes.push(format!("Pushed {} to origin", record.base_branch)),
            Err(e) => notes.push(format!("{} could not be pushed ({e})", record.base_branch)),
        }
    }
    if delete_after {
        notes.extend(delete(core, &record.site, false, false)?);
    }
    Ok((format!("Merged {} into {} of {}", record.name, record.base_branch, record.parent_site), notes))
}

/// The workspace `cwd` is in (the deepest), if any.
pub fn find_by_dir(paths: &HearthPaths, cwd: &Path) -> Option<Record> {
    let cwd = real(cwd);
    registry::load(paths).into_iter().filter(|r| within(&cwd, &real(&r.path))).max_by_key(|r| r.path.as_os_str().len())
}
