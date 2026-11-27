//! Carry a new revision of a dependency to every consumer, one PR each (ADR 0002, slices 3-4).
//!
//! Consumers come from [`crate::pin_plan::plan`], in bump order. A consumer that also pins an
//! earlier consumer is bumped to that consumer's new head, in the same pull request, once the
//! earlier one is merged; until then it is `Blocked`. A failed consumer stops the walk: the ones
//! after it could pin a revision that was never published.

use crate::forge::{Error, Forge, Runner};
use crate::pin_bump::bump;
use crate::pin_plan::{plan, Consumer};
use std::path::Path;
use std::time::Duration;

/// How far to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// A consumer's gate: program and arguments, run before its branch is pushed. Without one,
    /// nothing is merged.
    pub gate: Option<Vec<String>>,
    /// Report what would be done; touch nothing.
    pub dry_run: bool,
    /// Merge each pull request when it is safe (see [`crate::ship`]); otherwise leave it open.
    pub merge: bool,
    /// Pause between reads while the platform still computes mergeability.
    pub settle: Duration,
    /// Longest total wait for a run of CI that is still going, per consumer; zero never waits.
    pub ci_wait: Duration,
}

/// What happened to one consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It already pins what it should.
    UpToDate,
    /// A dry run: it would be bumped.
    Planned,
    /// Its pull request is open; merging was not asked.
    Opened { url: String },
    /// Its pull request was merged.
    Merged { url: String },
    /// Merging was asked and refused, for `reason`; the pull request is open.
    Held { url: String, reason: String },
    /// It pins consumer `on`, whose bump is not merged yet.
    Blocked { on: String },
    /// A step failed; `output` is the command's error text.
    Failed { step: String, output: String },
}

type Pins = Vec<(String, String)>;

/// The (package, rev) pairs of `known` that `consumer` pins at another revision.
fn pins_for(consumer: &Consumer, known: &Pins) -> Result<Pins, Error> {
    let mut pins = Vec::new();
    for (dep, rev) in known {
        if consumer.pinned_rev(dep)?.is_some_and(|old| old != *rev) {
            pins.push((dep.clone(), rev.clone()));
        }
    }
    Ok(pins)
}

/// The earlier consumer that `consumer` pins and whose pull request is still open.
fn waits_on(
    consumer: &Consumer,
    earlier: &[Consumer],
    done: &[(String, Outcome)],
) -> Result<Option<String>, Error> {
    for (other, (_, outcome)) in earlier.iter().zip(done) {
        let open = matches!(outcome, Outcome::Opened { .. } | Outcome::Held { .. });
        if open && consumer.pinned_rev(other.package())?.is_some() {
            return Ok(Some(other.name.clone()));
        }
    }
    Ok(None)
}

/// After `consumer` is merged or already current, the head of its base is what its own
/// dependants must pin: add it to `known`. A failed read replaces the outcome.
fn learn_head(
    runner: &impl Runner,
    all: &[Consumer],
    consumer: &Consumer,
    outcome: &Outcome,
    known: &mut Pins,
) -> Result<Option<Outcome>, Error> {
    if !matches!(outcome, Outcome::Merged { .. } | Outcome::UpToDate) {
        return Ok(None);
    }
    let mut wanted = false;
    for other in all {
        wanted |= other.name != consumer.name && other.pinned_rev(consumer.package())?.is_some();
    }
    if !wanted {
        return Ok(None);
    }
    let head = runner
        .run("git", &["fetch", "origin"], Some(&consumer.dir))
        .and_then(|_| runner.run("git", &["rev-parse", "origin/main"], Some(&consumer.dir)));
    Ok(match head {
        Ok(rev) => {
            known.push((consumer.package().to_string(), rev.trim().to_string()));
            None
        }
        Err(e) => Some(Outcome::Failed {
            step: "head".into(),
            output: e.to_string(),
        }),
    })
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
    let consumers = plan(root, dep, rev)?;
    let mut known: Pins = vec![(dep.to_string(), rev.to_string())];
    let mut done: Vec<(String, Outcome)> = Vec::new();
    for (i, consumer) in consumers.iter().enumerate() {
        let pins = pins_for(consumer, &known)?;
        let mut outcome = if let Some(on) = waits_on(consumer, &consumers[..i], &done)? {
            Outcome::Blocked { on }
        } else if pins.is_empty() {
            Outcome::UpToDate
        } else if options.dry_run {
            Outcome::Planned
        } else {
            bump(runner, forge_for, consumer, &pins, options)
        };
        if !options.dry_run {
            let head = learn_head(runner, &consumers, consumer, &outcome, &mut known)?;
            outcome = head.unwrap_or(outcome);
        }
        let failed = matches!(outcome, Outcome::Failed { .. });
        done.push((consumer.name.clone(), outcome));
        if failed {
            break;
        }
    }
    Ok(done)
}

pub use crate::pin_propagate_cli::{line, run, USAGE};
