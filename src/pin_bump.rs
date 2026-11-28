//! Bump one consumer: cut a worktree from `origin/main`, rewrite its pins, refresh the lock,
//! commit, and ship the branch, merging when asked and safe (ADR 0002, slices 3-4).

use crate::forge::{Error, Forge, Runner};
use crate::pin_advance::advance_checkout;
use crate::pin_manifest::with_rev;
use crate::pin_plan::Consumer;
use crate::pin_propagate::{Options, Outcome};
use crate::ship::{ship, Options as ShipOptions, Outcome as Shipped};
use std::fs;
use std::path::{Path, PathBuf};

/// A failed step and the error text.
type Failure = (&'static str, String);

type Pins = [(String, String)];

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

/// Rewrites every pin in the worktree's manifest; `false` when `origin/main` already holds them.
fn rewrite(wt: &Path, pins: &Pins) -> Result<bool, Failure> {
    let file = wt.join("Cargo.toml");
    let text = at(
        "rewrite",
        fs::read_to_string(&file).map_err(|e| Error::Run(format!("read {}: {e}", file.display()))),
    )?;
    let mut new = text.clone();
    for (dep, rev) in pins {
        let Some(next) = at("rewrite", with_rev(&new, dep, rev))? else {
            return Err((
                "rewrite",
                format!("{dep} is not a git dependency on origin/main"),
            ));
        };
        new = next;
    }
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

/// Whether `Cargo.toml` or `Cargo.lock` has uncommitted changes in the worktree.
fn has_pending(runner: &impl Runner, wt: &Path) -> Result<bool, Failure> {
    let args = ["status", "--porcelain", "--", "Cargo.toml", "Cargo.lock"];
    let out = at("status", runner.run("git", &args, Some(wt)))?;
    Ok(!out.trim().is_empty())
}

/// Whether the worktree's branch already holds a commit that `origin/main` lacks.
fn has_commits(runner: &impl Runner, wt: &Path) -> Result<bool, Failure> {
    let args = ["rev-list", "--count", "origin/main..HEAD"];
    let out = at("status", runner.run("git", &args, Some(wt)))?;
    let n = out.trim();
    Ok(!(n.is_empty() || n == "0"))
}

/// `cargo update -p A -p B`, then commit `Cargo.toml` and `Cargo.lock`.
fn commit(runner: &impl Runner, wt: &Path, pins: &Pins) -> Result<(), Failure> {
    let mut update = vec!["update"];
    for (dep, _) in pins {
        update.extend(["-p", dep]);
    }
    at("cargo update", runner.run("cargo", &update, Some(wt)))?;
    let git = |args: &[&str]| runner.run("git", args, Some(wt));
    at("commit", git(&["add", "Cargo.toml", "Cargo.lock"]))?;
    let list: Vec<String> = pins
        .iter()
        .map(|(dep, rev)| format!("{dep} at {}", short(rev)))
        .collect();
    at(
        "commit",
        git(&["commit", "-m", &format!("chore: pin {}", list.join(", "))]),
    )?;
    Ok(())
}

fn try_bump<F: Forge>(
    runner: &impl Runner,
    forge_for: &dyn Fn(&Path) -> F,
    consumer: &Consumer,
    pins: &Pins,
    options: &Options,
) -> Result<Outcome, Failure> {
    let branch = format!("chore/bump-{}-{}", pins[0].0, short(&pins[0].1));
    let wt = worktree_of(consumer, &branch);
    let path = wt.to_string_lossy();
    let git = |args: &[&str]| runner.run("git", args, Some(&consumer.dir));
    at("fetch", git(&["fetch", "origin"]))?;
    // A worktree left by an interrupted bump is resumed, not recreated.
    let resumed = wt.exists();
    if !resumed {
        at(
            "worktree",
            git(&["worktree", "add", "-b", &branch, &path, "origin/main"]),
        )?;
    }
    let rewritten = rewrite(&wt, pins)?;
    let pending = resumed && has_pending(runner, &wt)?;
    let committed = resumed && has_commits(runner, &wt)?;
    if !(rewritten || pending || committed) {
        discard(runner, consumer, &wt, &branch)?;
        return Ok(Outcome::UpToDate);
    }
    if rewritten || pending {
        commit(runner, &wt, pins)?;
    }
    let list: Vec<String> = pins
        .iter()
        .map(|(dep, rev)| format!("`{dep}` to {}", short(rev)))
        .collect();
    let ship_options = ShipOptions {
        gate: options.gate.clone(),
        merge: options.merge,
        body: Some(format!(
            "Bump {}. Opened by `actautoma pin-propagate`.",
            list.join(", ")
        )),
        settle: options.settle,
        ci_wait: options.ci_wait,
    };
    let shipped = at("ship", ship(&forge_for(&wt), runner, &wt, &ship_options))?;
    Ok(match shipped {
        // `ship` itself tried the primary checkout; `advance_checkout` below reports it again.
        Shipped::Merged { url, .. } | Shipped::MergedStale { url, .. } => {
            // The pull request is merged: its worktree and local branch have done their job.
            discard(runner, consumer, &wt, &branch)
                .map_err(|(step, output)| (step, format!("merged {url}, then: {output}")))?;
            match advance_checkout(runner, &consumer.dir) {
                Ok(()) => Outcome::Merged { url },
                Err(why) => Outcome::MergedStale { url, why },
            }
        }
        Shipped::Held { url, .. } if !options.merge => Outcome::Opened { url },
        Shipped::Held { url, reason, .. } => Outcome::Held { url, reason },
    })
}

/// Bumps `pins` (package, rev) in `consumer`, the first one naming the branch; a failed step is
/// an [`Outcome::Failed`], never a panic.
pub(crate) fn bump<F: Forge>(
    runner: &impl Runner,
    forge_for: &dyn Fn(&Path) -> F,
    consumer: &Consumer,
    pins: &Pins,
    options: &Options,
) -> Outcome {
    try_bump(runner, forge_for, consumer, pins, options).unwrap_or_else(|(step, output)| {
        Outcome::Failed {
            step: step.to_string(),
            output,
        }
    })
}
