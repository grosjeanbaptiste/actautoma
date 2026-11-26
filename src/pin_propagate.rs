//! Carry a new revision of a dependency to every consumer, one PR each (ADR 0002, slice 3).
//!
//! Consumers come from [`crate::pin_plan::plan`], in bump order. A failed consumer stops the walk:
//! the ones after it could pin a revision that was never published.

use crate::forge::{Error, Forge, Runner};
use crate::gh::Gh;
use crate::pin_bump::bump;
use crate::pin_plan::plan;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How far to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// A consumer's gate: program and arguments, run before its branch is pushed.
    pub gate: Option<Vec<String>>,
    /// Report what would be done; touch nothing.
    pub dry_run: bool,
    /// Pause between reads while the platform still computes mergeability.
    pub settle: Duration,
}

/// What happened to one consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It already pins the revision.
    UpToDate,
    /// A dry run: it would be bumped.
    Planned,
    /// Its pull request is open.
    Opened { url: String },
    /// A step failed; `output` is the command's error text.
    Failed { step: String, output: String },
}

/// Bumps `dep` to `rev` in every consumer under `root`; the result lists each consumer reached,
/// ending with the failed one when a step fails.
///
/// `forge_for` gives the forge of a consumer's worktree.
///
/// # Errors
/// When the plan cannot be made (bad `rev`, unreadable `root` or manifest, a cycle). Nothing runs
/// in that case.
pub fn propagate<F: Forge>(
    runner: &impl Runner,
    forge_for: &dyn Fn(&Path) -> F,
    root: &Path,
    dep: &str,
    rev: &str,
    options: &Options,
) -> Result<Vec<(String, Outcome)>, Error> {
    let mut done = Vec::new();
    for consumer in plan(root, dep, rev)? {
        let outcome = if consumer.up_to_date {
            Outcome::UpToDate
        } else if options.dry_run {
            Outcome::Planned
        } else {
            bump(runner, forge_for, &consumer, dep, rev, options)
        };
        let failed = matches!(outcome, Outcome::Failed { .. });
        done.push((consumer.name, outcome));
        if failed {
            break;
        }
    }
    Ok(done)
}

pub const USAGE: &str =
    "usage: actautoma pin-propagate DEP [--rev REV] [--root DIR] [--gate \"PROGRAM ARGS\"] [--dry-run]";

fn usage() -> Error {
    Error::Run(USAGE.to_string())
}

/// One report line per consumer.
fn line((name, outcome): &(String, Outcome)) -> String {
    match outcome {
        Outcome::UpToDate => format!("{name}: up to date"),
        Outcome::Planned => format!("{name}: planned"),
        Outcome::Opened { url } => format!("{name}: opened {url}"),
        Outcome::Failed { step, output } => format!("{name}: failed at {step}: {output}"),
    }
}

/// `actautoma pin-propagate DEP [--rev REV] [--root DIR] [--gate CMD] [--dry-run]`. `REV` is the
/// tip of `origin/main` of `ROOT/DEP` (fetched first) when absent; `ROOT` is
/// `~/Source/Acteble/openacte` by default.
///
/// # Errors
/// On a bad command line, a plan that cannot be made, or a consumer that failed (after its line
/// is printed).
pub fn run(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    runner: &impl Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let mut it = args.iter().skip(1);
    let dep = it
        .next()
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(usage)?;
    let (mut rev, mut root, mut gate, mut dry_run) = (None, None, None, false);
    while let Some(flag) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(usage);
        match flag.as_str() {
            "--dry-run" => dry_run = true,
            "--rev" => rev = Some(value()?),
            "--root" => root = Some(cwd.join(value()?)),
            "--gate" => {
                let words: Vec<String> = value()?.split_whitespace().map(str::to_owned).collect();
                if words.is_empty() {
                    return Err(usage());
                }
                gate = Some(words);
            }
            _ => return Err(usage()),
        }
    }
    let root: PathBuf = root.unwrap_or_else(|| {
        PathBuf::from(env("HOME").unwrap_or_default()).join("Source/Acteble/openacte")
    });
    let rev = match rev {
        Some(rev) => rev,
        None => {
            let dir = root.join(dep);
            runner.run("git", &["fetch", "origin"], Some(&dir))?;
            runner
                .run("git", &["rev-parse", "origin/main"], Some(&dir))?
                .trim()
                .to_owned()
        }
    };
    let options = Options {
        gate,
        dry_run,
        settle: Duration::from_secs(4),
    };
    let forge_for = |wt: &Path| Gh::new(runner).in_dir(wt);
    let results = propagate(runner, &forge_for, &root, dep, &rev, &options)?;
    for result in &results {
        writeln!(out, "{}", line(result)).map_err(|e| Error::Run(e.to_string()))?;
    }
    if results
        .iter()
        .any(|(_, o)| matches!(o, Outcome::Failed { .. }))
    {
        return Err(Error::Run(
            "pin-propagate stopped on a failed consumer".into(),
        ));
    }
    Ok(())
}
