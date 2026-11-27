//! After a repository rename, rename the Cargo binary that carries the old name:
//! the `[package]` and `[[bin]]` names of the root `Cargo.toml`, the root entry of
//! `Cargo.lock`, and the installed executable. A workspace member keeps its name.

use crate::forge::Error;
use crate::install_records::rename_install_records;
use crate::realign::Rename;
use std::fs;
use std::path::{Path, PathBuf};

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("cannot {what} {}: {e}", path.display()))
}

/// Does this line set `name = "old"`?
fn names(line: &str, old: &str) -> bool {
    let Some((key, value)) = line.split_once('=') else {
        return false;
    };
    key.trim() == "name" && value.trim() == format!("\"{old}\"")
}

/// Rewrites `name = "old"` in the tables `tables` of a manifest. A `[[package]]`
/// block of a lock file that has a `source` is a dependency and is left alone.
fn rewrite(text: &str, old: &str, new: &str, tables: &[&str]) -> (String, bool) {
    let mut out = String::new();
    let mut block: Vec<&str> = Vec::new();
    let mut table = "";
    let mut changed = false;
    let mut flush = |block: &mut Vec<&str>, table: &str, out: &mut String| {
        let dependency = table == "[[package]]" && block.iter().any(|l| l.starts_with("source ="));
        let mine = tables.contains(&table) && !dependency;
        for line in block.drain(..) {
            if mine && names(line, old) {
                out.push_str(&line.replacen(&format!("\"{old}\""), &format!("\"{new}\""), 1));
                changed = true;
            } else {
                out.push_str(line);
            }
        }
    };
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            flush(&mut block, table, &mut out);
            table = line.trim();
        }
        block.push(line);
    }
    flush(&mut block, table, &mut out);
    (out, changed)
}

fn rewrite_file(path: &Path, old: &str, new: &str, tables: &[&str]) -> Result<bool, Error> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(io(path, "read", &e)),
    };
    let (text, changed) = rewrite(&text, old, new, tables);
    if changed {
        fs::write(path, text).map_err(|e| io(path, "write", &e))?;
    }
    Ok(changed)
}

/// The `name` of the `[package]` table of a manifest, if it has one.
pub(crate) fn package_name(manifest: &str) -> Option<String> {
    let mut table = "";
    for line in manifest.lines() {
        if line.trim_start().starts_with('[') {
            table = line.trim();
        } else if let (true, Some((key, value))) = (table == "[package]", line.split_once('=')) {
            if key.trim() == "name" {
                return Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// The quoted strings of the `members = [...]` array of a manifest's `[workspace]`.
fn members(manifest: &str) -> Vec<String> {
    let Some(at) = manifest.find("[workspace]") else {
        return vec![];
    };
    let rest = &manifest[at + "[workspace]".len()..];
    let rest = rest.split("\n[").next().unwrap_or(rest);
    let Some(key) = rest.find("members") else {
        return vec![];
    };
    let list = rest[key..].split(']').next().unwrap_or("");
    list.split('"')
        .skip(1)
        .step_by(2)
        .map(String::from)
        .collect()
}

/// The root manifest's folder and the folders of its workspace members. `dir/*` is
/// expanded; any other wildcard is refused rather than skipped.
pub(crate) fn project_dirs(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut dirs = vec![dir.to_path_buf()];
    let path = dir.join("Cargo.toml");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(dirs),
        Err(e) => return Err(io(&path, "read", &e)),
    };
    for member in members(&text) {
        if let Some(parent) = member.strip_suffix("/*") {
            let parent = dir.join(parent);
            for entry in fs::read_dir(&parent).map_err(|e| io(&parent, "read", &e))? {
                let entry = entry.map_err(|e| io(&parent, "read", &e))?.path();
                if entry.join("Cargo.toml").is_file() {
                    dirs.push(entry);
                }
            }
        } else if member.contains(['*', '?', '[']) {
            return Err(Error::Run(format!(
                "workspace member pattern {member:?} in {} is not supported",
                path.display()
            )));
        } else {
            dirs.push(dir.join(member));
        }
    }
    Ok(dirs)
}

/// Refuses, before any change, a clash with an executable already installed as `new`
/// and a workspace member pattern it cannot expand.
pub fn check(dir: &Path, bin_dir: &Path, old: &str, new: &str) -> Result<(), Error> {
    project_dirs(dir)?;
    let (from, to) = (bin_dir.join(old), bin_dir.join(new));
    if from.is_file() && to.exists() && !same_file(&from, &to) {
        return Err(Error::Run(format!(
            "{} already exists",
            bin_dir.join(new).display()
        )));
    }
    Ok(())
}

/// Both names open the same file (one device and inode), as `Old` and `old` do on a
/// case-insensitive disk. A metadata error is not "same": the caller then reports the clash.
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    matches!((fs::metadata(a), fs::metadata(b)),
        (Ok(x), Ok(y)) if x.dev() == y.dev() && x.ino() == y.ino())
}

#[cfg(not(unix))]
fn same_file(_: &Path, _: &Path) -> bool {
    false
}

/// Renames the binary of the project in `dir` and of its workspace members. Returns the
/// executable it moved, if any.
pub fn rename_binary(
    dir: &Path,
    bin_dir: &Path,
    old: &str,
    new: &str,
    renames: &[Rename],
) -> Result<Option<PathBuf>, Error> {
    let mut manifest = false;
    for project in project_dirs(dir)? {
        let tables = ["[package]", "[[bin]]"];
        manifest |= rewrite_file(&project.join("Cargo.toml"), old, new, &tables)?;
    }
    rewrite_file(&dir.join("Cargo.lock"), old, new, &["[[package]]"])?;
    let from = bin_dir.join(old);
    if !manifest || !from.is_file() {
        return Ok(None);
    }
    let to = bin_dir.join(new);
    fs::rename(&from, &to).map_err(|e| io(&from, "move", &e))?;
    if let Some(cargo_home) = bin_dir.parent() {
        rename_install_records(cargo_home, old, new, renames)?;
    }
    Ok(Some(to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("actautoma-bin-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(p.join("proj")).unwrap();
        fs::create_dir_all(p.join("bin")).unwrap();
        p
    }

    const TOML: &str = "[package]\nname = \"old\"\n\n[[bin]]\nname = \"old\"\npath = \"src/main.rs\"\n\n[dependencies]\nname = \"old\"\n";
    const LOCK: &str = "[[package]]\nname = \"old\"\nversion = \"1\"\n\n[[package]]\nname = \"old\"\nversion = \"9\"\nsource = \"registry+x\"\n";

    #[test]
    fn renames_package_bin_lock_root_and_the_installed_executable() {
        let b = scratch("all");
        fs::write(b.join("proj/Cargo.toml"), TOML).unwrap();
        fs::write(b.join("proj/Cargo.lock"), LOCK).unwrap();
        fs::write(b.join("bin/old"), "elf").unwrap();
        let moved = rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &[]).unwrap();
        assert_eq!(moved, Some(b.join("bin/new")));
        assert!(!b.join("bin/old").exists());
        let toml = fs::read_to_string(b.join("proj/Cargo.toml")).unwrap();
        assert_eq!(toml, TOML.replacen("\"old\"", "\"new\"", 2)); // [dependencies] untouched
        let lock = fs::read_to_string(b.join("proj/Cargo.lock")).unwrap();
        assert_eq!(lock, LOCK.replacen("\"old\"", "\"new\"", 1)); // registry dependency untouched
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_project_without_cargo_manifest_changes_nothing() {
        let b = scratch("none");
        fs::write(b.join("bin/old"), "elf").unwrap();
        assert_eq!(
            rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &[]).unwrap(),
            None
        );
        assert!(b.join("bin/old").is_file());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_package_named_otherwise_keeps_its_name_and_its_executable() {
        let b = scratch("other");
        fs::write(b.join("proj/Cargo.toml"), "[package]\nname = \"tool\"\n").unwrap();
        fs::write(b.join("bin/old"), "elf").unwrap();
        assert_eq!(
            rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &[]).unwrap(),
            None
        );
        assert!(b.join("bin/old").is_file());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_installed_executable_already_named_new_is_refused_up_front() {
        let b = scratch("clash");
        fs::write(b.join("bin/old"), "a").unwrap();
        fs::write(b.join("bin/new"), "b").unwrap();
        let err = check(&b.join("proj"), &b.join("bin"), "old", "new").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_different_file_already_named_new_is_a_clash() {
        let b = scratch("clash");
        fs::write(b.join("bin/Old"), "a").unwrap();
        fs::write(b.join("bin/other"), "b").unwrap();
        let err = check(&b.join("proj"), &b.join("bin"), "Old", "other").unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_case_only_rename_on_a_case_insensitive_disk_is_not_a_clash() {
        // On macOS `Old` and `old` are one file: `new` "exists" because it is `old`.
        let b = scratch("case");
        fs::write(b.join("bin/Old"), "a").unwrap();
        // A case-insensitive disk (APFS) already answers `old` with the same file; on a
        // case-sensitive one the link below stands in for that.
        if !b.join("bin/old").exists() {
            fs::hard_link(b.join("bin/Old"), b.join("bin/old")).unwrap();
        }
        check(&b.join("proj"), &b.join("bin"), "Old", "old").unwrap();
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_unreadable_manifest_is_an_error_not_a_skip() {
        let b = scratch("bad");
        fs::create_dir(b.join("proj/Cargo.toml")).unwrap(); // a folder where a file belongs
        let err = rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &[]).unwrap_err();
        assert!(err.to_string().contains("cannot read"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }

    fn renames() -> Vec<Rename> {
        vec![Rename {
            from: "me/old".into(),
            to: "me/new".into(),
        }]
    }

    #[test]
    fn renames_a_workspace_member_named_after_the_repository() {
        let b = scratch("ws");
        let p = b.join("proj");
        fs::write(
            p.join("Cargo.toml"),
            "[workspace]\nmembers = [\n  \"crates/*\",\n  \"tool\",\n]\n",
        )
        .unwrap();
        for m in ["crates/a", "tool"] {
            fs::create_dir_all(p.join(m)).unwrap();
        }
        fs::write(p.join("crates/a/Cargo.toml"), "[package]\nname = \"old\"\n").unwrap();
        fs::write(p.join("tool/Cargo.toml"), "[package]\nname = \"other\"\n").unwrap();
        fs::write(b.join("bin/old"), "elf").unwrap();
        let moved = rename_binary(&p, &b.join("bin"), "old", "new", &[]).unwrap();
        assert_eq!(moved, Some(b.join("bin/new")));
        assert_eq!(
            fs::read_to_string(p.join("crates/a/Cargo.toml")).unwrap(),
            "[package]\nname = \"new\"\n"
        );
        assert_eq!(
            fs::read_to_string(p.join("tool/Cargo.toml")).unwrap(),
            "[package]\nname = \"other\"\n"
        );
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_member_pattern_it_cannot_expand_is_refused_up_front() {
        let b = scratch("glob");
        fs::write(
            b.join("proj/Cargo.toml"),
            "[workspace]\nmembers = [\"a/**\"]\n",
        )
        .unwrap();
        let err = check(&b.join("proj"), &b.join("bin"), "old", "new").unwrap_err();
        assert!(err.to_string().contains("not supported"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn keeps_the_cargo_install_records_true_to_the_renamed_executable() {
        let b = scratch("rec");
        fs::write(b.join("proj/Cargo.toml"), "[package]\nname = \"old\"\n").unwrap();
        fs::write(b.join("bin/old"), "elf").unwrap();
        let toml = "[v1]\n\"old 0.1.0 (git+https://github.com/me/old#abc)\" = [\"old\"]\n\"old-x 1.0.0 (registry+r)\" = [\"old-x\"]\n";
        fs::write(b.join(".crates.toml"), toml).unwrap();
        let json = r#"{"installs":{"old 0.1.0 (git+https://github.com/me/old#abc)":{"bins":["old"],"profile":"release"},"zz 1.0.0 (registry+r)":{"bins":["zz"]}}}"#;
        fs::write(b.join(".crates2.json"), json).unwrap();
        rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &renames()).unwrap();
        let toml = fs::read_to_string(b.join(".crates.toml")).unwrap();
        assert_eq!(
            toml,
            "[v1]\n\"new 0.1.0 (git+https://github.com/me/new#abc)\" = [\"new\"]\n\"old-x 1.0.0 (registry+r)\" = [\"old-x\"]\n"
        );
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(b.join(".crates2.json")).unwrap()).unwrap();
        let installs = json["installs"].as_object().unwrap();
        let new = &installs["new 0.1.0 (git+https://github.com/me/new#abc)"];
        assert_eq!(
            (new["bins"][0].as_str(), new["profile"].as_str()),
            (Some("new"), Some("release"))
        );
        assert_eq!(installs.len(), 2);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_corrupt_install_record_is_an_error_not_a_skip() {
        let b = scratch("badrec");
        fs::write(b.join("proj/Cargo.toml"), "[package]\nname = \"old\"\n").unwrap();
        fs::write(b.join("bin/old"), "elf").unwrap();
        fs::write(b.join(".crates2.json"), "{not json").unwrap();
        let err = rename_binary(&b.join("proj"), &b.join("bin"), "old", "new", &[]).unwrap_err();
        assert!(err.to_string().contains(".crates2.json"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }
}
