//! `actautoma rename-repo OWNER/OLD NEW [options]` as a library entry point, so another tool
//! (`repo`) can run it in-process; the `actautoma` binary only wraps it.

use crate::family;
use crate::forge::{Error, Forge, Runner};
use crate::rename::{check, execute, Outcome, Plan};
use std::io::Write;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: actautoma rename-repo OWNER/OLD NEW [--dir OLD_DIR] [--root DIR]... \
[--children DIR]... [--bin-dir DIR] [--local-only] [--dry-run]";

const FAMILY_USAGE: &str = "usage: actautoma rename-family OLD NEW [--dir DIR]";

fn fail(message: &str) -> Error {
    Error::Run(message.to_string())
}

fn say(out: &mut dyn Write, line: &str) -> Result<(), Error> {
    writeln!(out, "{line}").map_err(|e| fail(&format!("cannot write the answer: {e}")))
}

fn parse(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Plan, bool), Error> {
    let [cmd, slug, new, rest @ ..] = args else {
        return Err(fail(USAGE));
    };
    let (owner, old) = slug.split_once('/').ok_or_else(|| fail(USAGE))?;
    if cmd != "rename-repo" {
        return Err(fail(USAGE));
    }
    let home = env("HOME").ok_or_else(|| fail("HOME is not set"))?;
    let mut plan = Plan {
        owner: owner.into(),
        old: old.into(),
        new: new.clone(),
        old_dir: cwd.to_path_buf(),
        bin_dir: env("CARGO_HOME")
            .map_or_else(|| PathBuf::from(&home).join(".cargo"), PathBuf::from)
            .join("bin"),
        home: PathBuf::from(home),
        roots: vec![],
        children: vec![],
        on_platform: true,
    };
    let (mut dry, mut it) = (false, rest.iter());
    while let Some(flag) = it.next() {
        let mut value = || it.next().map(PathBuf::from).ok_or_else(|| fail(USAGE));
        match flag.as_str() {
            "--dir" => plan.old_dir = value()?,
            "--root" => plan.roots.push(value()?),
            "--bin-dir" => plan.bin_dir = value()?,
            "--children" => plan.children.push(value()?),
            "--local-only" => plan.on_platform = false,
            "--dry-run" => dry = true,
            other => return Err(fail(&format!("unknown option {other}\n{USAGE}"))),
        }
    }
    Ok((plan, dry))
}

fn report(plan: &Plan, outcome: &Outcome, out: &mut dyn Write) -> Result<(), Error> {
    let moved = format!(
        "moved {} -> {}",
        plan.old_dir.display(),
        plan.new_dir().display()
    );
    say(out, &moved)?;
    for (root, r) in &outcome.reports {
        let (c, n, l) = (r.changed.len(), r.not_text.len(), r.links.len());
        say(
            out,
            &format!("{}: {c} rewritten, {n} not text, {l} links", root.display()),
        )?;
    }
    if let Some(bin) = &outcome.binary {
        let line = format!(
            "renamed binary {} (run `cargo install --path` to refresh cargo's record)",
            bin.display()
        );
        say(out, &line)?;
    }
    if !outcome.family.is_empty() {
        say(
            out,
            &format!("renamed family {}", outcome.family.join(", ")),
        )?;
    }
    for path in &outcome.moved {
        say(out, &format!("renamed {}", path.display()))?;
    }
    Ok(())
}

/// `actautoma rename-family OLD NEW [--dir DIR]`: the family pass alone, for a repository
/// whose folder already carries the new name.
fn rename_family(args: &[String], cwd: &Path, out: &mut dyn Write) -> Result<(), Error> {
    let [_, old, new, rest @ ..] = args else {
        return Err(fail(FAMILY_USAGE));
    };
    let dir = match rest {
        [] => cwd.to_path_buf(),
        [flag, dir] if flag == "--dir" => PathBuf::from(dir),
        _ => return Err(fail(FAMILY_USAGE)),
    };
    let mut names = family::names(&dir, old)?;
    if names.is_empty() {
        names.push(old.clone()); // asked by name: rewrite the leftovers of an earlier pass
    }
    let done = family::apply(&dir, old, new, &names)?;
    let r = &done.report;
    let line = format!(
        "renamed family {}: {} rewritten, {} not text, {} links",
        names.join(", "),
        r.changed.len(),
        r.not_text.len(),
        r.links.len()
    );
    say(out, &line)?;
    for path in &done.moved {
        say(out, &format!("renamed {}", path.display()))?;
    }
    Ok(())
}

/// Run `actautoma <args>` in `cwd`; the report goes to `out`. `forge` renames on the platform,
/// `runner` runs `git`, `env` reads `HOME` and `CARGO_HOME`.
pub fn dispatch(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    forge: &impl Forge,
    runner: &impl Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    if args.first().map(String::as_str) == Some("rename-family") {
        return rename_family(args, cwd, out);
    }
    let (plan, dry) = parse(args, cwd, env)?;
    if dry {
        check(&plan)?;
        for r in plan.renames() {
            say(out, &format!("{} -> {}", r.from, r.to))?;
        }
        return say(out, "dry run: nothing was changed");
    }
    let outcome = execute(&plan, forge, runner)?;
    report(&plan, &outcome, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gh::Gh;
    use crate::testing::Fake;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p =
            std::env::temp_dir().join(format!("actautoma-cli-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn run(
        base: &Path,
        args: &[&str],
        answers: Vec<Result<String, Error>>,
    ) -> (Result<(), Error>, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let env = |k: &str| match k {
            "HOME" => Some(base.display().to_string()),
            _ => None,
        };
        let f = Fake::answering(answers);
        let mut out = Vec::new();
        let r = dispatch(&args, base, &env, &Gh::new(&f), &&f, &mut out);
        (r, String::from_utf8(out).unwrap())
    }

    fn refused(args: &[&str], needle: &str) {
        let b = scratch("usage");
        let (r, out) = run(&b, args, vec![]);
        match r {
            Err(Error::Run(m)) => assert!(m.contains(needle), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(out.is_empty());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn no_arguments_print_the_usage() {
        refused(&[], "usage");
    }

    #[test]
    fn a_slug_without_an_owner_is_refused() {
        refused(&["rename-repo", "old", "new"], "usage");
    }

    #[test]
    fn an_unknown_option_is_named() {
        refused(&["rename-repo", "me/old", "new", "--zap"], "--zap");
    }

    #[test]
    fn a_dry_run_lists_the_renames_and_changes_nothing() {
        let b = scratch("dry");
        fs::create_dir_all(b.join("old")).unwrap();
        let dir = b.join("old").display().to_string();
        let (r, out) = run(
            &b,
            &["rename-repo", "me/old", "new", "--dir", &dir, "--dry-run"],
            vec![],
        );
        r.unwrap();
        assert!(
            out.contains("me/old -> me/new") && out.contains("dry run"),
            "{out}"
        );
        assert!(b.join("old").is_dir() && !b.join("new").exists());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_real_run_moves_the_folder_and_reports_it() {
        let b = scratch("real");
        fs::create_dir_all(b.join("old")).unwrap();
        let dir = b.join("old").display().to_string();
        let answers = vec![
            Ok("gh 2".into()),
            Ok(String::new()),
            Ok("git@github.com:me/old.git".into()),
            Ok(String::new()),
        ];
        let (r, out) = run(
            &b,
            &["rename-repo", "me/old", "new", "--dir", &dir],
            answers,
        );
        r.unwrap();
        assert!(out.starts_with("moved ") && out.contains("old ->"), "{out}");
        assert!(b.join("new").is_dir() && !b.join("old").exists());
        fs::remove_dir_all(&b).unwrap();
    }
    fn family_workspace(base: &Path) -> String {
        let w = base.join("new");
        fs::create_dir_all(w.join("pk/new-ui")).unwrap();
        fs::write(w.join("Cargo.toml"), "[workspace]\nmembers = [\"pk/*\"]\n").unwrap();
        fs::write(
            w.join("pk/new-ui/Cargo.toml"),
            "[package]\nname = \"old-ui\"\n",
        )
        .unwrap();
        fs::write(w.join("pk/new-ui/lib.rs"), "use old_ui::X; // OLD_TAG\n").unwrap();
        w.display().to_string()
    }

    #[test]
    fn rename_family_rewrites_a_folder_that_was_already_moved() {
        let b = scratch("family");
        let dir = family_workspace(&b);
        let (r, out) = run(&b, &["rename-family", "old", "new", "--dir", &dir], vec![]);
        r.unwrap();
        assert!(out.starts_with("renamed family old-ui: "), "{out}");
        let lib = fs::read_to_string(b.join("new/pk/new-ui/lib.rs")).unwrap();
        assert_eq!(lib, "use new_ui::X; // NEW_TAG\n");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn rename_family_without_arguments_prints_its_usage() {
        refused(&["rename-family"], "rename-family OLD NEW");
    }

    #[test]
    fn rename_family_with_a_stray_argument_is_refused() {
        refused(
            &["rename-family", "old", "new", "--zap"],
            "rename-family OLD NEW",
        );
    }
}
