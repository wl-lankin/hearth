use std::path::PathBuf;

use super::resolve::{find_spec, parse_spec, resolve, Spec, Why};
use super::store::Installed;

fn v(parts: &[u64]) -> Spec {
    Spec::Version(parts.to_vec())
}

#[test]
fn every_nvmrc_format_is_understood() {
    for (text, spec) in [
        ("22", v(&[22])),
        ("v22", v(&[22])),
        ("22.12", v(&[22, 12])),
        ("v22.12.0", v(&[22, 12, 0])),
        ("lts/*", Spec::Lts(None)),
        ("lts/jod", Spec::Lts(Some("jod".into()))),
        ("lts/Jod", Spec::Lts(Some("jod".into()))),
        ("node", Spec::Latest),
        ("latest", Spec::Latest),
        ("current", Spec::Latest),
        ("stable", Spec::Latest),
        ("system", Spec::Default),
        ("\u{feff}20.11.1\r\n", v(&[20, 11, 1])),
        ("  # pinned for the build\n\n  v18 # old\n", v(&[18])),
    ] {
        assert_eq!(parse_spec(text).unwrap(), spec, "{text:?}");
    }
    assert_eq!(parse_spec("  \r\n# only a comment\n").unwrap_err(), "names no Node version");
    assert_eq!(parse_spec("iojs").unwrap_err(), "asks for \"iojs\", which Hearth does not understand");
    assert_eq!(parse_spec("22.x").unwrap_err(), "asks for \"22.x\", which Hearth does not understand");
}

#[test]
fn the_nearest_file_wins_and_nvmrc_beats_node_version() {
    let d = tempfile::tempdir().unwrap();
    let project = d.path().join("project");
    let deep = project.join("src").join("app");
    std::fs::create_dir_all(&deep).unwrap();
    assert!(find_spec(&deep).is_none(), "no file up to the root");
    std::fs::write(d.path().join(".node-version"), "18").unwrap();
    std::fs::write(project.join(".node-version"), "20").unwrap();
    let found = find_spec(&deep).unwrap().unwrap();
    assert_eq!((found.spec, found.file.clone()), (v(&[20]), project.join(".node-version")));
    std::fs::write(project.join(".nvmrc"), "22\n").unwrap();
    assert_eq!(find_spec(&deep).unwrap().unwrap().file, project.join(".nvmrc"));
    std::fs::write(project.join(".nvmrc"), "").unwrap();
    let err = find_spec(&deep).unwrap().unwrap_err().to_string();
    assert_eq!(err, format!("{} names no Node version", project.join(".nvmrc").display()));
}

fn installed(list: &[(&str, Option<&str>)]) -> Vec<Installed> {
    list.iter()
        .map(|(v, lts)| Installed { version: v.to_string(), lts: lts.map(String::from), dir: PathBuf::from(v) })
        .collect()
}

#[test]
fn the_highest_installed_match_is_picked() {
    let list =
        installed(&[("23.11.1", None), ("22.21.1", Some("Jod")), ("22.12.0", Some("Jod")), ("20.19.5", Some("Iron"))]);
    let file = PathBuf::from("/p/.nvmrc");
    let pick = |spec: Spec| resolve(&list, Some("20.19.5"), Some((&spec, file.as_path()))).map(|c| c.version);
    assert_eq!(pick(v(&[22])).unwrap(), "22.21.1");
    assert_eq!(pick(v(&[22, 12])).unwrap(), "22.12.0");
    assert_eq!(pick(v(&[22, 12, 0])).unwrap(), "22.12.0");
    assert_eq!(pick(Spec::Lts(None)).unwrap(), "22.21.1");
    assert_eq!(pick(Spec::Lts(Some("iron".into()))).unwrap(), "20.19.5");
    assert_eq!(pick(Spec::Latest).unwrap(), "23.11.1");
    assert_eq!(pick(Spec::Default).unwrap(), "20.19.5");
    let choice = resolve(&list, Some("20.19.5"), Some((&v(&[22]), file.as_path()))).unwrap();
    assert_eq!(choice.why, Why::File(file.clone()));
    assert_eq!(choice.dir, PathBuf::from("22.21.1"));
}

#[test]
fn a_missing_version_is_an_error_never_another_version() {
    let list = installed(&[("22.21.1", Some("Jod"))]);
    let file = PathBuf::from("/p/.nvmrc");
    let err = |spec: Spec| resolve(&list, Some("22.21.1"), Some((&spec, file.as_path()))).unwrap_err().to_string();
    assert_eq!(
        err(v(&[20])),
        format!(
            "This project wants Node 20 ({}), which is not installed. Install it with: hearth node:install 20",
            file.display()
        )
    );
    assert_eq!(
        err(v(&[22, 12])),
        format!(
            "This project wants Node 22.12 ({}), which is not installed. Hearth installs the newest Node 22 only: hearth node:install 22, then put 22 in the file",
            file.display()
        )
    );
    assert_eq!(
        err(Spec::Lts(Some("iron".into()))),
        format!("This project wants Node lts/iron ({}), which is not installed. Install it with: hearth node:install lts/iron", file.display())
    );
}

#[test]
fn without_a_file_the_default_is_used() {
    let list = installed(&[("22.21.1", Some("Jod")), ("20.19.5", Some("Iron"))]);
    let choice = resolve(&list, Some("20.19.5"), None).unwrap();
    assert_eq!((choice.version.as_str(), choice.why), ("20.19.5", Why::Default));
    // A default that is gone (should not happen): the newest version, not an error.
    assert_eq!(resolve(&list, None, None).unwrap().version, "22.21.1");
    assert_eq!(resolve(&[], None, None).unwrap_err().to_string(), "No Node version is installed in Hearth");
    let file = PathBuf::from("/p/.nvmrc");
    assert_eq!(
        resolve(&[], None, Some((&Spec::Default, file.as_path()))).unwrap_err().to_string(),
        "No Node version is installed in Hearth"
    );
}

#[test]
fn a_utf16_file_from_windows_powershell_is_read_and_garbage_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    // `echo 20 > .nvmrc` in Windows PowerShell 5.1 writes UTF-16LE with a BOM.
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend("20\r\n".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(d.path().join(".nvmrc"), &bytes).unwrap();
    assert_eq!(find_spec(d.path()).unwrap().unwrap().spec, v(&[20]));
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend("22".encode_utf16().flat_map(u16::to_be_bytes));
    std::fs::write(d.path().join(".nvmrc"), &bytes).unwrap();
    assert_eq!(find_spec(d.path()).unwrap().unwrap().spec, v(&[22]));
    std::fs::write(d.path().join(".nvmrc"), [0xc3, 0x28, 0x32]).unwrap();
    let err = find_spec(d.path()).unwrap().unwrap_err().to_string();
    assert_eq!(err, format!("{} is not a text file Hearth can read", d.path().join(".nvmrc").display()));
}
