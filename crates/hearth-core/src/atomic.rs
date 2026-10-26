use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::CoreError;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Renames `from` over `to`. Windows refuses for a moment while another writer replaces the same
/// file, so a refused rename is retried briefly.
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_retrying(from, to, 20, Duration::from_millis(10))
}

/// Renames the folder `from` to `to`. Right after unpacking, an antivirus scan holds the new
/// executables open for a moment and Windows refuses the rename, so it is retried for up to 5 s.
pub fn rename_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_retrying(from, to, 50, Duration::from_millis(100))
}

fn rename_retrying(from: &Path, to: &Path, attempts: u32, pause: Duration) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempt < attempts => {
                attempt += 1;
                std::thread::sleep(pause);
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
    #[cfg(windows)]
    #[test]
    fn a_folder_rename_waits_for_a_scanner_to_let_go() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("8.4.new");
        std::fs::create_dir(&from).unwrap();
        std::fs::write(from.join("php.exe"), b"x").unwrap();
        // Like an antivirus scan: open without FILE_SHARE_DELETE, so the folder cannot be renamed.
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(from.join("php.exe")).unwrap();
        let to = d.path().join("8.4");
        assert!(std::fs::rename(&from, &to).is_err(), "the held file blocks a plain rename");
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(held);
        });
        super::rename_dir(&from, &to).unwrap();
        release.join().unwrap();
        assert!(to.join("php.exe").exists());
    }

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
