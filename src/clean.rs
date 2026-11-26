//! Remove what is already merged: a branch, its worktree and its remote copy.
//!
//! Only a branch that `branches::classify` calls `Merged` is touched, and only when it was
//! pushed once (a branch just cut from the base has no commit either, and looks merged), its
//! worktree is clean and it is not checked out in the main checkout. A superseded branch whose
//! tree is exactly the base's is removed too, pushed or not: it holds nothing more than the base.
//! Everything else is kept and says why. Unmerged, other superseded and conflicting branches are
//! never touched: those are the owner's call.

use crate::branches::{classify, Class};
use crate::forge::{Error, Runner};
use std::path::{Path, PathBuf};

/// What `clean` did, or would do, for one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The branch, its worktree and its remote copy are gone.
    Removed { branch: String },
    /// The branch was left, for `reason`.
    Kept { branch: String, reason: String },
}

fn git(runner: &dyn Runner, dir: &Path, args: &[&str]) -> Result<String, Error> {
    runner.run("git", args, Some(dir))
}

/// The worktrees of the repository: their folder and the local branch each has checked out.
fn worktrees(runner: &dyn Runner, dir: &Path) -> Result<Vec<(PathBuf, String)>, Error> {
    let text = git(runner, dir, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    let mut path = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let (Some(b), Some(p)) = (line.strip_prefix("branch refs/heads/"), &path) {
            found.push((p.clone(), b.to_owned()));
        }
    }
    Ok(found)
}

/// Removes every merged branch with its worktree and remote copy; with `apply` false, reports
/// the same steps and deletes nothing.
///
/// # Errors
/// When a git call fails.
pub fn clean(runner: &dyn Runner, dir: &Path, base: &str, apply: bool) -> Result<Vec<Step>, Error> {
    let top = PathBuf::from(git(runner, dir, &["rev-parse", "--show-toplevel"])?.trim());
    let trees = worktrees(runner, dir)?;
    let mut steps = Vec::new();
    for b in classify(runner, dir, base)? {
        if b.name.starts_with("origin/") {
            continue;
        }
        // Superseded: merging it would change nothing, so the branch holds nothing the base
        // lacks, even when its commits were never pushed or landed another way.
        let identical = b.class == Class::Superseded;
        if b.class != Class::Merged && !identical {
            continue;
        }
        let name = b.name;
        let keep = |reason: &str| Step::Kept {
            branch: name.clone(),
            reason: reason.into(),
        };
        if !identical
            && git(
                runner,
                dir,
                &["config", "--get", &format!("branch.{name}.merge")],
            )
            .is_err()
        {
            steps.push(keep("never pushed: cannot tell merged from just cut"));
            continue;
        }
        let tree = trees
            .iter()
            .find(|(_, branch)| *branch == name)
            .map(|(p, _)| p);
        if tree.is_some_and(|p| *p == top) {
            steps.push(keep("checked out in the main checkout"));
            continue;
        }
        if let Some(p) = tree {
            if !git(runner, p, &["status", "--porcelain"])?
                .trim()
                .is_empty()
            {
                steps.push(keep("its worktree has local changes"));
                continue;
            }
        }
        if apply {
            if let Some(p) = tree {
                git(runner, dir, &["worktree", "remove", &p.to_string_lossy()])?;
            }
            // `-d` refuses a branch git cannot see as merged; superseded is the proof here.
            git(
                runner,
                dir,
                &["branch", if identical { "-D" } else { "-d" }, &name],
            )?;
            let on_remote = format!("refs/remotes/origin/{name}");
            if git(runner, dir, &["rev-parse", "--verify", "-q", &on_remote]).is_ok() {
                git(runner, dir, &["push", "origin", "--delete", &name])?;
            }
        }
        steps.push(Step::Removed { branch: name });
    }
    Ok(steps)
}
