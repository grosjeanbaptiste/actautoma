//! The `gh` (GitHub) adapter of [`Forge`].

use crate::forge::{
    fields, parse_open_prs, parse_repo, CiRun, Error, Forge, Mergeable, NewPullRequest, NewRepo,
    OpenPr, PullRequest, Repo, RepoSearch, Runner,
};
use std::path::{Path, PathBuf};

const FIELDS: &str =
    "number,title,headRefName,baseRefName,author,mergeable,reviewDecision,mergeStateStatus";
/// Unit separator: cannot occur in a title, unlike `|` or a tab.
const JQ: &str = r#"[.number,.title,.headRefName,.baseRefName,.author.login,.mergeable,.reviewDecision,.mergeStateStatus]|map(tostring)|join("\u001f")"#;

const OPEN_JQ: &str = r#".[]|[.number,.author.login,.updatedAt]|map(tostring)|join("\u001f")"#;
const RUN_JQ: &str =
    r#".[]|[.workflowName,.conclusion,.status,.createdAt[0:10]]|map(tostring)|join("\u001f")"#;

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
        parse_repo(self.gh(&args)?)
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

    fn open_prs(&self) -> Result<Vec<OpenPr>, Error> {
        let args = [
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "1000",
            "--json",
            "number,author,updatedAt",
            "--jq",
            OPEN_JQ,
        ];
        let raw = self.gh(&args)?;
        let me = || self.user();
        parse_open_prs(&raw, me, str::to_string)
    }

    fn last_run(&self, branch: &str) -> Result<Option<CiRun>, Error> {
        let args = [
            "run",
            "list",
            "--branch",
            branch,
            "--limit",
            "1",
            "--json",
            "workflowName,conclusion,status,createdAt",
            "--jq",
            RUN_JQ,
        ];
        let raw = self.gh(&args)?;
        Ok(fields::<4>(&raw)?
            .first()
            .map(|[workflow, conclusion, status, date]| CiRun {
                workflow: workflow.to_string(),
                // A run still going has an empty conclusion.
                state: if conclusion.is_empty() {
                    status
                } else {
                    conclusion
                }
                .to_string(),
                date: date.to_string(),
            }))
    }

    fn available(&self) -> Result<(), Error> {
        self.gh(&["--version"]).map(drop)
    }

    fn user(&self) -> Result<String, Error> {
        let raw = self.gh(&["api", "user", "-q", ".login"])?;
        match raw.trim() {
            "" => Err(Error::Parse(raw)),
            login => Ok(login.to_string()),
        }
    }

    fn repo_exists(&self, owner: &str, name: &str) -> Result<bool, Error> {
        match self.gh(&["api", &format!("repos/{owner}/{name}")]) {
            Ok(_) => Ok(true),
            Err(Error::Run(m)) if m.contains("Not Found") => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn search_repos(&self, text: &str) -> Result<RepoSearch, Error> {
        let query = format!("search/repositories?q={text}+in:name&per_page=100");
        let raw = self.gh(&["api", &query, "-q", ".total_count, .items[].full_name"])?;
        let mut lines = raw.lines();
        let total = lines.next().and_then(|l| l.trim().parse().ok());
        let Some(total) = total else {
            return Err(Error::Parse(raw));
        };
        Ok(RepoSearch {
            total,
            names: lines.map(String::from).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fake;

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

    #[test]
    fn user_asks_the_api_for_the_login() {
        let f = Fake::answering(vec![Ok("me\n".into())]);
        assert_eq!(Gh::new(&f).user().unwrap(), "me");
        assert_eq!(f.call(0).1, ["api", "user", "-q", ".login"]);
    }

    #[test]
    fn an_empty_login_is_a_parse_error() {
        let f = Fake::answering(vec![Ok("\n".into())]);
        assert_eq!(Gh::new(&f).user(), Err(Error::Parse("\n".into())));
    }

    #[test]
    fn repo_exists_is_true_when_the_api_answers() {
        let f = Fake::answering(vec![Ok("{}".into())]);
        assert!(Gh::new(&f).repo_exists("me", "ss").unwrap());
        assert_eq!(f.call(0).1, ["api", "repos/me/ss"]);
    }

    #[test]
    fn repo_exists_is_false_on_not_found_and_an_error_on_anything_else() {
        let f = Fake::answering(vec![Err(Error::Run("gh: Not Found (HTTP 404)".into()))]);
        assert!(!Gh::new(&f).repo_exists("me", "ss").unwrap());
        let f = Fake::answering(vec![Err(Error::Run("HTTP 403: rate limit".into()))]);
        assert_eq!(
            Gh::new(&f).repo_exists("me", "ss"),
            Err(Error::Run("HTTP 403: rate limit".into()))
        );
    }

    #[test]
    fn search_repos_reads_the_total_then_the_full_names() {
        let f = Fake::answering(vec![Ok("250\nbob/acte\nann/acte-x\n".into())]);
        let found = Gh::new(&f).search_repos("acte").unwrap();
        assert_eq!(
            found,
            RepoSearch {
                total: 250,
                names: vec!["bob/acte".into(), "ann/acte-x".into()]
            }
        );
        let args = f.call(0).1;
        assert_eq!(
            args[..2],
            ["api", "search/repositories?q=acte+in:name&per_page=100"]
        );
        assert_eq!(args[2], "-q");
    }

    #[test]
    fn search_repos_without_a_numeric_total_is_a_parse_error() {
        let f = Fake::answering(vec![Ok("many\nbob/acte\n".into())]);
        assert_eq!(
            Gh::new(&f).search_repos("acte"),
            Err(Error::Parse("many\nbob/acte\n".into()))
        );
    }

    fn pr_line(n: u32, who: &str, at: &str) -> String {
        format!("{n}{US}{who}{US}{at}\n")
    }

    #[test]
    fn open_prs_lists_open_pull_requests_and_marks_mine() {
        let list =
            pr_line(3, "me", "2026-01-01T00:00:00Z") + &pr_line(4, "bob", "2026-02-01T10:20:30Z");
        let f = Fake::answering(vec![Ok(list), Ok("me\n".into())]);
        let prs = Gh::new(&f).in_dir("/w/r").open_prs().unwrap();
        let (program, args, dir) = f.call(0);
        assert_eq!(program, "gh");
        assert_eq!(&args[..4], ["pr", "list", "--state", "open"]);
        assert!(args.contains(&"--json".to_string()));
        assert_eq!(dir, Some(PathBuf::from("/w/r")));
        assert_eq!(f.call(1).1, ["api", "user", "-q", ".login"]);
        assert_eq!(
            prs,
            [
                OpenPr {
                    number: 3,
                    mine: true,
                    updated_at: "2026-01-01T00:00:00Z".into()
                },
                OpenPr {
                    number: 4,
                    mine: false,
                    updated_at: "2026-02-01T10:20:30Z".into()
                },
            ]
        );
    }

    #[test]
    fn open_prs_of_a_repository_without_any_asks_nobody_who_i_am() {
        let f = Fake::answering(vec![Ok(String::new())]);
        assert_eq!(Gh::new(&f).open_prs().unwrap(), []);
        assert_eq!(f.calls_len(), 1);
    }

    #[test]
    fn open_prs_rejects_a_line_of_the_wrong_shape() {
        let f = Fake::answering(vec![Ok("3\u{1f}me\n".into()), Ok("me\n".into())]);
        assert_eq!(
            Gh::new(&f).open_prs(),
            Err(Error::Parse("3\u{1f}me\n".into()))
        );
    }

    #[test]
    fn open_prs_rejects_a_non_numeric_number() {
        let raw = pr_line(0, "me", "x").replacen('0', "x", 1);
        let f = Fake::answering(vec![Ok(raw.clone()), Ok("me\n".into())]);
        assert_eq!(Gh::new(&f).open_prs(), Err(Error::Parse(raw)));
    }

    #[test]
    fn open_prs_failures_reach_the_caller() {
        let f = Fake::answering(vec![Err(Error::Run("no auth".into()))]);
        assert_eq!(Gh::new(&f).open_prs(), Err(Error::Run("no auth".into())));
        let f = Fake::answering(vec![
            Ok(pr_line(3, "me", "t")),
            Err(Error::Run("no user".into())),
        ]);
        assert_eq!(Gh::new(&f).open_prs(), Err(Error::Run("no user".into())));
    }

    #[test]
    fn last_run_asks_for_the_one_latest_run_of_the_branch() {
        let f = Fake::answering(vec![Ok(format!(
            "CI{US}success{US}completed{US}2026-11-09\n"
        ))]);
        let run = Gh::new(&f).last_run("develop").unwrap();
        let (_, args, _) = f.call(0);
        assert_eq!(
            &args[..5],
            ["run", "list", "--branch", "develop", "--limit"]
        );
        assert_eq!(args[5], "1");
        assert_eq!(
            run,
            Some(CiRun {
                workflow: "CI".into(),
                state: "success".into(),
                date: "2026-11-09".into()
            })
        );
    }

    #[test]
    fn a_run_in_progress_has_no_conclusion_so_its_status_is_the_state() {
        let f = Fake::answering(vec![Ok(format!("CI{US}{US}in_progress{US}2026-11-09\n"))]);
        assert_eq!(
            Gh::new(&f).last_run("main").unwrap().unwrap().state,
            "in_progress"
        );
    }

    #[test]
    fn a_branch_that_never_ran_has_no_last_run() {
        let f = Fake::answering(vec![Ok(String::new())]);
        assert_eq!(Gh::new(&f).last_run("main").unwrap(), None);
    }

    #[test]
    fn last_run_rejects_the_wrong_shape_and_passes_failures_on() {
        let f = Fake::answering(vec![Ok("CI\n".into())]);
        assert_eq!(
            Gh::new(&f).last_run("main"),
            Err(Error::Parse("CI\n".into()))
        );
        let f = Fake::answering(vec![Err(Error::Run("boom".into()))]);
        assert_eq!(Gh::new(&f).last_run("main"), Err(Error::Run("boom".into())));
    }
}
