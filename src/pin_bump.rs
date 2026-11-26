//! Bump one consumer: cut a worktree from `origin/main`, rewrite the pin, refresh the lock,
//! commit, and ship the branch without merging (ADR 0002, slice 3).

use crate::forge::{Error, Forge, Runner};
use crate::pin_manifest::with_rev;
use crate::pin_plan::Consumer;
use crate::pin_propagate::{Options, Outcome};
use crate::ship::{ship, Options as ShipOptions};
use std::fs;
use std::path::{Path, PathBuf};

/// A failed step and the error text.
type Failure = (&'static str, String);

fn at<T>(step: &'static str, result: Result<T, Error>) -> Result<T, Failure> {
    result.map_err(|e| (step, e.to_string()))
}

fn short(rev: &str) -> &str {
    rev.get(..7).unwrap_or(rev)
}

/// The consumer's worktree for `branch`, inside its own `.worktrees` folder.
fn worktree_of(consumer: &Consumer, branch: &str) -> PathBuf {
    consumer
        .dir
        .join(".worktrees")
        .join(branch.replace('/', "-"))
}

/// Rewrites the pin in the worktree's manifest; `false` when `origin/main` already holds `rev`.
fn rewrite(wt: &Path, dep: &str, rev: &str) -> Result<bool, Failure> {
    let file = wt.join("Cargo.toml");
    let text = at(
        "rewrite",
        fs::read_to_string(&file).map_err(|e| Error::Run(format!("read {}: {e}", file.display()))),
    )?;
    let Some(new) = at("rewrite", with_rev(&text, dep, rev))? else {
        return Err((
            "rewrite",
            format!("{dep} is not a git dependency on origin/main"),
        ));
    };
    if new == text {
        return Ok(false);
    }
    let written =
        fs::write(&file, new).map_err(|e| Error::Run(format!("write {}: {e}", file.display())));
    at("rewrite", written)?;
    Ok(true)
}

/// Removes the worktree and branch this bump created and did not need.
fn discard(
    runner: &impl Runner,
    consumer: &Consumer,
    wt: &Path,
    branch: &str,
) -> Result<(), Failure> {
    let wt = wt.to_string_lossy();
    let run = |args: &[&str]| runner.run("git", args, Some(&consumer.dir));
    at("cleanup", run(&["worktree", "remove", &wt]))?;
    at("cleanup", run(&["branch", "-D", branch]))?;
    Ok(())
}

fn try_bump<F: Forge>(
    runner: &impl Runner,
    forge_for: &dyn Fn(&Path) -> F,
    consumer: &Consumer,
    (dep, rev): (&str, &str),
    options: &Options,
) -> Result<Outcome, Failure> {
    let branch = format!("chore/bump-{dep}-{}", short(rev));
    let wt = worktree_of(consumer, &branch);
    let git = |dir: &Path, args: &[&str]| runner.run("git", args, Some(dir));
    at("fetch", git(&consumer.dir, &["fetch", "origin"]))?;
    let path = wt.to_string_lossy();
    at(
        "worktree",
        git(
            &consumer.dir,
            &["worktree", "add", "-b", &branch, &path, "origin/main"],
        ),
    )?;
    if !rewrite(&wt, dep, rev)? {
        discard(runner, consumer, &wt, &branch)?;
        return Ok(Outcome::UpToDate);
    }
    at(
        "cargo update",
        runner.run("cargo", &["update", "-p", dep], Some(&wt)),
    )?;
    at("commit", git(&wt, &["add", "Cargo.toml", "Cargo.lock"]))?;
    let message = format!("chore: pin {dep} at {}", short(rev));
    at("commit", git(&wt, &["commit", "-m", &message]))?;
    let body = format!(
        "Bump `{dep}` from {} to {}. Opened by `actautoma pin-propagate`.",
        short(&consumer.old_rev),
        short(rev)
    );
    let ship_options = ShipOptions {
        gate: options.gate.clone(),
        merge: false,
        body: Some(body),
        settle: options.settle,
    };
    let shipped = ship(&forge_for(&wt), runner, &wt, &ship_options);
    match at("ship", shipped)? {
        crate::ship::Outcome::Merged { url, .. } | crate::ship::Outcome::Held { url, .. } => {
            Ok(Outcome::Opened { url })
        }
    }
}

/// Bumps `dep` to `rev` in `consumer`; a failed step is an [`Outcome::Failed`], never a panic.
pub(crate) fn bump<F: Forge>(
    runner: &impl Runner,
    forge_for: &dyn Fn(&Path) -> F,
    consumer: &Consumer,
    dep: &str,
    rev: &str,
    options: &Options,
) -> Outcome {
    try_bump(runner, forge_for, consumer, (dep, rev), options).unwrap_or_else(|(step, output)| {
        Outcome::Failed {
            step: step.to_string(),
            output,
        }
    })
}
