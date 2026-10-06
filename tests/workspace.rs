mod workspace {
    use std::fs;
    use std::io;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use ais_runner::workspace;

    fn temp() -> PathBuf {
        tempfile::tempdir().unwrap().keep()
    }

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    // case T01 begin
    #[test]
    fn case_T01() {
        let root = temp();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        write(&src.join("hello.txt"), "hello world");
        let dst_root = temp();
        let release = workspace::new_release(&src, &dst_root).unwrap();
        let releases = dst_root.join("releases");
        assert_eq!(release.parent().unwrap(), releases);
        assert_eq!(releases.read_dir().unwrap().count(), 1);
        assert_eq!(release.file_name().unwrap().to_string_lossy().parse::<u64>().is_ok(), true);
        assert_eq!(fs::read_to_string(release.join("hello.txt")).unwrap(), "hello world");
    }
    // case T01 end

    // case T02 begin
    #[test]
    fn case_T02() {
        let root = temp();
        let src = root.join("src");
        write(&src.join(".git/HEAD"), "ref: refs/heads/main");
        write(&src.join("main.rs"), "fn main() {}");
        let dst_root = temp();
        let release = workspace::new_release(&src, &dst_root).unwrap();
        assert!(release.join("main.rs").exists());
        assert!(!release.join(".git").exists());
    }
    // case T02 end


    // case T09 begin
    #[test]
    fn case_T09() {
        let root = temp();
        let src = root.join("nonexistent");
        let dst_root = temp();
        let result = workspace::new_release(&src, &dst_root);
        assert!(result.is_err());
    }
    // case T09 end

    // case T11 begin
    #[test]
    fn case_T11() {
        let root = temp();
        let src = root.join("src");
        write(&src.join("hello.txt"), "hi");
        let dst_root = temp();
        let release = workspace::new_release(&src, &dst_root).unwrap();
        workspace::promote(&dst_root, &release).unwrap();
        let current = dst_root.join("current");
        let meta = fs::symlink_metadata(&current).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(fs::read_link(&current).unwrap(), release);
        assert!(current.join("hello.txt").exists());
    }
    // case T11 end




    // case T17 begin
    #[test]
    fn case_T17() {
        let root = temp();
        let releases = root.join("releases");
        for name in ["1000000001", "1000000002", "1000000003"] {
            fs::create_dir_all(releases.join(name)).unwrap();
        }
        write(&root.join("notes.txt"), "notes");
        fs::create_dir_all(root.join("other")).unwrap();
        workspace::gc(&root, 2).unwrap();
        assert_eq!(fs::read_to_string(root.join("notes.txt")).unwrap(), "notes");
        assert!(root.join("other").is_dir());
    }
    // case T17 end


    // case T19 begin
    #[test]
    fn case_T19() {
        let root = temp();
        let src = root.join("src");
        write(&src.join("main.rs"), "fn main() {}");
        assert_eq!(workspace::checkout_replaced(&src), true);
    }
    // case T19 end



    // case T24 begin
    #[test]
    fn case_T24() {
        let root = temp();
        let src = root.join("src");
        fs::create_dir_all(src.join("empty_dir")).unwrap();
        let dst_root = temp();
        let release = workspace::new_release(&src, &dst_root).unwrap();
        assert!(release.join("empty_dir").is_dir());
    }
    // case T24 end

    // case g1_A7_3 begin
    #[test]
    fn case_g1_A7_3() {
        use std::io;
        use std::path::{Path, PathBuf};
        let new_release_f: fn(&Path, &Path) -> io::Result<PathBuf> =
            ais_runner::workspace::new_release;
        let promote_f: fn(&Path, &Path) -> io::Result<()> = ais_runner::workspace::promote;
        let gc_f: fn(&Path, usize) -> io::Result<()> = ais_runner::workspace::gc;
        let checkout_replaced_f: fn(&Path) -> bool = ais_runner::workspace::checkout_replaced;

        // Exercise the first function so the test fails while it is unimplemented.
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("sub/f.txt"), b"x").unwrap();
        let root = tempfile::tempdir().unwrap();
        let release = new_release_f(src.path(), root.path()).unwrap();
        assert!(release.exists(), "new_release must create the release directory");
        assert!(release.join("sub/f.txt").exists(), "copied files must exist");
        let _ = (promote_f, gc_f, checkout_replaced_f);
    }
    // case g1_A7_3 end


    fn unix_symlink(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(not(unix))]
        let _ = (target, link);
    }

    // exec-bit and symlink preservation checks (part of A3 coverage)
    #[test]
    fn case_T02_exec_bit_and_symlinks() {
        let root = temp();
        let src = root.join("src");
        let outside = root.join("outside.txt");
        write(&outside, "outside");
        write(&src.join("run.sh"), "#!/bin/sh\necho hi");
        fs::set_permissions(src.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, src.join("link_out")).unwrap();
        let dst_root = temp();
        let release = workspace::new_release(&src, &dst_root).unwrap();
        let mode = fs::metadata(release.join("run.sh")).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0);
        #[cfg(unix)]
        {
            let link_meta = fs::symlink_metadata(release.join("link_out")).unwrap();
            assert!(link_meta.file_type().is_symlink());
            assert_eq!(fs::read_link(release.join("link_out")).unwrap(), outside);
        }
    }
}
