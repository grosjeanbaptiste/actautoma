//! Advance a consumer's own checkout to `origin/main` after its bump was merged.
//!
//! Only a checkout that is on `main` with no tracked changes moves, and only by fast-forward. Any
//! other state is left alone and named, so the owner or the session that holds it decides.

use crate::forge::Runner;
use std::path::Path;

/// `Ok(())` when the checkout at `dir` now holds `origin/main`; `Err(why)` when it was left as
/// it was (or the fast-forward was refused), with the reason.
///
/// It fetches `origin` first: the merge happened after the bump's own fetch.
pub fn advance_checkout(runner: &impl Runner, dir: &Path) -> Result<(), String> {
    let git = |args: &[&str]| runner.run("git", args, Some(dir));
    git(&["fetch", "-q", "origin"]).map_err(|e| format!("fetch failed: {e}"))?;
    let branch = git(&["symbolic-ref", "--short", "HEAD"])
        .map_err(|e| format!("cannot read its branch: {e}"))?;
    let branch = branch.trim();
    if branch != "main" {
        return Err(format!("it is on `{branch}`, not main"));
    }
    let status = git(&["status", "--porcelain", "--untracked-files=no"])
        .map_err(|e| format!("cannot read its status: {e}"))?;
    if !status.trim().is_empty() {
        return Err("it has uncommitted changes".into());
    }
    git(&["merge", "--ff-only", "origin/main"])
        .map(drop)
        .map_err(|e| format!("fast-forward refused: {e}"))
}
