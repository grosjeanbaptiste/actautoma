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
const HEAD: &str = "3333333333333333333333333333333333333333";
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

/// A repository whose package is `package` and that pins each of `deps` (name, rev).
fn repo_of(root: &Path, folder: &str, package: &str, deps: &[(&str, &str)]) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let mut text =
        format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n\n[dependencies]\n");
    for (name, rev) in deps {
        text.push_str(&format!(
            "{name} = {{ git = \"https://github.com/o/{name}\", rev = \"{rev}\" }}\n"
        ));
    }
    fs::write(dir.join("Cargo.toml"), text).unwrap();
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
            ["rev-parse", "origin/main"] => Ok(format!("{HEAD}\n")),
            ["rev-parse", "--abbrev-ref", "HEAD"] => Ok(format!("{BRANCH}\n")),
            ["log", "-1", "--format=%s"] => Ok("chore: pin actit\n".into()),
            ["log", "-1", "--format=%b"] => Ok("Body.\n".into()),
            _ => Ok(String::new()),
        }
    }
}

#[derive(Clone, Default)]
struct Hub {
    slug: String,
    created: Rc<RefCell<Vec<NewPullRequest>>>,
    merged: Rc<RefCell<Vec<u64>>>,
}

impl Hub {
    /// The forge of the worktree `.../<folder>/.worktrees/<branch>`: its slug names the folder.
    fn at(&self, wt: &Path) -> Hub {
        let folder = wt.parent().and_then(Path::parent).and_then(Path::file_name);
        Hub {
            slug: format!(
                "me/{}",
                folder.map_or(String::new(), |f| f.to_string_lossy().into_owned())
            ),
            ..self.clone()
        }
    }
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
    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        self.merged.borrow_mut().push(number);
        Ok(())
    }
    fn view_repo(&self) -> Result<Repo, Error> {
        Ok(Repo {
            slug: self.slug.clone(),
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
        merge: false,
        settle: std::time::Duration::ZERO,
    }
}

fn go(
    tools: &Tools,
    hub: &Hub,
    root: &Path,
    opts: &Options,
) -> Result<Vec<(String, Outcome)>, Error> {
    propagate(tools, &|wt: &Path| hub.at(wt), root, "actit", NEW, opts)
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

fn merging() -> Options {
    Options {
        gate: Some(vec!["gate".into()]),
        merge: true,
        ..options()
    }
}

fn two_levels(r: &Path, mid_actit: &str) {
    repo_of(r, "mid", "mid", &[("actit", mid_actit)]);
    repo_of(r, "top", "top", &[("actit", OLD), ("mid", OLD)]);
}

fn wt_manifest(r: &Path, folder: &str, branch: &str) -> String {
    let p = r
        .join(folder)
        .join(".worktrees")
        .join(branch.replace('/', "-"));
    fs::read_to_string(p.join("Cargo.toml")).unwrap()
}

#[test]
fn a_green_gate_and_a_clean_pull_request_are_merged() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert_eq!(
        out,
        vec![(
            "alpha".to_string(),
            Outcome::Merged {
                url: "https://example.test/o/r/pull/7".into()
            }
        )]
    );
    assert_eq!(*hub.merged.borrow(), [7]);
}

#[test]
fn without_a_gate_the_pull_request_is_held_not_merged() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());
    let opts = Options {
        gate: None,
        ..merging()
    };

    let out = go(&tools, &hub, &r, &opts).unwrap();

    assert!(matches!(&out[0].1, Outcome::Held { reason, .. } if reason.contains("no gate")));
    assert!(hub.merged.borrow().is_empty());
}

#[test]
fn a_repository_never_merged_automatically_is_held() {
    let r = root();
    repo(&r, "acteble_api", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert!(matches!(&out[0].1, Outcome::Held { reason, .. } if reason.contains("never merged")));
    assert!(hub.merged.borrow().is_empty());
}

#[test]
fn a_dry_run_that_asks_to_merge_still_touches_nothing() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());
    let opts = Options {
        dry_run: true,
        ..merging()
    };

    let out = go(&tools, &hub, &r, &opts).unwrap();

    assert_eq!(outcomes(&out), vec![("alpha", &Outcome::Planned)]);
    assert!(tools.ran().is_empty());
}

#[test]
fn a_consumer_that_pins_a_merged_consumer_gets_both_pins_in_one_pull_request() {
    let r = root();
    two_levels(&r, OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["mid", "top"]);
    assert!(out.iter().all(|(_, o)| matches!(o, Outcome::Merged { .. })));
    let top = wt_manifest(&r, "top", BRANCH);
    assert!(top.contains(&format!(
        "actit = {{ git = \"https://github.com/o/actit\", rev = \"{NEW}\" }}"
    )));
    assert!(top.contains(&format!(
        "mid = {{ git = \"https://github.com/o/mid\", rev = \"{HEAD}\" }}"
    )));
    let ran = tools.ran();
    assert!(
        ran.contains(&"cargo update -p actit -p mid".to_string()),
        "{ran:?}"
    );
    assert!(ran.contains(&"git commit -m chore: pin actit at 2222222, mid at 3333333".to_string()));
    assert_eq!(*hub.merged.borrow(), [7, 7]);
}

#[test]
fn without_merge_a_dependant_waits_for_the_consumer_it_pins() {
    let r = root();
    two_levels(&r, OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &options()).unwrap();

    assert_eq!(out[0].0, "mid");
    assert!(matches!(&out[0].1, Outcome::Opened { .. }));
    assert_eq!(
        (out[1].0.as_str(), &out[1].1),
        ("top", &Outcome::Blocked { on: "mid".into() })
    );
    let adds = tools
        .ran()
        .iter()
        .filter(|l| l.starts_with("git worktree add"))
        .count();
    assert_eq!(adds, 1);
}

#[test]
fn a_held_consumer_blocks_its_dependant() {
    let r = root();
    two_levels(&r, OLD);
    let (tools, hub) = (Tools::new(), Hub::default());
    let opts = Options {
        gate: None,
        ..merging()
    };

    let out = go(&tools, &hub, &r, &opts).unwrap();

    assert!(matches!(&out[0].1, Outcome::Held { .. }));
    assert_eq!(&out[1].1, &Outcome::Blocked { on: "mid".into() });
}

#[test]
fn a_dependant_of_an_up_to_date_consumer_pins_that_consumers_base_head() {
    let r = root();
    two_levels(&r, NEW);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert_eq!(out[0], ("mid".to_string(), Outcome::UpToDate));
    assert!(matches!(&out[1].1, Outcome::Merged { .. }));
    let ran = tools.ran();
    assert_eq!(ran[0], "git fetch origin");
    assert_eq!(ran[1], "git rev-parse origin/main");
    assert!(wt_manifest(&r, "top", BRANCH).contains(&format!("rev = \"{HEAD}\"")));
}

#[test]
fn a_failure_reading_a_merged_consumers_head_stops_the_walk() {
    let r = root();
    two_levels(&r, OLD);
    let tools = Tools {
        fail_on: Some("git rev-parse origin/main"),
        ..Tools::new()
    };
    let hub = Hub::default();

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0].1, Outcome::Failed { step, .. } if step == "head"));
}

#[test]
fn the_head_of_a_merged_consumer_is_read_only_when_a_dependant_pins_it() {
    let r = root();
    repo(&r, "alpha", OLD);
    repo(&r, "beta", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert!(out.iter().all(|(_, o)| matches!(o, Outcome::Merged { .. })));
    assert!(!tools
        .ran()
        .contains(&"git rev-parse origin/main".to_string()));
}

#[test]
fn a_merged_bump_removes_its_worktree_and_its_local_branch() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    assert!(matches!(&out[0].1, Outcome::Merged { .. }));
    let ran = tools.ran();
    let wt = worktree(&r, "alpha");
    assert!(
        ran.contains(&format!("git worktree remove {}", wt.display())),
        "{ran:?}"
    );
    assert!(ran.contains(&format!("git branch -D {BRANCH}")));
}

#[test]
fn an_open_bump_keeps_its_worktree_for_the_pull_request() {
    let r = root();
    repo(&r, "alpha", OLD);
    let (tools, hub) = (Tools::new(), Hub::default());

    go(&tools, &hub, &r, &options()).unwrap();

    assert!(!tools
        .ran()
        .iter()
        .any(|l| l.starts_with("git worktree remove")));
}

#[test]
fn a_failed_cleanup_after_a_merge_is_reported_with_the_merged_pull_request() {
    let r = root();
    repo(&r, "alpha", OLD);
    let tools = Tools {
        fail_on: Some("git worktree remove"),
        ..Tools::new()
    };
    let hub = Hub::default();

    let out = go(&tools, &hub, &r, &merging()).unwrap();

    match &out[0].1 {
        Outcome::Failed { step, output } => {
            assert_eq!(step, "cleanup");
            assert!(
                output.contains("https://example.test/o/r/pull/7"),
                "{output}"
            );
            assert!(output.contains("RED"), "{output}");
        }
        other => panic!("{other:?}"),
    }
}
