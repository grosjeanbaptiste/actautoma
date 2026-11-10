//! The port: what a repository-hosting platform can do, whatever its CLI.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a platform operation failed. Never swallowed: callers see it.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The platform tool could not be run, or exited non-zero.
    Run(String),
    /// The platform tool answered something this adapter does not understand.
    Parse(String),
    /// This adapter does not implement the operation: nothing was run.
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Run(m) => write!(f, "{m}"),
            Error::Parse(m) => write!(f, "unexpected answer: {m}"),
            Error::Unsupported(m) => write!(f, "not supported: {m}"),
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

impl fmt::Display for Mergeable {
    /// The platform's own word: `MERGEABLE`, `CONFLICTING`, `UNKNOWN`, or the kept value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mergeable::Mergeable => "MERGEABLE",
            Mergeable::Conflicting => "CONFLICTING",
            Mergeable::Unknown => "UNKNOWN",
            Mergeable::Other(v) => v,
        })
    }
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

/// A hosted repository, as the platform names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// `owner/name`.
    pub slug: String,
    pub default_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRepo {
    pub name: String,
    /// The local repository the hosted one is created from.
    pub source: PathBuf,
    pub private: bool,
    /// Push the local branches right after creating it.
    pub push: bool,
}

/// What a search of repository names found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSearch {
    /// The platform's own count of matches, which may exceed `names`.
    pub total: u64,
    /// `owner/name` of the best matches, as many as the platform returned (first page).
    pub names: Vec<String>,
}

/// A repository-hosting platform. One adapter per platform tool (`gh`, `glab`, ...).
pub trait Forge {
    /// The pull request of the current branch, or number `number`.
    fn view_pr(&self, number: Option<u64>) -> Result<PullRequest, Error>;
    /// Opens a pull request and returns its URL.
    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error>;
    /// Merges pull request `number` and deletes its branch.
    fn merge_pr(&self, number: u64) -> Result<(), Error>;
    /// The hosted repository of the working directory.
    fn view_repo(&self) -> Result<Repo, Error>;
    /// Makes `branch` the default branch of repository `slug`.
    fn set_default_branch(&self, slug: &str, branch: &str) -> Result<(), Error>;
    fn create_repo(&self, repo: &NewRepo) -> Result<(), Error>;
    /// Fails unless the platform tool is installed: lets a caller refuse before changing anything.
    fn available(&self) -> Result<(), Error>;

    /// The login of the authenticated user.
    fn user(&self) -> Result<String, Error> {
        Err(Error::Unsupported("user".into()))
    }
    /// Whether repository `owner/name` exists; a platform error is not `false`.
    fn repo_exists(&self, _owner: &str, _name: &str) -> Result<bool, Error> {
        Err(Error::Unsupported("repo_exists".into()))
    }
    /// Repositories, under any owner, whose name matches `text` (first page, best first).
    fn search_repos(&self, _text: &str) -> Result<RepoSearch, Error> {
        Err(Error::Unsupported("search_repos".into()))
    }
}

/// Parses `"<slug> <default branch>"`, the answer both adapters ask their tool for.
pub(crate) fn parse_repo(raw: String) -> Result<Repo, Error> {
    let mut words = raw.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some(slug), Some(branch), None) => Ok(Repo {
            slug: slug.into(),
            default_branch: branch.into(),
        }),
        _ => Err(Error::Parse(raw)),
    }
}
