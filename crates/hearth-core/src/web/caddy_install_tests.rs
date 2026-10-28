use super::caddy_install::ensure_caddy;
use crate::php::service_tests::fixture;

#[tokio::test(flavor = "multi_thread")]
async fn caddy_is_installed_once_and_reused() {
    let f = fixture(false);
    let first = ensure_caddy(&f.core).await.unwrap();
    assert!(first.is_file(), "{}", first.display());
    assert!(first.ends_with(if cfg!(windows) { "caddy.exe" } else { "caddy" }));
    assert!(first.starts_with(f.core.paths.data.join("caddy").join("2.11.7")));
    let second = ensure_caddy(&f.core).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(f.hits.lock().unwrap().get("/caddy").copied(), Some(1), "downloaded only once");
    // Every web server sync asks for Caddy: once it is installed, no catalog request per sync.
    assert_eq!(f.hits.lock().unwrap().get("/manifest.json").copied(), Some(1), "catalog read only once");
}

#[test]
fn a_catalog_without_caddy_is_reported() {
    let m = crate::php::manifest::Manifest {
        schema: 1,
        generated: "x".into(),
        php: vec![],
        composer: crate::php::manifest::Artifact { url: "u".into(), sha256: "s".into(), version: None },
        cacert: crate::php::manifest::Artifact { url: "u".into(), sha256: "s".into(), version: None },
        tools: Default::default(),
        services: vec![],
    };
    let err = super::caddy_install::pick_caddy(&m).unwrap_err();
    assert_eq!(err.to_string(), "This catalog has no web server for this computer");
}
