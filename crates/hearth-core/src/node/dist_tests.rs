use super::dist::{hash_of, pick, Release};
use super::resolve::parse_spec;
use crate::php::manifest::{Arch, Os};

const INDEX: &str = r#"[
  {"version":"v22.12.0","lts":"Jod","files":["osx-arm64-tar","osx-x64-tar","win-x64-zip"]},
  {"version":"v20.11.1","lts":"Iron","files":["osx-arm64-tar","osx-x64-tar","win-x64-zip"]},
  {"version":"v20.11.0","lts":"Iron","files":["osx-arm64-tar","osx-x64-tar","win-x64-zip"]},
  {"version":"v19.9.0","lts":false,"files":["osx-arm64-tar","win-x64-zip"]},
  {"version":"v14.21.3","lts":"Fermium","files":["osx-x64-tar","win-x64-zip"]}
]"#;

fn release(version: &str, lts: Option<&str>) -> Option<Release> {
    Some(Release { version: version.into(), lts: lts.map(String::from) })
}

#[test]
fn pick_takes_the_newest_match_with_a_build_for_this_computer() {
    let p = |spec: &str, os, arch| pick(INDEX.as_bytes(), &parse_spec(spec).unwrap(), os, arch).unwrap();
    assert_eq!(p("20.11", Os::Windows, Arch::X64), release("20.11.1", Some("Iron")));
    assert_eq!(p("20.11.0", Os::Macos, Arch::Arm64), release("20.11.0", Some("Iron")));
    assert_eq!(p("22", Os::Macos, Arch::X86_64), release("22.12.0", Some("Jod")));
    // No Apple silicon build before Node 16: nothing, never another version.
    assert_eq!(p("14", Os::Macos, Arch::Arm64), None);
    assert_eq!(p("14", Os::Windows, Arch::X64), release("14.21.3", Some("Fermium")));
    assert_eq!(p("lts/iron", Os::Windows, Arch::X64), release("20.11.1", Some("Iron")));
    assert_eq!(p("19", Os::Windows, Arch::X64), release("19.9.0", None));
    assert_eq!(p("19", Os::Macos, Arch::X86_64), None);
    assert_eq!(p("9.9.9", Os::Windows, Arch::X64), None);
}

#[test]
fn hash_of_finds_the_file_line() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let shasums = format!("{a}  node-v20.11.1-win-x64.zip\n{b} *node-v20.11.1-darwin-arm64.tar.gz\n");
    assert_eq!(hash_of(&shasums, "node-v20.11.1-win-x64.zip"), Some(a));
    assert_eq!(hash_of(&shasums, "node-v20.11.1-darwin-arm64.tar.gz"), Some(b));
    assert_eq!(hash_of(&shasums, "node-v20.11.1-win-x64.7z"), None);
    assert_eq!(hash_of("not a list", "node-v20.11.1-win-x64.zip"), None);
}

#[test]
fn garbage_index_is_an_error_naming_nodejs_org() {
    let err = pick(b"<html>busy</html>", &parse_spec("20").unwrap(), Os::Windows, Arch::X64).unwrap_err();
    assert!(err.to_string().starts_with("nodejs.org's list of releases could not be read"), "{err}");
}
