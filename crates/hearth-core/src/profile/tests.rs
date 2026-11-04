use crate::profile::{data_dir, spx_key};
use crate::HearthPaths;

#[test]
fn the_spx_key_is_made_once_and_kept() {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path());
    paths.ensure().unwrap();
    let key = spx_key(&paths).unwrap();
    assert_eq!(key.len(), 32);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "{key}");
    assert_eq!(spx_key(&paths).unwrap(), key);
    assert_eq!(data_dir(&paths), paths.data.join("spx"));
}

/// The key goes into the Caddyfile unquoted: a hand-edited one that is not 32 hex is replaced.
#[test]
fn a_key_that_is_not_hex_is_replaced() {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path());
    paths.ensure().unwrap();
    std::fs::write(paths.config.join("spx.json"), r#"{"key":"0123456789abcdef0123456789abc{ }"}"#).unwrap();
    let key = spx_key(&paths).unwrap();
    assert!(key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit()), "{key}");
}

/// Two first calls at once (status and php.ini) end with the same key.
#[test]
fn first_calls_at_the_same_time_agree_on_one_key() {
    for _ in 0..20 {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path());
        paths.ensure().unwrap();
        let keys: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..4).map(|_| s.spawn(|| spx_key(&paths).unwrap())).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(keys.iter().all(|k| *k == keys[0]), "{keys:?}");
        assert_eq!(spx_key(&paths).unwrap(), keys[0]);
    }
}
