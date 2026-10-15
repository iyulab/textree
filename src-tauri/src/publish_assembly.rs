//! What a publish sends, built from the last version added of each note — never from the files
//! as they are in the folder. Only chosen notes go out, with only the files they link to.

#![allow(dead_code)] // consumed by the publish wiring in a later step

use crate::git_engine::{self, VaultRepo, NOTES_REF};
use crate::note_refs::{self, Target};
use crate::publish_scope;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assembly {
    /// Every file that goes out: (path relative to the notes folder, content).
    pub files: Vec<(String, Vec<u8>)>,
    /// Included notes going out.
    pub notes: Vec<String>,
    /// Files they link to that go out.
    pub assets: Vec<String>,
    /// Included notes with no version: left out.
    pub never_recorded: Vec<String>,
    /// Files linked to but not in the last versions: left out.
    pub missing: Vec<String>,
    /// Notes linked from published ones that do not go out.
    pub unpublished_links: Vec<String>,
    pub nothing_chosen: bool,
}

fn is_note(rel: &str) -> bool {
    rel.len() >= 3 && rel.as_bytes()[rel.len() - 3..].eq_ignore_ascii_case(b".md")
}

fn hidden(rel: &str) -> bool {
    rel.split('/').any(|s| s.starts_with('.'))
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Notes on disk under `root` (relative, `/`-separated) — only to name the chosen ones that have
/// no version yet.
fn notes_on_disk(root: &Path) -> Vec<String> {
    crate::publish::outgoing_notes(root).unwrap_or_default()
}

pub fn assemble(prepared: &VaultRepo, root: &Path) -> Result<Assembly, String> {
    let repo = prepared.repo();
    let err = |e: git2::Error| e.message().to_string();
    let vault_prefix = git_engine::slashed(&prepared.path_in_repo(Path::new("")))
        .trim_end_matches('/')
        .to_string();
    let choices = publish_scope::for_vault(
        &publish_scope::read(repo, publish_scope::PUBLIC_REF).map_err(err)?,
        &vault_prefix,
    );
    let mut out = Assembly { nothing_chosen: choices.iter().all(|c| !c.include), ..Assembly::default() };
    if out.nothing_chosen {
        return Ok(out);
    }

    // Everything recorded inside this folder, relative to it.
    let in_vault = |in_repo: &str| -> Option<String> {
        if vault_prefix.is_empty() {
            Some(in_repo.to_string())
        } else {
            in_repo.strip_prefix(&format!("{vault_prefix}/")).map(str::to_string)
        }
    };
    // Recorded, and still in the folder: a note or file the person deleted does not go out, even
    // though its last version stays in the history (that is what Deleted notes brings back).
    let on_disk = |rel: &str| {
        let mut path = root.to_path_buf();
        path.extend(rel.split('/'));
        path.is_file()
    };
    let recorded: BTreeSet<String> = git_engine::tip_paths(repo, NOTES_REF)
        .map_err(err)?
        .iter()
        .filter_map(|p| in_vault(p))
        .filter(|p| !hidden(p) && on_disk(p))
        .collect();
    let by_name: BTreeMap<String, Vec<String>> =
        recorded.iter().fold(BTreeMap::new(), |mut m, p| {
            m.entry(file_name(p).to_ascii_lowercase()).or_default().push(p.clone());
            m
        });
    let read = |rel: &str| -> Result<Option<Vec<u8>>, String> {
        git_engine::content_at_tip(repo, NOTES_REF, &prepared.path_in_repo(Path::new(rel)))
            .map_err(err)
    };
    // A resolved path that names a folder (`.`, `sub/`) is neither a file nor a note.
    let is_folder = |rel: &str| {
        rel.is_empty() || {
            let prefix = format!("{}/", rel.trim_end_matches('/'));
            recorded.iter().any(|p| p.starts_with(&prefix))
        }
    };

    let chosen_notes: Vec<&String> =
        recorded.iter().filter(|p| is_note(p) && publish_scope::is_included(p, &choices)).collect();
    let mut assets = BTreeSet::new();
    let mut missing = BTreeSet::new();
    let mut linked_notes = BTreeSet::new();
    for note in &chosen_notes {
        let Some(body) = read(note)? else { continue };
        out.files.push(((*note).clone(), body.clone()));
        out.notes.push((*note).clone());
        for target in note_refs::targets(&String::from_utf8_lossy(&body)) {
            let resolved = match target {
                Target::Path(t) => note_refs::resolve(note, &t),
                Target::Wiki(name) => {
                    let key = if name.contains('.') {
                        name.to_ascii_lowercase()
                    } else {
                        format!("{}.md", name.to_ascii_lowercase())
                    };
                    by_name.get(file_name(&key)).and_then(|c| (c.len() == 1).then(|| c[0].clone()))
                }
            };
            let Some(rel) = resolved else { continue };
            if hidden(&rel) || is_folder(&rel) {
                continue;
            }
            if is_note(&rel) {
                if !(recorded.contains(&rel) && publish_scope::is_included(&rel, &choices)) {
                    linked_notes.insert(rel);
                }
            } else if recorded.contains(&rel) {
                assets.insert(rel);
            } else {
                missing.insert(rel);
            }
        }
    }
    for asset in &assets {
        if let Some(bytes) = read(asset)? {
            out.files.push((asset.clone(), bytes));
        }
    }
    out.files.sort_by(|a, b| a.0.cmp(&b.0));
    out.assets = assets.into_iter().collect();
    out.missing = missing.into_iter().collect();
    out.unpublished_links = linked_notes.into_iter().filter(|n| recorded.contains(n)).collect();
    out.never_recorded = notes_on_disk(root)
        .into_iter()
        .filter(|n| !hidden(n) && publish_scope::is_included(n, &choices) && !recorded.contains(n))
        .collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn record(root: &Path, rel: &str, body: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        let prepared = crate::git_engine::prepare(root).unwrap();
        let (author, committer) = crate::git_engine::commit_identities(prepared.repo()).unwrap();
        crate::git_engine::commit_paths(
            prepared.repo(),
            crate::git_engine::NOTES_REF,
            &[(prepared.path_in_repo(Path::new(rel)), body.to_vec())],
            "test",
            &author,
            &committer,
            crate::git_engine::WhenUnchanged::Skip,
        )
        .unwrap();
    }

    fn choose(root: &Path, rel: &str, include: bool) {
        let prepared = crate::git_engine::prepare(root).unwrap();
        let in_repo = crate::git_engine::slashed(&prepared.path_in_repo(Path::new(rel)));
        crate::publish_scope::set(prepared.repo(), in_repo.trim_end_matches('/'), Some(include))
            .unwrap();
    }

    fn assembled(root: &Path) -> Assembly {
        assemble(&crate::git_engine::prepare(root).unwrap(), root).unwrap()
    }

    fn paths(a: &Assembly) -> Vec<&str> {
        a.files.iter().map(|(p, _)| p.as_str()).collect()
    }

    #[test]
    fn nothing_chosen_sends_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "a.md", b"# A");
        let a = assembled(tmp.path());
        assert!(a.nothing_chosen);
        assert!(a.files.is_empty());
    }

    #[test]
    fn gate_l2_8_a_note_not_chosen_never_goes_out() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "blog/post.md", b"# Post");
        record(tmp.path(), "private/diary.md", b"# Diary");
        record(tmp.path(), "blog/draft.md", b"# Draft");
        choose(tmp.path(), "blog", true);
        choose(tmp.path(), "blog/draft.md", false);
        assert_eq!(paths(&assembled(tmp.path())), vec!["blog/post.md"]);
    }

    #[test]
    fn gate_l2_5_an_edit_not_added_as_a_version_never_goes_out() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "a.md", b"recorded");
        std::fs::write(tmp.path().join("a.md"), b"typed later, never added").unwrap();
        choose(tmp.path(), "", true);
        let a = assembled(tmp.path());
        assert_eq!(a.files, vec![("a.md".to_string(), b"recorded".to_vec())]);
    }

    #[test]
    fn a_public_note_never_recorded_stays_out() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "a.md", b"# A");
        std::fs::write(tmp.path().join("fresh.md"), b"# Fresh").unwrap();
        choose(tmp.path(), "", true);
        let a = assembled(tmp.path());
        assert_eq!(a.notes, vec!["a.md"]);
        assert_eq!(a.never_recorded, vec!["fresh.md"]);
    }

    #[test]
    fn only_linked_files_go_out_and_missing_ones_are_named() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "img/chart.png", b"png");
        record(tmp.path(), "img/unused.png", b"png");
        record(tmp.path(), "secret.txt", b"key");
        record(
            tmp.path(),
            "blog/post.md",
            b"![c](../img/chart.png) ![g](gone.png) ![[photo.jpg]] [[Diary]]",
        );
        record(tmp.path(), "photos/photo.jpg", b"jpg");
        record(tmp.path(), "diary.md", b"# Diary");
        choose(tmp.path(), "blog", true);
        let a = assembled(tmp.path());
        assert_eq!(paths(&a), vec!["blog/post.md", "img/chart.png", "photos/photo.jpg"]);
        assert_eq!(a.missing, vec!["blog/gone.png"]);
        assert_eq!(a.unpublished_links, vec!["diary.md"]);
    }

    #[test]
    fn a_deleted_note_never_goes_out() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "a.md", b"# A");
        record(tmp.path(), "b.md", b"# B ![](pic.png)");
        record(tmp.path(), "pic.png", b"png");
        choose(tmp.path(), "", true);
        std::fs::remove_file(tmp.path().join("a.md")).unwrap();
        std::fs::remove_file(tmp.path().join("pic.png")).unwrap();
        let a = assembled(tmp.path());
        assert_eq!(paths(&a), vec!["b.md"]);
        assert_eq!(a.missing, vec!["pic.png"]);
    }

    #[test]
    fn dot_files_never_go_out_even_when_linked() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), ".env", b"KEY=1");
        record(tmp.path(), "a.md", b"[x](.env)");
        choose(tmp.path(), "", true);
        assert_eq!(paths(&assembled(tmp.path())), vec!["a.md"]);
    }

    #[test]
    fn a_folder_inside_a_bigger_repository_sends_its_own_notes_only() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let vault = tmp.path().join("docs");
        std::fs::create_dir_all(&vault).unwrap();
        record(&vault, "a.md", b"# A");
        record(tmp.path(), "README.md", b"# outside");
        choose(&vault, "", true);
        assert_eq!(paths(&assembled(&vault)), vec!["a.md"]);
    }

    #[test]
    fn a_link_to_a_folder_is_not_listed_anywhere() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), "sub/inner.md", b"# Inner");
        record(tmp.path(), "a.md", b"[x](./) [y](sub/) [z](.)");
        choose(tmp.path(), "a.md", true);
        let a = assembled(tmp.path());
        assert_eq!(paths(&a), vec!["a.md"]);
        assert!(a.missing.is_empty());
        assert!(a.unpublished_links.is_empty());
        assert!(a.assets.is_empty());
    }
}
