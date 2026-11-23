//! Reading and editing pull requests beyond create and merge: state, list, new base, close.

use crate::forge::{Error, Mergeable, Runner};
use crate::gh::Gh;

/// The lifecycle state of a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
    /// A value this crate has no name for, kept verbatim.
    Other(String),
}

impl PrState {
    fn parse(word: &str) -> PrState {
        match word {
            "OPEN" => PrState::Open,
            "CLOSED" => PrState::Closed,
            "MERGED" => PrState::Merged,
            other => PrState::Other(other.into()),
        }
    }
}

/// Which pull requests a listing keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrFilter {
    Open,
    Closed,
    Merged,
    All,
}

impl PrFilter {
    fn word(self) -> &'static str {
        match self {
            PrFilter::Open => "open",
            PrFilter::Closed => "closed",
            PrFilter::Merged => "merged",
            PrFilter::All => "all",
        }
    }
}

/// One line of a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub head: String,
    pub base: String,
    pub author: String,
    pub state: PrState,
    pub mergeable: Mergeable,
}

/// The pull-request verbs that read or edit one without creating or merging it.
pub trait Pulls {
    fn pr_state(&self, number: Option<u64>) -> Result<PrState, Error>;
    fn list_prs(&self, filter: PrFilter) -> Result<Vec<PrSummary>, Error>;
    fn retarget_pr(&self, number: u64, base: &str) -> Result<(), Error>;
    fn close_pr(&self, number: u64, comment: Option<&str>) -> Result<(), Error>;
}

/// Unit separator: cannot occur in a title, unlike `|` or a tab.
const LIST_JQ: &str = r#".[]|[.number,.title,.headRefName,.baseRefName,.author.login,.state,.mergeable]|map(tostring)|join("\u001f")"#;
const LIST_FIELDS: &str = "number,title,headRefName,baseRefName,author,state,mergeable";

fn summary(line: &str) -> Result<PrSummary, Error> {
    let f: Vec<&str> = line.split('\u{1f}').collect();
    let [number, title, head, base, author, state, mergeable] = f[..] else {
        return Err(Error::Parse(line.into()));
    };
    Ok(PrSummary {
        number: number.parse().map_err(|_| Error::Parse(line.into()))?,
        title: title.into(),
        head: head.into(),
        base: base.into(),
        author: author.into(),
        state: PrState::parse(state),
        mergeable: match mergeable {
            "MERGEABLE" => Mergeable::Mergeable,
            "CONFLICTING" => Mergeable::Conflicting,
            "UNKNOWN" => Mergeable::Unknown,
            other => Mergeable::Other(other.into()),
        },
    })
}

impl<R: Runner> Pulls for Gh<R> {
    fn pr_state(&self, number: Option<u64>) -> Result<PrState, Error> {
        let n = number.map(|n| n.to_string());
        let mut args = vec!["pr", "view"];
        args.extend(n.as_deref());
        args.extend(["--json", "state", "--jq", ".state"]);
        Ok(PrState::parse(self.gh(&args)?.trim_end_matches('\n')))
    }

    fn list_prs(&self, filter: PrFilter) -> Result<Vec<PrSummary>, Error> {
        let raw = self.gh(&[
            "pr",
            "list",
            "--state",
            filter.word(),
            "--limit",
            "1000",
            "--json",
            LIST_FIELDS,
            "--jq",
            LIST_JQ,
        ])?;
        let parsed: Result<Vec<PrSummary>, Error> =
            raw.lines().filter(|l| !l.is_empty()).map(summary).collect();
        // The whole raw answer is the evidence when one line is wrong.
        parsed.map_err(|e| match e {
            Error::Parse(_) => Error::Parse(raw.clone()),
            other => other,
        })
    }

    fn retarget_pr(&self, number: u64, base: &str) -> Result<(), Error> {
        self.gh(&["pr", "edit", &number.to_string(), "--base", base])
            .map(drop)
    }

    fn close_pr(&self, number: u64, comment: Option<&str>) -> Result<(), Error> {
        let n = number.to_string();
        let mut args = vec!["pr", "close", n.as_str()];
        args.extend(comment.into_iter().flat_map(|c| ["--comment", c]));
        self.gh(&args).map(drop)
    }
}
