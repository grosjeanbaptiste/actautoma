//! After a repository rename, rename its family: the workspace members named
//! `old` or `old-*`, their folders, and every whole-name mention of those
//! names in the moved folder (`old_x::`, `old-x`, `Old`, `OLD`).
//!
//! Only the moved folder is rewritten: other repositories that depend on a
//! renamed crate update their own dependency.

use crate::binary::{package_name, project_dirs};
use crate::forge::Error;
use crate::realign::replace_words;
use crate::realign::Rename;
use crate::realign_fs::{realign_tree_with, rename_children, Report};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// What a family rename did.
#[derive(Debug, Default)]
pub struct Done {
    pub report: Report,
    pub moved: Vec<PathBuf>,
}

/// The package names of the workspace members (and of the root) equal to `old` or
/// starting with `old-`, sorted.
pub fn names(dir: &Path, old: &str) -> Result<Vec<String>, Error> {
    let prefix = format!("{old}-");
    let mut found = Vec::new();
    for project in project_dirs(dir)? {
        let path = project.join("Cargo.toml");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(Error::Run(format!("cannot read {}: {e}", path.display()))),
        };
        found.extend(package_name(&text).filter(|n| n == old || n.starts_with(&prefix)));
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn titled(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The literal rewrites for `names`: each name and its `_` form, then `old` itself,
/// `Old` and `OLD`.
pub fn renames(names: &[String], old: &str, new: &str) -> Vec<Rename> {
    let mut forms: Vec<(String, String)> = Vec::new();
    for name in names {
        let to = format!("{new}{}", &name[old.len()..]);
        forms.push((name.clone(), to.clone()));
        forms.push((name.replace('-', "_"), to.replace('-', "_")));
    }
    forms.push((old.to_string(), new.to_string())); // the facade may already be renamed
    forms.push((titled(old), titled(new)));
    forms.push((old.to_uppercase(), new.to_uppercase()));
    let mut seen = std::collections::HashSet::new();
    forms
        .into_iter()
        .filter(|(from, to)| from != to && seen.insert(from.clone()))
        .map(|(from, to)| Rename { from, to })
        .collect()
}

/// Rewrites the text under `dir` and renames the member folders. A folder that
/// already carries the new name is refused before anything changes.
pub fn apply(dir: &Path, old: &str, new: &str, names: &[String]) -> Result<Done, Error> {
    if names.is_empty() {
        return Ok(Done::default());
    }
    let all = renames(names, old, new);
    let folders: Vec<Rename> = all
        .iter()
        .filter(|r| names.contains(&r.from))
        .cloned()
        .collect();
    let parents: BTreeSet<PathBuf> = project_dirs(dir)?
        .into_iter()
        .filter(|p| p != dir)
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    for parent in &parents {
        refuse_clashes(parent, &folders)?;
    }
    let report = realign_tree_with(dir, &all, replace_words)?;
    let mut moved = Vec::new();
    for parent in &parents {
        moved.extend(rename_children(parent, &folders)?);
    }
    Ok(Done { report, moved })
}

/// Refuses, before any change, a member folder whose new name is already taken.
pub fn check(dir: &Path, old: &str, new: &str) -> Result<(), Error> {
    let names = names(dir, old)?;
    let folders: Vec<Rename> = renames(&names, old, new)
        .into_iter()
        .filter(|r| names.contains(&r.from))
        .collect();
    for project in project_dirs(dir)?.into_iter().filter(|p| p != dir) {
        if let Some(parent) = project.parent() {
            refuse_clashes(parent, &folders)?;
        }
    }
    Ok(())
}

/// Fails when a folder to rename has its target name already taken.
fn refuse_clashes(parent: &Path, folders: &[Rename]) -> Result<(), Error> {
    for r in folders {
        if parent.join(&r.from).exists() && parent.join(&r.to).exists() {
            return Err(Error::Run(format!(
                "{} already exists",
                parent.join(&r.to).display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("actautoma-fam-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn member(root: &Path, dir: &str, name: &str, body: &str) {
        fs::create_dir_all(root.join(dir)).unwrap();
        let manifest = format!("[package]\nname = \"{name}\"\n\n[dependencies]\n{body}");
        fs::write(root.join(dir).join("Cargo.toml"), manifest).unwrap();
    }

    /// A workspace shaped like biorcrux: a facade `old`, members `old-*`, one stranger.
    fn workspace(b: &Path) -> PathBuf {
        let w = b.join("old");
        fs::create_dir_all(&w).unwrap();
        fs::write(
            w.join("Cargo.toml"),
            "[workspace]\nmembers = [\"packages/*\"]\n",
        )
        .unwrap();
        member(
            &w,
            "packages/old",
            "old",
            "old-widgets = { path = \"../old-widgets\" }\n",
        );
        member(&w, "packages/old-widgets", "old-widgets", "");
        member(&w, "packages/older", "older", "");
        fs::write(
            w.join("packages/old/lib.rs"),
            "use old::widgets;\nuse old_widgets::Shell;\n// Old and OLD_TAG, old-store-{}, not older\n",
        )
        .unwrap();
        w
    }

    #[test]
    fn names_are_the_members_called_old_or_prefixed_by_it() {
        let b = scratch("names");
        let w = workspace(&b);
        assert_eq!(names(&w, "old").unwrap(), ["old", "old-widgets"]);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_folder_without_manifest_has_no_family() {
        let b = scratch("none");
        assert_eq!(names(&b, "old").unwrap(), Vec::<String>::new());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_member_pattern_it_cannot_expand_is_an_error() {
        let b = scratch("glob");
        fs::write(b.join("Cargo.toml"), "[workspace]\nmembers = [\"a/**\"]\n").unwrap();
        let err = names(&b, "old").unwrap_err();
        assert!(err.to_string().contains("not supported"), "{err}");
        fs::remove_dir_all(&b).unwrap();
    }

    fn pairs(r: &[Rename]) -> Vec<(&str, &str)> {
        r.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect()
    }

    #[test]
    fn renames_cover_dash_and_underscore_forms_and_the_brand_cases() {
        let n = vec!["old".to_string(), "old-widgets".to_string()];
        assert_eq!(
            pairs(&renames(&n, "old", "new")),
            [
                ("old", "new"),
                ("old-widgets", "new-widgets"),
                ("old_widgets", "new_widgets"),
                ("Old", "New"),
                ("OLD", "NEW"),
            ]
        );
    }

    #[test]
    fn the_bare_name_is_rewritten_even_when_the_facade_was_already_renamed() {
        let n = vec!["old-widgets".to_string()];
        assert_eq!(
            pairs(&renames(&n, "old", "new")),
            [
                ("old-widgets", "new-widgets"),
                ("old_widgets", "new_widgets"),
                ("old", "new"),
                ("Old", "New"),
                ("OLD", "NEW"),
            ]
        );
    }

    #[test]
    fn the_same_name_has_nothing_to_rewrite() {
        let n = vec!["old".to_string()];
        assert_eq!(renames(&n, "old", "old"), vec![]);
    }

    #[test]
    fn apply_rewrites_the_text_and_moves_the_member_folders() {
        let b = scratch("apply");
        let w = workspace(&b);
        let n = names(&w, "old").unwrap();
        let done = apply(&w, "old", "new", &n).unwrap();
        assert!(w.join("packages/new/Cargo.toml").is_file());
        assert!(w.join("packages/new-widgets/Cargo.toml").is_file());
        assert!(w.join("packages/older/Cargo.toml").is_file()); // not in the family
        let lib = fs::read_to_string(w.join("packages/new/lib.rs")).unwrap();
        assert_eq!(
            lib,
            "use new::widgets;\nuse new_widgets::Shell;\n// New and NEW_TAG, new-store-{}, not older\n"
        );
        let toml = fs::read_to_string(w.join("packages/new/Cargo.toml")).unwrap();
        assert_eq!(
            toml,
            "[package]\nname = \"new\"\n\n[dependencies]\nnew-widgets = { path = \"../new-widgets\" }\n"
        );
        assert_eq!(done.moved.len(), 2);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_existing_target_folder_is_refused_before_anything_changes() {
        let b = scratch("clash");
        let w = workspace(&b);
        fs::create_dir_all(w.join("packages/new-widgets")).unwrap();
        let n = names(&w, "old").unwrap();
        let err = apply(&w, "old", "new", &n).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        let lib = fs::read_to_string(w.join("packages/old/lib.rs")).unwrap();
        assert!(lib.starts_with("use old::widgets;"), "text was rewritten");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_empty_family_changes_nothing() {
        let b = scratch("empty");
        let w = workspace(&b);
        let done = apply(&w, "old", "new", &[]).unwrap();
        assert!(done.moved.is_empty() && done.report.changed.is_empty());
        assert!(w.join("packages/old").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }
}
