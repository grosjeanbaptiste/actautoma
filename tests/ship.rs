//! `actautoma ship`: gate, push, pull request, and a merge only where it is safe.

use actautoma::forge::{
    Error, Forge, Mergeable, NewPullRequest, NewRepo, PullRequest, Repo, Runner,
};
use actautoma::ship::{ship, Options, Outcome, NEVER_MERGED};
use std::cell::RefCell;
use std::path::Path;

struct Tools {
    /// Every command run, as one line.
    ran: RefCell<Vec<String>>,
    status: &'static str,
    gate_fails: bool,
}

impl Tools {
    fn new() -> Self {
        Tools {
            ran: RefCell::default(),
            status: "",
            gate_fails: false,
        }
    }
    fn ran(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }
}

impl Runner for &Tools {
    fn run(&self, program: &str, args: &[&str], _dir: Option<&Path>) -> Result<String, Error> {
        let line = format!("{program} {}", args.join(" "));
        self.ran.borrow_mut().push(line.clone());
        match line.as_str() {
            "git rev-parse --abbrev-ref HEAD" => Ok("feat/x\n".into()),
            "git status --porcelain" => Ok(self.status.into()),
            "git log -1 --format=%s" => Ok("feat: add x\n".into()),
            "git log -1 --format=%b" => Ok("Body of x.\n".into()),
            "gate --all" if self.gate_fails => Err(Error::Run("gate: RED".into())),
            _ => Ok(String::new()),
        }
    }
}

struct Hub {
    slug: &'static str,
    merge_state: &'static str,
    mergeable: Mergeable,
    already_open: bool,
    unknown_reads: std::cell::Cell<u32>,
    merged: RefCell<Vec<u64>>,
    created: RefCell<Vec<NewPullRequest>>,
}

impl Hub {
    fn new(slug: &'static str) -> Self {
        Hub {
            slug,
            merge_state: "CLEAN",
            mergeable: Mergeable::Mergeable,
            already_open: false,
            unknown_reads: std::cell::Cell::new(0),
            merged: RefCell::default(),
            created: RefCell::default(),
        }
    }
}

impl Forge for Hub {
    fn view_pr(&self, _: Option<u64>) -> Result<PullRequest, Error> {
        Ok(PullRequest {
            number: 7,
            title: "feat: add x".into(),
            head: "feat/x".into(),
            base: "main".into(),
            author: "me".into(),
            mergeable: if self.unknown_reads.get() > 0 {
                self.unknown_reads.set(self.unknown_reads.get() - 1);
                Mergeable::Unknown
            } else {
                self.mergeable.clone()
            },
            review: String::new(),
            merge_state: self.merge_state.into(),
        })
    }
    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error> {
        if self.already_open {
            return Err(Error::Run(
                "a pull request for branch feat/x already exists".into(),
            ));
        }
        self.created.borrow_mut().push(pr.clone());
        Ok("https://example.test/o/r/pull/7".into())
    }
    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        self.merged.borrow_mut().push(number);
        Ok(())
    }
    fn view_repo(&self) -> Result<Repo, Error> {
        Ok(Repo {
            slug: self.slug.into(),
            default_branch: "main".into(),
        })
    }
    fn set_default_branch(&self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported("x".into()))
    }
    fn create_repo(&self, _: &NewRepo) -> Result<(), Error> {
        Err(Error::Unsupported("x".into()))
    }
    fn available(&self) -> Result<(), Error> {
        Ok(())
    }
}

fn options() -> Options {
    Options {
        gate: Some(vec!["gate".into(), "--all".into()]),
        merge: true,
        body: None,
        settle: std::time::Duration::ZERO,
        ci_wait: std::time::Duration::ZERO,
    }
}

fn go(tools: &Tools, hub: &Hub, options: &Options) -> Result<Outcome, Error> {
    ship(hub, &tools, Path::new("."), options)
}

#[test]
fn a_green_gate_and_a_clean_pull_request_are_merged() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    let out = go(&tools, &hub, &options()).unwrap();
    assert_eq!(
        out,
        Outcome::Merged {
            number: 7,
            url: "https://example.test/o/r/pull/7".into()
        }
    );
    assert_eq!(*hub.merged.borrow(), [7]);
}

#[test]
fn the_gate_runs_before_the_push_and_the_push_before_the_pull_request() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    go(&tools, &hub, &options()).unwrap();
    let ran = tools.ran();
    let at = |what: &str| ran.iter().position(|l| l == what).unwrap();
    assert!(
        at("gate --all") < at("git push -u origin feat/x"),
        "{ran:?}"
    );
}

#[test]
fn a_red_gate_stops_everything_and_says_so() {
    let (mut tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    tools.gate_fails = true;
    let err = go(&tools, &hub, &options()).unwrap_err().to_string();
    assert!(err.contains("gate") && err.contains("RED"), "{err}");
    assert!(!tools.ran().iter().any(|l| l.starts_with("git push")));
    assert!(hub.merged.borrow().is_empty() && hub.created.borrow().is_empty());
}

#[test]
fn a_dirty_tree_is_refused_before_anything_runs() {
    let (mut tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    tools.status = " M src/lib.rs\n";
    let err = go(&tools, &hub, &options()).unwrap_err().to_string();
    assert!(err.contains("uncommitted"), "{err}");
    assert!(!tools
        .ran()
        .iter()
        .any(|l| l.starts_with("git push") || l == "gate --all"));
}

#[test]
fn the_default_branch_is_never_shipped() {
    struct OnMain;
    impl Runner for OnMain {
        fn run(&self, _: &str, args: &[&str], _: Option<&Path>) -> Result<String, Error> {
            Ok(if args.contains(&"--abbrev-ref") {
                "main\n".into()
            } else {
                String::new()
            })
        }
    }
    let hub = Hub::new("me/actindexer");
    let err = ship(&hub, &OnMain, Path::new("."), &options())
        .unwrap_err()
        .to_string();
    assert!(err.contains("main"), "{err}");
}

#[test]
fn the_pull_request_takes_the_last_commit_subject_and_body() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    go(&tools, &hub, &options()).unwrap();
    let made = hub.created.borrow();
    assert_eq!(made[0].title, "feat: add x");
    assert!(made[0].body.contains("Body of x."));
    assert_eq!(
        (made[0].base.as_str(), made[0].head.as_str()),
        ("main", "feat/x")
    );
}

#[test]
fn a_pull_request_already_open_is_reused_not_created_twice() {
    let (tools, mut hub) = (Tools::new(), Hub::new("me/actindexer"));
    hub.already_open = true;
    let out = go(&tools, &hub, &options()).unwrap();
    assert!(matches!(out, Outcome::Merged { number: 7, .. }), "{out:?}");
}

#[test]
fn a_conflict_holds_the_merge_with_the_reason() {
    let (tools, mut hub) = (Tools::new(), Hub::new("me/actindexer"));
    hub.mergeable = Mergeable::Conflicting;
    let out = go(&tools, &hub, &options()).unwrap();
    assert!(
        matches!(&out, Outcome::Held { reason, .. } if reason.contains("conflict")),
        "{out:?}"
    );
    assert!(hub.merged.borrow().is_empty());
}

#[test]
fn a_branch_behind_its_base_or_blocked_is_not_merged() {
    for state in ["BEHIND", "BLOCKED", "UNSTABLE", "DIRTY"] {
        let (tools, mut hub) = (Tools::new(), Hub::new("me/actindexer"));
        hub.merge_state = state;
        let out = go(&tools, &hub, &options()).unwrap();
        assert!(
            matches!(&out, Outcome::Held { reason, .. } if reason.contains(state)),
            "{state}: {out:?}"
        );
        assert!(hub.merged.borrow().is_empty(), "{state}");
    }
}

#[test]
fn a_mergeability_not_computed_yet_holds_the_merge() {
    let (tools, mut hub) = (Tools::new(), Hub::new("me/actindexer"));
    hub.mergeable = Mergeable::Unknown;
    let out = go(&tools, &hub, &options()).unwrap();
    assert!(matches!(out, Outcome::Held { .. }), "{out:?}");
}

#[test]
fn a_mergeability_computed_a_moment_later_is_waited_for() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    hub.unknown_reads.set(3);
    let out = go(&tools, &hub, &options()).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
}

#[test]
fn without_a_gate_nothing_is_merged() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    let opts = Options {
        gate: None,
        ..options()
    };
    let out = go(&tools, &hub, &opts).unwrap();
    assert!(
        matches!(&out, Outcome::Held { reason, .. } if reason.contains("gate")),
        "{out:?}"
    );
    assert!(hub.merged.borrow().is_empty());
}

#[test]
fn no_merge_pushes_and_opens_the_pull_request_only() {
    let (tools, hub) = (Tools::new(), Hub::new("me/actindexer"));
    let opts = Options {
        merge: false,
        ..options()
    };
    let out = go(&tools, &hub, &opts).unwrap();
    assert!(matches!(out, Outcome::Held { .. }), "{out:?}");
    assert_eq!(hub.created.borrow().len(), 1);
    assert!(hub.merged.borrow().is_empty());
}

#[test]
fn acteble_api_and_melos_apps_are_never_merged_whatever_the_owner() {
    assert_eq!(NEVER_MERGED, ["acteble_api", "melos_apps"]);
    for slug in ["me/acteble_api", "someone-else/melos_apps"] {
        let (tools, hub) = (Tools::new(), Hub::new(slug));
        let out = go(&tools, &hub, &options()).unwrap();
        assert!(
            matches!(&out, Outcome::Held { reason, .. } if reason.contains("never merged automatically")),
            "{slug}: {out:?}"
        );
        assert!(hub.merged.borrow().is_empty(), "{slug}");
        assert_eq!(
            hub.created.borrow().len(),
            1,
            "{slug}: the PR is still opened"
        );
    }
}

#[test]
fn a_repository_whose_name_only_contains_an_excluded_one_is_merged() {
    let (tools, hub) = (Tools::new(), Hub::new("me/acteble_api_docs"));
    let out = go(&tools, &hub, &options()).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
}

/// A forge whose CI runs through `states` (one per read, the last repeating) and whose pull
/// request turns CLEAN once a run ends in `success`.
struct Ci {
    hub: Hub,
    states: RefCell<Vec<&'static str>>,
    reads: std::cell::Cell<u32>,
}

impl Ci {
    fn new(states: &[&'static str], pr_state: &'static str) -> Self {
        let mut hub = Hub::new("me/actindexer");
        hub.merge_state = pr_state;
        Ci {
            hub,
            states: RefCell::new(states.iter().rev().copied().collect()),
            reads: std::cell::Cell::new(0),
        }
    }
    fn clean(&self) -> bool {
        self.states.borrow().last() == Some(&"success") && self.states.borrow().len() == 1
    }
}

impl Forge for Ci {
    fn view_pr(&self, n: Option<u64>) -> Result<PullRequest, Error> {
        let mut pr = self.hub.view_pr(n)?;
        if self.clean() {
            pr.merge_state = "CLEAN".into();
        }
        Ok(pr)
    }
    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error> {
        self.hub.create_pr(pr)
    }
    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        self.hub.merge_pr(number)
    }
    fn view_repo(&self) -> Result<Repo, Error> {
        self.hub.view_repo()
    }
    fn set_default_branch(&self, s: &str, b: &str) -> Result<(), Error> {
        self.hub.set_default_branch(s, b)
    }
    fn create_repo(&self, r: &NewRepo) -> Result<(), Error> {
        self.hub.create_repo(r)
    }
    fn available(&self) -> Result<(), Error> {
        Ok(())
    }
    fn last_run(&self, branch: &str) -> Result<Option<actautoma::forge::CiRun>, Error> {
        assert_eq!(branch, "feat/x");
        self.reads.set(self.reads.get() + 1);
        let mut states = self.states.borrow_mut();
        let state = if states.len() > 1 {
            states.pop().unwrap()
        } else {
            states[0]
        };
        Ok(Some(actautoma::forge::CiRun {
            workflow: "ci".into(),
            state: state.into(),
            date: "2026-11-27".into(),
        }))
    }
}

fn waiting() -> Options {
    Options {
        ci_wait: std::time::Duration::from_millis(200),
        settle: std::time::Duration::from_millis(1),
        ..options()
    }
}

#[test]
fn a_run_still_going_is_waited_for_then_the_clean_pull_request_is_merged() {
    let tools = Tools::new();
    let ci = Ci::new(&["queued", "in_progress", "success"], "UNSTABLE");
    let out = ship(&ci, &&tools, Path::new("."), &waiting()).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    assert_eq!(*ci.hub.merged.borrow(), [7]);
}

#[test]
fn a_run_that_never_ends_holds_the_merge_and_names_the_wait() {
    let tools = Tools::new();
    let ci = Ci::new(&["in_progress"], "UNSTABLE");
    let out = ship(&ci, &&tools, Path::new("."), &waiting()).unwrap();
    assert!(
        matches!(&out, Outcome::Held { reason, .. } if reason.contains("CI") && reason.contains("200")),
        "{out:?}"
    );
    assert!(ci.hub.merged.borrow().is_empty());
}

#[test]
fn a_run_that_failed_holds_the_merge_without_waiting() {
    let tools = Tools::new();
    let ci = Ci::new(&["failure"], "UNSTABLE");
    let out = ship(&ci, &&tools, Path::new("."), &waiting()).unwrap();
    assert!(
        matches!(&out, Outcome::Held { reason, .. } if reason.contains("UNSTABLE")),
        "{out:?}"
    );
    assert_eq!(ci.reads.get(), 1, "a finished run is read once");
    assert!(ci.hub.merged.borrow().is_empty());
}

#[test]
fn without_a_ci_wait_the_run_is_never_read() {
    let tools = Tools::new();
    let ci = Ci::new(&["in_progress"], "UNSTABLE");
    let opts = Options {
        ci_wait: std::time::Duration::ZERO,
        ..waiting()
    };
    let out = ship(&ci, &&tools, Path::new("."), &opts).unwrap();
    assert!(matches!(out, Outcome::Held { .. }), "{out:?}");
    assert_eq!(ci.reads.get(), 0);
}

#[test]
fn a_forge_that_cannot_read_runs_holds_the_merge_as_before() {
    let (tools, mut hub) = (Tools::new(), Hub::new("me/actindexer"));
    hub.merge_state = "UNSTABLE";
    let out = go(&tools, &hub, &waiting()).unwrap();
    assert!(
        matches!(&out, Outcome::Held { reason, .. } if reason.contains("UNSTABLE")),
        "{out:?}"
    );
}
