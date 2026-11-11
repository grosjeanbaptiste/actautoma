//! Apply a [`Rename`] list to files and to folder names on disk.
//!
//! Nothing is skipped quietly: a file that is not text, a symlink and a
//! folder left alone are all named in the [`Report`].

use crate::forge::Error;
use crate::realign::{replace, Rename};
use std::fs;
use std::path::{Path, PathBuf};

/// Folders that hold caches, builds or vendored code, never references.
const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "target",
    "node_modules",
    ".lake",
    "__pycache__",
    ".mypy_cache",
    ".ruff_cache",
    ".pytest_cache",
];

/// What a pass did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files rewritten.
    pub changed: Vec<PathBuf>,
    /// Files left alone because they are not UTF-8 text.
    pub not_text: Vec<PathBuf>,
    /// Symlinks left alone: their target is not this pass's to rewrite.
    pub links: Vec<PathBuf>,
}

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("{what} {}: {e}", path.display()))
}

fn visit(dir: &Path, renames: &[Rename], report: &mut Report) -> Result<(), Error> {
    let entries = fs::read_dir(dir).map_err(|e| io(dir, "cannot read", &e))?;
    for entry in entries {
        let path = entry.map_err(|e| io(dir, "cannot list", &e))?.path();
        let kind = fs::symlink_metadata(&path).map_err(|e| io(&path, "cannot stat", &e))?;
        if kind.is_symlink() {
            report.links.push(path);
        } else if kind.is_dir() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if !SKIPPED_DIRS.contains(&name) {
                visit(&path, renames, report)?;
            }
        } else {
            rewrite(&path, renames, report)?;
        }
    }
    Ok(())
}

fn rewrite(path: &Path, renames: &[Rename], report: &mut Report) -> Result<(), Error> {
    let Ok(text) = fs::read_to_string(path) else {
        // Not UTF-8 (or unreadable): a read error must not look like "no text".
        fs::File::open(path).map_err(|e| io(path, "cannot read", &e))?;
        report.not_text.push(path.to_path_buf());
        return Ok(());
    };
    let (out, count) = replace(&text, renames);
    if count > 0 {
        fs::write(path, out).map_err(|e| io(path, "cannot write", &e))?;
        report.changed.push(path.to_path_buf());
    }
    Ok(())
}

/// Rewrite every text file under `root`; `root` itself must be a directory.
pub fn realign_tree(root: &Path, renames: &[Rename]) -> Result<Report, Error> {
    let mut report = Report::default();
    visit(root, renames, &mut report)?;
    report.changed.sort();
    Ok(report)
}

/// Rename the direct children of `dir` whose whole name is a `from`.
///
/// Fails, before renaming that child, when the target name already exists.
pub fn rename_children(dir: &Path, renames: &[Rename]) -> Result<Vec<PathBuf>, Error> {
    let mut moved = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| io(dir, "cannot read", &e))? {
        let path = entry.map_err(|e| io(dir, "cannot list", &e))?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if let Some(r) = renames.iter().find(|r| r.from == name) {
            let target = dir.join(&r.to);
            if target.exists() {
                return Err(Error::Run(format!("{} already exists", target.display())));
            }
            fs::rename(&path, &target).map_err(|e| io(&path, "cannot rename", &e))?;
            moved.push(target);
        }
    }
    moved.sort();
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("actautoma-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
            fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0); // best-effort cleanup of a temp dir
        }
    }

    fn r() -> Vec<Rename> {
        vec![Rename {
            from: "me/old".into(),
            to: "me/new".into(),
        }]
    }

    #[test]
    fn rewrites_text_files_in_subfolders_and_reports_them() {
        let s = Scratch::new("tree");
        fs::create_dir_all(s.0.join("a/b")).unwrap();
        fs::write(s.0.join("a/b/x.txt"), "url me/old\n").unwrap();
        fs::write(s.0.join("keep.txt"), "nothing\n").unwrap();
        let report = realign_tree(&s.0, &r()).unwrap();
        assert_eq!(report.changed, vec![s.0.join("a/b/x.txt")]);
        assert_eq!(
            fs::read_to_string(s.0.join("a/b/x.txt")).unwrap(),
            "url me/new\n"
        );
        assert_eq!(
            fs::read_to_string(s.0.join("keep.txt")).unwrap(),
            "nothing\n"
        );
    }

    #[test]
    fn skips_caches_and_vendored_folders() {
        let s = Scratch::new("skip");
        for d in [".git", "target", ".venv"] {
            fs::create_dir_all(s.0.join(d)).unwrap();
            fs::write(s.0.join(d).join("f"), "me/old").unwrap();
        }
        assert_eq!(realign_tree(&s.0, &r()).unwrap(), Report::default());
        assert_eq!(fs::read_to_string(s.0.join(".git/f")).unwrap(), "me/old");
    }

    #[test]
    fn names_a_binary_file_and_a_symlink_instead_of_skipping_them_quietly() {
        let s = Scratch::new("odd");
        fs::write(s.0.join("b.bin"), [0xff, 0xfe, 0x00]).unwrap();
        std::os::unix::fs::symlink("b.bin", s.0.join("link")).unwrap();
        let report = realign_tree(&s.0, &r()).unwrap();
        assert_eq!(report.not_text, vec![s.0.join("b.bin")]);
        assert_eq!(report.links, vec![s.0.join("link")]);
        assert!(report.changed.is_empty());
    }

    #[test]
    fn a_missing_root_is_an_error_naming_it() {
        let s = Scratch::new("missing");
        let err = realign_tree(&s.0.join("absent"), &r()).unwrap_err();
        assert!(err.to_string().contains("absent"), "{err}");
    }

    #[test]
    fn renames_only_children_whose_whole_name_matches() {
        let s = Scratch::new("kids");
        fs::create_dir(s.0.join("-src-old")).unwrap();
        fs::create_dir(s.0.join("-src-old-extra")).unwrap();
        let ren = vec![Rename {
            from: "-src-old".into(),
            to: "-src-new".into(),
        }];
        assert_eq!(
            rename_children(&s.0, &ren).unwrap(),
            vec![s.0.join("-src-new")]
        );
        assert!(s.0.join("-src-new").is_dir() && s.0.join("-src-old-extra").is_dir());
        assert!(!s.0.join("-src-old").exists());
    }

    #[test]
    fn refuses_to_overwrite_an_existing_target() {
        let s = Scratch::new("clash");
        fs::create_dir(s.0.join("-src-old")).unwrap();
        fs::create_dir(s.0.join("-src-new")).unwrap();
        let ren = vec![Rename {
            from: "-src-old".into(),
            to: "-src-new".into(),
        }];
        let err = rename_children(&s.0, &ren).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert!(s.0.join("-src-old").is_dir());
    }
}
