//! `actautoma rename-repo OWNER/OLD NEW [options]`: see `actautoma::rename`.

use actautoma::forge::Error;
use actautoma::gh::Gh;
use actautoma::process::ProcessRunner;
use actautoma::rename::{check, execute, Plan};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: actautoma rename-repo OWNER/OLD NEW [--dir OLD_DIR] [--root DIR]... \
[--children DIR]... [--local-only] [--dry-run]";

fn fail(message: &str) -> Error {
    Error::Run(message.to_string())
}

fn parse(args: &[String]) -> Result<(Plan, bool), Error> {
    let [cmd, slug, new, rest @ ..] = args else {
        return Err(fail(USAGE));
    };
    let (owner, old) = slug.split_once('/').ok_or_else(|| fail(USAGE))?;
    if cmd != "rename-repo" {
        return Err(fail(USAGE));
    }
    let cwd = std::env::current_dir().map_err(|e| fail(&format!("no current folder: {e}")))?;
    let home = std::env::var_os("HOME").ok_or_else(|| fail("HOME is not set"))?;
    let mut plan = Plan {
        owner: owner.into(),
        old: old.into(),
        new: new.clone(),
        old_dir: cwd,
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
            "--children" => plan.children.push(value()?),
            "--local-only" => plan.on_platform = false,
            "--dry-run" => dry = true,
            other => return Err(fail(&format!("unknown option {other}\n{USAGE}"))),
        }
    }
    Ok((plan, dry))
}

fn run(args: &[String]) -> Result<(), Error> {
    let (plan, dry) = parse(args)?;
    if dry {
        check(&plan)?;
        for r in plan.renames() {
            println!("{} -> {}", r.from, r.to);
        }
        println!("dry run: nothing was changed");
        return Ok(());
    }
    let out = execute(&plan, &Gh::new(ProcessRunner), &ProcessRunner)?;
    println!(
        "moved {} -> {}",
        plan.old_dir.display(),
        plan.new_dir().display()
    );
    for (root, report) in &out.reports {
        let (c, n, l) = (
            report.changed.len(),
            report.not_text.len(),
            report.links.len(),
        );
        println!("{}: {c} rewritten, {n} not text, {l} links", root.display());
    }
    for path in &out.moved {
        println!("renamed {}", path.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("actautoma: {e}");
            ExitCode::FAILURE
        }
    }
}
