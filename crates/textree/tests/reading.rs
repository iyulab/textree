//! Behaviour of the engine over in-memory hierarchies: resolution, link forms, code, positions.

use textree::{build_graph, Graph, LinkKind, MemoryStore};

fn graph(files: &[(&str, &str)]) -> Graph {
    let mut store = MemoryStore::new();
    for (path, body) in files {
        store = store.with_file(path, body);
    }
    build_graph(&store)
}

fn target_of(graph: &Graph, from: &str) -> Option<String> {
    graph
        .links
        .iter()
        .find(|l| l.from == from)
        .and_then(|l| l.to.clone())
}

// --- the nearest document wins -------------------------------------------------------------

#[test]
fn same_folder_beats_a_deeper_folder() {
    let g = graph(&[
        ("a/b/link.md", "[[note]]"),
        ("a/b/note.md", "x"),
        ("a/b/c/note.md", "x"),
    ]);
    assert_eq!(target_of(&g, "a/b/link.md").as_deref(), Some("a/b/note.md"));
}

#[test]
fn deeper_shared_folder_beats_shorter_path() {
    // `a/b/c/d/note.md` shares `a/b` with the link; `a/note.md` only shares `a` but is shorter.
    let g = graph(&[
        ("a/b/link.md", "[[note]]"),
        ("a/b/c/d/note.md", "x"),
        ("a/note.md", "x"),
    ]);
    assert_eq!(
        target_of(&g, "a/b/link.md").as_deref(),
        Some("a/b/c/d/note.md")
    );
}

#[test]
fn shorter_path_beats_alphabetical_order() {
    let g = graph(&[
        ("link.md", "[[note]]"),
        ("z/note.md", "x"),
        ("a/b/note.md", "x"),
    ]);
    assert_eq!(target_of(&g, "link.md").as_deref(), Some("z/note.md"));
}

#[test]
fn alphabetical_order_breaks_a_complete_tie() {
    let g = graph(&[
        ("link.md", "[[note]]"),
        ("m/note.md", "x"),
        ("n/note.md", "x"),
    ]);
    assert_eq!(target_of(&g, "link.md").as_deref(), Some("m/note.md"));
    assert_eq!(g.links[0].alternatives, vec!["n/note.md".to_string()]);
}

#[test]
fn listing_order_does_not_change_the_result() {
    let forward = graph(&[
        ("link.md", "[[note]]"),
        ("m/note.md", "x"),
        ("n/note.md", "x"),
    ]);
    let backward = graph(&[
        ("n/note.md", "x"),
        ("m/note.md", "x"),
        ("link.md", "[[note]]"),
    ]);
    assert_eq!(
        serde_json::to_string(&forward).unwrap(),
        serde_json::to_string(&backward).unwrap()
    );
}

// --- the forms a link can take -------------------------------------------------------------

const INBOX: &str = "# Top\n[[note|the note]] [[note#Intro]] [[#Top]] ![[note]] [[Sub/Note]]\n\
[p](my%20file.md) [q](sub\\note.md) [ext](https://example.com) [anc](#top) [out](../outside.md) [ref][r]\n\n[r]: note.md\n";

fn link_forms() -> Graph {
    graph(&[
        ("inbox.md", INBOX),
        ("note.md", "# Intro\n"),
        ("sub/note.md", "x\n"),
        ("my file.md", "x\n"),
    ])
}

#[test]
fn wiki_link_forms_resolve() {
    let g = link_forms();
    let by_text = |text: &str| {
        g.links
            .iter()
            .find(|l| l.link == text)
            .unwrap_or_else(|| panic!("no link {text}"))
    };

    let aliased = by_text("[[note|the note]]");
    assert_eq!(aliased.to.as_deref(), Some("note.md"));
    assert_eq!(aliased.alias.as_deref(), Some("the note"));

    let headed = by_text("[[note#Intro]]");
    assert_eq!(headed.to.as_deref(), Some("note.md"));
    assert_eq!(headed.heading.as_deref(), Some("Intro"));

    let embed = by_text("![[note]]");
    assert_eq!(embed.to.as_deref(), Some("note.md"));
    assert_eq!(embed.kind, LinkKind::Embed);

    assert_eq!(by_text("[[Sub/Note]]").to.as_deref(), Some("sub/note.md"));
}

#[test]
fn path_link_forms_resolve() {
    let g = link_forms();
    let by_text = |text: &str| {
        g.links
            .iter()
            .find(|l| l.link == text)
            .unwrap_or_else(|| panic!("no link {text}"))
    };
    assert_eq!(
        by_text("[p](my%20file.md)").to.as_deref(),
        Some("my file.md")
    );
    assert_eq!(
        by_text("[q](sub\\note.md)").to.as_deref(),
        Some("sub/note.md")
    );
    assert_eq!(by_text("[ref][r]").to.as_deref(), Some("note.md"));
}

#[test]
fn outside_links_and_own_headings_are_not_in_the_graph() {
    let g = link_forms();
    for gone in [
        "[[#Top]]",
        "[ext](https://example.com)",
        "[anc](#top)",
        "[out](../outside.md)",
    ] {
        assert!(
            g.links.iter().all(|l| l.link != gone),
            "{gone} should not be a link"
        );
    }
    assert!(g.broken.is_empty());
}

#[test]
fn backlinks_count_a_source_once_and_orphans_follow() {
    let g = link_forms();
    for target in ["note.md", "sub/note.md", "my file.md"] {
        assert_eq!(
            g.backlinks[target],
            vec!["inbox.md".to_string()],
            "{target}"
        );
    }
    assert!(!g.backlinks.contains_key("inbox.md"));
    assert_eq!(g.orphans, vec!["inbox.md".to_string()]);
}

// --- code is never read --------------------------------------------------------------------

const CODE: &str = "# Real\n\nText with `[[inline ghost]]` and #real-tag here.\n\n```\n[[fenced ghost]]\n#fenced-tag\n# not a heading\n```\n\n~~~\n[x](ghost.md)\n~~~\n";
const FRONT: &str = "---\ntags: [alpha, beta]\n---\nBody with #gamma.\n";

#[test]
fn code_is_not_read_and_tags_come_from_body_and_front_matter() {
    let g = graph(&[("code.md", CODE), ("fm.md", FRONT)]);
    assert!(g.links.is_empty());
    assert!(g.broken.is_empty());
    let tags: Vec<(&str, Vec<&str>)> = g
        .tags
        .iter()
        .map(|(t, d)| (t.as_str(), d.iter().map(String::as_str).collect()))
        .collect();
    assert_eq!(
        tags,
        vec![
            ("alpha", vec!["fm.md"]),
            ("beta", vec!["fm.md"]),
            ("gamma", vec!["fm.md"]),
            ("real-tag", vec!["code.md"])
        ]
    );
    let code = g.documents.iter().find(|d| d.path == "code.md").unwrap();
    assert_eq!(
        code.headings
            .iter()
            .map(|h| h.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Real"]
    );
}

#[test]
fn tags_need_a_word_boundary_and_a_letter() {
    let g = graph(&[("t.md", "C# issue #123 url/#frag a#b (#ok) #fine, #also.\n")]);
    let names: Vec<&str> = g.tags.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["also", "fine", "ok"]);
}

#[test]
fn tag_case_is_ignored_and_the_commonest_spelling_is_shown() {
    let g = graph(&[("a.md", "#Garden #garden #garden"), ("b.md", "#GARDEN")]);
    assert_eq!(g.tags.keys().collect::<Vec<_>>(), vec!["garden"]);
    assert_eq!(
        g.tags["garden"],
        vec!["a.md".to_string(), "b.md".to_string()]
    );
}

#[test]
fn footnote_definitions_are_not_links() {
    // Without footnote support `[^1]: Plain` would be a reference definition and `[^1]` a link.
    let g = graph(&[("f.md", "Text[^1].\n\n[^1]: Plain\n")]);
    assert!(g.links.is_empty() && g.broken.is_empty());
}

#[test]
fn a_document_linking_to_itself_gets_no_backlink() {
    let g = graph(&[("a.md", "see [[a]] and [me](a.md)\n"), ("b.md", "[[a]]\n")]);
    assert_eq!(g.links.iter().filter(|l| l.from == "a.md").count(), 2);
    assert_eq!(g.backlinks["a.md"], vec!["b.md".to_string()]);
    assert_eq!(g.orphans, vec!["b.md".to_string()]);
}

// --- positions -----------------------------------------------------------------------------

#[test]
fn ranges_cut_the_exact_text_out_of_the_source_bytes() {
    // Non-ASCII on purpose: byte offsets and character offsets differ here.
    let source = "한글 🌱 시작 [[note]] 끝 #태그\n## 제목\n";
    let g = graph(&[("ko.md", source), ("note.md", "x\n")]);
    let bytes = source.as_bytes();
    let cut = |r: &textree::Range| {
        std::str::from_utf8(&bytes[r.start..r.end])
            .unwrap()
            .to_string()
    };

    let link = &g.links[0];
    assert_eq!(cut(&link.range), "[[note]]");
    assert_eq!(link.line, 1);

    let doc = g.documents.iter().find(|d| d.path == "ko.md").unwrap();
    assert_eq!(doc.tags.len(), 1);
    assert_eq!(cut(&doc.tags[0].range), "#태그");
    assert_eq!(doc.headings.len(), 1);
    assert_eq!(cut(&doc.headings[0].range), "## 제목");
    assert_eq!(doc.headings[0].line, 2);
}

#[test]
fn ranges_include_a_byte_order_mark_and_lines_count_crlf() {
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(b"[[note]]\n");
    let store = MemoryStore::new()
        .with_file("note.md", "x\n")
        .with_file("bom.md", bom.clone())
        .with_file("crlf.md", "a\r\n[[note]]\r\n")
        .with_file("empty.md", "");
    let g = build_graph(&store);

    let bom_link = g.links.iter().find(|l| l.from == "bom.md").unwrap();
    assert_eq!(bom_link.to.as_deref(), Some("note.md"));
    assert_eq!(&bom[bom_link.range.start..bom_link.range.end], b"[[note]]");

    let crlf_link = g.links.iter().find(|l| l.from == "crlf.md").unwrap();
    assert_eq!(crlf_link.to.as_deref(), Some("note.md"));
    assert_eq!(crlf_link.line, 2);

    assert!(g.documents.iter().any(|d| d.path == "empty.md"));
}

#[test]
fn a_document_that_is_not_utf8_is_reported_and_the_rest_are_read() {
    let store = MemoryStore::new()
        .with_file("note.md", "x\n")
        .with_file("ok.md", "[[note]]\n")
        .with_file("bad.md", [0xFFu8, 0xFE, 0x00, 0x41]);
    let g = build_graph(&store);
    assert_eq!(g.errors.len(), 1);
    assert_eq!(g.errors[0].path, "bad.md");
    assert!(g.errors[0].message.contains("UTF-8"));
    assert_eq!(target_of(&g, "ok.md").as_deref(), Some("note.md"));
}

#[test]
fn broken_links_carry_their_line() {
    let a = "# A\n\nsee [[nope]] here\n\nand [x](missing.md) and [ok](b.md)\n\n[[Sub/Missing]]\n";
    let g = graph(&[("a.md", a), ("b.md", "# B\n")]);
    let lines: Vec<(usize, &str)> = g.broken.iter().map(|b| (b.line, b.link.as_str())).collect();
    assert_eq!(
        lines,
        vec![
            (3, "[[nope]]"),
            (5, "[x](missing.md)"),
            (7, "[[Sub/Missing]]")
        ]
    );
}
