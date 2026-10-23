use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::CoreError;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Renames `from` over `to`. Windows refuses for a moment while another writer replaces the same
/// file, so a refused rename is retried briefly.
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

/// Writes `bytes` to a temp file next to `path` (unique per writer, so concurrent writers never
/// share one), syncs it and renames it over `path`: a crash never leaves a half-written file.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let tmp = Path::new(&tmp);
    let result = (|| {
        let mut file = std::fs::File::create(tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace(tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(tmp);
        return Err(CoreError::io(path, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn concurrent_writers_of_one_file_never_fail() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("php.json");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..25 {
                        super::write(&path, format!("{i}-{j}").as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let leftovers = std::fs::read_dir(d.path()).unwrap().count();
        assert_eq!(leftovers, 1, "only the file itself remains");
    }
}
