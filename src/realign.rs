//! Realign every written reference to a repository after it was renamed.
//!
//! A rename changes a slug (`owner/old`), a directory and the form of that
//! directory encoded as a file name (`/` becomes `-`, as Claude Code does for
//! its project folders). [`for_repo`] lists those literal forms, [`replace`]
//! rewrites them in a text, and `realign_fs` applies that to a tree. A match
//! only counts when it is a whole name: `polytech` is not touched inside
//! `polytech-corpus`.

use std::path::Path;

/// One literal text and what replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub from: String,
    pub to: String,
}

fn rename(from: impl Into<String>, to: impl Into<String>) -> Rename {
    Rename {
        from: from.into(),
        to: to.into(),
    }
}

fn encoded(dir: &Path) -> String {
    dir.to_string_lossy().replace('/', "-")
}

/// The forms a renamed repository is written in: slug, directory, `~` form
/// of the directory when it lies under `home`, and the encoded directory.
pub fn for_repo(
    owner: &str,
    old: &str,
    new: &str,
    old_dir: &Path,
    new_dir: &Path,
    home: &Path,
) -> Vec<Rename> {
    let mut renames = vec![
        rename(format!("{owner}/{old}"), format!("{owner}/{new}")),
        rename(old_dir.to_string_lossy(), new_dir.to_string_lossy()),
        rename(encoded(old_dir), encoded(new_dir)),
    ];
    if let (Ok(o), Ok(n)) = (old_dir.strip_prefix(home), new_dir.strip_prefix(home)) {
        renames.push(rename(
            format!("~/{}", o.display()),
            format!("~/{}", n.display()),
        ));
    }
    renames
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Does `rest` carry on the name that just matched? A `.git` suffix does not.
fn continues(rest: &str) -> bool {
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if is_name(c) => true,
        Some('.') => {
            let after = &rest[1..];
            let git = after.strip_prefix("git");
            match git {
                Some(tail) => tail.chars().next().is_some_and(is_name),
                None => after.chars().next().is_some_and(char::is_alphanumeric),
            }
        }
        _ => false,
    }
}

/// `text` with every whole-name match rewritten, and how many there were.
pub fn replace(text: &str, renames: &[Rename]) -> (String, usize) {
    let mut ordered: Vec<&Rename> = renames.iter().filter(|r| !r.from.is_empty()).collect();
    ordered.sort_by_key(|r| std::cmp::Reverse(r.from.len()));
    let mut out = String::with_capacity(text.len());
    let (mut count, mut i, mut prev) = (0, 0, None::<char>);
    while i < text.len() {
        let rest = &text[i..];
        let hit = ordered.iter().find(|r| {
            rest.starts_with(&r.from)
                && !prev.is_some_and(is_name)
                && !continues(&rest[r.from.len()..])
        });
        if let Some(r) = hit {
            out.push_str(&r.to);
            i += r.from.len();
            count += 1;
            prev = r.to.chars().last();
        } else {
            let c = rest.chars().next().expect("rest is not empty");
            out.push(c);
            i += c.len_utf8();
            prev = Some(c);
        }
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo() -> Vec<Rename> {
        for_repo(
            "me",
            "polytech-corpus",
            "corpus",
            &PathBuf::from("/Users/me/Source/polytech-corpus"),
            &PathBuf::from("/Users/me/Source/corpus"),
            &PathBuf::from("/Users/me"),
        )
    }

    fn done(text: &str) -> String {
        replace(text, &repo()).0
    }

    #[test]
    fn for_repo_lists_slug_dir_home_form_and_encoded_dir() {
        let froms: Vec<String> = repo().into_iter().map(|r| r.from).collect();
        assert_eq!(
            froms,
            [
                "me/polytech-corpus",
                "/Users/me/Source/polytech-corpus",
                "-Users-me-Source-polytech-corpus",
                "~/Source/polytech-corpus"
            ]
        );
    }

    #[test]
    fn a_directory_outside_home_has_no_home_form() {
        let r = for_repo(
            "o",
            "a",
            "b",
            Path::new("/srv/a"),
            Path::new("/srv/b"),
            Path::new("/Users/me"),
        );
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn rewrites_slug_url_directory_home_form_and_encoded_directory() {
        assert_eq!(
            done("git@github.com:me/polytech-corpus.git"),
            "git@github.com:me/corpus.git"
        );
        assert_eq!(
            done("https://github.com/me/polytech-corpus"),
            "https://github.com/me/corpus"
        );
        assert_eq!(
            done("cd /Users/me/Source/polytech-corpus/tests"),
            "cd /Users/me/Source/corpus/tests"
        );
        assert_eq!(done("~/Source/polytech-corpus"), "~/Source/corpus");
        assert_eq!(
            done("-Users-me-Source-polytech-corpus/memory"),
            "-Users-me-Source-corpus/memory"
        );
    }

    #[test]
    fn counts_each_rewrite() {
        let (_, n) = replace("me/polytech-corpus and me/polytech-corpus", &repo());
        assert_eq!(n, 2);
    }

    #[test]
    fn leaves_a_longer_name_alone() {
        assert_eq!(done("me/polytech-corpus-v2"), "me/polytech-corpus-v2");
        assert_eq!(done("me/polytech-corpus_old"), "me/polytech-corpus_old");
        assert_eq!(done("me/polytech-corpus.bak"), "me/polytech-corpus.bak");
    }

    #[test]
    fn leaves_a_longer_name_alone_before_the_match_too() {
        assert_eq!(done("xme/polytech-corpus"), "xme/polytech-corpus");
    }

    #[test]
    fn a_sentence_period_does_not_hide_a_match() {
        assert_eq!(done("see me/polytech-corpus."), "see me/corpus.");
        assert_eq!(done("me/polytech-corpus.git."), "me/corpus.git.");
    }

    #[test]
    fn the_shorter_old_name_is_not_rewritten_inside_the_longer_one() {
        let r = for_repo(
            "me",
            "polytech",
            "lib",
            Path::new("/Users/me/Source/polytech"),
            Path::new("/Users/me/Source/lib"),
            Path::new("/Users/me"),
        );
        let (out, n) = replace(
            "/Users/me/Source/polytech-corpus and /Users/me/Source/polytech",
            &r,
        );
        assert_eq!(
            out,
            "/Users/me/Source/polytech-corpus and /Users/me/Source/lib"
        );
        assert_eq!(n, 1);
    }

    #[test]
    fn handles_non_ascii_text_and_empty_input() {
        assert_eq!(done("é me/polytech-corpus é"), "é me/corpus é");
        assert_eq!(done(""), "");
    }

    #[test]
    fn an_empty_from_is_ignored() {
        assert_eq!(replace("abc", &[rename("", "x")]).0, "abc");
    }
}
