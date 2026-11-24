//! Classify the branches of a repository against its base: what each one still
//! has to say, so the next step (delete, ship, resolve, ask) is decided by the
//! facts and not by hand. Read-only: nothing is created, merged or deleted.

use crate::forge::{Error, Runner};
use std::path::Path;

/// What a branch still adds to the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// Every commit is already in the base: safe to delete.
    Merged,
    /// Not merged, but merging it would change nothing: the base already says it.
    Superseded,
    /// Merging it into the base brings something new and has no conflict.
    Mergeable,
    /// Merging it into the base conflicts in these files.
    Conflicting(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// `feat/x` for a local branch, `origin/feat/x` for a remote one.
    pub name: String,
    /// Commits in the branch and not in the base.
    pub ahead: usize,
    pub class: Class,
}

fn git(runner: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String, Error> {
    runner
        .run("git", args, Some(dir))
        .map(|out| out.trim().to_string())
}

/// What merging `branch` into `base` would do, from `git merge-tree`: the tree
/// of the result and the files in conflict. The command exits 1 on a conflict
/// and still prints the answer, so it runs through `sh` and its exit status is
/// ignored; an empty answer is the real failure.
fn merge_result(
    runner: &dyn Runner,
    dir: &Path,
    base: &str,
    branch: &str,
) -> Result<(String, Vec<String>), Error> {
    let script = r#"git merge-tree --write-tree --name-only "$1" "$2"; true"#;
    let out = runner.run("sh", &["-c", script, "sh", base, branch], Some(dir))?;
    let mut lines = out.lines();
    let tree = lines
        .next()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| Error::Parse(format!("git merge-tree {base} {branch} printed nothing")))?
        .to_string();
    let files = lines
        .take_while(|l| !l.is_empty())
        .map(String::from)
        .collect();
    Ok((tree, files))
}

/// Every branch of the repository except the base, local and `origin/`.
pub fn classify(runner: &dyn Runner, dir: &Path, base: &str) -> Result<Vec<Branch>, Error> {
    git(
        runner,
        dir,
        &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")],
    )
    .map_err(|_| Error::Run(format!("unknown base `{base}`")))?;
    let base_tree = git(runner, dir, &["rev-parse", &format!("{base}^{{tree}}")])?;
    let names = git(
        runner,
        dir,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )?;
    let remote_base = format!("origin/{base}");
    let mut all = Vec::new();
    for name in names.lines().filter(|n| {
        !n.is_empty() && *n != base && *n != remote_base && *n != "origin" && *n != "origin/HEAD"
    }) {
        let ahead: usize = git(
            runner,
            dir,
            &["rev-list", "--count", &format!("{base}..{name}")],
        )?
        .parse()
        .map_err(|_| Error::Parse(format!("commit count of {name}")))?;
        let class = if ahead == 0 {
            Class::Merged
        } else {
            let (tree, conflicts) = merge_result(runner, dir, base, name)?;
            if !conflicts.is_empty() {
                Class::Conflicting(conflicts)
            } else if tree == base_tree {
                Class::Superseded
            } else {
                Class::Mergeable
            }
        };
        all.push(Branch {
            name: name.to_string(),
            ahead,
            class,
        });
    }
    Ok(all)
}
