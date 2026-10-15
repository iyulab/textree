//! What goes out when a folder is published, as the person chose it: nothing unless chosen,
//! a chosen folder covers everything under it, and the nearest explicit choice wins.

use serde::{Deserialize, Serialize};

/// One explicit choice. `path` is `/`-separated; `""` is the whole folder it is resolved against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub path: String,
    pub include: bool,
}

fn covers(choice: &str, rel: &str) -> bool {
    choice.is_empty()
        || rel.eq_ignore_ascii_case(choice)
        || (rel.len() > choice.len()
            && rel.is_char_boundary(choice.len())
            && rel.as_bytes()[choice.len()] == b'/'
            && rel[..choice.len()].eq_ignore_ascii_case(choice))
}

/// Whether `rel` goes out under `choices`.
#[allow(dead_code)] // consumed by the publish wiring in later steps
pub fn is_included(rel: &str, choices: &[Choice]) -> bool {
    choices
        .iter()
        .filter(|c| covers(&c.path, rel))
        .max_by_key(|c| c.path.len())
        .is_some_and(|c| c.include)
}

use git2::{Repository, Tree};
use std::path::{Path, PathBuf};

/// Where the choices are recorded. It travels with the notes to a connected remote.
pub const PUBLIC_REF: &str = "refs/textree/public";
const FILE: &str = "choices";

fn parse(text: &str) -> Vec<Choice> {
    text.lines()
        .filter_map(|line| {
            let (kind, path) = line.split_once('\t')?;
            let path = if path == "." { String::new() } else { path.to_string() };
            match kind {
                "include" => Some(Choice { path, include: true }),
                "leave-out" => Some(Choice { path, include: false }),
                _ => None,
            }
        })
        .collect()
}

fn render(choices: &[Choice]) -> String {
    let mut sorted = choices.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    sorted
        .iter()
        .map(|c| {
            let kind = if c.include { "include" } else { "leave-out" };
            let path = if c.path.is_empty() { "." } else { c.path.as_str() };
            format!("{kind}\t{path}\n")
        })
        .collect()
}

fn choices_in(repo: &Repository, tree: Option<&Tree<'_>>) -> Result<Vec<Choice>, git2::Error> {
    let Some(tree) = tree else {
        return Ok(Vec::new());
    };
    let Some(id) = crate::git_engine::blob_in_tree(repo, tree, Path::new(FILE)) else {
        return Ok(Vec::new());
    };
    Ok(parse(&String::from_utf8_lossy(repo.find_blob(id)?.content())))
}

/// The choices recorded on `at` (a reference name), `/`-separated and relative to the repository.
pub fn read(repo: &Repository, at: &str) -> Result<Vec<Choice>, git2::Error> {
    choices_in(repo, crate::git_engine::tip_tree(repo, at).as_ref())
}

/// Reads the choices at the tip, lets `change` alter them, and records the result, all in one
/// turn: a change made by someone else meanwhile is read again, never overwritten.
fn update(
    repo: &Repository,
    message: &str,
    mut change: impl FnMut(&mut Vec<Choice>) -> bool,
) -> Result<(), git2::Error> {
    let (author, committer) = crate::git_engine::commit_identities(repo)?;
    crate::git_engine::update_tree_on(repo, PUBLIC_REF, message, &author, &committer, |tree| {
        let mut choices = choices_in(repo, tree)?;
        if !change(&mut choices) {
            return Ok(None);
        }
        let blob = repo.blob(render(&choices).as_bytes())?;
        crate::git_engine::tree_with_file(repo, None, Path::new(FILE), Some(blob)).map(Some)
    })?;
    Ok(())
}

/// Records the choice for `rel_in_repo`: include, leave out, or (`None`) follow the folder above.
pub fn set(repo: &Repository, rel_in_repo: &str, choice: Option<bool>) -> Result<(), git2::Error> {
    update(repo, "Publish choice", |choices| {
        choices.retain(|c| !c.path.eq_ignore_ascii_case(rel_in_repo));
        if let Some(include) = choice {
            choices.push(Choice { path: rel_in_repo.to_string(), include });
        }
        true
    })
}

fn slashed(p: &Path) -> String {
    crate::git_engine::slashed(p)
}

/// Carries the choices of moved notes and folders to where they went.
pub fn follow_moves(repo: &Repository, moves: &[(PathBuf, PathBuf)]) -> Result<(), git2::Error> {
    update(repo, "Publish choice follows a move", |choices| {
        let mut changed = false;
        for (from, to) in moves {
            let (from, to) = (slashed(from), slashed(to));
            for c in choices.iter_mut() {
                if c.path.eq_ignore_ascii_case(&from) {
                    c.path = to.clone();
                    changed = true;
                } else if covers(&from, &c.path) && !from.is_empty() {
                    c.path = format!("{to}{}", &c.path[from.len()..]);
                    changed = true;
                }
            }
        }
        changed
    })
}

/// The choices as seen from a notes folder at `vault_in_repo` (`""` when it is the repository root).
pub fn for_vault(choices: &[Choice], vault_in_repo: &str) -> Vec<Choice> {
    if vault_in_repo.is_empty() {
        return choices.to_vec();
    }
    let mut out = Vec::new();
    // The nearest choice above the folder decides what the folder starts as.
    if let Some(above) = choices
        .iter()
        .filter(|c| covers(&c.path, vault_in_repo) && !c.path.eq_ignore_ascii_case(vault_in_repo))
        .max_by_key(|c| c.path.len())
    {
        out.push(Choice { path: String::new(), include: above.include });
    }
    for c in choices {
        if c.path.eq_ignore_ascii_case(vault_in_repo) {
            out.retain(|o: &Choice| !o.path.is_empty());
            out.push(Choice { path: String::new(), include: c.include });
        } else if covers(vault_in_repo, &c.path) {
            out.push(Choice { path: c.path[vault_in_repo.len() + 1..].to_string(), include: c.include });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Case {
        why: String,
        choices: Vec<Choice>,
        rel: String,
        included: bool,
    }

    #[test]
    fn the_rule_matches_the_shared_cases() {
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../src/lib/publishScope.cases.json")).unwrap();
        for c in cases {
            assert_eq!(is_included(&c.rel, &c.choices), c.included, "{}", c.why);
        }
    }

    use std::path::PathBuf;

    fn repo() -> (tempfile::TempDir, git2::Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        (dir, repo)
    }

    #[test]
    fn choices_round_trip_through_the_reference() {
        let (_d, repo) = repo();
        assert_eq!(read(&repo, PUBLIC_REF).unwrap(), vec![]);
        set(&repo, "", Some(true)).unwrap();
        set(&repo, "work", Some(false)).unwrap();
        set(&repo, "work/press release.md", Some(true)).unwrap();
        let got = read(&repo, PUBLIC_REF).unwrap();
        assert!(is_included("a.md", &got));
        assert!(!is_included("work/memo.md", &got));
        assert!(is_included("work/press release.md", &got));
        set(&repo, "work", None).unwrap();
        assert!(is_included("work/memo.md", &read(&repo, PUBLIC_REF).unwrap()));
    }

    #[test]
    fn each_change_is_one_revision_on_top_of_the_last() {
        let (_d, repo) = repo();
        set(&repo, "a.md", Some(true)).unwrap();
        let first = repo.refname_to_id(PUBLIC_REF).unwrap();
        set(&repo, "b.md", Some(true)).unwrap();
        let second = repo.find_commit(repo.refname_to_id(PUBLIC_REF).unwrap()).unwrap();
        assert_eq!(second.parent_ids().collect::<Vec<_>>(), vec![first]);
        set(&repo, "b.md", Some(true)).unwrap(); // nothing changed
        assert_eq!(repo.refname_to_id(PUBLIC_REF).unwrap(), second.id());
    }

    #[test]
    fn a_write_based_on_a_stale_read_cannot_erase_a_newer_choice() {
        let (_d, repo) = repo();
        set(&repo, "a.md", Some(true)).unwrap();
        let (author, committer) = crate::git_engine::commit_identities(&repo).unwrap();
        let mut first = true;
        crate::git_engine::update_tree_on(&repo, PUBLIC_REF, "Publish choice", &author, &committer, |tree| {
            let mut choices = choices_in(&repo, tree)?;
            if first {
                first = false;
                // Someone else records a choice after this attempt has read the tip.
                let mut theirs = choices.clone();
                theirs.push(Choice { path: "secret".into(), include: false });
                let blob = repo.blob(render(&theirs).as_bytes())?;
                let t = crate::git_engine::tree_with_file(&repo, None, Path::new(FILE), Some(blob))?;
                let tip = repo.find_commit(repo.refname_to_id(PUBLIC_REF)?)?;
                let c = repo.commit(None, &author, &committer, "other", &repo.find_tree(t)?, &[&tip])?;
                repo.reference(PUBLIC_REF, c, true, "other")?;
            }
            choices.push(Choice { path: "b.md".into(), include: true });
            let blob = repo.blob(render(&choices).as_bytes())?;
            crate::git_engine::tree_with_file(&repo, None, Path::new(FILE), Some(blob)).map(Some)
        })
        .unwrap();
        let got = read(&repo, PUBLIC_REF).unwrap();
        assert!(got.contains(&Choice { path: "secret".into(), include: false }));
        assert!(got.contains(&Choice { path: "b.md".into(), include: true }));
        assert!(got.contains(&Choice { path: "a.md".into(), include: true }));
    }

    #[test]
    fn a_move_carries_the_choice() {
        let (_d, repo) = repo();
        set(&repo, "blog", Some(true)).unwrap();
        set(&repo, "blog/draft.md", Some(false)).unwrap();
        set(&repo, "notes/a.md", Some(true)).unwrap();
        follow_moves(
            &repo,
            &[(PathBuf::from("blog"), PathBuf::from("site/blog")), (PathBuf::from("notes/a.md"), PathBuf::from("notes/b.md"))],
        )
        .unwrap();
        let got = read(&repo, PUBLIC_REF).unwrap();
        assert!(is_included("site/blog/x.md", &got));
        assert!(!is_included("site/blog/draft.md", &got));
        assert!(!is_included("blog/x.md", &got));
        assert!(is_included("notes/b.md", &got));
        assert!(!is_included("notes/a.md", &got));
    }

    #[test]
    fn a_vault_inside_a_bigger_repository_sees_its_own_choices() {
        let choices = vec![
            Choice { path: "docs".into(), include: true },
            Choice { path: "docs/internal".into(), include: false },
            Choice { path: "src".into(), include: true },
        ];
        assert_eq!(
            for_vault(&choices, "docs"),
            vec![Choice { path: "".into(), include: true }, Choice { path: "internal".into(), include: false }]
        );
        assert_eq!(for_vault(&choices, ""), choices);
    }
}
