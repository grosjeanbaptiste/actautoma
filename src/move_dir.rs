//! `actautoma move DIR --into PARENT`: move a folder under another parent, keeping its name.
//!
//! One command for a single repository and for a folder of repositories: the
//! repositories (and their linked worktrees) under `DIR` are found by scanning,
//! never listed by hand. The folder moves in one `rename`, so the relative
//! `../sibling` paths between its repositories stay true; then the linked
//! worktrees are re-linked and every written path is rewritten.
//!
//! Every check that can fail runs before the move, so a refusal leaves the
//! machine untouched.

use crate::forge::{Error, Runner};
use crate::realign::{encoded, replace, Rename};
use crate::realign_fs::{realign_tree_files, rename_children, Report, SKIPPED_DIRS};
use crate::rename::linked_worktrees;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub const USAGE: &str = "usage: actautoma move DIR --into PARENT [--root DIR]... \
[--children DIR]... [--registry FILE] [--cargo-home DIR] [--already-moved] [--dry-run]";

/// What to move and where the references live.
#[derive(Clone)]
pub struct Plan {
    pub dir: PathBuf,
    /// Parent folder of the target; the folder keeps its name.
    pub into: PathBuf,
    pub home: PathBuf,
    /// Folders whose text files are rewritten, besides the moved folder itself.
    pub roots: Vec<PathBuf>,
    /// Folders whose children named after an encoded repository path are renamed.
    pub children: Vec<PathBuf>,
    /// Single text files rewritten when they exist (the registry, cargo's install records).
    pub files: Vec<PathBuf>,
    /// The folder was already moved by something else: `dir` is where it was,
    /// `into` its new parent. Nothing is moved; links and references are repaired.
    pub already_moved: bool,
}

impl Plan {
    pub fn new_dir(&self) -> PathBuf {
        self.into.join(self.dir.file_name().unwrap_or_default())
    }
}

/// What a move did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The repositories found, at their new place.
    pub repos: Vec<PathBuf>,
    pub reports: Vec<(PathBuf, Report)>,
    pub files: Vec<PathBuf>,
    pub moved: Vec<PathBuf>,
    /// The new parent is a repository and now ignores the moved folder.
    pub ignored: bool,
}

fn fail<T>(m: String) -> Result<T, Error> {
    Err(Error::Run(m))
}

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("{what} {}: {e}", path.display()))
}

/// Every repository under `dir` (`dir` itself when it is one): a folder holding `.git`,
/// a folder (main clone) or a file (linked worktree). A repository is not searched inside.
pub fn repos_under(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut found = Vec::new();
    scan(dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn scan(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), Error> {
    if dir.join(".git").exists() {
        found.push(dir.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, "cannot read", &e))? {
        let path = entry.map_err(|e| io(dir, "cannot list", &e))?.path();
        let kind = std::fs::symlink_metadata(&path).map_err(|e| io(&path, "cannot stat", &e))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if kind.is_dir() && !SKIPPED_DIRS.contains(&name) {
            scan(&path, found)?;
        }
    }
    Ok(())
}

pub fn check(plan: &Plan) -> Result<(), Error> {
    if !plan.into.is_dir() {
        return fail(format!("{} is not a folder", plan.into.display()));
    }
    let new_dir = plan.new_dir();
    if plan.already_moved {
        if plan.dir.exists() {
            return fail(format!("{} still exists", plan.dir.display()));
        }
        if !new_dir.is_dir() {
            return fail(format!("{} is not a folder", new_dir.display()));
        }
    } else if !plan.dir.is_dir() {
        return fail(format!("{} is not a folder", plan.dir.display()));
    } else if new_dir.exists() {
        return fail(format!("{} already exists", new_dir.display()));
    }
    if new_dir.starts_with(&plan.dir) {
        return fail(format!(
            "{} is inside the folder to move",
            plan.into.display()
        ));
    }
    match plan
        .roots
        .iter()
        .chain(&plan.children)
        .find(|d| !d.is_dir())
    {
        Some(dir) => fail(format!("{} is not a folder", dir.display())),
        None => Ok(()),
    }
}

/// `path` (new tree) as it was before the move.
fn before(path: &Path, old: &Path, new: &Path) -> PathBuf {
    match path.strip_prefix(new) {
        Ok(rest) => old.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// `path` as it is after the move.
fn after(path: &Path, old: &Path, new: &Path) -> PathBuf {
    path.strip_prefix(old)
        .map_or_else(|_| path.to_path_buf(), |rest| new.join(rest))
}

/// The literal rewrites of one moved path: itself, its `~` form and its encoded form.
fn path_renames(old: &Path, new: &Path, home: &Path) -> Vec<Rename> {
    let pair = |from: String, to: String| Rename { from, to };
    let mut all = vec![
        pair(old.to_string_lossy().into(), new.to_string_lossy().into()),
        pair(encoded(old), encoded(new)),
    ];
    if let (Ok(o), Ok(n)) = (old.strip_prefix(home), new.strip_prefix(home)) {
        for form in ["~", "$HOME", "${HOME}"] {
            all.push(pair(
                format!("{form}/{}", o.display()),
                format!("{form}/{}", n.display()),
            ));
        }
    }
    all
}

/// `path` with `.` and `..` folded away, without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The path that leads from folder `from` to `to`.
fn relative(from: &Path, to: &Path) -> PathBuf {
    let (f, t): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let shared = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let ups = (shared..f.len()).map(|_| Component::ParentDir);
    ups.chain(t[shared..].iter().copied()).collect()
}

fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-')
}

/// Rewrite the `../x` paths of a text written next to the file `from_dir` (before the move)
/// that still point at an existing target after the move. A path hanging on a variable
/// (`$D/../x`) is not one of them, and neither is one whose target does not exist.
fn relative_paths(
    text: &str,
    old_base: &Path,
    new_base: &Path,
    old: &Path,
    new: &Path,
) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let (mut count, mut prev, mut i) = (0, None::<char>, 0);
    while i < text.len() {
        let rest = &text[i..];
        if !prev.is_some_and(is_path_char) && rest.starts_with("../") {
            let end = rest.find(|c: char| !is_path_char(c)).unwrap_or(rest.len());
            let token = &rest[..end];
            let core = token.trim_end_matches('.');
            let target = after(&lexical(&old_base.join(core)), old, new);
            if target.exists() {
                let mut fixed = relative(new_base, &target).display().to_string();
                if core.ends_with('/') {
                    fixed.push('/');
                }
                if !fixed.is_empty() && fixed != core {
                    out.push_str(&fixed);
                    out.push_str(&token[core.len()..]);
                    count += 1;
                    i += end;
                    prev = out.chars().last();
                    continue;
                }
            }
            out.push_str(token);
            i += end;
            prev = token.chars().last();
            continue;
        }
        let c = rest.chars().next().expect("rest is not empty");
        out.push(c);
        i += c.len_utf8();
        prev = Some(c);
    }
    (out, count)
}

/// The tail both paths share a parent for (`Source/old` -> `Source/Dest/old`): how a path is
/// written when it hangs on a variable (`$ROOT/../Source/old`) and no prefix names the folder.
fn tail_rename(old: &Path, new: &Path) -> Option<Rename> {
    let shared: PathBuf = old
        .components()
        .zip(new.components())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a)
        .collect();
    let anchor = shared.file_name()?.to_string_lossy().into_owned();
    let rel = |p: &Path| {
        p.strip_prefix(&shared)
            .map(|r| format!("{anchor}/{}", r.display()))
    };
    Some(Rename {
        from: rel(old).ok()?,
        to: rel(new).ok()?,
    })
}

pub fn renames(plan: &Plan) -> Vec<Rename> {
    let new = plan.new_dir();
    let mut all = path_renames(&plan.dir, &new, &plan.home);
    all.extend(tail_rename(&plan.dir, &new));
    all
}

/// Fill in what the user does not have to list: the folders around the moved one, Claude's
/// session folders, cargo's install records and the user-wide Claude files.
pub fn with_defaults(plan: &mut Plan, cargo_home: &Path) {
    let claude = plan.home.join(".claude");
    if let Some(parent) = plan.dir.parent().filter(|p| p.is_dir()) {
        plan.roots.push(parent.to_path_buf());
    }
    if claude.join("projects").is_dir() {
        plan.children.push(claude.join("projects"));
    }
    plan.files.extend([
        cargo_home.join(".crates.toml"),
        cargo_home.join(".crates2.json"),
    ]);
    plan.files.extend(
        [
            "CLAUDE.md",
            "AGENTS.md",
            "settings.json",
            "settings.local.json",
        ]
        .map(|f| claude.join(f)),
    );
    for list in [&mut plan.roots, &mut plan.children, &mut plan.files] {
        list.sort();
        list.dedup();
    }
}

/// The parent repository ignores the moved folder: one `/name/` line, added once.
fn ignore_in_parent(into: &Path, name: &str) -> Result<bool, Error> {
    if !into.join(".git").exists() {
        return Ok(false);
    }
    let file = into.join(".gitignore");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    if text.lines().any(|l| matches!(l.trim(), n if n == name || n == format!("/{name}") || n == format!("{name}/") || n == format!("/{name}/"))) {
        return Ok(false);
    }
    let sep = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&file, format!("{text}{sep}/{name}/\n"))
        .map_err(|e| io(&file, "cannot write", &e))?;
    Ok(true)
}

fn rewrite_file(file: &Path, renames: &[Rename]) -> Result<bool, Error> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Ok(false); // absent on this host, or not text: nothing of ours to fix
    };
    let (fixed, count) = replace(&text, renames);
    if count == 0 {
        return Ok(false);
    }
    std::fs::write(file, fixed).map_err(|e| io(file, "cannot write", &e))?;
    Ok(true)
}

/// Move the folder, re-link the worktrees, rewrite the references.
pub fn execute(plan: &Plan, runner: &impl Runner) -> Result<Outcome, Error> {
    check(plan)?;
    // git prints real paths (`/private/var`, not `/var`): compare like with like
    let real = |p: &Path| p.canonicalize().map_err(|e| io(p, "cannot resolve", &e));
    let plan = &Plan {
        // an already-moved folder is gone from `dir`: its text is kept as given
        dir: if plan.already_moved {
            plan.dir.clone()
        } else {
            real(&plan.dir)?
        },
        into: real(&plan.into)?,
        ..plan.clone()
    };
    let (old, new) = (&plan.dir, plan.new_dir());
    // Where a path of the old tree lives now.
    let now = |p: &Path| {
        if plan.already_moved {
            after(p, old, &new)
        } else {
            p.to_path_buf()
        }
    };
    let repos: Vec<PathBuf> = if plan.already_moved {
        repos_under(&new)?
            .iter()
            .map(|r| before(r, old, &new))
            .collect()
    } else {
        repos_under(old)?
    };
    let mut linked = Vec::new(); // (main clone, its linked worktrees)
    for main in repos.iter().filter(|r| now(r).join(".git").is_dir()) {
        linked.push((main, linked_worktrees(runner, &now(main))?));
    }
    if !plan.already_moved {
        std::fs::rename(old, &new).map_err(|e| io(old, "cannot move", &e))?;
    }
    for (main, worktrees) in linked.iter().filter(|(_, w)| !w.is_empty()) {
        let paths: Vec<String> = worktrees
            .iter()
            .map(|w| after(w, old, &new))
            .filter(|w| w.is_dir())
            .map(|w| w.display().to_string())
            .collect();
        let mut args = vec!["worktree", "repair"];
        args.extend(paths.iter().map(String::as_str));
        runner.run("git", &args, Some(&after(main, old, &new)))?;
    }
    let all = renames(plan);
    let mut out = Outcome {
        repos: repos.iter().map(|r| after(r, old, &new)).collect(),
        ..Outcome::default()
    };
    let covered = plan.roots.iter().any(|r| new.starts_with(r));
    for root in std::iter::once(&new)
        .filter(|_| !covered)
        .chain(&plan.roots)
    {
        let fix = |file: &Path, text: &str| {
            let (text, a) = replace(text, &all);
            let new_base = file.parent().unwrap_or(file);
            let old_base = new_base
                .strip_prefix(&new)
                .map_or_else(|_| new_base.to_path_buf(), |rest| old.join(rest));
            let (text, b) = relative_paths(&text, &old_base, new_base, old, &new);
            (text, a + b)
        };
        out.reports
            .push((root.clone(), realign_tree_files(root, &fix)?));
    }
    let per_repo: Vec<Rename> = repos
        .iter()
        .flat_map(|r| path_renames(r, &after(r, old, &new), &plan.home))
        .collect();
    for dir in &plan.children {
        out.moved.extend(rename_children(dir, &per_repo)?);
    }
    for file in &plan.files {
        if rewrite_file(file, &all)? {
            out.files.push(file.clone());
        }
    }
    let name = new.file_name().unwrap_or_default().to_string_lossy();
    out.ignored = ignore_in_parent(&plan.into, &name)?;
    Ok(out)
}

fn parse(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Plan, bool), Error> {
    let usage = || Error::Run(USAGE.to_string());
    let [_, dir, rest @ ..] = args else {
        return Err(usage());
    };
    let home = PathBuf::from(env("HOME").ok_or_else(|| Error::Run("HOME is not set".into()))?);
    let cargo_home = env("CARGO_HOME").map_or_else(|| home.join(".cargo"), PathBuf::from);
    let abs = |p: &str| cwd.join(p);
    let (mut into, mut dry) = (None, false);
    let mut plan = Plan {
        dir: abs(dir),
        into: PathBuf::new(),
        home,
        roots: vec![],
        children: vec![],
        files: vec![],
        already_moved: false,
    };
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        let mut value = || it.next().map(|v| abs(v)).ok_or_else(usage);
        match flag.as_str() {
            "--into" => into = Some(value()?),
            "--root" => plan.roots.push(value()?),
            "--children" => plan.children.push(value()?),
            "--registry" => plan.files.push(value()?),
            "--cargo-home" => {
                let dir = value()?;
                plan.files
                    .extend([dir.join(".crates.toml"), dir.join(".crates2.json")]);
            }
            "--already-moved" => plan.already_moved = true,
            "--dry-run" => dry = true,
            other => return Err(Error::Run(format!("unknown option {other}\n{USAGE}"))),
        }
    }
    plan.into = into.ok_or_else(usage)?;
    with_defaults(&mut plan, &cargo_home);
    Ok((plan, dry))
}

fn say(out: &mut dyn Write, line: &str) -> Result<(), Error> {
    writeln!(out, "{line}").map_err(|e| Error::Run(format!("cannot write the answer: {e}")))
}

/// `actautoma move ...` in `cwd`; the report goes to `out`.
pub fn run(
    args: &[String],
    cwd: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    runner: &impl Runner,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let (plan, dry) = parse(args, cwd, env)?;
    if dry {
        check(&plan)?;
        let scanned = if plan.already_moved {
            plan.new_dir()
        } else {
            plan.dir.clone()
        };
        let repos = repos_under(&scanned)?;
        say(
            out,
            &format!("{} -> {}", plan.dir.display(), plan.new_dir().display()),
        )?;
        for r in &repos {
            say(out, &format!("repo {}", r.display()))?;
        }
        return say(
            out,
            &format!(
                "dry run: {} repositories found, nothing was changed",
                repos.len()
            ),
        );
    }
    let done = execute(&plan, runner)?;
    say(
        out,
        &format!(
            "moved {} -> {}",
            plan.dir.display(),
            plan.new_dir().display()
        ),
    )?;
    for r in &done.repos {
        say(out, &format!("repo {}", r.display()))?;
    }
    for (root, r) in &done.reports {
        let line = format!(
            "{}: {} rewritten, {} not text, {} links",
            root.display(),
            r.changed.len(),
            r.not_text.len(),
            r.links.len()
        );
        say(out, &line)?;
    }
    for file in &done.files {
        say(out, &format!("rewrote {}", file.display()))?;
    }
    for path in &done.moved {
        say(out, &format!("renamed {}", path.display()))?;
    }
    if done.ignored {
        say(
            out,
            &format!(
                "{}/.gitignore: now ignores the moved folder",
                plan.into.display()
            ),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessRunner;
    use std::fs;
    use std::process::Command;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("actautoma-mv-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p); // a leftover of a crashed run
        fs::create_dir_all(&p).unwrap();
        p.canonicalize().unwrap() // /var is /private/var on macOS: git prints the latter
    }

    fn git(dir: &Path, args: &[&str]) {
        let s = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(s.success(), "git {args:?}");
    }

    fn repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "x"]);
    }

    fn plan(base: &Path) -> Plan {
        fs::create_dir_all(base.join("into")).unwrap();
        Plan {
            dir: base.join("src/group"),
            into: base.join("into"),
            home: base.to_path_buf(),
            roots: vec![],
            children: vec![],
            files: vec![],
            already_moved: false,
        }
    }

    #[test]
    fn finds_every_repository_under_a_folder_without_a_list() {
        let b = scratch("scan");
        let g = b.join("g");
        for r in ["a", "b", "deep/c"] {
            repo(&g.join(r));
        }
        fs::create_dir_all(g.join("a/nested")).unwrap();
        repo(&g.join("a/nested/inner")); // inside a repository: not searched
        fs::create_dir_all(g.join("target/x/.git")).unwrap(); // a skipped folder
        fs::create_dir_all(g.join("plain")).unwrap();
        let got = repos_under(&g).unwrap();
        assert_eq!(got, [g.join("a"), g.join("b"), g.join("deep/c")]);
        assert_eq!(repos_under(&g.join("a")).unwrap(), [g.join("a")]);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn moves_the_folder_with_its_repositories_and_rewrites_the_paths() {
        let b = scratch("all");
        let p = plan(&b);
        repo(&p.dir.join("a"));
        repo(&p.dir.join("b"));
        fs::write(
            p.dir.join("a/NOTES.md"),
            format!("see {}/b", p.dir.display()),
        )
        .unwrap();
        fs::create_dir_all(b.join("cfg")).unwrap();
        fs::write(
            b.join("cfg/x.txt"),
            format!("~/src/group/a {}", p.dir.display()),
        )
        .unwrap();
        fs::create_dir_all(b.join("claude")).unwrap();
        fs::create_dir_all(b.join(format!("claude/{}", encoded(&p.dir.join("a"))))).unwrap();
        let reg = b.join("projects.txt");
        fs::write(&reg, format!("a|{}/a\nz|/else/z\n", p.dir.display())).unwrap();
        let mut p = p;
        p.roots = vec![b.join("cfg")];
        p.children = vec![b.join("claude")];
        p.files = vec![reg.clone(), b.join("absent.json")];
        let out = execute(&p, &ProcessRunner).unwrap();
        let g = b.join("into/group");
        assert!(g.join("a/.git").is_dir() && !b.join("src/group").exists());
        assert_eq!(out.repos, [g.join("a"), g.join("b")]);
        assert_eq!(
            fs::read_to_string(g.join("a/NOTES.md")).unwrap(),
            format!("see {}/b", g.display())
        );
        assert_eq!(
            fs::read_to_string(b.join("cfg/x.txt")).unwrap(),
            format!("~/into/group/a {}", g.display())
        );
        assert_eq!(
            fs::read_to_string(&reg).unwrap(),
            format!("a|{}/a\nz|/else/z\n", g.display())
        );
        assert!(b.join(format!("claude/{}", encoded(&g.join("a")))).is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn linked_worktrees_inside_and_outside_stay_usable() {
        let b = scratch("wt");
        let p = plan(&b);
        repo(&p.dir.join("a"));
        git(
            &p.dir.join("a"),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "w1",
                b.join("outside").to_str().unwrap(),
            ],
        );
        git(
            &p.dir.join("a"),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "w2",
                p.dir.join("a-wt").to_str().unwrap(),
            ],
        );
        execute(&p, &ProcessRunner).unwrap();
        let g = b.join("into/group");
        git(&g.join("a"), &["worktree", "list"]);
        git(&b.join("outside"), &["status", "-s"]);
        git(&g.join("a-wt"), &["status", "-s"]);
        let list = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(g.join("a"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&list.stdout).to_string();
        assert!(!text.contains("prunable"), "{text}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_single_repository_moves_the_same_way() {
        let b = scratch("one");
        let p = plan(&b);
        repo(&p.dir);
        let out = execute(&p, &ProcessRunner).unwrap();
        assert_eq!(out.repos, [b.join("into/group")]);
        assert!(b.join("into/group/.git").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_refusal_leaves_everything_untouched() {
        let b = scratch("refuse");
        let mut p = plan(&b);
        repo(&p.dir.join("a"));
        fs::create_dir_all(b.join("into/group")).unwrap();
        let err = execute(&p, &ProcessRunner).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        fs::remove_dir_all(b.join("into/group")).unwrap();
        p.roots = vec![b.join("absent")];
        let err = execute(&p, &ProcessRunner).unwrap_err();
        assert!(err.to_string().contains("absent"), "{err}");
        p.roots = vec![];
        p.into = p.dir.join("a");
        let err = execute(&p, &ProcessRunner).unwrap_err();
        assert!(err.to_string().contains("inside"), "{err}");
        assert!(p.dir.join("a/.git").is_dir());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn the_command_detects_the_children_and_a_dry_run_changes_nothing() {
        let b = scratch("cli");
        let p = plan(&b);
        repo(&p.dir.join("a"));
        repo(&p.dir.join("b"));
        let env = |k: &str| (k == "HOME").then(|| b.display().to_string());
        let args: Vec<String> = ["move", "src/group", "--into", "into", "--dry-run"]
            .map(String::from)
            .to_vec();
        let mut out = Vec::new();
        run(&args, &b, &env, &ProcessRunner, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("2 repositories found"), "{text}");
        assert!(p.dir.join("a/.git").is_dir() && !b.join("into/group").exists());
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_path_written_from_a_variable_is_rewritten_by_its_tail() {
        let b = scratch("tail");
        let p = plan(&b);
        repo(&p.dir.join("a"));
        let sh = b.join("cfg/run.sh");
        fs::create_dir_all(b.join("cfg")).unwrap();
        let (lca, old, new) = (
            b.file_name().unwrap().to_string_lossy().to_string(),
            "src/group",
            "into/group",
        );
        fs::write(
            &sh,
            format!("X=\"$D/../../{lca}/{old}/a/bin\"\nY=\"$HOME/{old}\"\n"),
        )
        .unwrap();
        let mut p = p;
        p.roots = vec![b.join("cfg")];
        execute(&p, &ProcessRunner).unwrap();
        assert_eq!(
            fs::read_to_string(&sh).unwrap(),
            format!("X=\"$D/../../{lca}/{new}/a/bin\"\nY=\"$HOME/{new}\"\n")
        );
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn the_defaults_name_the_references_the_user_does_not_have_to_list() {
        let b = scratch("auto");
        let home = b.join("h");
        fs::create_dir_all(home.join(".claude/projects")).unwrap();
        fs::create_dir_all(home.join("Source/grp")).unwrap();
        fs::create_dir_all(home.join("Source/Dest")).unwrap();
        let mut p = Plan {
            dir: home.join("Source/grp"),
            into: home.join("Source/Dest"),
            home: home.clone(),
            roots: vec![],
            children: vec![],
            files: vec![],
            already_moved: false,
        };
        with_defaults(&mut p, &home.join(".cargo"));
        assert_eq!(p.roots, [home.join("Source")]);
        assert_eq!(p.children, [home.join(".claude/projects")]);
        assert!(p.files.contains(&home.join(".cargo/.crates.toml")));
        assert!(p.files.contains(&home.join(".claude/CLAUDE.md")));
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn the_new_parent_repository_starts_ignoring_the_moved_folder() {
        let b = scratch("ignore");
        let p = plan(&b);
        repo(&p.dir.join("a"));
        repo(&b.join("into"));
        execute(&p, &ProcessRunner).unwrap();
        assert_eq!(
            fs::read_to_string(b.join("into/.gitignore")).unwrap(),
            "/group/\n"
        );
        let again = Plan {
            dir: b.join("into/group"),
            into: b.join("src"),
            ..p.clone()
        };
        fs::create_dir_all(b.join("src")).unwrap();
        execute(&again, &ProcessRunner).unwrap();
        execute(
            &Plan {
                dir: b.join("src/group"),
                into: b.join("into"),
                ..p
            },
            &ProcessRunner,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(b.join("into/.gitignore")).unwrap(),
            "/group/\n"
        );
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn relative_paths_that_cross_the_moved_folder_are_recomputed() {
        let b = scratch("rel");
        let mut p = plan(&b);
        repo(&p.dir.join("a"));
        fs::create_dir_all(b.join("src/other/x")).unwrap();
        fs::write(
            b.join("src/other/Cargo.toml"),
            "p = \"../group/a\"\nq = \"../group/a/\"\nz = \"../../etc/passwd\"\n",
        )
        .unwrap();
        fs::write(
            p.dir.join("a/Cargo.toml"),
            "out = \"../../other/x\"\nin = \"../a\"\n",
        )
        .unwrap();
        p.roots = vec![b.join("src/other")];
        execute(&p, &ProcessRunner).unwrap();
        assert_eq!(
            fs::read_to_string(b.join("src/other/Cargo.toml")).unwrap(),
            "p = \"../../into/group/a\"\nq = \"../../into/group/a/\"\nz = \"../../etc/passwd\"\n"
        );
        assert_eq!(
            fs::read_to_string(b.join("into/group/a/Cargo.toml")).unwrap(),
            "out = \"../../../src/other/x\"\nin = \"../a\"\n"
        );
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_folder_moved_by_hand_gets_its_references_rewritten() {
        let b = scratch("already");
        let mut p = plan(&b);
        repo(&p.dir.join("a"));
        fs::create_dir_all(b.join("cfg")).unwrap();
        fs::write(b.join("cfg/x.txt"), format!("at {}/a", p.dir.display())).unwrap();
        let reg = b.join("records.json");
        fs::write(&reg, format!("path+file://{}/a", p.dir.display())).unwrap();
        let new = b.join("into/group");
        fs::rename(&p.dir, &new).unwrap(); // moved by something else
        (p.already_moved, p.roots, p.files) = (true, vec![b.join("cfg")], vec![reg.clone()]);
        let out = execute(&p, &ProcessRunner).unwrap();
        assert_eq!(out.repos, [new.join("a")]);
        assert!(new.join("a/.git").is_dir() && !p.dir.exists());
        assert_eq!(
            fs::read_to_string(b.join("cfg/x.txt")).unwrap(),
            format!("at {}/a", new.display())
        );
        assert_eq!(
            fs::read_to_string(&reg).unwrap(),
            format!("path+file://{}/a", new.display())
        );
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn a_linked_worktree_outside_is_relinked_after_a_move_by_hand() {
        let b = scratch("already-wt");
        let mut p = plan(&b);
        repo(&p.dir.join("a"));
        let outside = b.join("outside");
        git(
            &p.dir.join("a"),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "w1",
                outside.to_str().unwrap(),
            ],
        );
        let new = b.join("into/group");
        fs::rename(&p.dir, &new).unwrap();
        p.already_moved = true;
        execute(&p, &ProcessRunner).unwrap();
        git(&outside, &["status", "--short"]); // fails while the link points at the old clone
        git(&new.join("a"), &["worktree", "list"]);
        let list = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(new.join("a"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&list.stdout).into_owned();
        assert!(!text.contains("prunable"), "{text}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_already_moved_run_refuses_when_the_old_folder_is_still_there() {
        let b = scratch("already-old");
        let mut p = plan(&b);
        repo(&p.dir.join("a"));
        repo(&b.join("into/group/a"));
        p.already_moved = true;
        let error = execute(&p, &ProcessRunner).unwrap_err().to_string();
        assert!(error.contains("still exists"), "{error}");
        assert!(p.dir.join("a/.git").is_dir()); // untouched
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_already_moved_run_refuses_when_the_new_folder_is_missing() {
        let b = scratch("already-new");
        let mut p = plan(&b);
        p.already_moved = true;
        let error = execute(&p, &ProcessRunner).unwrap_err().to_string();
        assert!(error.ends_with("into/group is not a folder"), "{error}");
        fs::remove_dir_all(&b).unwrap();
    }
}
