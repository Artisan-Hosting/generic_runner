mod workspace {
    use ais_runner::workspace::{checkout_replaced, gc, new_release, promote};
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// A sorted listing of everything under `dir`: relative path, kind, size, mode.
    fn listing(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for e in fs::read_dir(dir).unwrap() {
                let e = e.unwrap();
                let meta = fs::symlink_metadata(e.path()).unwrap();
                out.push(format!("{} {} {} {:o}", e.path().strip_prefix(base).unwrap().display(), meta.file_type().is_dir(), meta.len(), meta.permissions().mode() & 0o7777));
                if meta.file_type().is_dir() {
                    walk(base, &e.path(), out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let root = tmp.path().join("root");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&root).unwrap();
        (tmp, src, root)
    }

    fn release_dirs(root: &Path) -> Vec<String> {
        let mut names: Vec<String> = match fs::read_dir(root.join("releases")) {
            Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    #[test]
    fn case_a1_release_is_a_timestamped_copy_of_every_file() {
        let (_tmp, src, root) = setup();
        write(&src.join("a.txt"), "alpha");
        write(&src.join("deep/er/b.txt"), "beta");
        let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let release = new_release(&src, &root).unwrap();
        let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(release.parent().unwrap(), root.join("releases"));
        let n: u64 = release.file_name().unwrap().to_str().unwrap().parse().expect("release name is a unix time");
        assert!(n + 5 >= before && n <= after + 5, "{n} not near {before}..{after}");
        assert!(release.is_dir());
        assert_eq!(fs::read_to_string(release.join("a.txt")).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(release.join("deep/er/b.txt")).unwrap(), "beta");
    }

    #[test]
    fn case_a2_git_directory_is_not_copied() {
        let (_tmp, src, root) = setup();
        write(&src.join(".git/HEAD"), "ref: refs/heads/main");
        write(&src.join("keep.txt"), "x");
        let release = new_release(&src, &root).unwrap();
        assert!(release.join("keep.txt").is_file());
        assert!(!release.join(".git").exists());
    }

    #[test]
    fn case_a3_executable_bit_is_preserved() {
        let (_tmp, src, root) = setup();
        write(&src.join("run.sh"), "#!/bin/sh\n");
        write(&src.join("data.txt"), "d");
        fs::set_permissions(src.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(src.join("data.txt"), fs::Permissions::from_mode(0o644)).unwrap();
        let release = new_release(&src, &root).unwrap();
        assert_eq!(mode(&release.join("run.sh")), 0o755);
        assert_eq!(mode(&release.join("data.txt")), 0o644);
    }

    #[test]
    fn case_a4_symlinks_stay_symlinks_and_outside_targets_are_not_copied() {
        let (tmp, src, root) = setup();
        write(&src.join("real.txt"), "real");
        symlink("real.txt", src.join("inside")).unwrap();
        symlink("/etc/hostname", src.join("host")).unwrap();
        let outside = tmp.path().join("outside");
        write(&outside.join("secret"), "TOP-SECRET-CONTENTS");
        symlink(&outside, src.join("out")).unwrap();
        let release = new_release(&src, &root).unwrap();
        let inside = release.join("inside");
        assert!(fs::symlink_metadata(&inside).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&inside).unwrap(), PathBuf::from("real.txt"));
        for name in ["host", "out"] {
            assert!(fs::symlink_metadata(release.join(name)).unwrap().file_type().is_symlink(), "{name} must stay a symlink");
        }
        assert_eq!(fs::read_link(release.join("host")).unwrap(), PathBuf::from("/etc/hostname"));
        assert_eq!(fs::read_link(release.join("out")).unwrap(), outside);
        fn holds_secret(dir: &Path) -> bool {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let meta = fs::symlink_metadata(&p).unwrap();
                if meta.file_type().is_dir() {
                    if holds_secret(&p) {
                        return true;
                    }
                } else if meta.file_type().is_file() && fs::read_to_string(&p).map(|t| t.contains("TOP-SECRET-CONTENTS")).unwrap_or(false) {
                    return true;
                }
            }
            false
        }
        assert!(!holds_secret(&release), "the release holds a regular copy of the outside file");
    }

    #[test]
    fn case_a5_the_source_is_not_changed() {
        let (_tmp, src, root) = setup();
        write(&src.join("a.txt"), "alpha");
        write(&src.join("dir/run.sh"), "#!/bin/sh\n");
        fs::set_permissions(src.join("dir/run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("a.txt", src.join("link")).unwrap();
        let before = listing(&src);
        new_release(&src, &root).unwrap();
        assert_eq!(listing(&src), before);
    }

    #[test]
    fn case_a6_two_releases_in_one_second_do_not_collide() {
        let (_tmp, src, root) = setup();
        write(&src.join("a.txt"), "alpha");
        let first = new_release(&src, &root).unwrap();
        let second = new_release(&src, &root).unwrap();
        assert_ne!(first, second);
        assert!(first.is_dir() && second.is_dir());
        assert_eq!(fs::read_to_string(first.join("a.txt")).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(second.join("a.txt")).unwrap(), "alpha");
    }

    #[test]
    fn case_a7_promote_switches_current_atomically_and_leaves_nothing_behind() {
        let (_tmp, _src, root) = setup();
        let one = root.join("releases/1000");
        let two = root.join("releases/2000");
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        promote(&root, &one).unwrap();
        assert_eq!(fs::read_link(root.join("current")).unwrap(), one);
        promote(&root, &two).unwrap();
        assert_eq!(fs::read_link(root.join("current")).unwrap(), two);
        let mut names: Vec<String> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["current".to_string(), "releases".to_string()], "a temporary link was left in root");
    }

    #[test]
    fn case_a8_gc_keeps_the_newest_and_the_current_release() {
        let (_tmp, _src, root) = setup();
        for n in ["1000", "2000", "3000", "4000"] {
            fs::create_dir_all(root.join("releases").join(n)).unwrap();
        }
        symlink(root.join("releases/2000"), root.join("current")).unwrap();
        gc(&root, 2).unwrap();
        assert_eq!(release_dirs(&root), vec!["2000", "3000", "4000"]);

        let (_tmp2, _src2, root2) = setup();
        for n in ["1000", "2000", "3000", "4000"] {
            fs::create_dir_all(root2.join("releases").join(n)).unwrap();
        }
        symlink(root2.join("releases/4000"), root2.join("current")).unwrap();
        gc(&root2, 2).unwrap();
        assert_eq!(release_dirs(&root2), vec!["3000", "4000"]);
    }

    #[test]
    fn case_a9_gc_with_nothing_to_remove_succeeds() {
        let (_tmp, _src, root) = setup();
        gc(&root, 2).unwrap();
        assert!(release_dirs(&root).is_empty());

        let (_tmp2, _src2, root2) = setup();
        fs::create_dir_all(root2.join("releases/1000")).unwrap();
        gc(&root2, 2).unwrap();
        assert_eq!(release_dirs(&root2), vec!["1000"]);
    }

    #[test]
    fn case_a10_a_checkout_without_git_or_unlinked_counts_as_replaced() {
        let (tmp, src, _root) = setup();
        fs::create_dir_all(src.join(".git")).unwrap();
        assert!(!checkout_replaced(&src));
        let plain = tmp.path().join("plain");
        fs::create_dir_all(&plain).unwrap();
        assert!(checkout_replaced(&plain));
        fs::remove_dir_all(src.join(".git")).unwrap();
        assert!(checkout_replaced(&src));
        assert!(checkout_replaced(&tmp.path().join("gone")));
    }
}
