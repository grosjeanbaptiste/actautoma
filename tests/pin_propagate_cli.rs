//! `actautoma pin-propagate DEP [--rev REV] [--root DIR] [--gate CMD] [--dry-run]`.

use actautoma::cli::dispatch;
use actautoma::forge::{Error, Runner};
use actautoma::gh::Gh;
use actautoma::process::ProcessRunner;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);
const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

/// Records every command; `answers` are the replies in order, then success with no output.
struct Tools {
    ran: RefCell<Vec<(String, Option<PathBuf>)>>,
    answers: RefCell<Vec<Result<String, Error>>>,
}

impl Runner for Tools {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error> {
        self.ran.borrow_mut().push((
            format!("{program} {}", args.join(" ")),
            dir.map(Path::to_path_buf),
        ));
        let mut answers = self.answers.borrow_mut();
        if answers.is_empty() {
            Ok(String::new())
        } else {
            answers.remove(0)
        }
    }
}

fn tools(answers: Vec<Result<String, Error>>) -> Tools {
    Tools {
        ran: RefCell::default(),
        answers: RefCell::new(answers),
    }
}

fn root() -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("actautoma-pin-cli-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&p); // a leftover of an earlier run: start empty
    fs::create_dir_all(&p).unwrap();
    p
}

fn repo(root: &Path, folder: &str, rev: &str) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{folder}\"\nversion = \"0.1.0\"\n\n[dependencies]\nactit = {{ git = \"https://github.com/o/actit\", rev = \"{rev}\" }}\n"),
    )
    .unwrap();
}

fn run(root: &Path, args: &[&str], tools: &Tools) -> (Result<(), Error>, String) {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    let mut out = Vec::new();
    let result = dispatch(
        &args,
        root,
        &|_| None,
        &Gh::new(ProcessRunner),
        tools,
        &mut out,
    );
    (result, String::from_utf8(out).unwrap())
}

#[test]
fn a_dry_run_names_each_consumer_and_runs_nothing() {
    let root = root();
    repo(&root, "actrepo", OLD);
    repo(&root, "actinstall", NEW);
    let t = tools(vec![]);
    let (result, out) = run(
        &root,
        &[
            "pin-propagate",
            "actit",
            "--rev",
            NEW,
            "--root",
            root.to_str().unwrap(),
            "--dry-run",
        ],
        &t,
    );
    result.unwrap();
    assert!(out.contains("actrepo: planned"), "{out}");
    assert!(out.contains("actinstall: up to date"), "{out}");
    assert!(t.ran.borrow().is_empty());
}

#[test]
fn without_a_rev_the_tip_of_the_dependency_origin_main_is_used() {
    let root = root();
    repo(&root, "actrepo", OLD);
    let t = tools(vec![Ok(String::new()), Ok(format!("{NEW}\n"))]);
    let (result, out) = run(
        &root,
        &[
            "pin-propagate",
            "actit",
            "--root",
            root.to_str().unwrap(),
            "--dry-run",
        ],
        &t,
    );
    result.unwrap();
    let ran = t.ran.borrow();
    let dep_dir = Some(root.join("actit"));
    assert_eq!(ran[0], ("git fetch origin".to_string(), dep_dir.clone()));
    assert_eq!(ran[1], ("git rev-parse origin/main".to_string(), dep_dir));
    assert!(out.contains("actrepo: planned"), "{out}");
}

#[test]
fn a_failed_step_is_reported_and_the_exit_is_an_error() {
    let root = root();
    repo(&root, "actrepo", OLD);
    let t = tools(vec![Err(Error::Run("no network".into()))]);
    let (result, out) = run(
        &root,
        &[
            "pin-propagate",
            "actit",
            "--rev",
            NEW,
            "--root",
            root.to_str().unwrap(),
        ],
        &t,
    );
    assert!(out.contains("actrepo: failed at fetch: "), "{out}");
    assert!(out.contains("no network"), "{out}");
    assert!(matches!(result, Err(Error::Run(m)) if m.contains("pin-propagate")));
}

#[test]
fn a_missing_dependency_or_an_unknown_option_prints_the_usage() {
    let root = root();
    for args in [
        vec!["pin-propagate"],
        vec!["pin-propagate", "actit", "--zap"],
        vec!["pin-propagate", "actit", "--rev"],
    ] {
        let (result, _) = run(&root, &args, &tools(vec![]));
        assert!(
            matches!(result, Err(Error::Run(m)) if m.contains("usage: actautoma pin-propagate")),
            "{args:?}"
        );
    }
}

#[test]
fn a_gate_is_accepted_and_an_empty_gate_is_refused() {
    let root = root();
    repo(&root, "actrepo", OLD);
    let r = root.to_str().unwrap();
    let base = [
        "pin-propagate",
        "actit",
        "--rev",
        NEW,
        "--root",
        r,
        "--dry-run",
    ];
    let with = |gate: &'static str| [&base[..], &["--gate", gate]].concat();
    let (result, out) = run(&root, &with("cargo test"), &tools(vec![]));
    result.unwrap();
    assert!(out.contains("actrepo: planned"), "{out}");
    let (result, _) = run(&root, &with("  "), &tools(vec![]));
    assert!(matches!(result, Err(Error::Run(m)) if m.contains("usage: actautoma pin-propagate")));
}

#[test]
fn a_borrowed_runner_runs_the_commands_of_the_runner_it_borrows() {
    let t = tools(vec![Ok("out".to_string())]);
    let borrowed = &t;
    let got = <&Tools as Runner>::run(&borrowed, "git", &["status"], None).unwrap();
    assert_eq!(got, "out");
    assert_eq!(t.ran.borrow()[0].0, "git status");
}

#[test]
fn a_merge_request_is_accepted_and_a_dry_run_still_touches_nothing() {
    let root = root();
    repo(&root, "actrepo", OLD);
    let t = tools(vec![]);
    let (result, out) = run(
        &root,
        &[
            "pin-propagate",
            "actit",
            "--rev",
            NEW,
            "--root",
            root.to_str().unwrap(),
            "--dry-run",
            "--merge",
        ],
        &t,
    );
    result.unwrap();
    assert!(out.contains("actrepo: planned"), "{out}");
    assert!(t.ran.borrow().is_empty());
}

#[test]
fn the_usage_names_the_merge_option() {
    assert!(actautoma::pin_propagate::USAGE.contains("[--merge]"));
}

#[test]
fn each_outcome_has_its_own_report_line() {
    use actautoma::pin_propagate::{line, Outcome};
    let url = || "https://example.test/pull/7".to_string();
    let cases = [
        (Outcome::UpToDate, "x: up to date"),
        (Outcome::Planned, "x: planned"),
        (
            Outcome::Opened { url: url() },
            "x: opened https://example.test/pull/7",
        ),
        (
            Outcome::Merged { url: url() },
            "x: merged https://example.test/pull/7",
        ),
        (
            Outcome::Held {
                url: url(),
                reason: "no gate".into(),
            },
            "x: held https://example.test/pull/7: no gate",
        ),
        (Outcome::Blocked { on: "mid".into() }, "x: blocked on mid"),
        (
            Outcome::Failed {
                step: "fetch".into(),
                output: "boom".into(),
            },
            "x: failed at fetch: boom",
        ),
    ];
    for (outcome, expected) in cases {
        assert_eq!(line(&("x".to_string(), outcome)), expected);
    }
}
