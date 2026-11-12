//! `textree apply` writes a saved plan only when its operations, planned again on the folder as
//! it is now, give exactly that plan.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("textree-recheck-test-{}-{n}", std::process::id()));
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

    fn state(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().to_string();
                if p.is_dir() {
                    out.insert(format!("{rel}/"), Vec::new());
                    walk(root, &p, out);
                } else {
                    out.insert(rel, fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.0, &self.0, &mut out);
        out
    }

    fn plan_file(&self, name: &str) -> PathBuf {
        self.0.with_extension(format!("{name}.json"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn textree(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_textree"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn folder() -> Scratch {
    let s = Scratch::new();
    s.file(
        "index.md",
        "# Index\n\nSee [trip](notes/trip.md) and [[plan]].\n",
    )
    .file("notes/trip.md", "# Trip\n\nBack to [[index]].\n")
    .file("plan.md", "# Plan\n\nThe [[trip]].\n")
    .file("other.md", "# Other\n\nUntouched.\n");
    s
}

/// Saves the plan of `textree move` and returns it.
fn saved_move(s: &Scratch, extra: &[&str]) -> (PathBuf, Value) {
    let file = s.plan_file("move");
    let root = s.0.to_str().unwrap();
    let mut args = vec![
        "move",
        root,
        "notes/trip.md",
        "archive/trip.md",
        "--plan",
        file.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    let (code, _, err) = textree(&args);
    assert_eq!(code, 0, "{err}");
    let plan: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    (file, plan)
}

fn apply(s: &Scratch, plan: &Value) -> (i32, String, String) {
    let file = s.plan_file("tampered");
    fs::write(&file, serde_json::to_string_pretty(plan).unwrap()).unwrap();
    let out = textree(&["apply", s.0.to_str().unwrap(), file.to_str().unwrap()]);
    let _ = fs::remove_file(&file);
    out
}

type Tamper = Box<dyn Fn(&mut Value)>;

const NOT_MATCHING: &str =
    "refused [plan]: the plan does not match what its operations produce on this folder\n";

#[test]
fn a_saved_plan_carries_its_operation_and_policy() {
    let s = folder();
    let (file, plan) = saved_move(&s, &["--max-touched", "9"]);
    let _ = fs::remove_file(file);
    assert_eq!(plan["version"], 2);
    assert_eq!(
        plan["ops"],
        json!([{"op": "move", "from": "notes/trip.md", "to": "archive/trip.md"}])
    );
    // Limits decide whether a plan is made; the plan records only the permission it needs.
    assert_eq!(plan["policy"], json!({"break_links": false}));
}

#[test]
fn an_untouched_plan_applies_once_and_is_then_stale() {
    let s = folder();
    let (file, _) = saved_move(&s, &[]);
    let root = s.0.to_str().unwrap();
    let (code, out, err) = textree(&["apply", root, file.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("Applied"), "{out}");
    let after = s.state();
    let (code, _, err) = textree(&["apply", root, file.to_str().unwrap()]);
    let _ = fs::remove_file(file);
    assert_eq!(code, 1);
    assert!(err.starts_with("stale: "), "{err}");
    assert_eq!(s.state(), after);
}

#[test]
fn a_tampered_plan_is_refused_and_nothing_is_written() {
    let s = folder();
    let (file, plan) = saved_move(&s, &[]);
    let _ = fs::remove_file(file);
    let before = s.state();
    let tampered: Vec<(&str, Tamper)> = vec![
        (
            "a: a write's content",
            Box::new(|p| {
                let w = &mut p["writes"][0]["content"];
                *w = json!(format!("{}PWNED\n", w.as_str().unwrap()));
            }),
        ),
        (
            "b: an unrelated document overwritten",
            Box::new(|p| {
                p["writes"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"path": "other.md", "content": "PWNED\n"}));
            }),
        ),
        (
            "c: the operation's target",
            Box::new(|p| p["ops"][0]["to"] = json!("elsewhere/trip.md")),
        ),
        (
            "d: the rewrites removed",
            Box::new(|p| p["rewrites"] = json!([])),
        ),
        (
            "e: a created path",
            Box::new(|p| {
                p["creates"] = json!([{"path": "notes/extra.md", "content": "x\n"}]);
            }),
        ),
        (
            "f: the policy",
            Box::new(|p| p["policy"]["break_links"] = json!(true)),
        ),
        (
            "an unknown field at the top",
            Box::new(|p| p["extra"] = json!(1)),
        ),
        (
            "an unknown field in an operation",
            Box::new(|p| p["ops"][0]["extra"] = json!("x")),
        ),
        (
            "an unknown field in a read version",
            Box::new(|p| p["reads"][0]["extra"] = json!(true)),
        ),
        (
            "an unknown field in the policy",
            Box::new(|p| p["policy"]["extra"] = json!(0)),
        ),
        (
            "a limit added to the policy",
            Box::new(|p| p["policy"]["max_touched"] = json!(50)),
        ),
        (
            "an empty list left out",
            Box::new(|p| {
                p.as_object_mut().unwrap().remove("creates");
            }),
        ),
        (
            "the policy left out",
            Box::new(|p| {
                p.as_object_mut().unwrap().remove("policy");
            }),
        ),
    ];
    for (label, change) in tampered {
        let mut p = plan.clone();
        change(&mut p);
        let (code, _, err) = apply(&s, &p);
        assert_eq!(code, 1, "{label}: {err}");
        assert_eq!(err, NOT_MATCHING, "{label}");
        assert_eq!(s.state(), before, "{label}: nothing may be written");
    }
    // g: a plan in the old format, without operations.
    let mut p = plan.clone();
    p["version"] = json!(1);
    p.as_object_mut().unwrap().remove("ops");
    p.as_object_mut().unwrap().remove("policy");
    let (code, _, err) = apply(&s, &p);
    assert_eq!(code, 1, "{err}");
    assert_eq!(
        err,
        "refused [plan]: this plan was saved by an older version and has no operations; make it again\n"
    );
    assert_eq!(s.state(), before);
}

#[test]
fn staleness_is_reported_before_tampering() {
    let s = folder();
    let (file, plan) = saved_move(&s, &[]);
    let _ = fs::remove_file(file);
    s.file("plan.md", "# Plan\n\nChanged since.\n");
    let mut p = plan.clone();
    p["rewrites"] = json!([]);
    let (code, _, err) = apply(&s, &p);
    assert_eq!(code, 1);
    assert!(err.starts_with("stale: plan.md\n"), "{err}");
}

#[test]
fn a_plan_records_break_links_only_when_it_breaks_a_link() {
    let s = folder();
    let root = s.0.to_str().unwrap();
    let file = s.plan_file("delete");
    let f = file.to_str().unwrap();
    // `other.md` has no links to it: the switch is not needed and not recorded.
    let (code, _, err) = textree(&["delete", root, "other.md", "--break-links", "--plan", f]);
    assert_eq!(code, 0, "{err}");
    let plan: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(plan["policy"]["break_links"], false);
    // `plan.md` is linked from the index: the plan needs it, and does not apply without it.
    let (code, _, err) = textree(&["delete", root, "plan.md", "--break-links", "--plan", f]);
    assert_eq!(code, 0, "{err}");
    let mut plan: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let _ = fs::remove_file(&file);
    assert_eq!(plan["policy"]["break_links"], true);
    let before = s.state();
    plan["policy"]["break_links"] = json!(false);
    let (code, _, err) = apply(&s, &plan);
    assert_eq!(code, 1);
    assert_eq!(err, NOT_MATCHING);
    assert_eq!(s.state(), before);
}
