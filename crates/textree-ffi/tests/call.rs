//! The JSON interface gives what the command line gives for the same folder.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("textree-ffi-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn file(&self, path: &str, body: &str) -> &Self {
        let full = self.0.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
        self
    }

    fn call(&self, op: &str, args: Value) -> Value {
        let request = json!({"op": op, "root": self.0.to_str().unwrap(), "args": args});
        serde_json::from_str(&textree_ffi::call(&request.to_string())).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn folder() -> Scratch {
    let s = Scratch::new();
    s.file(
        "index.md",
        "# Index\n\nSee [trip](notes/trip.md) and [[nowhere]].\n",
    )
    .file(
        "notes/trip.md",
        "# Trip\n\n## Day 1\n\nBack to [[index]].\n",
    );
    s
}

#[test]
fn check_lists_broken_links_as_the_command_line_does() {
    let s = folder();
    let r = s.call("check", json!({}));
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"]["text"], "index.md:3: broken link [[nowhere]]\n");
    assert_eq!(
        r["result"]["broken"],
        json!([{"path": "index.md", "line": 3, "link": "[[nowhere]]"}])
    );
}

#[test]
fn read_document_gives_text_links_and_backlinks() {
    let s = folder();
    let r = s.call("read_document", json!({"path": "notes/trip.md"}));
    assert_eq!(r["ok"], true, "{r}");
    let d = &r["result"];
    assert_eq!(d["text"], "# Trip\n\n## Day 1\n\nBack to [[index]].\n");
    assert_eq!(d["headings"][1]["text"], "Day 1");
    assert_eq!(d["links"][0]["to"], "index.md");
    assert_eq!(d["backlinks"][0]["from"], "index.md");
}

#[test]
fn tree_is_the_hierarchy() {
    let s = folder();
    let r = s.call("tree", json!({}));
    let root = &r["result"];
    assert_eq!(root["children"][0]["path"], "index.md");
    assert_eq!(root["children"][0]["title"], "Index");
    assert_eq!(root["children"][1]["kind"], "collection");
    assert_eq!(root["children"][1]["children"][0]["path"], "notes/trip.md");
    let sub = s.call("tree", json!({"path": "notes"}));
    assert_eq!(sub["result"]["children"].as_array().unwrap().len(), 1);
}

#[test]
fn a_plan_applies_once_then_is_stale() {
    let s = folder();
    let plan = s.call(
        "plan",
        json!({"operation": {"op": "move", "from": "notes/trip.md", "to": "archive/trip.md"}}),
    );
    assert_eq!(plan["ok"], true, "{plan}");
    assert!(
        s.0.join("notes/trip.md").exists(),
        "planning writes nothing"
    );
    let applied = s.call("apply", json!({"plan": plan["result"]}));
    assert_eq!(
        applied["result"]["applied"],
        "Applied: 1 move, 1 link rewritten."
    );
    assert!(s.0.join("archive/trip.md").exists());
    let again = s.call("apply", json!({"plan": plan["result"]}));
    assert_eq!(again["ok"], false);
    assert_eq!(again["error"]["kind"], "stale");
    assert!(again["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("stale: "));
}

#[test]
fn a_tampered_plan_is_refused() {
    let s = folder();
    let mut plan = s.call(
        "plan",
        json!({"operation": {"op": "move", "from": "notes/trip.md", "to": "archive/trip.md"}}),
    )["result"]
        .clone();
    plan["writes"][0]["content"] = json!("PWNED\n");
    let r = s.call("apply", json!({"plan": plan}));
    assert_eq!(r["error"]["kind"], "refused");
    assert_eq!(
        r["error"]["message"],
        "refused [plan]: the plan does not match what its operations produce on this folder\n"
    );
    assert!(s.0.join("notes/trip.md").exists());
}

#[test]
fn paths_outside_the_folder_are_usage_errors() {
    let s = folder();
    for path in ["../x.md", "/etc/passwd", ".git/x.md", "a\\..\\..\\x.md"] {
        let r = s.call("read_document", json!({"path": path}));
        assert_eq!(r["error"]["kind"], "usage", "{path}: {r}");
        let r = s.call(
            "plan",
            json!({"operation": {"op": "add", "path": path, "content": "x"}}),
        );
        assert_eq!(r["ok"], false, "{path}: {r}");
    }
}

#[test]
fn a_bad_request_is_an_error_not_a_crash() {
    let s = folder();
    for r in [
        textree_ffi::call("not json"),
        s.call("nope", json!({})).to_string(),
        s.call("read_document", json!({})).to_string(),
        s.call("read_document", json!({"path": 3})).to_string(),
        s.call("plan", json!({"operation": {"op": "fly"}}))
            .to_string(),
    ] {
        let r: Value = serde_json::from_str(&r).unwrap();
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(r["error"]["kind"], "usage", "{r}");
        assert!(r["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("error: "));
    }
}

#[test]
fn a_path_in_the_other_normalization_form_reads_the_stored_document() {
    // The same syllable, decomposed (as some file systems store names) and composed.
    let (decomposed, composed) = ("\u{1100}\u{1161}", "\u{AC00}");
    let s = Scratch::new();
    s.file(&format!("{decomposed}/{decomposed}.md"), "# A\n");
    let r = s.call(
        "read_document",
        json!({"path": format!("{composed}/{composed}.md")}),
    );
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["path"], format!("{decomposed}/{decomposed}.md"));
    let r = s.call("tree", json!({"path": composed}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["path"], decomposed);
}
