//! The copy of a merge request's branch that `Glab::merge_pr` takes before merging.

use crate::forge::{Error, Runner};
use crate::glab::Glab;

/// `<branch>` and `<tip>` of the merge request, joined by a unit separator.
const TIP_JQ: &str = r#"[.source_branch,.sha]|join("\u001f")"#;

impl<R: Runner> Glab<R> {
    /// Pins the tip of the source branch of merge request `number` as the
    /// branch `backup/<branch>/<tip>` and returns its name. GitLab has no
    /// custom refs through its API, so the copy is a branch; its name carries
    /// the tip, so a reused branch name never replaces an earlier copy, and
    /// asking for the same tip twice (a merge retried) succeeds.
    pub(crate) fn backup_mr_branch(&self, number: u64) -> Result<String, Error> {
        let n = number.to_string();
        let view = ["mr", "view", &n, "--output", "json", "--jq", TIP_JQ];
        let raw = self.glab(&view)?;
        let (head, tip) = raw
            .trim_end_matches('\n')
            .split_once('\u{1f}')
            .filter(|(h, t)| !h.is_empty() && !t.is_empty())
            .ok_or_else(|| Error::Parse(raw.clone()))?;
        let name = format!("backup/{head}/{tip}");
        let (name_arg, tip_arg) = (format!("branch={name}"), format!("ref={tip}"));
        let create = [
            "api",
            "--method",
            "POST",
            "projects/:id/repository/branches",
            "-f",
            &name_arg,
            "-f",
            &tip_arg,
        ];
        match self.glab(&create) {
            Err(Error::Run(why)) if why.contains("already exists") => Ok(name),
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
    fn the_tip_is_read_then_pinned_as_a_backup_branch() {
        let f = Fake::answering(vec![tip(), Ok(String::new())]);
        let name = Glab::new(&f).backup_mr_branch(8).unwrap();
        assert_eq!(name, "backup/feat/abc123");
        assert_eq!(
            f.call(0).1,
            ["mr", "view", "8", "--output", "json", "--jq", TIP_JQ]
        );
        assert_eq!(
            f.call(1).1,
            [
                "api",
                "--method",
                "POST",
                "projects/:id/repository/branches",
                "-f",
                "branch=backup/feat/abc123",
                "-f",
                "ref=abc123"
            ]
        );
    }

    #[test]
    fn a_branch_name_with_a_slash_keeps_it() {
        let f = Fake::answering(vec![Ok("feat/x\u{1f}f00\n".into()), Ok(String::new())]);
        assert_eq!(
            Glab::new(&f).backup_mr_branch(1).unwrap(),
            "backup/feat/x/f00"
        );
    }

    #[test]
    fn a_failing_view_creates_nothing() {
        let f = Fake::answering(vec![Err(Error::Run("not logged in".into()))]);
        assert_eq!(
            Glab::new(&f).backup_mr_branch(8),
            Err(Error::Run("not logged in".into()))
        );
        assert_eq!(f.calls_len(), 1);
    }

    #[test]
    fn an_unreadable_or_empty_tip_is_a_parse_error_and_creates_nothing() {
        for raw in ["", "\n", "feat\n", "\u{1f}abc\n", "feat\u{1f}\n"] {
            let f = Fake::answering(vec![Ok(raw.into())]);
            assert_eq!(
                Glab::new(&f).backup_mr_branch(8),
                Err(Error::Parse(raw.into())),
                "{raw:?}"
            );
            assert_eq!(f.calls_len(), 1, "{raw:?}");
        }
    }

    #[test]
    fn a_failing_branch_creation_reaches_the_caller() {
        let f = Fake::answering(vec![tip(), Err(Error::Run("403 Forbidden".into()))]);
        assert_eq!(
            Glab::new(&f).backup_mr_branch(8),
            Err(Error::Run("403 Forbidden".into()))
        );
    }

    #[test]
    fn a_copy_that_already_exists_is_the_same_copy() {
        let f = Fake::answering(vec![
            tip(),
            Err(Error::Run("400 Bad request - Branch already exists".into())),
        ]);
        assert_eq!(
            Glab::new(&f).backup_mr_branch(8).unwrap(),
            "backup/feat/abc123"
        );
    }

    #[test]
    fn merge_pr_copies_the_branch_first_and_merges_only_then() {
        let f = Fake::answering(vec![tip(), Ok(String::new()), Ok(String::new())]);
        Glab::new(&f).merge_pr(8).unwrap();
        assert_eq!(f.calls_len(), 3);
        assert_eq!(f.call(1).1[0], "api");
        assert_eq!(
            f.call(2).1,
            ["mr", "merge", "8", "--remove-source-branch", "--yes"]
        );
    }

    #[test]
    fn merge_pr_does_not_merge_when_the_copy_fails() {
        let f = Fake::answering(vec![tip(), Err(Error::Run("403 Forbidden".into()))]);
        assert_eq!(
            Glab::new(&f).merge_pr(8),
            Err(Error::Run("403 Forbidden".into()))
        );
        assert_eq!(f.calls_len(), 2, "no merge call");
    }
}
