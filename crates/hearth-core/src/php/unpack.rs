//! Unpacks a verified PHP archive next to its target and swaps it into place, so a PHP version is
//! either fully installed or not touched at all.

use std::fs::File;
use std::path::{Component, Path, PathBuf};

use crate::php::manifest::Format;
use crate::CoreError;

fn damaged() -> CoreError {
    CoreError::Php("The PHP download is damaged; try again".into())
}

fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

/// The archive path below `root`, refused when it escapes (absolute, `..`, drive prefix).
fn relative(path: &Path, root: &str) -> Result<Option<PathBuf>, CoreError> {
    if path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(damaged());
    }
    let inner = if root.is_empty() {
        path
    } else {
        match path.strip_prefix(root) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        }
    };
    Ok((inner.components().next().is_some()).then(|| inner.to_path_buf()))
}

fn extract_zip(archive: &Path, root: &str, dest: &Path) -> Result<(), CoreError> {
    let file = File::open(archive).map_err(|e| CoreError::io(archive, e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| damaged())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| damaged())?;
        let Some(rel) = relative(Path::new(entry.name()), root)? else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| CoreError::io(&out, e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
        }
        let mut f = File::create(&out).map_err(|e| CoreError::io(&out, e))?;
        std::io::copy(&mut entry, &mut f).map_err(|_| damaged())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o755))
                .map_err(|e| CoreError::io(&out, e))?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, root: &str, dest: &Path) -> Result<(), CoreError> {
    let file = File::open(archive).map_err(|e| CoreError::io(archive, e))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in tar.entries().map_err(|_| damaged())? {
        let mut entry = entry.map_err(|_| damaged())?;
        let kind = entry.header().entry_type();
        // PHP builds hold only files and folders; links could point outside the target.
        if !(kind.is_file() || kind.is_dir()) {
            return Err(damaged());
        }
        let path = entry.path().map_err(|_| damaged())?.into_owned();
        let Some(rel) = relative(&path, root)? else { continue };
        let out = dest.join(rel);
        if kind.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| CoreError::io(&out, e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
        }
        entry.unpack(&out).map_err(|_| damaged())?;
    }
    Ok(())
}

/// Unpacks `archive` into `<target>.new`, then swaps it for `target`. When the old install cannot be
/// moved (Windows locks a running php.exe), the old install stays and nothing new is left behind.
pub fn unpack(archive: &Path, format: Format, root: &str, target: &Path) -> Result<(), CoreError> {
    unpack_with(archive, format, root, target, |_| Ok(()))
}

/// Like `unpack`, with `prepare(new_dir)` run on the unpacked folder before the swap (extra files
/// that must be there before the install counts as complete). Its failure leaves the old install.
pub fn unpack_with(
    archive: &Path,
    format: Format,
    root: &str,
    target: &Path,
    prepare: impl FnOnce(&Path) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    let new = sibling(target, ".new");
    let old = sibling(target, ".old");
    let _ = std::fs::remove_dir_all(&new);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir_all(&new).map_err(|e| CoreError::io(&new, e))?;
    let extracted = match format {
        Format::Zip => extract_zip(archive, root, &new),
        Format::TarGz => extract_tar_gz(archive, root, &new),
    }
    .and_then(|()| prepare(&new));
    if let Err(e) = extracted {
        let _ = std::fs::remove_dir_all(&new);
        return Err(e);
    }
    if target.exists() && std::fs::rename(target, &old).is_err() {
        let _ = std::fs::remove_dir_all(&new);
        let minor = target.file_name().unwrap_or_default().to_string_lossy();
        return Err(CoreError::Php(format!("Close the programs using PHP {minor} and try again")));
    }
    if let Err(e) = crate::atomic::rename_dir(&new, target) {
        let _ = std::fs::rename(&old, target);
        let _ = std::fs::remove_dir_all(&new);
        return Err(CoreError::io(target, e));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        for (name, body) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap();
        buf.into_inner()
    }

    fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, body) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, name, *body).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    fn archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn zip_unpacks_into_the_target() {
        let d = tempfile::tempdir().unwrap();
        let a = archive(d.path(), "php.zip", &zip_with(&[("php.exe", b"exe"), ("ext/php_curl.dll", b"dll")]));
        let target = d.path().join("8.4");
        unpack(&a, Format::Zip, "", &target).unwrap();
        assert_eq!(std::fs::read(target.join("php.exe")).unwrap(), b"exe");
        assert!(target.join("ext").join("php_curl.dll").is_file());
        assert!(!d.path().join("8.4.new").exists());
    }

    #[test]
    fn tar_gz_root_is_stripped() {
        let d = tempfile::tempdir().unwrap();
        let a = archive(
            d.path(),
            "php.tar.gz",
            &tar_gz_with(&[("php-8.4.27/bin/php", b"bin"), ("php-8.4.27/lib/xdebug.so", b"so")]),
        );
        let target = d.path().join("8.4");
        unpack(&a, Format::TarGz, "php-8.4.27", &target).unwrap();
        assert_eq!(std::fs::read(target.join("bin").join("php")).unwrap(), b"bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(target.join("bin/php")).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    #[test]
    fn an_escaping_zip_entry_is_refused_and_nothing_is_written() {
        let d = tempfile::tempdir().unwrap();
        let a = archive(d.path(), "evil.zip", &zip_with(&[("php.exe", b"exe"), ("../evil.txt", b"x")]));
        let target = d.path().join("8.4");
        let err = unpack(&a, Format::Zip, "", &target).unwrap_err();
        assert!(err.to_string().contains("damaged"), "{err}");
        assert!(!d.path().join("evil.txt").exists());
        assert!(!target.exists());
        assert!(!d.path().join("8.4.new").exists());
    }

    #[test]
    fn an_escaping_tar_entry_is_refused() {
        let d = tempfile::tempdir().unwrap();
        // tar::Builder refuses ".." names, so the header is written by hand.
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../evil.txt\0");
        h.set_size(1);
        h.set_cksum();
        b.append(&h, &b"x"[..]).unwrap();
        let bytes = b.into_inner().unwrap().finish().unwrap();
        let a = archive(d.path(), "evil.tar.gz", &bytes);
        assert!(unpack(&a, Format::TarGz, "", &d.path().join("8.4")).is_err());
        assert!(!d.path().join("evil.txt").exists());
    }

    #[test]
    fn an_update_replaces_the_old_install() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("8.4");
        let first = archive(d.path(), "a.zip", &zip_with(&[("php.exe", b"old"), ("stale.dll", b"x")]));
        unpack(&first, Format::Zip, "", &target).unwrap();
        let second = archive(d.path(), "b.zip", &zip_with(&[("php.exe", b"new")]));
        unpack(&second, Format::Zip, "", &target).unwrap();
        assert_eq!(std::fs::read(target.join("php.exe")).unwrap(), b"new");
        assert!(!target.join("stale.dll").exists());
        assert!(!d.path().join("8.4.old").exists());
    }

    #[cfg(windows)]
    #[test]
    fn a_php_in_use_keeps_the_old_install() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("8.4");
        unpack(&archive(d.path(), "a.zip", &zip_with(&[("php.exe", b"old")])), Format::Zip, "", &target).unwrap();
        let running = std::fs::File::open(target.join("php.exe")).unwrap();
        let err = unpack(&archive(d.path(), "b.zip", &zip_with(&[("php.exe", b"new")])), Format::Zip, "", &target)
            .unwrap_err();
        assert_eq!(err.to_string(), "Close the programs using PHP 8.4 and try again");
        drop(running);
        assert_eq!(std::fs::read(target.join("php.exe")).unwrap(), b"old");
        assert!(!d.path().join("8.4.new").exists());
    }

    #[test]
    fn a_failing_prepare_keeps_the_old_install() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("8.4");
        unpack(&archive(d.path(), "a.zip", &zip_with(&[("php.exe", b"old")])), Format::Zip, "", &target).unwrap();
        let b = archive(d.path(), "b.zip", &zip_with(&[("php.exe", b"new")]));
        let err = unpack_with(&b, Format::Zip, "", &target, |_| Err(CoreError::Php("no redis".into()))).unwrap_err();
        assert_eq!(err.to_string(), "no redis");
        assert_eq!(std::fs::read(target.join("php.exe")).unwrap(), b"old");
        assert!(!d.path().join("8.4.new").exists());
    }

    #[test]
    fn a_unicode_target_works() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("Jörg Müller").join("8.4");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        unpack(&archive(d.path(), "a.zip", &zip_with(&[("php.exe", b"exe")])), Format::Zip, "", &target).unwrap();
        assert!(target.join("php.exe").is_file());
    }
}
