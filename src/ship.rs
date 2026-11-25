//! `actautoma ship`: the gate, the push, the pull request and, where it is safe, the merge.
//!
//! The merge happens only when the repository's gate was run green on the commit pushed, the
//! platform calls the pull request mergeable and its merge state clean (no conflict, branch up
//! to date, nothing blocking). Anything else stops after the pull request is open and says why.
//! Repositories in [`NEVER_MERGED`] are never merged by it, whatever their state.

use std::time::Duration;

use crate::forge::{Error, Forge, Mergeable, NewPullRequest, Runner};
use std::path::Path;

/// Repositories (by name, any owner) `ship` opens pull requests in but never merges.
pub const NEVER_MERGED: [&str; 2] = ["acteble_api", "melos_apps"];

/// What to ship and how far to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The repository's gate: program and arguments, run before the push. Without one, nothing
    /// is merged.
    pub gate: Option<Vec<String>>,
    /// Merge when it is safe; `false` stops after the pull request is open.
    pub merge: bool,
    /// The pull request body; the last commit's body when absent.
    pub body: Option<String>,
    /// Pause between reads while the platform still computes mergeability.
    pub settle: Duration,
}

/// Reads of the pull request while the platform answers UNKNOWN.
const SETTLE_READS: u32 = 8;

/// Where `ship` stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The pull request was merged.
    Merged { number: u64, url: String },
    /// The pull request is open and was not merged, for `reason`.
    Held {
        number: u64,
        url: String,
        reason: String,
    },
}

fn fail(message: impl Into<String>) -> Error {
    Error::Run(message.into())
}

fn git(runner: &impl Runner, dir: &Path, args: &[&str]) -> Result<String, Error> {
    runner.run("git", args, Some(dir))
}

/// Why `ship` does not merge, if it does not.
fn hold(
    options: &Options,
    repo_name: &str,
    mergeable: &Mergeable,
    merge_state: &str,
) -> Option<String> {
    if !options.merge {
        return Some("asked not to merge".into());
    }
    if NEVER_MERGED.contains(&repo_name) {
        return Some(format!("`{repo_name}` is never merged automatically"));
    }
    if options.gate.is_none() {
        return Some("no gate given, so nothing proves the commit is good".into());
    }
    match mergeable {
        Mergeable::Mergeable => {}
        Mergeable::Conflicting => return Some("the pull request has a conflict".into()),
        other => return Some(format!("the platform says {other}, not MERGEABLE")),
    }
    (merge_state != "CLEAN").then(|| format!("merge state is {merge_state}, not CLEAN"))
}

/// Runs the gate, pushes the current branch, opens (or finds) its pull request and merges it
/// when [`Outcome`] allows.
///
/// # Errors
/// On the default branch or a detached head, with uncommitted changes, a failing gate, push or
/// platform call: nothing later runs.
pub fn ship(
    forge: &impl Forge,
    runner: &impl Runner,
    dir: &Path,
    options: &Options,
) -> Result<Outcome, Error> {
    let branch = git(runner, dir, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_owned();
    let repo = forge.view_repo()?;
    if branch == "HEAD" {
        return Err(fail("a detached head cannot be shipped"));
    }
    if branch == repo.default_branch {
        return Err(fail(format!(
            "`{branch}` is the default branch: ship a feature branch"
        )));
    }
    if !git(runner, dir, &["status", "--porcelain"])?
        .trim()
        .is_empty()
    {
        return Err(fail("uncommitted changes: commit or set them aside first"));
    }
    if let Some([program, args @ ..]) = options.gate.as_deref() {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        runner
            .run(program, &args, Some(dir))
            .map_err(|e| fail(format!("the gate failed, nothing was pushed: {e}")))?;
    }
    git(runner, dir, &["push", "-u", "origin", &branch])?;
    let subject = git(runner, dir, &["log", "-1", "--format=%s"])?
        .trim()
        .to_owned();
    let body = match &options.body {
        Some(body) => body.clone(),
        None => git(runner, dir, &["log", "-1", "--format=%b"])?
            .trim()
            .to_owned(),
    };
    let url = match forge.create_pr(&NewPullRequest {
        base: repo.default_branch.clone(),
        head: branch,
        title: subject,
        body,
    }) {
        Ok(url) => url,
        Err(Error::Run(m)) if m.contains("already exists") => String::new(),
        Err(e) => return Err(e),
    };
    let mut pr = forge.view_pr(None)?;
    for _ in 1..SETTLE_READS {
        if pr.mergeable != Mergeable::Unknown {
            break;
        }
        std::thread::sleep(options.settle);
        pr = forge.view_pr(None)?;
    }
    let name = repo.slug.rsplit('/').next().unwrap_or(&repo.slug);
    if let Some(reason) = hold(options, name, &pr.mergeable, &pr.merge_state) {
        return Ok(Outcome::Held {
            number: pr.number,
            url,
            reason,
        });
    }
    forge.merge_pr(pr.number)?;
    Ok(Outcome::Merged {
        number: pr.number,
        url,
    })
}
