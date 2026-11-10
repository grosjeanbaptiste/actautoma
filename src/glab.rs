//! The `glab` (GitLab) adapter of [`Forge`]. A pull request is a GitLab merge request.

use crate::forge::{
    parse_repo, Error, Forge, Mergeable, NewPullRequest, NewRepo, PullRequest, Repo, Runner,
};
use std::path::{Path, PathBuf};

const FIELDS_JQ: &str = r#"[.iid,.title,.source_branch,.target_branch,.author.username,.detailed_merge_status,.has_conflicts]|map(tostring)|join("\u001f")"#;
const REPO_JQ: &str = r#".path_with_namespace + " " + .default_branch"#;

/// `glab`, the GitLab CLI, behind a [`Runner`].
pub struct Glab<R> {
    runner: R,
    program: String,
    dir: Option<PathBuf>,
}

impl<R: Runner> Glab<R> {
    pub fn new(runner: R) -> Self {
        Glab {
            runner,
            program: "glab".into(),
            dir: None,
        }
    }

    /// The `glab` binary to run: a name on `PATH` or a path.
    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = program.into();
        self
    }

    /// The repository directory `glab` runs in; the current one when unset.
    pub fn in_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.dir = Some(dir.as_ref().to_path_buf());
        self
    }

    fn glab(&self, args: &[&str]) -> Result<String, Error> {
        self.runner.run(&self.program, args, self.dir.as_deref())
    }
}

/// GitLab's `detailed_merge_status` (plus `has_conflicts`) in the port's words.
fn mergeable(detailed: &str, has_conflicts: &str) -> Mergeable {
    match (has_conflicts, detailed) {
        ("true", _) | (_, "conflict") => Mergeable::Conflicting,
        (_, "mergeable") => Mergeable::Mergeable,
        (_, "checking" | "unchecked" | "preparing") => Mergeable::Unknown,
        (_, other) => Mergeable::Other(other.into()),
    }
}

impl<R: Runner> Forge for Glab<R> {
    fn view_pr(&self, number: Option<u64>) -> Result<PullRequest, Error> {
        let n = number.map(|n| n.to_string());
        let mut args = vec!["mr", "view"];
        args.extend(n.as_deref());
        args.extend(["--output", "json", "--jq", FIELDS_JQ]);
        let raw = self.glab(&args)?;
        let f: Vec<&str> = raw.trim_end_matches('\n').split('\u{1f}').collect();
        let [number, title, head, base, author, detailed, conflicts] = f[..] else {
            return Err(Error::Parse(raw));
        };
        let number = number.parse().map_err(|_| Error::Parse(raw.clone()))?;
        Ok(PullRequest {
            number,
            title: title.into(),
            head: head.into(),
            base: base.into(),
            author: author.into(),
            mergeable: mergeable(detailed, conflicts),
            // `glab mr view` does not report a review decision.
            review: String::new(),
            merge_state: detailed.into(),
        })
    }

    fn create_pr(&self, pr: &NewPullRequest) -> Result<String, Error> {
        let args = [
            "mr",
            "create",
            "--target-branch",
            &pr.base,
            "--source-branch",
            &pr.head,
            "--title",
            &pr.title,
            "--description",
            &pr.body,
            "--yes",
        ];
        let raw = self.glab(&args)?;
        match raw.lines().map(str::trim).find(|l| l.starts_with("http")) {
            Some(url) => Ok(url.to_string()),
            None => Err(Error::Parse(raw)),
        }
    }

    fn merge_pr(&self, number: u64) -> Result<(), Error> {
        let n = number.to_string();
        self.glab(&["mr", "merge", &n, "--remove-source-branch", "--yes"])
            .map(drop)
    }

    fn view_repo(&self) -> Result<Repo, Error> {
        let args = ["repo", "view", "--output", "json", "--jq", REPO_JQ];
        parse_repo(self.glab(&args)?)
    }

    fn set_default_branch(&self, slug: &str, branch: &str) -> Result<(), Error> {
        self.glab(&["repo", "update", slug, "--defaultBranch", branch])
            .map(drop)
    }

    /// `glab repo create` adds the remote from inside the repository; it has no push flag,
    /// so a requested push is a `git push` of every branch afterwards.
    fn create_repo(&self, repo: &NewRepo) -> Result<(), Error> {
        let vis = if repo.private {
            "--private"
        } else {
            "--public"
        };
        let dir = Some(repo.source.as_path());
        self.runner
            .run(&self.program, &["repo", "create", &repo.name, vis], dir)?;
        if repo.push {
            self.runner
                .run("git", &["push", "-u", "origin", "--all"], dir)?;
        }
        Ok(())
    }

    fn available(&self) -> Result<(), Error> {
        self.glab(&["--version"]).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fake;

    const US: char = '\u{1f}';

    fn view_answer(detailed: &str, conflicts: &str) -> String {
        format!("8{US}Add x{US}feat{US}main{US}bob{US}{detailed}{US}{conflicts}\n")
    }

    #[test]
    fn view_pr_asks_glab_for_json_filtered_by_jq() {
        let f = Fake::answering(vec![Ok(view_answer("mergeable", "false"))]);
        let pr = Glab::new(&f).view_pr(None).unwrap();
        let (program, args, _) = f.call(0);
        assert_eq!(program, "glab");
        assert_eq!(&args[..2], ["mr", "view"]);
        assert_eq!(&args[2..4], ["--output", "json"]);
        assert_eq!(args[4], "--jq");
        assert_eq!(
            pr,
            PullRequest {
                number: 8,
                title: "Add x".into(),
                head: "feat".into(),
                base: "main".into(),
                author: "bob".into(),
                mergeable: Mergeable::Mergeable,
                review: String::new(),
                merge_state: "mergeable".into(),
            }
        );
    }

    #[test]
    fn view_pr_with_a_number_puts_it_after_view() {
        let f = Fake::answering(vec![Ok(view_answer("mergeable", "false"))]);
        Glab::new(&f).view_pr(Some(8)).unwrap();
        assert_eq!(&f.call(0).1[..3], ["mr", "view", "8"]);
    }

    #[test]
    fn gitlab_merge_statuses_map_to_mergeable() {
        for (detailed, conflicts, want) in [
            ("mergeable", "false", Mergeable::Mergeable),
            ("conflict", "false", Mergeable::Conflicting),
            ("mergeable", "true", Mergeable::Conflicting),
            ("checking", "false", Mergeable::Unknown),
            ("unchecked", "false", Mergeable::Unknown),
            ("preparing", "false", Mergeable::Unknown),
            (
                "ci_must_pass",
                "false",
                Mergeable::Other("ci_must_pass".into()),
            ),
        ] {
            let f = Fake::answering(vec![Ok(view_answer(detailed, conflicts))]);
            let got = Glab::new(&f).view_pr(None).unwrap().mergeable;
            assert_eq!(got, want, "{detailed} conflicts={conflicts}");
        }
    }

    #[test]
    fn view_pr_rejects_a_wrong_shape_and_a_non_numeric_number() {
        for raw in [
            "nope\n".to_string(),
            view_answer("mergeable", "false").replacen('8', "x", 1),
        ] {
            let f = Fake::answering(vec![Ok(raw.clone())]);
            assert_eq!(Glab::new(&f).view_pr(None), Err(Error::Parse(raw)));
        }
    }

    #[test]
    fn create_pr_passes_every_field_and_returns_the_url() {
        let out = "Creating merge request for feat into main\nhttps://gitlab.com/o/r/-/merge_requests/9\n";
        let f = Fake::answering(vec![Ok(out.into())]);
        let url = Glab::new(&f)
            .create_pr(&NewPullRequest {
                base: "main".into(),
                head: "feat".into(),
                title: "T".into(),
                body: "B".into(),
            })
            .unwrap();
        assert_eq!(url, "https://gitlab.com/o/r/-/merge_requests/9");
        assert_eq!(
            f.call(0).1,
            [
                "mr",
                "create",
                "--target-branch",
                "main",
                "--source-branch",
                "feat",
                "--title",
                "T",
                "--description",
                "B",
                "--yes"
            ]
        );
    }

    #[test]
    fn create_pr_without_a_url_in_the_answer_is_a_parse_error() {
        let f = Fake::answering(vec![Ok("done\n".into())]);
        let new = NewPullRequest {
            base: "m".into(),
            head: "h".into(),
            title: "t".into(),
            body: "b".into(),
        };
        assert_eq!(
            Glab::new(&f).create_pr(&new),
            Err(Error::Parse("done\n".into()))
        );
    }

    #[test]
    fn merge_pr_merges_and_removes_the_source_branch() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Glab::new(&f).merge_pr(8).unwrap();
        assert_eq!(
            f.call(0).1,
            ["mr", "merge", "8", "--remove-source-branch", "--yes"]
        );
    }

    #[test]
    fn view_repo_asks_for_the_path_and_the_default_branch() {
        let f = Fake::answering(vec![Ok("grp/sub/ss main\n".into())]);
        let repo = Glab::new(&f).in_dir("/w/r").view_repo().unwrap();
        assert_eq!(
            repo,
            Repo {
                slug: "grp/sub/ss".into(),
                default_branch: "main".into()
            }
        );
        let (_, args, dir) = f.call(0);
        assert_eq!(&args[..4], ["repo", "view", "--output", "json"]);
        assert_eq!(args[4], "--jq");
        assert_eq!(dir, Some(PathBuf::from("/w/r")));
    }

    #[test]
    fn view_repo_rejects_a_wrong_shape() {
        let f = Fake::answering(vec![Ok("only\n".into())]);
        assert_eq!(
            Glab::new(&f).view_repo(),
            Err(Error::Parse("only\n".into()))
        );
    }

    #[test]
    fn set_default_branch_updates_the_project() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Glab::new(&f).set_default_branch("grp/ss", "main").unwrap();
        assert_eq!(
            f.call(0).1,
            ["repo", "update", "grp/ss", "--defaultBranch", "main"]
        );
    }

    #[test]
    fn create_repo_runs_in_the_source_directory_and_pushes_when_asked() {
        let f = Fake::answering(vec![Ok(String::new()), Ok(String::new())]);
        let repo = NewRepo {
            name: "ss".into(),
            source: "/w/ss".into(),
            private: true,
            push: true,
        };
        Glab::new(&f)
            .in_dir("/elsewhere")
            .create_repo(&repo)
            .unwrap();
        let (program, args, dir) = f.call(0);
        assert_eq!(
            (program.as_str(), dir),
            ("glab", Some(PathBuf::from("/w/ss")))
        );
        assert_eq!(args, ["repo", "create", "ss", "--private"]);
        let (program, args, dir) = f.call(1);
        assert_eq!(
            (program.as_str(), dir),
            ("git", Some(PathBuf::from("/w/ss")))
        );
        assert_eq!(args, ["push", "-u", "origin", "--all"]);
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
        Glab::new(&f).create_repo(&repo).unwrap();
        assert_eq!(f.call(0).1, ["repo", "create", "ss", "--public"]);
    }

    #[test]
    fn a_failed_creation_does_not_push() {
        let f = Fake::answering(vec![Err(Error::Run("name taken".into()))]);
        let repo = NewRepo {
            name: "ss".into(),
            source: "/w/ss".into(),
            private: true,
            push: true,
        };
        assert_eq!(
            Glab::new(&f).create_repo(&repo),
            Err(Error::Run("name taken".into()))
        );
    }

    #[test]
    fn available_asks_the_tool_for_its_version() {
        let f = Fake::answering(vec![Ok("glab 1.0\n".into())]);
        Glab::new(&f).available().unwrap();
        assert_eq!(f.call(0).1, ["--version"]);
    }

    #[test]
    fn program_and_directory_are_settings_not_constants() {
        let f = Fake::answering(vec![Ok(String::new())]);
        Glab::new(&f)
            .program("/opt/glab")
            .in_dir("/work/r")
            .merge_pr(1)
            .unwrap();
        let (program, _, dir) = f.call(0);
        assert_eq!(program, "/opt/glab");
        assert_eq!(dir, Some(PathBuf::from("/work/r")));
    }

    #[test]
    fn a_runner_failure_reaches_the_caller() {
        let f = Fake::answering(vec![Err(Error::Run("glab: not logged in".into()))]);
        assert_eq!(
            Glab::new(&f).view_pr(None),
            Err(Error::Run("glab: not logged in".into()))
        );
    }

    #[test]
    fn name_search_is_not_supported_and_says_so() {
        let f = Fake::answering(vec![]);
        let glab = Glab::new(&f);
        assert!(matches!(glab.user(), Err(Error::Unsupported(_))));
        assert!(matches!(
            glab.repo_exists("a", "b"),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(glab.search_repos("x"), Err(Error::Unsupported(_))));
        assert!(
            f.calls_len() == 0,
            "an unsupported operation must not run anything"
        );
    }
}
