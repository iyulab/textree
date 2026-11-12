//! A C interface to Textree over a folder: one call, JSON in and JSON out.
//!
//! ```text
//! char *textree_call(const char *request);   // request and response are UTF-8 JSON
//! void  textree_free(char *response);        // every response is freed with this
//! ```
//!
//! A request is `{"op": <name>, "root": <folder>, "args": {...}}`. A response is
//! `{"ok": true, "result": ...}` or `{"ok": false, "error": {"kind": ..., "message": ...}}`, where
//! `kind` is `usage`, `refused`, `stale` or `failed` and `message` is the text the command line
//! prints for the same input. The binding carries the engine's results as they are; it adds no
//! rules of its own.
//!
//! | `op` | `args` | `result` |
//! |---|---|---|
//! | `graph` | — | the relation graph (as `textree graph --json`) |
//! | `check` | — | `{broken: [{path, line, link}], text}` (`text` as `textree check` prints it) |
//! | `tree` | `path?` | the folder and document hierarchy, from `path` (default: the root) |
//! | `read_document` | `path` | `{path, text, headings, tags, links, backlinks}` |
//! | `plan` | `operation`, `policy?` | the plan (a change set, format 2) |
//! | `apply` | `plan` | `{applied}` — the plan is written only if it is exactly what its operations give now |

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use serde::Deserialize;
use serde_json::{json, Value};
use textree::{build_graph, ChangeSet, Graph, NodeKind, Op, Policy, ReadPort};
use textree_folder::edit::{applied_line, apply_outside, plan_change, Failure, FailureKind};
use textree_folder::FolderStore;

#[derive(Deserialize)]
struct Request {
    op: String,
    root: String,
    #[serde(default)]
    args: Value,
}

/// Handles one request (the same as [`textree_call`], for Rust callers and tests).
pub fn call(request: &str) -> String {
    let response = match catch_unwind(AssertUnwindSafe(|| handle(request))) {
        Ok(Ok(result)) => json!({"ok": true, "result": result}),
        Ok(Err(f)) => json!({"ok": false, "error": failure_json(&f)}),
        Err(_) => {
            json!({"ok": false, "error": {"kind": "failed", "message": "error: internal error\n"}})
        }
    };
    response.to_string()
}

fn failure_json(f: &Failure) -> Value {
    let kind = match f.kind {
        FailureKind::Usage => "usage",
        FailureKind::Refused => "refused",
        FailureKind::Stale => "stale",
        FailureKind::Failed => "failed",
    };
    json!({"kind": kind, "message": f.text()})
}

fn handle(request: &str) -> Result<Value, Failure> {
    let request: Request =
        serde_json::from_str(request).map_err(|e| Failure::usage(format!("not a request: {e}")))?;
    let store = FolderStore::open(&request.root).map_err(Failure::usage)?;
    let args = &request.args;
    match request.op.as_str() {
        "graph" => to_value(&graph_of(&store)),
        "check" => Ok(check(&store)),
        "tree" => tree(&store, optional_str(args, "path")?.unwrap_or("")),
        "read_document" => read_document(&store, required_str(args, "path")?),
        "plan" => {
            let op: Op = field(args, "operation")?;
            let policy: Policy = match args.get("policy") {
                None | Some(Value::Null) => Policy::default(),
                Some(_) => field(args, "policy")?,
            };
            let planned = plan_change(&store, &op, &policy)?;
            to_value(&planned.changes)
        }
        "apply" => {
            let changes: ChangeSet = field(args, "plan")?;
            apply_outside(&store, &changes)?;
            Ok(json!({"applied": applied_line(&changes)}))
        }
        other => Err(Failure::usage(format!("unknown op: {other}"))),
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(Failure::usage)
}

fn field<T: serde::de::DeserializeOwned>(args: &Value, name: &str) -> Result<T, Failure> {
    let value = args
        .get(name)
        .ok_or_else(|| Failure::usage(format!("missing argument: {name}")))?;
    serde_json::from_value(value.clone()).map_err(|e| Failure::usage(format!("{name}: {e}")))
}

fn required_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, Failure> {
    optional_str(args, name)?.ok_or_else(|| Failure::usage(format!("missing argument: {name}")))
}

fn optional_str<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, Failure> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(Failure::usage(format!("{name}: expected a string"))),
    }
}

fn graph_of(store: &FolderStore) -> Graph {
    build_graph(store)
}

fn check(store: &FolderStore) -> Value {
    let graph = graph_of(store);
    let mut text = String::new();
    for b in &graph.broken {
        text.push_str(&format!("{}:{}: broken link {}\n", b.from, b.line, b.link));
    }
    if graph.broken.is_empty() {
        text.push_str("no broken links\n");
    }
    let broken: Vec<Value> = graph
        .broken
        .iter()
        .map(|b| json!({"path": b.from, "line": b.line, "link": b.link}))
        .collect();
    let unreadable: Vec<Value> = graph
        .errors
        .iter()
        .map(|e| json!({"path": e.path, "message": e.message}))
        .collect();
    json!({"broken": broken, "unreadable": unreadable, "text": text})
}

/// A path given to a read tool has to name a node of the folder as the engine lists it (hidden
/// names and what lies behind symbolic links to folders are never listed).
fn known_node(store: &FolderStore, path: &str) -> Result<NodeKind, Failure> {
    textree::path::check_inside_root(path)
        .map_err(|why| Failure::usage(format!("path {path:?} {why}")))?;
    store
        .list()
        .nodes
        .iter()
        .find(|n| n.path == path)
        .map(|n| n.kind)
        .ok_or_else(|| Failure::usage(format!("no such document or folder: {path}")))
}

fn tree(store: &FolderStore, path: &str) -> Result<Value, Failure> {
    let path = path.trim_end_matches('/');
    if !path.is_empty() && known_node(store, path)? != NodeKind::Collection {
        return Err(Failure::usage(format!("not a folder: {path}")));
    }
    let graph = graph_of(store);
    let titles: std::collections::HashMap<&str, &str> = graph
        .documents
        .iter()
        .filter_map(|d| {
            d.headings
                .iter()
                .find(|h| h.level == 1)
                .map(|h| (d.path.as_str(), h.text.as_str()))
        })
        .collect();
    let mut nodes: Vec<(String, NodeKind)> = store
        .list()
        .nodes
        .into_iter()
        .map(|n| (n.path, n.kind))
        .collect();
    nodes.sort_by(|a, b| a.0.cmp(&b.0));
    fn build(
        at: &str,
        nodes: &[(String, NodeKind)],
        titles: &std::collections::HashMap<&str, &str>,
    ) -> Vec<Value> {
        nodes
            .iter()
            .filter(|(p, _)| textree::path::dir_of(p) == at)
            .map(|(p, kind)| match kind {
                NodeKind::Collection => {
                    json!({"path": p, "kind": "collection", "children": build(p, nodes, titles)})
                }
                NodeKind::Document => match titles.get(p.as_str()) {
                    Some(t) => json!({"path": p, "kind": "document", "title": t}),
                    None => json!({"path": p, "kind": "document"}),
                },
                NodeKind::Asset => json!({"path": p, "kind": "asset"}),
            })
            .collect()
    }
    Ok(json!({"path": path, "kind": "collection", "children": build(path, &nodes, &titles)}))
}

fn read_document(store: &FolderStore, path: &str) -> Result<Value, Failure> {
    if known_node(store, path)? != NodeKind::Document {
        return Err(Failure::usage(format!("not a document: {path}")));
    }
    let bytes = store
        .read(path)
        .map_err(|e| Failure::usage(format!("{path}: {e}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| Failure::usage(format!("{path}: not valid UTF-8 text")))?;
    let graph = graph_of(store);
    let doc = graph.documents.iter().find(|d| d.path == path);
    let links: Vec<&textree::Link> = graph.links.iter().filter(|l| l.from == path).collect();
    let backlinks: Vec<&textree::Link> = graph
        .links
        .iter()
        .filter(|l| l.to.as_deref() == Some(path))
        .collect();
    Ok(json!({
        "path": path,
        "text": text,
        "headings": doc.map(|d| &d.headings),
        "tags": doc.map(|d| &d.tags),
        "links": links,
        "backlinks": backlinks,
    }))
}

/// Handles one request. `request` is a NUL-terminated UTF-8 JSON string; the response is one too
/// and has to be freed with [`textree_free`]. Never returns null.
///
/// # Safety
///
/// `request` has to be null or point at a NUL-terminated string that stays valid for the call.
#[no_mangle]
pub unsafe extern "C" fn textree_call(request: *const c_char) -> *mut c_char {
    let response = if request.is_null() {
        json!({"ok": false, "error": {"kind": "usage", "message": "error: no request\n"}})
            .to_string()
    } else {
        match CStr::from_ptr(request).to_str() {
            Ok(text) => call(text),
            Err(_) => json!({"ok": false, "error": {"kind": "usage", "message": "error: the request is not UTF-8\n"}})
                .to_string(),
        }
    };
    // JSON text never contains a NUL byte (it would be escaped).
    CString::new(response).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Frees a response of [`textree_call`].
///
/// # Safety
///
/// `response` has to be null or a pointer returned by [`textree_call`] that was not freed yet.
#[no_mangle]
pub unsafe extern "C" fn textree_free(response: *mut c_char) {
    if !response.is_null() {
        drop(CString::from_raw(response));
    }
}
