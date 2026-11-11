//! Rename a repository everywhere in one pass: the platform, the local
//! remote, the folder, then every written reference.
//!
//! Every check that can fail runs before the first change, so a refusal
//! leaves the machine untouched.

use crate::forge::{Error, Forge, Runner};
use crate::realign::{for_repo, replace, Rename};
use crate::realign_fs::{realign_tree, rename_children, Report};
use std::path::PathBuf;

/// What to rename and where the references live.
pub struct Plan {
    pub owner: String,
    pub old: String,
    pub new: String,
    /// The local clone, before the move; its sibling `new` is the target.
    pub old_dir: PathBuf,
    pub home: PathBuf,
    /// Folders whose text files are rewritten (given as they are after the move).
    pub roots: Vec<PathBuf>,
    /// Folders whose children named after the encoded old folder are renamed.
    pub children: Vec<PathBuf>,
    /// False when the platform already carries the new name.
    pub on_platform: bool,
}

impl Plan {
    pub fn new_dir(&self) -> PathBuf {
        self.old_dir.with_file_name(&self.new)
    }
    pub fn renames(&self) -> Vec<Rename> {
        let (o, n) = (&self.old_dir, self.new_dir());
        for_repo(&self.owner, &self.old, &self.new, o, &n, &self.home)
    }
}

/// What a rename did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub reports: Vec<(PathBuf, Report)>,
    pub moved: Vec<PathBuf>,
}

pub fn check(plan: &Plan) -> Result<(), Error> {
    let fail = |m: String| Err(Error::Run(m));
    if !plan.old_dir.is_dir() {
        return fail(format!("{} is not a folder", plan.old_dir.display()));
    }
    if plan.new_dir().exists() {
        return fail(format!("{} already exists", plan.new_dir().display()));
    }
    for dir in plan.roots.iter().chain(&plan.children) {
        if !dir.is_dir() && dir != &plan.new_dir() {
            return fail(format!("{} is not a folder", dir.display()));
        }
    }
    Ok(())
}

fn set_remote(runner: &impl Runner, plan: &Plan, renames: &[Rename]) -> Result<(), Error> {
    let dir = Some(plan.old_dir.as_path());
    let url = runner.run("git", &["remote", "get-url", "origin"], dir)?;
    let (fixed, count) = replace(url.trim(), renames);
    if count > 0 {
        runner.run("git", &["remote", "set-url", "origin", &fixed], dir)?;
    }
    Ok(())
}

/// Do the rename. `forge` renames on the platform, `runner` runs `git`.
pub fn execute(plan: &Plan, forge: &impl Forge, runner: &impl Runner) -> Result<Outcome, Error> {
    check(plan)?;
    let renames = plan.renames();
    if plan.on_platform {
        forge.available()?;
        forge.rename_repo(&format!("{}/{}", plan.owner, plan.old), &plan.new)?;
    }
    set_remote(runner, plan, &renames)?;
    std::fs::rename(&plan.old_dir, plan.new_dir())
        .map_err(|e| Error::Run(format!("cannot move {}: {e}", plan.old_dir.display())))?;
    let mut out = Outcome::default();
    for root in &plan.roots {
        out.reports
            .push((root.clone(), realign_tree(root, &renames)?));
    }
    for dir in &plan.children {
        out.moved.extend(rename_children(dir, &renames)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gh::Gh;
    use crate::testing::Fake;
    use std::fs;
    use std::path::Path;

    fn plan(base: &Path) -> Plan {
        let old = base.join("src/old");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(base.join("cfg")).unwrap();
        fs::write(
            base.join("cfg/a.txt"),
            format!("{}\n me/old", old.display()),
        )
        .unwrap();
        Plan {
            owner: "me".into(),
            old: "old".into(),
            new: "new".into(),
            old_dir: old,
            home: base.to_path_buf(),
            roots: vec![base.join("cfg")],
            children: vec![],
            on_platform: true,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("actautoma-rn-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn ok(s: &str) -> Result<String, Error> {
        Ok(s.to_string())
    }

    #[test]
    fn renames_on_the_platform_the_remote_the_folder_and_the_references() {
        let b = scratch("all");
        let p = plan(&b);
        let f = Fake::answering(vec![
            ok("gh 2"),
            ok(""),
            ok("git@github.com:me/old.git\n"),
            ok(""),
        ]);
        let out = execute(&p, &Gh::new(&f), &&f).unwrap();
        assert_eq!(
            f.call(1).1,
            ["repo", "rename", "new", "--repo", "me/old", "--yes"]
        );
        assert_eq!(
            f.call(3).1,
            ["remote", "set-url", "origin", "git@github.com:me/new.git"]
        );
        assert!(b.join("src/new").is_dir() && !b.join("src/old").exists());
        let text = fs::read_to_string(b.join("cfg/a.txt")).unwrap();
        assert_eq!(text, format!("{}\n me/new", b.join("src/new").display()));
        assert_eq!(out.reports[0].1.changed.len(), 1);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn skips_the_platform_when_it_already_has_the_new_name() {
        let b = scratch("local");
        let mut p = plan(&b);
        p.on_platform = false;
        let f = Fake::answering(vec![ok("git@github.com:me/new.git"), ok("")]);
        execute(&p, &Gh::new(&f), &&f).unwrap();
        assert_eq!(f.calls_len(), 1); // the remote already says `new`: only `get-url` ran
        assert!(b.join("src/new").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_refusal_before_any_change_leaves_everything_untouched() {
        let b = scratch("clash");
        let p = plan(&b);
        fs::create_dir_all(b.join("src/new")).unwrap();
        let f = Fake::answering(vec![]);
        let err = execute(&p, &Gh::new(&f), &&f).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(f.calls_len(), 0);
        assert!(b.join("src/old").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_platform_failure_stops_before_the_folder_moves() {
        let b = scratch("fail");
        let p = plan(&b);
        let f = Fake::answering(vec![ok("gh 2"), Err(Error::Run("name taken".into()))]);
        let err = execute(&p, &Gh::new(&f), &&f).unwrap_err();
        assert_eq!(err, Error::Run("name taken".into()));
        assert!(b.join("src/old").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_missing_root_is_refused_up_front() {
        let b = scratch("root");
        let mut p = plan(&b);
        p.roots.push(b.join("absent"));
        let f = Fake::answering(vec![]);
        let err = execute(&p, &Gh::new(&f), &&f).unwrap_err();
        assert!(err.to_string().contains("absent"), "{err}");
        assert_eq!(f.calls_len(), 0);
        fs::remove_dir_all(&b).unwrap();
    }
}
