//! Where a project's web root is.

use std::path::{Path, PathBuf};

/// The folder to serve: `public/` for Laravel and most frameworks (it holds `index.php`), else the
/// project folder when it has an `index.php` or a static `index.html`.
pub fn docroot(project: &Path) -> Result<PathBuf, String> {
    let public = project.join("public");
    if public.join("index.php").is_file() {
        return Ok(public);
    }
    if project.join("index.php").is_file() || project.join("index.html").is_file() {
        return Ok(project.to_path_buf());
    }
    Err("No index.php or index.html found".into())
}
