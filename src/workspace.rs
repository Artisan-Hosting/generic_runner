use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::linux::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Recursively copy `src` into a new release directory `root/releases/<unix_ts>`.
///
/// `.git` is excluded, executable bits are preserved, and symlinks are copied
/// as symlinks. Symlinks pointing outside `src` are never followed.
pub fn new_release(src: &Path, root: &Path) -> io::Result<PathBuf> {
    if !src.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "src does not exist"));
    }

    let releases = root.join("releases");
    fs::create_dir_all(&releases)?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let release = releases.join(timestamp.to_string());
    fs::create_dir_all(&release)?;

    copy_dir_recursive(src, &release)?;

    Ok(release)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;

        let name = entry.file_name();
        if name == ".git" {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(name);

        if file_type.is_symlink() {
            let target = fs::read_link(&src_path)?;
            symlink(&target, &dst_path)?;
        } else if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            let mut copy = fs::File::create(&dst_path)?;
            let mut reader = fs::File::open(&src_path)?;
            io::copy(&mut reader, &mut copy)?;

            let perms = entry.metadata()?.permissions();
            let mode = perms.mode();
            if mode & 0o111 != 0 {
                fs::set_permissions(&dst_path, fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// Atomically replace `root/current` so it points at `release`.
///
/// A temporary symlink is created and then renamed over `current`, so `current`
/// is always a valid symlink.
pub fn promote(root: &Path, release: &Path) -> io::Result<()> {
    let current = root.join("current");
    let releases = root.join("releases");
    let release = releases.join(release.file_name().unwrap_or_default());

    fs::create_dir_all(&releases)?;

    let temp_link = current.with_extension("tmp");
    if temp_link.exists() {
        fs::remove_file(&temp_link)?;
    }
    symlink(&release, &temp_link)?;

    if current.exists() || current.is_symlink() {
        fs::remove_file(&current)?;
    }
    fs::rename(&temp_link, &current)?;

    Ok(())
}

/// Delete release directories beyond `keep`, never the one `current` points to.
pub fn gc(root: &Path, keep: usize) -> io::Result<()> {
    let releases = root.join("releases");
    if !releases.exists() {
        return Ok(());
    }

    let current_target = current_release_target(&releases);

    let mut releases: Vec<PathBuf> = fs::read_dir(&releases)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    releases.sort();

    let len = releases.len();
    let to_delete = len.saturating_sub(keep);
    for release in releases.into_iter().take(to_delete) {
        if Some(release.as_path()) == current_target.as_deref() {
            continue;
        }
        fs::remove_dir_all(&release)?;
    }

    Ok(())
}

fn current_release_target(releases: &Path) -> Option<PathBuf> {
    let current = releases.parent()?.join("current");
    if current.is_symlink() {
        fs::read_link(&current).ok().and_then(|link| link.parent().map(Path::to_path_buf))
    } else {
        None
    }
}

/// Returns `true` if `src` has no `.git` or its directory was unlinked.
pub fn checkout_replaced(src: &Path) -> bool {
    if !src.join(".git").exists() {
        return true;
    }

    match fs::symlink_metadata(src) {
        Ok(meta) => meta.st_nlink() <= 1,
        Err(_) => true,
    }
}
