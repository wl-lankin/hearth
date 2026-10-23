use std::io::Write;
use std::path::Path;

use crate::CoreError;

/// Writes `bytes` to `<path>.tmp`, syncs it and renames it over `path`, so a crash never leaves a
/// half-written file behind.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    let result = (|| {
        let mut file = std::fs::File::create(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(tmp);
        return Err(CoreError::io(path, e));
    }
    Ok(())
}
