//! The port: what a repository-hosting platform can do, whatever its CLI.

use std::fmt;
use std::path::Path;

/// Why a platform operation failed. Never swallowed: callers see it.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The platform tool could not be run, or exited non-zero.
    Run(String),
    /// The platform tool answered something this adapter does not understand.
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Run(m) => write!(f, "{m}"),
            Error::Parse(m) => write!(f, "unexpected answer: {m}"),
        }
    }
}

impl std::error::Error for Error {}

/// The seam to the outside world: runs a platform tool and returns its stdout.
pub trait Runner {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error>;
}

/// Whether the platform can merge a pull request now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    /// Not computed yet; platforms compute it lazily, ask again later.
    Unknown,
    /// A value this crate has no name for, kept verbatim.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub mergeable: Mergeable,
    pub review: String,
    pub merge_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPullRequest {
    pub base: String,
    pub head: String,
    pub title: String,
    pub body: String,
}

/// A repository-hosting platform. One adapter per platform tool (`gh`, `glab`, ...).
pub trait Forge {
    /// The pull request of the current branch, or number `number`.
    fn view_pr(&self, number: Option<u64>) -> Result<PullRequest, Error>;
    /// Opens a pull request and returns its URL.
    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error>;
    /// Merges pull request `number` and deletes its branch.
    fn merge_pr(&self, number: u64) -> Result<(), Error>;
}
