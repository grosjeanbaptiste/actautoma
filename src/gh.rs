//! The `gh` (GitHub) adapter of [`Forge`].

use crate::forge::{Error, Forge, Mergeable, NewPullRequest, NewRepo, PullRequest, Repo, Runner};
use std::path::{Path, PathBuf};

const FIELDS: &str =
    "number,title,headRefName,baseRefName,author,mergeable,reviewDecision,mergeStateStatus";
/// Unit separator: cannot occur in a title, unlike `|` or a tab.
const JQ: &str = r#"[.number,.title,.headRefName,.baseRefName,.author.login,.mergeable,.reviewDecision,.mergeStateStatus]|map(tostring)|join("\u001f")"#;

/// `gh`, the GitHub CLI, behind a [`Runner`].
pub struct Gh<R> {
    runner: R,
    program: String,
    dir: Option<PathBuf>,
}

impl<R: Runner> Gh<R> {
    pub fn new(runner: R) -> Self {
        Gh {
            runner,
            program: "gh".into(),
            dir: None,
        }
    }

    /// The `gh` binary to run: a name on `PATH` or a path.
    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = program.into();
        self
    }

    /// The repository directory `gh` runs in; the current one when unset.
    pub fn in_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.dir = Some(dir.as_ref().to_path_buf());
        self
    }

    fn gh(&self, args: &[&str]) -> Result<String, Error> {
        self.runner.run(&self.program, args, self.dir.as_deref())
    }
}

impl<R: Runner> Forge for Gh<R> {
    fn view_pr(&self, number: Option<u64>) -> Result<PullRequest, Error> {
        let n = number.map(|n| n.to_string());
        let mut args = vec!["pr", "view"];
        args.extend(n.as_deref());
        args.extend(["--json", FIELDS, "--jq", JQ]);
        let raw = self.gh(&args)?;
        let f: Vec<&str> = raw.trim_end_matches('\n').split('\u{1f}').collect();
        let [number, title, head, base, author, mergeable, review, merge_state] = f[..] else {
            return Err(Error::Parse(raw));
        };
        let number = number.parse().map_err(|_| Error::Parse(raw.clone()))?;
        Ok(PullRequest {
            number,
            title: title.into(),
            head: head.into(),
            base: base.into(),
            author: author.into(),
            mergeable: match mergeable {
                "MERGEABLE" => Mergeable::Mergeable,
                "CONFLICTING" => Mergeable::Conflicting,
                "UNKNOWN" => Mergeable::Unknown,
                other => Mergeable::Other(other.into()),
            },
            review: review.into(),
            merge_state: merge_state.into(),
        })
    }

    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error> {
        let args = [
            "pr", "create", "--base", &pr.base, "--head", &pr.head, "--title", &pr.title, "--body",
            &pr.body,
        ];
        Ok(self.gh(&args)?.trim().to_string())
    }

    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        let n = number.to_string();
        self.gh(&["pr", "merge", &n, "--merge", "--delete-branch"])
            .map(drop)
    }

    fn view_repo(&self) -> Result<Repo, Error> {
        let query = r#".nameWithOwner + " " + .defaultBranchRef.name"#;
        let args = [
            "repo",
            "view",
            "--json",
            "nameWithOwner,defaultBranchRef",
            "-q",
            query,
        ];
        let raw = self.gh(&args)?;
        let mut words = raw.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some(slug), Some(branch), None) => Ok(Repo {
                slug: slug.into(),
                default_branch: branch.into(),
            }),
            _ => Err(Error::Parse(raw)),
        }
    }

    fn set_default_branch(&self, slug: &str, branch: &str) -> Result<(), Error> {
        self.gh(&["repo", "edit", slug, "--default-branch", branch])
            .map(drop)
    }

    fn create_repo(&self, repo: &NewRepo) -> Result<(), Error> {
        let source = repo.source.to_string_lossy();
        let mut args = vec![
            "repo",
            "create",
            &repo.name,
            if repo.private {
                "--private"
            } else {
                "--public"
            },
            "--source",
            &source,
        ];
        if repo.push {
            args.push("--push");
        }
        self.gh(&args).map(drop)
    }

    fn available(&self) -> Result<(), Error> {
        self.gh(&["--version"]).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    type Call = (String, Vec<String>, Option<PathBuf>);

    /// Scripted runner: records every call, answers from a queue.
    struct Fake {
        calls: RefCell<Vec<Call>>,
        answers: RefCell<Vec<Result<String, Error>>>,
    }

    impl Fake {
        fn answering(answers: Vec<Result<String, Error>>) -> Self {
            Fake {
                calls: RefCell::default(),
                answers: RefCell::new(answers),
            }
        }
        fn call(&self, i: usize) -> Call {
            self.calls.borrow()[i].clone()
        }
    }

    impl Runner for &Fake {
        fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, Error> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
                dir.map(Path::to_path_buf),
            ));
            self.answers.borrow_mut().remove(0)
        }
    }

    const US: char = '\u{1f}';

    fn view_answer(mergeable: &str) -> String {
        format!("8{US}Add x{US}feat{US}main{US}bob{US}{mergeable}{US}APPROVED{US}CLEAN\n")
    }

    #[test]
    fn view_pr_of_current_branch_asks_gh_without_a_number() {
        let f = Fake::answering(vec![Ok(view_answer("MERGEABLE"))]);
        let pr = Gh::new(&f).view_pr(None).unwrap();
        let (program, args, _) = f.call(0);
        assert_eq!(program, "gh");
        assert_eq!(&args[..2], ["pr", "view"]);
        assert_eq!(args[2], "--json");
        assert!(!args.contains(&"8".to_string()));
        assert_eq!(
            pr,
            PullRequest {
                number: 8,
                title: "Add x".into(),
                head: "feat".into(),
                base: "main".into(),
                author: "bob".into(),
                mergeable: Mergeable::Mergeable,
                review: "APPROVED".into(),
                merge_state: "CLEAN".into(),
            }
        );
    }

    #[test]
    fn view_pr_with_a_number_puts_it_after_view() {
        let f = Fake::answering(vec![Ok(view_answer("MERGEABLE"))]);
        Gh::new(&f).view_pr(Some(8)).unwrap();
        assert_eq!(&f.call(0).1[..3], ["pr", "view", "8"]);
    }

    #[test]
    fn mergeable_values_are_named_and_unknown_ones_kept_verbatim() {
        for (raw, want) in [
            ("MERGEABLE", Mergeable::Mergeable),
            ("CONFLICTING", Mergeable::Conflicting),
            ("UNKNOWN", Mergeable::Unknown),
            ("WEIRD", Mergeable::Other("WEIRD".into())),
        ] {
            let f = Fake::answering(vec![Ok(view_answer(raw))]);
            assert_eq!(Gh::new(&f).view_pr(None).unwrap().mergeable, want, "{raw}");
        }
    }

    #[test]
    fn mergeable_prints_as_the_platform_wrote_it() {
        for raw in ["MERGEABLE", "CONFLICTING", "UNKNOWN", "WEIRD"] {
            let f = Fake::answering(vec![Ok(view_answer(raw))]);
            assert_eq!(
                Gh::new(&f).view_pr(None).unwrap().mergeable.to_string(),
                raw
            );
        }
    }

    #[test]
    fn view_pr_rejects_an_answer_with_the_wrong_shape() {
        let f = Fake::answering(vec![Ok("nope\n".into())]);
        let err = Gh::new(&f).view_pr(None).unwrap_err();
        assert!(
            matches!(&err, Error::Parse(m) if m.contains("nope")),
            "{err:?}"
        );
    }

    #[test]
    fn view_pr_rejects_a_non_numeric_number() {
        let f = Fake::answering(vec![Ok(view_answer("MERGEABLE").replacen('8', "x", 1))]);
        assert!(matches!(Gh::new(&f).view_pr(None), Err(Error::Parse(_))));
    }

    #[test]
    fn a_runner_failure_reaches_the_caller() {
        let f = Fake::answering(vec![Err(Error::Run("gh: not logged in".into()))]);
        assert_eq!(
            Gh::new(&f).view_pr(None),
            Err(Error::Run("gh: not logged in".into()))
        );
    }

    #[test]
    fn create_pr_passes_every_field_and_returns_the_url() {
        let f = Fake::answering(vec![Ok("https://github.com/o/r/pull/9\n".into())]);
        let url = Gh::new(&f)
            .create_pr(&NewPullRequest {
                base: "main".into(),
                head: "feat".into(),
                title: "T".into(),
                body: "B".into(),
            })
            .unwrap();
        assert_eq!(url, "https://github.com/o/r/pull/9");
        assert_eq!(
            f.call(0).1,
            ["pr", "create", "--base", "main", "--head", "feat", "--title", "T", "--body", "B"]
        );
    }

    #[test]
    fn merge_pr_merges_and_deletes_the_branch() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Gh::new(&f).merge_pr(8).unwrap();
        assert_eq!(
            f.call(0).1,
            ["pr", "merge", "8", "--merge", "--delete-branch"]
        );
    }

    #[test]
    fn merge_failure_reaches_the_caller() {
        let f = Fake::answering(vec![Err(Error::Run("conflict".into()))]);
        assert_eq!(Gh::new(&f).merge_pr(8), Err(Error::Run("conflict".into())));
    }

    #[test]
    fn program_and_directory_are_settings_not_constants() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Gh::new(&f)
            .program("/opt/gh")
            .in_dir("/work/r")
            .merge_pr(1)
            .unwrap();
        let (program, _, dir) = f.call(0);
        assert_eq!(program, "/opt/gh");
        assert_eq!(dir, Some(PathBuf::from("/work/r")));
    }

    #[test]
    fn view_repo_asks_for_the_slug_and_the_default_branch() {
        let f = Fake::answering(vec![Ok("me/ss master\n".into())]);
        let repo = Gh::new(&f).in_dir("/work/r").view_repo().unwrap();
        assert_eq!(
            repo,
            Repo {
                slug: "me/ss".into(),
                default_branch: "master".into()
            }
        );
        let (program, args, dir) = f.call(0);
        assert_eq!(program, "gh");
        assert_eq!(
            &args[..4],
            ["repo", "view", "--json", "nameWithOwner,defaultBranchRef"]
        );
        assert_eq!(args[4], "-q");
        assert_eq!(dir, Some(PathBuf::from("/work/r")));
    }

    #[test]
    fn view_repo_rejects_an_answer_with_the_wrong_shape() {
        for raw in ["", "me/ss\n", "a b c\n"] {
            let f = Fake::answering(vec![Ok(raw.into())]);
            let err = Gh::new(&f).view_repo().unwrap_err();
            assert!(
                matches!(&err, Error::Parse(m) if m == raw),
                "{raw:?}: {err:?}"
            );
        }
    }

    #[test]
    fn set_default_branch_edits_the_repository() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Gh::new(&f).set_default_branch("me/ss", "main").unwrap();
        assert_eq!(
            f.call(0).1,
            ["repo", "edit", "me/ss", "--default-branch", "main"]
        );
    }

    #[test]
    fn create_repo_is_private_and_pushed_when_asked() {
        let f = Fake::answering(vec![Ok(String::new())]);
        let repo = NewRepo {
            name: "ss".into(),
            source: "/w/ss".into(),
            private: true,
            push: true,
        };
        Gh::new(&f).create_repo(&repo).unwrap();
        assert_eq!(
            f.call(0).1,
            [
                "repo",
                "create",
                "ss",
                "--private",
                "--source",
                "/w/ss",
                "--push"
            ]
        );
    }

    #[test]
    fn create_repo_can_be_public_and_unpushed() {
        let f = Fake::answering(vec![Ok(String::new())]);
        let repo = NewRepo {
            name: "ss".into(),
            source: "/w/ss".into(),
            private: false,
            push: false,
        };
        Gh::new(&f).create_repo(&repo).unwrap();
        assert_eq!(
            f.call(0).1,
            ["repo", "create", "ss", "--public", "--source", "/w/ss"]
        );
    }

    #[test]
    fn available_asks_the_tool_for_its_version() {
        let f = Fake::answering(vec![Ok("gh version 2\n".into())]);
        Gh::new(&f).available().unwrap();
        assert_eq!(f.call(0).1, ["--version"]);
    }

    #[test]
    fn repo_operations_report_the_runner_failure() {
        let boom = || Err(Error::Run("boom".into()));
        let f = Fake::answering(vec![boom(), boom(), boom(), boom()]);
        let gh = Gh::new(&f);
        let new = NewRepo {
            name: "a".into(),
            source: "/a".into(),
            private: true,
            push: true,
        };
        assert_eq!(gh.view_repo(), Err(Error::Run("boom".into())));
        assert_eq!(
            gh.set_default_branch("a/b", "main"),
            Err(Error::Run("boom".into()))
        );
        assert_eq!(gh.create_repo(&new), Err(Error::Run("boom".into())));
        assert_eq!(gh.available(), Err(Error::Run("boom".into())));
    }
}
