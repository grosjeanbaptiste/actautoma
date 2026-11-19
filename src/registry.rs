//! The registry follows a rename. It holds one `name|path[|...]` line per
//! repository; the entry whose path is the moved folder takes the new name.
//!
//! A registry that is absent or does not list the folder is reported, never
//! silently skipped; one that cannot be read or written is an error.

use crate::forge::Error;
use std::fs;
use std::path::Path;

/// What the pass found.
#[derive(Debug, PartialEq, Eq)]
pub enum Registry {
    /// No registry file on this host.
    Missing,
    /// The registry exists but no entry has this path; the file is untouched.
    Unlisted,
    /// This many entries took the new name.
    Renamed(usize),
}

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("{what} {}: {e}", path.display()))
}

/// `text` with the name of every entry at `path` replaced, and how many there were.
fn rename_lines(text: &str, path: &str, name: &str) -> (String, usize) {
    let mut count = 0;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        match line.split_once('|') {
            Some((_, rest)) if rest.split(['|', '\n']).next() == Some(path) => {
                count += 1;
                out.push_str(name);
                out.push('|');
                out.push_str(rest);
            }
            _ => out.push_str(line),
        }
    }
    (out, count)
}

/// Rename the entry at `path` to `name` in the registry `file`.
pub fn rename_entry(file: &Path, path: &Path, name: &str) -> Result<Registry, Error> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Registry::Missing),
        Err(e) => return Err(io(file, "cannot read", &e)),
    };
    let (fixed, count) = rename_lines(&text, &path.to_string_lossy(), name);
    if count == 0 {
        return Ok(Registry::Unlisted);
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".actautoma-tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let mode = fs::metadata(file).map_err(|e| io(file, "cannot stat", &e))?;
    fs::write(&tmp, fixed).map_err(|e| io(&tmp, "cannot write", &e))?;
    let moved = fs::set_permissions(&tmp, mode.permissions())
        .map_err(|e| io(&tmp, "cannot set permissions of", &e))
        .and_then(|()| fs::rename(&tmp, file).map_err(|e| io(file, "cannot replace", &e)));
    if let Err(e) = moved {
        // The original error is the one to report; a leftover temp file is named in it.
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(Registry::Renamed(count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn file(dir: &Path, text: &str) -> std::path::PathBuf {
        let f = dir.join("projects.txt");
        fs::write(&f, text).unwrap();
        f
    }

    fn tmp() -> tempfile_dir::Dir {
        tempfile_dir::Dir::new()
    }

    #[test]
    fn the_entry_at_the_path_takes_the_new_name_and_the_rest_is_untouched() {
        let d = tmp();
        let f = file(d.path(), "a|/x/a|extra\nold|/x/old|k\nz|/x/z\n");
        let got = rename_entry(&f, Path::new("/x/old"), "new").unwrap();
        assert_eq!(got, Registry::Renamed(1));
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            "a|/x/a|extra\nnew|/x/old|k\nz|/x/z\n"
        );
    }

    #[test]
    fn a_last_line_without_newline_stays_without_newline() {
        let d = tmp();
        let f = file(d.path(), "old|/x/old");
        rename_entry(&f, Path::new("/x/old"), "new").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "new|/x/old");
    }

    #[test]
    fn two_entries_at_the_same_path_are_both_renamed_and_counted() {
        let d = tmp();
        let f = file(d.path(), "o|/x/old\np|/x/old\n");
        assert_eq!(
            rename_entry(&f, Path::new("/x/old"), "n").unwrap(),
            Registry::Renamed(2)
        );
        assert_eq!(fs::read_to_string(&f).unwrap(), "n|/x/old\nn|/x/old\n");
    }

    #[test]
    fn only_the_path_column_counts_not_the_name_column_nor_a_longer_path() {
        let d = tmp();
        let text = "/x/old|/y/other\nm|/x/old-more\nm|/x/old/sub\n";
        let f = file(d.path(), text);
        assert_eq!(
            rename_entry(&f, Path::new("/x/old"), "n").unwrap(),
            Registry::Unlisted
        );
        assert_eq!(fs::read_to_string(&f).unwrap(), text);
    }

    #[test]
    fn an_absent_registry_is_reported_as_missing() {
        let d = tmp();
        let got = rename_entry(&d.path().join("none"), Path::new("/x"), "n").unwrap();
        assert_eq!(got, Registry::Missing);
    }

    #[test]
    fn a_registry_that_cannot_be_read_is_an_error_naming_it() {
        let d = tmp();
        let err = rename_entry(d.path(), Path::new("/x"), "n").unwrap_err();
        assert!(err.to_string().contains("cannot read"), "{err}");
    }

    #[test]
    fn the_permissions_of_the_registry_survive_the_rewrite() {
        let d = tmp();
        let f = file(d.path(), "o|/x/old\n");
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
        rename_entry(&f, Path::new("/x/old"), "n").unwrap();
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn no_temporary_file_is_left_behind() {
        let d = tmp();
        let f = file(d.path(), "o|/x/old\n");
        rename_entry(&f, Path::new("/x/old"), "n").unwrap();
        let names: Vec<_> = fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    mod tempfile_dir {
        use std::path::{Path, PathBuf};
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!(
                    "actautoma-registry-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
    }
}
