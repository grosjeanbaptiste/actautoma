//! The pull-request verbs of `actrepo` that read or edit a PR beyond create and merge:
//! its state, the list, a new base, a close (through `gh`).

use actautoma::forge::{Error, Mergeable, Runner};
use actautoma::gh::Gh;
use actautoma::pulls::{PrFilter, PrState, PrSummary, Pulls};
use std::cell::RefCell;
use std::path::Path;

const US: char = '\u{1f}';

struct Fake {
    calls: RefCell<Vec<Vec<String>>>,
    answers: RefCell<Vec<Result<String, Error>>>,
}

impl Fake {
    fn answering(answers: Vec<Result<String, Error>>) -> Fake {
        Fake {
            calls: RefCell::default(),
            answers: RefCell::new(answers),
        }
    }
    fn args(&self, i: usize) -> Vec<String> {
        self.calls.borrow()[i].clone()
    }
}

impl Runner for &Fake {
    fn run(&self, program: &str, args: &[&str], _dir: Option<&Path>) -> Result<String, Error> {
        assert_eq!(program, "gh");
        self.calls
            .borrow_mut()
            .push(args.iter().map(|a| a.to_string()).collect());
        self.answers.borrow_mut().remove(0)
    }
}

fn line(n: u64, state: &str, mergeable: &str) -> String {
    format!("{n}{US}Add x{US}feat{US}main{US}bob{US}{state}{US}{mergeable}\n")
}

#[test]
fn the_state_of_a_pr_is_asked_with_or_without_a_number() {
    let f = Fake::answering(vec![Ok("MERGED\n".into()), Ok("OPEN\n".into())]);
    assert_eq!(Gh::new(&f).pr_state(Some(8)).unwrap(), PrState::Merged);
    assert_eq!(
        f.args(0),
        ["pr", "view", "8", "--json", "state", "--jq", ".state"]
    );
    assert_eq!(Gh::new(&f).pr_state(None).unwrap(), PrState::Open);
    assert_eq!(
        f.args(1),
        ["pr", "view", "--json", "state", "--jq", ".state"]
    );
}

#[test]
fn closed_and_an_unknown_state_are_kept_not_guessed() {
    let f = Fake::answering(vec![Ok("CLOSED\n".into()), Ok("DRAFT\n".into())]);
    assert_eq!(Gh::new(&f).pr_state(Some(1)).unwrap(), PrState::Closed);
    assert_eq!(
        Gh::new(&f).pr_state(Some(1)).unwrap(),
        PrState::Other("DRAFT".into())
    );
}

#[test]
fn a_state_failure_reaches_the_caller() {
    let f = Fake::answering(vec![Err(Error::Run("no auth".into()))]);
    assert_eq!(
        Gh::new(&f).pr_state(Some(1)),
        Err(Error::Run("no auth".into()))
    );
}

#[test]
fn the_list_asks_for_the_filter_and_reads_one_summary_per_line() {
    let raw = line(3, "OPEN", "MERGEABLE")
        + &line(4, "MERGED", "UNKNOWN")
        + &line(5, "CLOSED", "CONFLICTING");
    let f = Fake::answering(vec![Ok(raw)]);
    let prs = Gh::new(&f).list_prs(PrFilter::All).unwrap();
    let a = f.args(0);
    assert_eq!(&a[..6], ["pr", "list", "--state", "all", "--limit", "1000"]);
    assert!(a.contains(&"--json".to_string()) && a.contains(&"--jq".to_string()));
    assert_eq!(prs.len(), 3);
    assert_eq!(
        prs[0],
        PrSummary {
            number: 3,
            title: "Add x".into(),
            head: "feat".into(),
            base: "main".into(),
            author: "bob".into(),
            state: PrState::Open,
            mergeable: Mergeable::Mergeable,
        }
    );
    assert_eq!(
        (prs[1].state.clone(), prs[1].mergeable.clone()),
        (PrState::Merged, Mergeable::Unknown)
    );
    assert_eq!(
        (prs[2].state.clone(), prs[2].mergeable.clone()),
        (PrState::Closed, Mergeable::Conflicting)
    );
}

#[test]
fn each_filter_has_its_gh_word() {
    for (filter, word) in [
        (PrFilter::Open, "open"),
        (PrFilter::Closed, "closed"),
        (PrFilter::Merged, "merged"),
        (PrFilter::All, "all"),
    ] {
        let f = Fake::answering(vec![Ok(String::new())]);
        assert_eq!(Gh::new(&f).list_prs(filter).unwrap(), []);
        assert_eq!(f.args(0)[3], word);
    }
}

#[test]
fn a_list_line_of_the_wrong_shape_or_a_non_numeric_number_is_a_parse_error() {
    let f = Fake::answering(vec![Ok("3\u{1f}x\n".into())]);
    assert_eq!(
        Gh::new(&f).list_prs(PrFilter::Open),
        Err(Error::Parse("3\u{1f}x\n".into()))
    );
    let bad = line(0, "OPEN", "MERGEABLE").replacen('0', "x", 1);
    let f = Fake::answering(vec![Ok(bad.clone())]);
    assert_eq!(Gh::new(&f).list_prs(PrFilter::Open), Err(Error::Parse(bad)));
}

#[test]
fn a_list_failure_reaches_the_caller() {
    let f = Fake::answering(vec![Err(Error::Run("rate limit".into()))]);
    assert_eq!(
        Gh::new(&f).list_prs(PrFilter::Open),
        Err(Error::Run("rate limit".into()))
    );
}

#[test]
fn retargeting_edits_the_base_of_that_pr() {
    let f = Fake::answering(vec![Ok(String::new())]);
    Gh::new(&f).retarget_pr(8, "main").unwrap();
    assert_eq!(f.args(0), ["pr", "edit", "8", "--base", "main"]);
    let f = Fake::answering(vec![Err(Error::Run("closed".into()))]);
    assert_eq!(
        Gh::new(&f).retarget_pr(8, "main"),
        Err(Error::Run("closed".into()))
    );
}

#[test]
fn closing_may_carry_a_comment() {
    let f = Fake::answering(vec![Ok(String::new()), Ok(String::new())]);
    Gh::new(&f).close_pr(8, Some("contained in #11")).unwrap();
    assert_eq!(
        f.args(0),
        ["pr", "close", "8", "--comment", "contained in #11"]
    );
    Gh::new(&f).close_pr(9, None).unwrap();
    assert_eq!(f.args(1), ["pr", "close", "9"]);
    let f = Fake::answering(vec![Err(Error::Run("gone".into()))]);
    assert_eq!(
        Gh::new(&f).close_pr(8, None),
        Err(Error::Run("gone".into()))
    );
}
