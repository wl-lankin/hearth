use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::real::without_legacy_block;
use super::*;
use crate::StokePaths;

#[derive(Default)]
struct FakeHost {
    stopped: Vec<PathBuf>,
    linked: Vec<(PathBuf, PathBuf)>,
    app: Option<PathBuf>,
}

impl Host for FakeHost {
    fn stop_programs(&mut self, folders: &[PathBuf]) {
        self.stopped.extend_from_slice(folders);
    }
    fn link(&mut self, link: &Path, target: &Path) -> Result<(), CoreError> {
        self.linked.push((link.to_path_buf(), target.to_path_buf()));
        Ok(())
    }
    fn hearth_app(&self) -> Option<PathBuf> {
        self.app.clone()
    }
}

struct Home {
    _dir: tempfile::TempDir,
    legacy: PathBuf,
    paths: StokePaths,
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// A Hearth home as 1.0.x leaves it, with a workspace checkout next to it.
fn hearth_home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(".config").join("hearth");
    let paths = StokePaths::at(dir.path().join(".config").join("stoke"));
    let old = StokePaths::at(&legacy);
    write(&old.data.join("php").join("versions").join("8.4").join(".hearth-build.json"), "{}");
    write(&old.data.join("php").join("versions").join("8.4").join(".hearth-version"), "8.4.27");
    write(&old.data.join("node").join("v22.12.0").join(".hearth-build.json"), "{}");
    let adminer = old.data.join("adminer");
    let sites = json!({ "links": { "database.hearth": adminer, "shop": "/work/shop" }, "parked": [] });
    write(&old.config.join("sites.json"), &sites.to_string());
    let cli = old.data.join("services").join("bin").join("redis").join("redis-cli.exe");
    write(&old.config.join("forwarders.json"), &json!([["redis-cli", cli]]).to_string());
    let checkout = dir.path().join("shop-feature");
    write(&checkout.join(".hearth-workspace.json"), "{}");
    write(&checkout.join(".git").join("info").join("exclude"), "vendor\n.hearth-workspace.json\n");
    write(&old.config.join("workspaces.json"), &json!([{ "name": "feature", "path": checkout }]).to_string());
    write(&old.bin.join("hearth.exe"), "cli");
    write(&old.bin.join("old-tool.bat"), "@echo off\r\nrem hearth-shim\r\n\"x\" %*\r\n");
    write(&old.bin.join("composer.phar"), "<?php");
    write(&old.run.join("hearth.lock"), "1");
    Home { _dir: dir, legacy, paths }
}

#[test]
fn hearths_home_is_due_only_as_a_real_folder_while_stoke_has_none() {
    let home = hearth_home();
    assert!(due(&home.legacy, &home.paths.root));
    std::fs::create_dir_all(&home.paths.root).unwrap();
    assert!(!due(&home.legacy, &home.paths.root), "Stoke already has a home");
    assert!(!due(&home.paths.root.join("nothing"), &home.paths.root.join("other")), "no Hearth home");
}

#[cfg(unix)]
#[test]
fn the_link_a_move_leaves_is_not_moved_again() {
    let dir = tempfile::tempdir().unwrap();
    let (legacy, root) = (dir.path().join("hearth"), dir.path().join("stoke"));
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&root, &legacy).unwrap();
    std::fs::remove_dir(&root).unwrap();
    assert!(!due(&legacy, &root), "a link (even a dangling one) is never moved");
}

#[test]
fn paths_inside_the_old_folder_move_and_others_stay() {
    let (legacy, root) = (Path::new("/u/.config/hearth"), Path::new("/u/.config/stoke"));
    assert_eq!(moved("/u/.config/hearth/bin", legacy, root).as_deref(), Some("/u/.config/stoke/bin"));
    assert_eq!(moved("/u/.config/hearth", legacy, root).as_deref(), Some("/u/.config/stoke"));
    assert_eq!(moved("/u/.config/hearth2/bin", legacy, root), None, "only the folder itself");
    assert_eq!(moved("/work/shop", legacy, root), None);
}

#[cfg(windows)]
#[test]
fn windows_paths_move_without_regard_to_case_or_slash() {
    let (legacy, root) = (Path::new(r"C:\Users\x\.config\hearth"), Path::new(r"C:\Users\x\.config\stoke"));
    assert_eq!(moved(r"c:\users\X\.Config\Hearth\bin", legacy, root).as_deref(), Some(r"C:\Users\x\.config\stoke\bin"));
    assert_eq!(moved("C:/Users/x/.config/hearth/bin", legacy, root).as_deref(), Some(r"C:\Users\x\.config\stoke/bin"));
}

#[test]
fn json_strings_and_keys_inside_the_old_folder_move() {
    let (legacy, root) = (Path::new("/h"), Path::new("/s"));
    let mut value = json!({ "/h/a": ["/h/b", "/x", 1], "c": { "d": "/h" } });
    assert!(rewrite_paths(&mut value, legacy, root));
    assert_eq!(value, json!({ "/s/a": ["/s/b", "/x", 1], "c": { "d": "/s" } }));
    assert!(!rewrite_paths(&mut value, legacy, root), "nothing left to move");
}

#[test]
fn the_move_renames_and_rewrites_what_names_hearth() {
    let home = hearth_home();
    let mut host = FakeHost { app: Some(PathBuf::from("/apps/Hearth")), ..FakeHost::default() };
    move_home(&home.legacy, &home.paths, &mut host).unwrap();
    let p = &home.paths;

    assert!(!home.legacy.exists() && p.root.is_dir(), "the folder was moved");
    assert_eq!(host.stopped, vec![home.legacy.clone(), PathBuf::from("/apps/Hearth")]);
    assert_eq!(host.linked, vec![(home.legacy.clone(), p.root.clone())]);

    let php = p.data.join("php").join("versions").join("8.4");
    assert!(php.join(".stoke-build.json").is_file() && php.join(".stoke-version").is_file());
    assert!(!php.join(".hearth-build.json").exists());
    assert!(p.data.join("node").join("v22.12.0").join(".stoke-build.json").is_file());

    let sites: Value = serde_json::from_str(&read(&p.config.join("sites.json"))).unwrap();
    assert_eq!(sites["links"]["database.hearth"], json!(p.data.join("adminer")), "the site keeps its name");
    assert_eq!(sites["links"]["shop"], json!("/work/shop"));
    let forwarders = read(&p.config.join("forwarders.json"));
    assert!(!forwarders.contains("hearth") && forwarders.contains("stoke"), "{forwarders}");

    let checkout = home.legacy.parent().unwrap().parent().unwrap().join("shop-feature");
    assert!(checkout.join(".stoke-workspace.json").is_file() && !checkout.join(".hearth-workspace.json").exists());
    assert_eq!(read(&checkout.join(".git").join("info").join("exclude")), "vendor\n.stoke-workspace.json\n");

    assert!(!p.bin.join("hearth.exe").exists(), "Hearth's CLI goes");
    assert!(!p.bin.join("old-tool.bat").exists(), "Hearth's shims are recognized as Stoke's own");
    assert!(p.bin.join("composer.phar").is_file());
    assert!(!p.run.join("hearth.lock").exists());

    let record = Record::load(p).unwrap();
    assert!(record.home && record.finished.is_empty() && record.pending());
    assert_eq!(record.legacy, home.legacy);
}

#[test]
fn a_move_cut_short_after_the_folder_moved_finishes_on_the_next_start() {
    let home = hearth_home();
    std::fs::rename(&home.legacy, &home.paths.root).unwrap();
    Record { legacy: home.legacy.clone(), ..Record::default() }.save(&home.paths).unwrap();
    let mut host = FakeHost::default();
    move_home(&home.legacy, &home.paths, &mut host).unwrap();
    assert!(host.stopped.is_empty() && host.linked.is_empty(), "the folder is not moved twice");
    assert!(home.paths.data.join("php").join("versions").join("8.4").join(".stoke-version").is_file());
    assert!(Record::load(&home.paths).unwrap().home);
}

#[test]
fn without_hearth_nothing_happens() {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    move_home(&dir.path().join("hearth"), &paths, &mut FakeHost::default()).unwrap();
    assert!(Record::load(&paths).is_none() && !paths.root.exists());
}

#[derive(Default)]
struct FakeOutside {
    user_path: Option<String>,
    claude: Option<Value>,
    claude_calls: Vec<Vec<String>>,
    files: Vec<PathBuf>,
    helper_installed: bool,
    helper_fails: bool,
    calls: Vec<&'static str>,
}

impl Outside for FakeOutside {
    fn user_path(&self) -> Option<String> {
        self.user_path.clone()
    }
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError> {
        self.user_path = Some(value.to_string());
        Ok(())
    }
    fn move_profiles(&mut self, _: &StokePaths) -> Result<(), CoreError> {
        self.calls.push("profiles");
        Ok(())
    }
    fn remove_login_entry(&mut self) -> Result<(), CoreError> {
        self.calls.push("login");
        Ok(())
    }
    fn claude_servers(&self) -> Option<Value> {
        self.claude.clone()
    }
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError> {
        self.claude_calls.push(args.to_vec());
        Ok(())
    }
    fn mcp_files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
    fn hearth_helper(&self) -> bool {
        self.helper_installed
    }
    fn setup_helper(&mut self) -> Result<(), CoreError> {
        self.calls.push("helper");
        if self.helper_fails {
            return Err(CoreError::Php("The user said no".into()));
        }
        self.helper_installed = false;
        Ok(())
    }
    fn uninstall_hearth(&mut self) -> Result<(), CoreError> {
        self.calls.push("app");
        Ok(())
    }
}

fn moved_home() -> Home {
    let home = hearth_home();
    move_home(&home.legacy, &home.paths, &mut FakeHost::default()).unwrap();
    home
}

#[test]
fn the_steps_after_the_start_move_path_mcp_helper_and_app() {
    let home = moved_home();
    let (old_bin, new_bin) = (home.legacy.join("bin"), home.paths.bin.clone());
    let cursor = home.paths.root.join("cursor-mcp.json");
    let cli = |bin: &Path, name: &str| bin.join(name).to_string_lossy().into_owned();
    write(
        &cursor,
        &json!({ "mcpServers": { "hearth": { "command": cli(&old_bin, "hearth.exe"), "args": ["mcp"] }, "other": { "command": "x" } } })
            .to_string(),
    );
    let mut outside = FakeOutside {
        user_path: Some(format!("{};C:\\Tools", old_bin.display())),
        claude: Some(
            json!({ "herd": { "type": "stdio", "command": cli(&old_bin, "hearth.exe"), "args": ["mcp"] }, "x": { "command": "y" } }),
        ),
        files: vec![cursor.clone()],
        helper_installed: true,
        ..FakeOutside::default()
    };
    let record = finish(&home.paths, &mut outside).unwrap();

    assert!(!record.pending(), "{:?}", record.errors);
    assert_eq!(outside.user_path.unwrap(), format!("{};C:\\Tools", new_bin.display()));
    assert_eq!(outside.calls, ["profiles", "login", "helper", "app"]);
    let added = outside.claude_calls.iter().find(|c| c[1] == "add-json").unwrap();
    assert_eq!(&added[..5], ["mcp", "add-json", "--scope", "user", "herd"]);
    let entry: Value = serde_json::from_str(&added[5]).unwrap();
    assert_eq!(entry["command"], json!(cli(&new_bin, "stoke.exe")));
    assert_eq!(outside.claude_calls.len(), 2, "only Hearth's entry is touched: {:?}", outside.claude_calls);
    let cursor: Value = serde_json::from_str(&read(&cursor)).unwrap();
    assert_eq!(cursor["mcpServers"]["stoke"]["command"], json!(cli(&new_bin, "stoke.exe")));
    assert!(cursor["mcpServers"].get("hearth").is_none());
    assert_eq!(cursor["mcpServers"]["other"]["command"], "x");
}

#[test]
fn a_failed_step_is_noted_and_runs_again_on_the_next_start() {
    let home = moved_home();
    let mut outside = FakeOutside { helper_installed: true, helper_fails: true, ..FakeOutside::default() };
    let record = finish(&home.paths, &mut outside).unwrap();
    assert_eq!(record.errors.get("helper").map(String::as_str), Some("The user said no"));
    assert!(record.finished("app"), "the other steps still ran");
    assert!(record.pending());

    let mut again = FakeOutside { helper_installed: true, ..FakeOutside::default() };
    let record = finish(&home.paths, &mut again).unwrap();
    assert_eq!(again.calls, ["helper"], "only what was left");
    assert!(!record.pending() && record.errors.is_empty());
}

#[test]
fn without_hearths_helper_none_is_set_up() {
    let home = moved_home();
    let mut outside = FakeOutside::default();
    assert!(!finish(&home.paths, &mut outside).unwrap().pending());
    assert!(!outside.calls.contains(&"helper"));
}

#[test]
fn the_path_keeps_its_order_and_holds_stokes_bin_once() {
    let (legacy, root) = (Path::new("/h"), Path::new("/s"));
    assert_eq!(moved_path("/a;/h/bin;/b;/s/bin;", legacy, root), "/a;/s/bin;/b");
}

#[test]
fn hearths_profile_block_goes_and_the_rest_stays() {
    let profile = "export A=1\n\n# >>> hearth >>>\nexport PATH=\"$HOME/.config/hearth/bin:$PATH\"\n# <<< hearth <<<\nexport B=2\n";
    assert_eq!(without_legacy_block(profile), "export A=1\nexport B=2\n");
    assert_eq!(without_legacy_block("x\n"), "x\n");
}
