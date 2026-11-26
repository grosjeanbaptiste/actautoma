//! Carry a new revision of a dependency to every consumer, one PR each (ADR 0002, slice 3).
//!
//! Consumers come from [`crate::pin_plan::plan`], in bump order. A failed consumer stops the walk:
//! the ones after it could pin a revision that was never published.

use crate::forge::{Error, Forge, Runner};
use crate::pin_bump::bump;
use crate::pin_plan::plan;
use std::path::Path;
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
