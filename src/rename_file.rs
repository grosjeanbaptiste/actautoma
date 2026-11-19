//! `actautoma rename-file OLD NEW`: rename one tracked file with `git mv`, then
//! rewrite what points at it. Written so that renames of every kind live here.

use crate::forge::{Error, Runner};
use crate::realign::Rename;
use crate::realign_fs::{realign_tree, Report};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const USAGE: &str =
    "usage: actautoma rename-file OLD NEW [--root DIR]... [--replace FROM TO]... [--dry-run]";

/// What to rename and where the references live.
pub struct FilePlan {
    pub from: PathBuf,
    pub to: PathBuf,
    pub roots: Vec<PathBuf>,
    pub replace: Vec<Rename>,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub reports: Vec<(PathBuf, Report)>,
}

fn fail<T>(m: String) -> Result<T, Error> {
    Err(Error::Run(m))
}

pub fn check(plan: &FilePlan) -> Result<(), Error> {
    if !plan.from.is_file() {
        return fail(format!("{} is not a file", plan.from.display()));
    }
    if plan.to.exists() {
        return fail(format!("{} already exists", plan.to.display()));
    }
    let parent = plan.to.parent().unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return fail(format!("{} is not a folder", parent.display()));
    }
    match plan.roots.iter().find(|r| !r.is_dir()) {
        Some(root) => fail(format!("{} is not a folder", root.display())),
        None => Ok(()),
    }
}

fn name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The literal rewrites: the file's name, its full path, then the caller's pairs.
pub fn renames(plan: &FilePlan) -> Vec<Rename> {
    let pair = |from: String, to: String| Rename { from, to };
    let mut all = vec![
        pair(name(&plan.from), name(&plan.to)),
        pair(
            plan.from.display().to_string(),
            plan.to.display().to_string(),
        ),
    ];
    all.extend(plan.replace.iter().cloned());
    all
}

/// Move the file with `git mv`, then rewrite the references under each root.
pub fn execute(plan: &FilePlan, runner: &impl Runner) -> Result<Outcome, Error> {
    check(plan)?;
    let (from, to) = (
        plan.from.display().to_string(),
        plan.to.display().to_string(),
    );
    runner.run("git", &["mv", &from, &to], plan.from.parent())?;
    let all = renames(plan);
    let mut out = Outcome::default();
    for root in &plan.roots {
        out.reports.push((root.clone(), realign_tree(root, &all)?));
    }
    Ok(out)
}

fn parse(args: &[String], cwd: &Path) -> Result<(FilePlan, bool), Error> {
    let usage = || Error::Run(USAGE.to_string());
    let [_, from, to, rest @ ..] = args else {
        return Err(usage());
    };
    if from == to {
        return Err(usage());
    }
    let abs = |p: &str| cwd.join(p);
    let mut plan = FilePlan {
        from: abs(from),
        to: abs(to),
        roots: vec![],
        replace: vec![],
    };
    let (mut dry, mut it) = (false, rest.iter());
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--root" => plan.roots.push(abs(it.next().ok_or_else(usage)?)),
            "--replace" => {
                let (a, b) = (it.next().ok_or_else(usage)?, it.next().ok_or_else(usage)?);
                plan.replace.push(Rename {
                    from: a.clone(),
                    to: b.clone(),
                });
            }
            "--dry-run" => dry = true,
            other => return Err(Error::Run(format!("unknown option {other}\n{USAGE}"))),
        }
    }
    Ok((plan, dry))
}

/// `actautoma rename-file ...` in `cwd`; the report goes to `out`.
pub fn run(
    args: &[String],
    cwd: &Path,
    runner: &impl Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let say = |out: &mut dyn Write, l: String| {
        writeln!(out, "{l}").map_err(|e| Error::Run(format!("cannot write the answer: {e}")))
    };
    let (plan, dry) = parse(args, cwd)?;
    if dry {
        check(&plan)?;
        for r in renames(&plan) {
            say(out, format!("{} -> {}", r.from, r.to))?;
        }
        return say(out, "dry run: nothing was changed".into());
    }
    let done = execute(&plan, runner)?;
    say(
        out,
        format!("moved {} -> {}", plan.from.display(), plan.to.display()),
    )?;
    for (root, r) in &done.reports {
        let line = format!(
            "{}: {} rewritten, {} not text, {} links",
            root.display(),
            r.changed.len(),
            r.not_text.len(),
            r.links.len()
        );
        say(out, line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessRunner;
    use crate::testing::Fake;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("actautoma-rf-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn plan(b: &Path) -> FilePlan {
        fs::write(b.join("042-a.md"), "# ADR-042\n").unwrap();
        fs::write(
            b.join("README.md"),
            "[a](042-a.md) 1042-a.md ADR-042 ADR-0421\n",
        )
        .unwrap();
        FilePlan {
            from: b.join("042-a.md"),
            to: b.join("043-a.md"),
            roots: vec![b.to_path_buf()],
            replace: vec![Rename {
                from: "ADR-042".into(),
                to: "ADR-043".into(),
            }],
        }
    }

    fn ok(s: &str) -> Result<String, Error> {
        Ok(s.to_string())
    }

    fn msg(r: Result<impl std::fmt::Debug, Error>) -> String {
        match r {
            Err(e) => e.to_string(),
            Ok(v) => panic!("expected an error, got {v:?}"),
        }
    }

    #[test]
    fn a_missing_source_is_refused_naming_it() {
        let b = scratch("missing");
        let mut p = plan(&b);
        p.from = b.join("nope.md");
        assert!(msg(check(&p)).contains("nope.md"));
    }

    #[test]
    fn an_existing_target_is_refused_and_nothing_changes() {
        let b = scratch("exists");
        let p = plan(&b);
        fs::write(&p.to, "x").unwrap();
        let f = Fake::answering(vec![]);
        assert!(msg(execute(&p, &&f)).contains("already exists"));
        assert_eq!(f.calls_len(), 0);
        assert!(fs::read_to_string(b.join("README.md"))
            .unwrap()
            .contains("[a](042-a.md)"));
    }

    #[test]
    fn a_missing_target_folder_is_refused() {
        let b = scratch("parent");
        let mut p = plan(&b);
        p.to = b.join("no/such/043-a.md");
        assert!(msg(check(&p)).contains("no/such"));
    }

    #[test]
    fn a_folder_is_not_a_file() {
        let b = scratch("dir");
        let mut p = plan(&b);
        fs::create_dir_all(b.join("d")).unwrap();
        p.from = b.join("d");
        assert!(msg(check(&p)).contains("not a file"));
    }

    #[test]
    fn the_move_is_a_git_mv_run_in_the_source_folder() {
        let b = scratch("gitmv");
        let p = plan(&b);
        let f = Fake::answering(vec![ok("")]);
        execute(&p, &&f).unwrap();
        let (prog, args, dir) = f.call(0);
        assert_eq!(prog, "git");
        assert_eq!(
            args,
            [
                "mv",
                &p.from.display().to_string(),
                &p.to.display().to_string()
            ]
        );
        assert_eq!(dir.as_deref(), Some(b.as_path()));
    }

    #[test]
    fn a_failing_git_mv_stops_before_any_rewrite() {
        let b = scratch("fail");
        let p = plan(&b);
        let f = Fake::answering(vec![Err(Error::Run("not tracked".into()))]);
        assert!(msg(execute(&p, &&f)).contains("not tracked"));
        assert!(fs::read_to_string(b.join("README.md"))
            .unwrap()
            .contains("[a](042-a.md)"));
    }

    #[test]
    fn references_are_rewritten_by_whole_name_only() {
        let b = scratch("refs");
        let p = plan(&b);
        let f = Fake::answering(vec![ok("")]);
        let out = execute(&p, &&f).unwrap();
        let text = fs::read_to_string(b.join("README.md")).unwrap();
        assert_eq!(text, "[a](043-a.md) 1042-a.md ADR-043 ADR-0421\n");
        assert_eq!(out.reports[0].1.changed.len(), 2); // README.md and the moved file's own heading
    }

    #[test]
    fn end_to_end_with_real_git() {
        let b = scratch("real");
        let p = plan(&b);
        let git = |a: &[&str]| {
            let s = std::process::Command::new("git")
                .args(a)
                .current_dir(&b)
                .status()
                .unwrap();
            assert!(s.success());
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        execute(&p, &ProcessRunner).unwrap();
        assert!(b.join("043-a.md").is_file() && !b.join("042-a.md").exists());
        let listed = std::process::Command::new("git")
            .args(["ls-files"])
            .current_dir(&b)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&listed.stdout).contains("043-a.md"));
    }

    fn cli(
        b: &Path,
        args: &[&str],
        answers: Vec<Result<String, Error>>,
    ) -> (Result<(), Error>, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let f = Fake::answering(answers);
        let mut out = Vec::new();
        let r = run(&args, b, &&f, &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    #[test]
    fn the_command_line_moves_and_reports_each_root() {
        let b = scratch("cli");
        plan(&b);
        let (r, out) = cli(
            &b,
            &[
                "rename-file",
                "042-a.md",
                "043-a.md",
                "--root",
                ".",
                "--replace",
                "ADR-042",
                "ADR-043",
            ],
            vec![ok("")],
        );
        r.unwrap();
        assert!(out.contains("moved ") && out.contains("rewritten"), "{out}");
        assert!(fs::read_to_string(b.join("README.md"))
            .unwrap()
            .contains("ADR-043"));
    }

    #[test]
    fn a_dry_run_lists_the_rewrites_and_changes_nothing() {
        let b = scratch("dry");
        plan(&b);
        let (r, out) = cli(
            &b,
            &[
                "rename-file",
                "042-a.md",
                "043-a.md",
                "--root",
                ".",
                "--dry-run",
            ],
            vec![],
        );
        r.unwrap();
        assert!(
            out.contains("042-a.md -> 043-a.md") && out.contains("dry run"),
            "{out}"
        );
        assert!(b.join("042-a.md").is_file());
    }

    #[test]
    fn bad_arguments_print_the_usage() {
        let b = scratch("usage");
        for args in [
            vec!["rename-file"],
            vec!["rename-file", "a"],
            vec!["rename-file", "a", "a"],
            vec!["rename-file", "a", "b", "--replace", "x"],
            vec!["rename-file", "a", "b", "--bogus"],
        ] {
            let (r, _) = cli(&b, &args, vec![]);
            assert!(msg(r).contains("usage: actautoma rename-file"), "{args:?}");
        }
    }
}
