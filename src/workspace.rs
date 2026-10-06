use std::io;
use std::path::{Path, PathBuf};

pub fn new_release(_src: &Path, _root: &Path) -> io::Result<PathBuf> {
    todo!()
}

pub fn promote(_root: &Path, _release: &Path) -> io::Result<()> {
    todo!()
}

pub fn gc(_root: &Path, _keep: usize) -> io::Result<()> {
    todo!()
}

pub fn checkout_replaced(_src: &Path) -> bool {
    todo!()
}
