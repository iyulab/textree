//! Names, headings and tags written in one Unicode normalization form find what is stored in the
//! other. Test data uses Korean text, whose syllables decompose into conjoining letters, and an
//! accented Latin word.

use textree::path::{in_form_of, nfc};
use textree::{build_graph, plan, MemoryStore, Op, Policy, Snapshot};
use unicode_normalization::UnicodeNormalization;

fn nfd(s: &str) -> String {
    s.nfd().collect()
}

/// A folder where the document's name and headings are in one form and the links to it in the
/// other.
fn folder(name_form: fn(&str) -> String, link_form: fn(&str) -> String) -> MemoryStore {
    let doc = name_form("회의록");
    let index = format!(
        "# Index\n\n[[{n}]] · [[{n}#{h}]] · [link]({n}.md) · [[{c}]] #{t}\n",
        n = link_form("회의록"),
        h = link_form("결정 사항"),
        c = link_form("café"),
        t = link_form("회의"),
    );
    MemoryStore::new()
        .with_file(
            &format!("{doc}.md"),
            format!(
                "# {}\n\n## {}\n\nText #{}\n",
                name_form("회의록"),
                name_form("결정 사항"),
                name_form("회의")
            ),
        )
        .with_file(&format!("{}.md", name_form("café")), "# Café\n")
        .with_file("index.md", index)
}

#[test]
fn links_resolve_across_normalization_forms() {
    for (store, doc) in [
        (folder(nfd, nfc), nfd("회의록.md")),
        (folder(nfc, nfd), nfc("회의록.md")),
    ] {
        let graph = build_graph(&store);
        assert!(graph.broken.is_empty(), "broken: {:?}", graph.broken);
        let targets: Vec<_> = graph
            .links
            .iter()
            .filter(|l| l.from == "index.md")
            .map(|l| l.to.clone())
            .collect();
        assert_eq!(targets.len(), 4);
        // The path in the graph is the stored spelling.
        assert_eq!(targets[0].as_deref(), Some(doc.as_str()));
        assert_eq!(targets[1].as_deref(), Some(doc.as_str()));
        assert_eq!(targets[2].as_deref(), Some(doc.as_str()));
    }
}

#[test]
fn tags_in_both_forms_are_one_tag_shown_in_form_c() {
    let graph = build_graph(&folder(nfd, nfc));
    assert_eq!(graph.tags.len(), 1, "{:?}", graph.tags);
    let (shown, docs) = graph.tags.iter().next().unwrap();
    assert_eq!(shown, &nfc("회의"));
    assert_eq!(docs.len(), 2);
}

#[test]
fn a_rewritten_link_keeps_the_form_it_was_written_in() {
    for (store, written) in [
        (folder(nfd, nfc), nfc as fn(&str) -> String),
        (folder(nfc, nfd), nfd as fn(&str) -> String),
    ] {
        let snap = Snapshot::read(&store);
        let from = snap
            .nodes
            .iter()
            .find(|n| nfc(&n.path) == nfc("회의록.md"))
            .unwrap()
            .path
            .clone();
        let to = format!("{}/{from}", written("기록"));
        let planned = plan(&snap, &Op::Move { from, to }, &Policy::default()).unwrap();
        let index = planned
            .changes
            .writes
            .iter()
            .find(|w| w.path == "index.md")
            .unwrap();
        let want = format!("[link]({}.md)", written("기록/회의록"));
        assert!(index.content.contains(&want), "{}", index.content);
        // Everything else in the document is as it was.
        assert!(index
            .content
            .contains(&format!("[[{}]]", written("회의록"))));
    }
}

#[test]
fn names_that_differ_only_in_form_clash() {
    let store = MemoryStore::new()
        .with_file(&format!("{}.md", nfd("회의록")), "# A\n")
        .with_file("b.md", "# B\n");
    let snap = Snapshot::read(&store);
    let refused = plan(
        &snap,
        &Op::Move {
            from: "b.md".to_string(),
            to: format!("{}.md", nfc("회의록")),
        },
        &Policy::default(),
    );
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn form_of_a_replacement() {
    assert_eq!(in_form_of(&nfc("가"), &nfd("나")), nfc("나"));
    assert_eq!(in_form_of(&nfd("가"), &nfc("나")), nfd("나"));
    assert_eq!(in_form_of("plain", &nfd("나")), nfd("나"));
    assert_eq!(in_form_of(&nfc("가"), "ascii"), "ascii");
}
