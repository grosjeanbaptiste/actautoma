//! Wait for a run of CI that is still going before `ship` decides whether to merge.
//!
//! A pull request whose checks have not finished is not CLEAN, so without the wait `ship` holds
//! every freshly pushed branch. The wait ends when the latest run of the branch ends, when the
//! forge cannot read runs, or after `ci_wait`; the pull request is then read again.

use crate::forge::{Error, Forge, Mergeable, PullRequest};
use std::time::{Duration, Instant};

/// Run states that mean the run has not ended (GitHub statuses, GitLab pipeline statuses).
const RUNNING: [&str; 6] = [
    "queued",
    "in_progress",
    "pending",
    "waiting",
    "requested",
    "running",
];

/// Waits for CI of `branch` when merging was asked, the pull request is mergeable but not CLEAN,
/// and `ci_wait` is not zero. Returns the pull request read last and `true` when the wait ran out
/// with a run still going.
///
/// # Errors
/// When the forge fails reading a run or the pull request. A forge that does not support reading
/// runs is not an error: there is nothing to wait for.
pub fn wait_for_ci(
    forge: &impl Forge,
    branch: &str,
    pr: PullRequest,
    merge: bool,
    settle: Duration,
    ci_wait: Duration,
) -> Result<(PullRequest, bool), Error> {
    if !merge
        || ci_wait.is_zero()
        || pr.mergeable != Mergeable::Mergeable
        || pr.merge_state == "CLEAN"
    {
        return Ok((pr, false));
    }
    let deadline = Instant::now() + ci_wait;
    loop {
        match forge.last_run(branch) {
            Ok(Some(run)) if RUNNING.contains(&run.state.as_str()) => {}
            Ok(_) | Err(Error::Unsupported(_)) => return Ok((forge.view_pr(None)?, false)),
            Err(e) => return Err(e),
        }
        if Instant::now() >= deadline {
            return Ok((forge.view_pr(None)?, true));
        }
        std::thread::sleep(settle);
    }
}
