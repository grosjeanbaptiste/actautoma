//! `pin_propagate::propagate`: bump one dependency in every consumer, one worktree and one PR each.

use actautoma::forge::{
    Error, Forge, Mergeable, NewPullRequest, NewRepo, PullRequest, Repo, Runner,
};
use actautoma::pin_propagate::{propagate, Options, Outcome};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";
const BRANCH: &str = "chore/bump-actit-2222222";

fn root() -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("actautoma-pin-prop-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p); // a leftover of an earlier run: start empty
    fs::create_dir_all(&p).unwrap();
    p
}

fn manifest(rev: &str) -> String {
    format!(
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nactit = {{ git = \"https://github.com/o/actit\", rev = \"{rev}\" }}\n"
    )
}

fn repo(root: &Path, folder: &str, rev: &str) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("Cargo.toml"), manifest(rev)).unwrap();
}

fn worktree(root: &Path, folder: &str) -> PathBuf {
    root.join(folder)
        .join(".worktrees")
        .join(BRANCH.replace('/', "-"))
}

/// Every command run, as one line; `git worktree add` creates the folder with what `origin/main`
/// holds (`origin`, or the consumer's own manifest).
struct Tools {
    ran: RefCell<Vec<String>>,
    origin: Option<String>,
    fail_on: Option<&'static str>,
}

impl Tools {
    fn new() -> Self {
        Tools {
            ran: RefCell::default(),
            origin: None,
            fail_on: None,
        }
    }
    fn ran(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }
}

impl Runner for Tools {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error> {
        let line = format!("{program} {}", args.join(" "));
        self.ran.borrow_mut().push(line.clone());
        if self.fail_on.is_some_and(|p| line.starts_with(p)) {
            return Err(Error::Run(format!("{line}: RED")));
        }
        match args {
            ["worktree", "add", "-b", _, path, "origin/main"] => {
                // A relative path would write into the current directory, the crate under test.
                assert!(Path::new(path).is_absolute(), "worktree path {path:?}");
                fs::create_dir_all(path).unwrap();
                let text = self.origin.clone().unwrap_or_else(|| {
                    fs::read_to_string(dir.unwrap().join("Cargo.toml")).unwrap()
                });
                fs::write(Path::new(path).join("Cargo.toml"), text).unwrap();
                Ok(String::new())
            }
            ["rev-parse", "--abbrev-ref", "HEAD"] => Ok(format!("{BRANCH}\n")),
            ["log", "-1", "--format=%s"] => Ok("chore: pin actit\n".into()),
            ["log", "-1", "--format=%b"] => Ok("Body.\n".into()),
            _ => Ok(String::new()),
        }
    }
}

#[derive(Clone, Default)]
struct Hub {
    created: Rc<RefCell<Vec<NewPullRequest>>>,
}

impl Forge for Hub {
    fn view_pr(&self, _: Option<u64>) -> Result<PullRequest, Error> {
        Ok(PullRequest {
            number: 7,
            title: "t".into(),
            head: BRANCH.into(),
            base: "main".into(),
            author: "me".into(),
            mergeable: Mergeable::Mergeable,
            review: String::new(),
            merge_state: "CLEAN".into(),
        })
    }
    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error> {
        self.created.borrow_mut().push(pr.clone());
        Ok("https://example.test/o/r/pull/7".into())
    }
    fn merge_pr(&self, _: u64) -> Result<(), Error> {
        Err(Error::Unsupported("merge".into()))
    }
    fn view_repo(&self) -> Result<Repo, Error> {
        Ok(Repo {
            slug: "me/x".into(),
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
        gate: None,
        dry_run: false,
        settle: std::time::Duration::ZERO,
    }
}

fn go(
    tools: &Tools,
    hub: &Hub,
    root: &Path,
    opts: &Options,
) -> Result<Vec<(String, Outcome)>, Error> {
    propagate(tools, &|_: &Path| hub.clone(), root, "actit", NEW, opts)
}

fn outcomes(r: &[(String, Outcome)]) -> Vec<(&str, &Outcome)> {
    r.iter().map(|(n, o)| (n.as_str(), o)).collect()
}

#[test]
fn a_dry_run_plans_every_consumer_and_touches_nothing() {
    let r = root();
    repo(&r, "alpha", OLD);
    repo(&r, "beta", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());
    let opts = Options {
        dry_run: true,
        ..options()
    };

    let out = go(&tools, &hub, &r, &opts).unwrap();

    assert_eq!(
        outcomes(&out),
        vec![("alpha", &Outcome::Planned), ("beta", &Outcome::Planned)]
    );
    assert!(tools.ran().is_empty());
    assert!(hub.created.borrow().is_empty());
    assert_eq!(
        fs::read_to_string(r.join("alpha/Cargo.toml")).unwrap(),
        manifest(OLD)
    );
}

#[test]
fn a_consumer_already_on_the_revision_is_up_to_date_and_nothing_runs() {
    let r = root();
    repo(&r, "alpha", NEW);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &options()).unwrap();

    assert_eq!(outcomes(&out), vec![("alpha", &Outcome::UpToDate)]);
    assert!(tools.ran().is_empty());
}

#[test]
fn a_consumer_is_bumped_in_its_own_worktree_and_a_pull_request_is_opened() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &options()).unwrap();

    assert_eq!(
        out,
        vec![(
            "alpha".to_string(),
            Outcome::Opened {
                url: "https://example.test/o/r/pull/7".into()
            }
        )]
    );
    let wt = worktree(&r, "alpha");
    assert_eq!(
        fs::read_to_string(wt.join("Cargo.toml")).unwrap(),
        manifest(NEW)
    );
    assert_eq!(
        fs::read_to_string(r.join("alpha/Cargo.toml")).unwrap(),
        manifest(OLD)
    );
    let ran = tools.ran();
    let add = format!("git worktree add -b {BRANCH} {} origin/main", wt.display());
    let expected = [
        "git fetch origin",
        add.as_str(),
        "cargo update -p actit",
        "git add Cargo.toml Cargo.lock",
        "git commit -m chore: pin actit at 2222222",
    ];
    assert_eq!(ran[..5], expected);
    assert!(ran.contains(&format!("git push -u origin {BRANCH}")));
    let created = hub.created.borrow();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].head, BRANCH);
    assert_eq!(created[0].base, "main");
}

#[test]
fn consumers_are_bumped_in_plan_order() {
    let r = root();
    repo(&r, "beta", OLD);
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &options()).unwrap();

    let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    assert_eq!(hub.created.borrow().len(), 2);
}

#[test]
fn a_failing_step_stops_the_walk_and_names_the_step() {
    for (fail_on, step) in [
        ("git fetch", "fetch"),
        ("git worktree add", "worktree"),
        ("cargo update", "cargo update"),
        ("git commit", "commit"),
    ] {
        let r = root();
        repo(&r, "alpha", OLD);
        repo(&r, "beta", OLD);
        let tools = Tools {
            fail_on: Some(fail_on),
            ..Tools::new()
        };
        let hub = Hub::default();

        let out = go(&tools, &hub, &r, &options()).unwrap();

        assert_eq!(out.len(), 1, "{step}: the walk stops at the first failure");
        let (name, outcome) = &out[0];
        assert_eq!(name, "alpha");
        match outcome {
            Outcome::Failed { step: s, output } => {
                assert_eq!(s, step);
                assert!(output.contains("RED"), "{step}: {output}");
            }
            other => panic!("{step}: {other:?}"),
        }
        assert!(hub.created.borrow().is_empty(), "{step}");
    }
}

#[test]
fn a_failing_gate_is_a_failure_at_the_ship_step_and_opens_no_pull_request() {
    let r = root();
    repo(&r, "alpha", OLD);
    let tools = Tools {
        fail_on: Some("gate"),
        ..Tools::new()
    };
    let hub = Hub::default();
    let opts = Options {
        gate: Some(vec!["gate".into(), "--all".into()]),
        ..options()
    };

    let out = go(&tools, &hub, &r, &opts).unwrap();

    assert!(matches!(&out[0].1, Outcome::Failed { step, .. } if step == "ship"));
    assert!(hub.created.borrow().is_empty());
}

#[test]
fn a_base_that_already_holds_the_revision_is_up_to_date_and_its_worktree_is_removed() {
    let r = root();
    repo(&r, "alpha", OLD);
    let tools = Tools {
        origin: Some(manifest(NEW)),
        ..Tools::new()
    };
    let hub = Hub::default();

    let out = go(&tools, &hub, &r, &options()).unwrap();

    assert_eq!(outcomes(&out), vec![("alpha", &Outcome::UpToDate)]);
    let ran = tools.ran();
    assert!(
        ran.iter().any(|l| l.starts_with("git worktree remove")),
        "{ran:?}"
    );
    assert!(ran.contains(&format!("git branch -D {BRANCH}")));
    assert!(!ran.iter().any(|l| l.starts_with("cargo update")));
}

#[test]
fn a_base_without_the_dependency_is_a_failure_at_the_rewrite_step() {
    let r = root();
    repo(&r, "alpha", OLD);
    let tools = Tools {
        origin: Some("[package]\nname = \"x\"\n".into()),
        ..Tools::new()
    };
    let hub = Hub::default();

    let out = go(&tools, &hub, &r, &options()).unwrap();

    assert!(matches!(&out[0].1, Outcome::Failed { step, .. } if step == "rewrite"));
}

#[test]
fn a_plan_error_is_an_error_and_runs_nothing() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let e = propagate(
        &tools,
        &|_: &Path| hub.clone(),
        &r,
        "actit",
        "main",
        &options(),
    );

    assert!(e.is_err());
    assert!(tools.ran().is_empty());
}
