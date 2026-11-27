//! After a repository folder moves or changes case, point its linked worktrees at the new
//! path. `git worktree repair` cannot: on a case-insensitive disk it sees old and new as
//! one path and writes nothing, so both link files keep the old text.

use crate::forge::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("cannot {what} {}: {e}", path.display()))
}

/// `text` with the folder `old` replaced by `new` where it starts the path (after an optional
/// `gitdir: `), as a whole folder name; `None` when `text` does not name `old`.
fn swapped(text: &str, old: &Path, new: &Path) -> Option<String> {
    let (lead, path) = match text.strip_prefix("gitdir: ") {
        Some(rest) => ("gitdir: ", rest),
        None => ("", text),
    };
    let rest = path.strip_prefix(old.to_str()?)?;
    if !(rest.is_empty() || rest.starts_with(['/', '\n'])) {
        return None; // `Repo-x` is not `Repo`
    }
    Some(format!("{lead}{}{rest}", new.to_str()?))
}

/// Rewrites `file` when it names `old`; true when it changed.
fn rewrite(file: &Path, old: &Path, new: &Path) -> Result<bool, Error> {
    let text = fs::read_to_string(file).map_err(|e| io(file, "read", &e))?;
    match swapped(&text, old, new) {
        Some(fresh) => fs::write(file, fresh)
            .map(|()| true)
            .map_err(|e| io(file, "write", &e)),
        None => Ok(false),
    }
}

/// Rewrites the old folder path in the link files of every linked worktree of the repository
/// now at `new_dir`: `.git/worktrees/<name>/gitdir` and the `.git` file of each worktree.
/// Returns the number of files rewritten.
pub fn relink(old_dir: &Path, new_dir: &Path) -> Result<usize, Error> {
    let admin = new_dir.join(".git/worktrees");
    let entries = match fs::read_dir(&admin) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(io(&admin, "read", &e)),
    };
    let mut changed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| io(&admin, "read", &e))?.path();
        let gitdir = entry.join("gitdir");
        changed += usize::from(rewrite(&gitdir, old_dir, new_dir)?);
        let text = fs::read_to_string(&gitdir).map_err(|e| io(&gitdir, "read", &e))?;
        let worktree_git = PathBuf::from(text.trim_end());
        if worktree_git.is_file() {
            changed += usize::from(rewrite(&worktree_git, old_dir, new_dir)?);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("actautoma-wtl-{name}-{}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// A repository at `new` whose one linked worktree `w` still names `old`.
    fn moved_repo(b: &Path, old: &Path, new: &Path) {
        let admin = new.join(".git/worktrees/w");
        fs::create_dir_all(&admin).unwrap();
        fs::create_dir_all(new.join(".worktrees/w")).unwrap();
        let wt_git = old.join(".worktrees/w/.git");
        fs::write(admin.join("gitdir"), format!("{}\n", wt_git.display())).unwrap();
        let admin_old = old.join(".git/worktrees/w");
        fs::write(
            new.join(".worktrees/w/.git"),
            format!("gitdir: {}\n", admin_old.display()),
        )
        .unwrap();
        assert!(b.is_dir());
    }

    #[test]
    fn a_nested_worktree_is_pointed_at_the_new_path_in_both_link_files() {
        let b = scratch("nested");
        let (old, new) = (b.join("Repo"), b.join("repo"));
        moved_repo(&b, &old, &new);
        let n = relink(&old, &new).unwrap();
        let gitdir = fs::read_to_string(new.join(".git/worktrees/w/gitdir")).unwrap();
        let dotgit = fs::read_to_string(new.join(".worktrees/w/.git")).unwrap();
        assert_eq!(
            gitdir,
            format!("{}\n", new.join(".worktrees/w/.git").display())
        );
        assert_eq!(
            dotgit,
            format!("gitdir: {}\n", new.join(".git/worktrees/w").display())
        );
        assert_eq!(n, 2);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_second_run_changes_nothing() {
        let b = scratch("again");
        let (old, new) = (b.join("Repo"), b.join("repo"));
        moved_repo(&b, &old, &new);
        relink(&old, &new).unwrap();
        assert_eq!(relink(&old, &new).unwrap(), 0);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_repository_without_linked_worktrees_is_left_alone() {
        let b = scratch("none");
        let (old, new) = (b.join("Repo"), b.join("repo"));
        fs::create_dir_all(new.join(".git")).unwrap();
        assert_eq!(relink(&old, &new).unwrap(), 0);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_unreadable_link_file_is_an_error_not_a_skip() {
        let b = scratch("bad");
        let (old, new) = (b.join("Repo"), b.join("repo"));
        fs::create_dir_all(new.join(".git/worktrees/w/gitdir")).unwrap(); // a folder where a file belongs
        let err = relink(&old, &new).unwrap_err();
        assert!(err.to_string().contains("gitdir"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }
}
