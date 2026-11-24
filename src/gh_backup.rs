//! The copy of a pull request's branch that `Gh::merge_pr` takes before merging.

use crate::forge::{Error, Runner};
use crate::gh::Gh;

/// `<branch>` and `<tip>` of the pull request, joined by a unit separator.
const TIP_JQ: &str = r#"[.headRefName,.headRefOid]|join("\u001f")"#;

impl<R: Runner> Gh<R> {
    /// Pins the tip of the branch of pull request `number` as the remote ref
    /// `refs/backup/<branch>/<tip>` and returns its name. The name carries the
    /// tip, so a reused branch name never replaces an earlier copy; asking for
    /// the same tip twice (a merge retried) succeeds.
    pub(crate) fn backup_pr_branch(&self, number: u64) -> Result<String, Error> {
        let n = number.to_string();
        let args = [
            "pr",
            "view",
            &n,
            "--json",
            "headRefName,headRefOid",
            "--jq",
            TIP_JQ,
        ];
        let raw = self.gh(&args)?;
        let (head, tip) = raw
            .trim_end_matches('\n')
            .split_once('\u{1f}')
            .filter(|(h, t)| !h.is_empty() && !t.is_empty())
            .ok_or_else(|| Error::Parse(raw.clone()))?;
        let name = format!("refs/backup/{head}/{tip}");
        let (name_arg, tip_arg) = (format!("ref={name}"), format!("sha={tip}"));
        let create = [
            "api",
            "repos/{owner}/{repo}/git/refs",
            "-f",
            &name_arg,
            "-f",
            &tip_arg,
        ];
        match self.gh(&create) {
            Err(Error::Run(why)) if why.contains("Reference already exists") => Ok(name),
            other => other.map(|_| name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::Forge;
    use crate::testing::Fake;

    fn tip() -> Result<String, Error> {
        Ok("feat\u{1f}abc123\n".into())
    }

    #[test]
    fn the_tip_is_read_then_pinned_as_a_remote_backup_ref() {
        let f = Fake::answering(vec![tip(), Ok(String::new())]);
        let name = Gh::new(&f).backup_pr_branch(8).unwrap();
        assert_eq!(name, "refs/backup/feat/abc123");
        assert_eq!(
            f.call(0).1,
            [
                "pr",
                "view",
                "8",
                "--json",
                "headRefName,headRefOid",
                "--jq",
                TIP_JQ
            ]
        );
        assert_eq!(
            f.call(1).1,
            [
                "api",
                "repos/{owner}/{repo}/git/refs",
                "-f",
                "ref=refs/backup/feat/abc123",
                "-f",
                "sha=abc123"
            ]
        );
    }

    #[test]
    fn a_branch_name_with_a_slash_keeps_it() {
        let f = Fake::answering(vec![Ok("feat/x\u{1f}f00\n".into()), Ok(String::new())]);
        assert_eq!(
            Gh::new(&f).backup_pr_branch(1).unwrap(),
            "refs/backup/feat/x/f00"
        );
    }

    #[test]
    fn a_failing_view_creates_nothing() {
        let f = Fake::answering(vec![Err(Error::Run("not logged in".into()))]);
        assert_eq!(
            Gh::new(&f).backup_pr_branch(8),
            Err(Error::Run("not logged in".into()))
        );
        assert_eq!(f.calls_len(), 1);
    }

    #[test]
    fn an_unreadable_or_empty_tip_is_a_parse_error_and_creates_nothing() {
        for raw in ["", "\n", "feat\n", "\u{1f}abc\n", "feat\u{1f}\n"] {
            let f = Fake::answering(vec![Ok(raw.into())]);
            assert_eq!(
                Gh::new(&f).backup_pr_branch(8),
                Err(Error::Parse(raw.into())),
                "{raw:?}"
            );
            assert_eq!(f.calls_len(), 1, "{raw:?}");
        }
    }

    #[test]
    fn a_failing_ref_creation_reaches_the_caller() {
        let f = Fake::answering(vec![tip(), Err(Error::Run("HTTP 403".into()))]);
        assert_eq!(
            Gh::new(&f).backup_pr_branch(8),
            Err(Error::Run("HTTP 403".into()))
        );
    }

    #[test]
    fn a_copy_that_already_exists_is_the_same_copy() {
        let f = Fake::answering(vec![
            tip(),
            Err(Error::Run("Reference already exists".into())),
        ]);
        assert_eq!(
            Gh::new(&f).backup_pr_branch(8).unwrap(),
            "refs/backup/feat/abc123"
        );
    }

    #[test]
    fn merge_pr_copies_the_branch_first_and_merges_only_then() {
        let f = Fake::answering(vec![tip(), Ok(String::new()), Ok(String::new())]);
        Gh::new(&f).merge_pr(8).unwrap();
        assert_eq!(f.calls_len(), 3);
        assert_eq!(f.call(1).1[0], "api");
        assert_eq!(
            f.call(2).1,
            ["pr", "merge", "8", "--merge", "--delete-branch"]
        );
    }

    #[test]
    fn merge_pr_does_not_merge_when_the_copy_fails() {
        let f = Fake::answering(vec![tip(), Err(Error::Run("HTTP 403".into()))]);
        assert_eq!(Gh::new(&f).merge_pr(8), Err(Error::Run("HTTP 403".into())));
        assert_eq!(f.calls_len(), 2, "no merge call");
    }
}
