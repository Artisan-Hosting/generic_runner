use std::fs;
use std::io;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn new_release(src: &Path, root: &Path) -> io::Result<PathBuf> {
    let releases_dir = root.join("releases");
    fs::create_dir_all(&releases_dir)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        .as_secs();

    let mut release_path = releases_dir.join(format!("{}", now));
    for i in 0..1_000_000 {
        if !release_path.exists() {
            break;
        }
        release_path = releases_dir.join(format!("{}", now + i));
    }

    fs::create_dir_all(&release_path)?;
    copy_dir_contents(src, &release_path)?;
    Ok(release_path)
}

fn copy_dir_contents(src: &Path, dest: &Path) -> io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(name);
        let meta = fs::symlink_metadata(&src_path)?;
        let file_type = meta.file_type();

        if file_type.is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_dir_contents(&src_path, &dest_path)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&src_path)?;
            if let Err(_) = symlink(&target, &dest_path) {
                continue;
            }
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path)?;
            let perms = meta.permissions();
            fs::set_permissions(&dest_path, perms)?;
        }
    }
    Ok(())
}

pub fn promote(root: &Path, release: &Path) -> io::Result<()> {
    let current = root.join("current");
    let temp = root.join(format!(".current.tmp.{}", std::process::id()));

    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    symlink(release, &temp)?;

    if current.exists() {
        fs::remove_file(&current)?;
    }
    fs::rename(&temp, &current)?;
    Ok(())
}

pub fn gc(root: &Path, keep: usize) -> io::Result<()> {
    let releases_dir = root.join("releases");
    if !releases_dir.exists() {
        return Ok(());
    }

    let mut releases: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&releases_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            releases.push(path);
        }
    }

    releases.sort();

    let current_path = root.join("current");
    let current_target = if current_path.exists() {
        fs::read_link(&current_path).ok()
    } else {
        None
    };

    // Protect the current release and the newest releases.
    let newest: Vec<PathBuf> = releases.iter().rev().take(keep).cloned().collect();

    let mut to_delete = Vec::new();
    for release in &releases {
        if Some(release) == current_target.as_ref() || newest.contains(release) {
            continue;
        }
        to_delete.push(release.clone());
    }

    for release in to_delete {
        fs::remove_dir_all(release)?;
    }

    Ok(())
}

pub fn checkout_replaced(src: &Path) -> bool {
    if !src.exists() {
        return true;
    }

    let meta = match fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(_) => return true,
    };

    if meta.file_type().is_symlink() {
        if meta.nlink() == 0 {
            return true;
        }
    }

    let git_path = src.join(".git");
    !git_path.exists()
}
