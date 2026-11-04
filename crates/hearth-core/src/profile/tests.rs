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
