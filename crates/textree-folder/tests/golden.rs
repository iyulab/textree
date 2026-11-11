//! The relation graph of the corpus tree must match the golden file.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;
use textree::build_graph;
use textree_folder::FolderStore;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/corpus")
}

fn link_key(v: &Value) -> String {
    let get = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    format!(
        "{}|{}|{}|{}|{}",
        get("from"),
        get("link"),
        get("to"),
        get("kind"),
        get("heading")
    )
}

fn as_set(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn corpus_graph_matches_the_golden() {
    let store = FolderStore::open(corpus().join("tree")).unwrap();
    let actual: Value = serde_json::to_value(build_graph(&store)).unwrap();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(corpus().join("graph.json")).unwrap())
            .unwrap();

    let actual_links: BTreeSet<String> = actual["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(link_key)
        .collect();
    let golden_links: BTreeSet<String> = golden["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(link_key)
        .collect();
    assert_eq!(actual_links, golden_links);

    // tags and backlinks: object of sets
    for key in ["tags", "backlinks"] {
        let a = actual[key].as_object().unwrap();
        let g = golden[key].as_object().unwrap();
        assert_eq!(
            a.keys().collect::<Vec<_>>(),
            g.keys().collect::<Vec<_>>(),
            "{key} keys"
        );
        for (name, docs) in g {
            assert_eq!(as_set(&a[name]), as_set(docs), "{key}[{name}]");
        }
    }
    assert_eq!(as_set(&actual["orphans"]), as_set(&golden["orphans"]));
    assert_eq!(
        actual["broken"].as_array().unwrap().len(),
        golden["broken"].as_array().unwrap().len()
    );

    // Text the golden says is not a link or a tag appears nowhere in the output.
    let text = actual.to_string();
    for item in golden["not_links"]
        .as_array()
        .unwrap()
        .iter()
        .chain(golden["not_tags"].as_array().unwrap())
    {
        let shown = item["text"].as_str().unwrap();
        assert!(!text.contains(shown), "{shown} must not be read");
    }
}

#[test]
fn the_two_notes_named_soil_resolve_by_distance() {
    let store = FolderStore::open(corpus().join("tree")).unwrap();
    let graph = build_graph(&store);
    let soil = graph.links.iter().find(|l| l.link == "[[soil]]").unwrap();
    assert_eq!(soil.to.as_deref(), Some("projects/garden/soil.md"));
    assert_eq!(soil.alternatives, vec!["archive/2025/soil.md".to_string()]);
}
