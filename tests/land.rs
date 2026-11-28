//! `actautoma land`: commit, catch up (every conflict settled), gate, push, pull request, merge,
//! pull. Real git against a bare origin; only the hosting platform is faked.

use actautoma::forge::{
    Error, Forge, Mergeable, NewPullRequest, NewRepo, PullRequest, Repo,
};
use actautoma::land::{land, Options};
use actautoma::process::ProcessRunner;
use actautoma::ship::{Options as ShipOptions, Outcome};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
}

fn commit(dir: &Path, file: &str, text: &str) {
    write(dir, file, text);
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

struct World {
    work: PathBuf,
    peer: PathBuf,
    origin: PathBuf,
}

/// A bare origin with `main`, a working clone on `feat/x` and a peer clone.
fn world() -> World {
    let root = std::env::temp_dir().join(format!(
        "actautoma-land-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
    let origin = root.join("origin.git");
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
    commit(&work, "a.txt", "one\ntwo\nthree\n");
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&root, &["clone", "-q", origin.to_str().unwrap(), "peer"]);
    let peer = root.join("peer");
    git(&peer, &["config", "user.name", "t"]);
    git(&peer, &["config", "user.email", "t@t"]);
    git(&work, &["switch", "-q", "-c", "feat/x"]);
    World { work, peer, origin }
}

fn peer_ships(w: &World, file: &str, text: &str) {
    commit(&w.peer, file, text);
    git(&w.peer, &["push", "-q", "origin", "main"]);
}

/// The platform: a merge fast-forwards `main` on the bare origin to the pushed branch.
struct Hub {
    origin: PathBuf,
    merged: RefCell<Vec<u64>>,
}

impl Forge for Hub {
    fn view_pr(&self, _: Option<u64>) -> Result<PullRequest, Error> {
        Ok(PullRequest {
            number: 7,
            title: "t".into(),
            head: "feat/x".into(),
            base: "main".into(),
            author: "me".into(),
            mergeable: Mergeable::Mergeable,
            review: String::new(),
            merge_state: "CLEAN".into(),
        })
    }
    fn create_pr(&self, _: &NewPullRequest) -> Result<String, Error> {
        Ok("https://example.test/o/r/pull/7".into())
    }
    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        let tip = git(&self.origin, &["rev-parse", "refs/heads/feat/x"]);
        git(&self.origin, &["update-ref", "refs/heads/main", &tip]);
        self.merged.borrow_mut().push(number);
        Ok(())
    }
    fn view_repo(&self) -> Result<Repo, Error> {
        Ok(Repo {
            slug: "me/actindexer".into(),
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

fn hub(w: &World) -> Hub {
    Hub {
        origin: w.origin.clone(),
        merged: RefCell::default(),
    }
}

fn options(paths: &[&str], gate: &str) -> Options {
    Options {
        message: "feat: add b".into(),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        trailer: None,
        base: "main".into(),
        ship: ShipOptions {
            gate: Some(vec!["sh".into(), "-c".into(), gate.into()]),
            merge: true,
            body: None,
            settle: std::time::Duration::ZERO,
            ci_wait: std::time::Duration::ZERO,
        },
    }
}

fn go(w: &World, options: &Options) -> Result<Outcome, Error> {
    land(&hub(w), &ProcessRunner, &w.work, options)
}

#[test]
fn an_uncommitted_file_is_committed_pushed_merged_and_pulled() {
    let w = world();
    write(&w.work, "b.txt", "b\n");
    let out = go(&w, &options(&["b.txt"], "true")).unwrap();
    assert!(matches!(out, Outcome::Merged { number: 7, .. }), "{out:?}");
    assert_eq!(git(&w.work, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(
        git(&w.work, &["rev-parse", "HEAD"]),
        git(&w.origin, &["rev-parse", "refs/heads/main"])
    );
    assert!(w.work.join("b.txt").exists());
    assert_eq!(git(&w.work, &["branch", "--list", "feat/x"]), "");
}

#[test]
fn work_already_committed_needs_no_path() {
    let w = world();
    commit(&w.work, "b.txt", "b\n");
    let out = go(&w, &options(&[], "true")).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
}

#[test]
fn paths_with_nothing_left_to_commit_do_not_stop_the_landing() {
    let w = world();
    commit(&w.work, "b.txt", "b\n");
    let out = go(&w, &options(&["b.txt"], "true")).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
}

#[test]
fn a_conflict_with_main_is_settled_by_itself_and_lands() {
    let w = world();
    commit(&w.work, "a.txt", "one\ntwo\nMINE\nthree\n");
    peer_ships(&w, "a.txt", "one\ntwo\nPEER\nthree\n");
    let out = go(&w, &options(&[], "true")).unwrap();
    assert!(matches!(out, Outcome::Merged { .. }), "{out:?}");
    let text = std::fs::read_to_string(w.work.join("a.txt")).unwrap();
    assert!(text.contains("MINE") && text.contains("PEER"), "{text}");
    assert!(!text.contains("<<<<<<<"), "{text}");
    assert_eq!(
        git(&w.work, &["rev-parse", "HEAD"]),
        git(&w.origin, &["rev-parse", "refs/heads/main"])
    );
}

#[test]
fn a_gate_that_fails_after_the_settlement_stops_before_the_push() {
    let w = world();
    commit(&w.work, "a.txt", "one\ntwo\nMINE\nthree\n");
    let before = git(&w.work, &["rev-parse", "HEAD"]);
    peer_ships(&w, "a.txt", "one\ntwo\nPEER\nthree\n");
    let main = git(&w.origin, &["rev-parse", "refs/heads/main"]);
    let err = go(&w, &options(&[], "exit 3")).unwrap_err().to_string();
    assert!(err.contains("gate"), "{err}");
    assert_eq!(git(&w.work, &["rev-parse", "HEAD"]), before);
    assert_eq!(git(&w.origin, &["rev-parse", "refs/heads/main"]), main);
}

#[test]
fn landing_from_the_base_branch_is_refused_and_touches_nothing() {
    let w = world();
    git(&w.work, &["switch", "-q", "main"]);
    let main = git(&w.work, &["rev-parse", "HEAD"]);
    peer_ships(&w, "p.txt", "p\n");
    let err = go(&w, &options(&[], "true")).unwrap_err().to_string();
    assert!(err.contains("main"), "{err}");
    assert_eq!(git(&w.work, &["rev-parse", "HEAD"]), main);
}

#[test]
fn a_held_pull_request_leaves_the_branch_checked_out() {
    let w = world();
    commit(&w.work, "b.txt", "b\n");
    let mut o = options(&[], "true");
    o.ship.merge = false;
    let out = go(&w, &o).unwrap();
    assert!(matches!(out, Outcome::Held { .. }), "{out:?}");
    assert_eq!(git(&w.work, &["rev-parse", "--abbrev-ref", "HEAD"]), "feat/x");
}
