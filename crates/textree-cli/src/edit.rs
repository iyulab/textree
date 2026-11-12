//! `move`, `delete` and `apply`: plan a change, show it, and write it when asked.

use std::collections::BTreeSet;
use std::fs;
use std::process::ExitCode;

use clap::Args;
use textree::{
    build_graph, plan, recheck, validate_paths, ChangeSet, CommitError, Graph, Op, Plan, PlanError,
    Policy, RecheckError, Snapshot,
};
use textree_folder::FolderStore;

#[derive(Args, Debug)]
pub struct Options {
    /// Write the change. Without it only the plan is shown
    #[arg(long)]
    apply: bool,
    /// Save the plan to a file (apply it later with `textree apply`)
    #[arg(long, value_name = "FILE")]
    plan: Option<String>,
    /// Refuse a change that would touch more than this many nodes
    #[arg(long, value_name = "N")]
    max_touched: Option<usize>,
    /// Refuse a change that would rewrite more than this many bytes of links
    #[arg(long, value_name = "N")]
    max_bytes: Option<usize>,
}

/// The text of a file, or of standard input for `-`.
pub fn read_text(file: &str) -> Result<String, ExitCode> {
    let bytes = if file == "-" {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf).map_err(usage)?;
        buf
    } else {
        fs::read(file).map_err(|e| usage(format!("{file}: {e}")))?
    };
    String::from_utf8(bytes).map_err(|_| usage(format!("{file}: not valid UTF-8 text")))
}

fn usage(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {message}");
    ExitCode::from(2)
}

pub fn run(folder: &str, op: Op, break_links: bool, opts: &Options) -> ExitCode {
    let store = match FolderStore::open(folder) {
        Ok(store) => store,
        Err(message) => return usage(message),
    };
    let policy = Policy {
        max_touched: opts.max_touched,
        max_changed_bytes: opts.max_bytes,
        break_links,
    };
    let snapshot = Snapshot::read(&store);
    let planned = match plan(&snapshot, &op, &policy) {
        Ok(planned) => planned,
        Err(PlanError::Usage(message)) => return usage(message),
        Err(PlanError::Refused(violations)) => {
            for v in &violations {
                eprintln!("{v}");
            }
            return ExitCode::from(1);
        }
    };
    let changes = &planned.changes;
    if changes.is_empty() {
        println!("nothing to do");
        return ExitCode::SUCCESS;
    }
    if let Some(file) = &opts.plan {
        let text = match serde_json::to_string_pretty(changes) {
            Ok(text) => text,
            Err(e) => return usage(e),
        };
        if let Err(e) = fs::write(file, text + "\n") {
            return usage(format!("{file}: {e}"));
        }
    }
    // The disk can say no where the plan cannot: a destination behind a symbolic link.
    if let Err(CommitError::Invalid(why)) = store.preflight(changes) {
        eprintln!("refused [I5] {why}");
        return ExitCode::from(1);
    }
    print_plan(changes, &op);
    if !opts.apply {
        println!("Dry run: nothing was written. Use --apply to write.");
        return ExitCode::SUCCESS;
    }
    if let Err(e) = commit_verified(&store, &planned) {
        return report_commit_error(e);
    }
    println!("{}", applied_line(changes));
    ExitCode::SUCCESS
}

pub fn apply_saved(folder: &str, plan_file: &str) -> ExitCode {
    let store = match FolderStore::open(folder) {
        Ok(store) => store,
        Err(message) => return usage(message),
    };
    let text = match fs::read_to_string(plan_file) {
        Ok(text) => text,
        Err(e) => return usage(format!("{plan_file}: {e}")),
    };
    let changes: ChangeSet = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => return usage(format!("{plan_file}: not a plan: {e}")),
    };
    if let Err(why) = validate_paths(&changes) {
        return usage(format!("{plan_file}: {why}"));
    }
    if let Err(e) = store.preflight(&changes) {
        return report_commit_error(e);
    }
    // Only a plan that the engine would make now, from its operations, is written.
    let planned = match recheck(&Snapshot::read(&store), &changes) {
        Ok(planned) => planned,
        Err(RecheckError::Stale(paths)) => return report_commit_error(CommitError::Stale(paths)),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    match commit_verified(&store, &planned) {
        Ok(()) => {
            println!("{}", applied_line(&changes));
            ExitCode::SUCCESS
        }
        Err(e) => report_commit_error(e),
    }
}

/// Commits a plan; after writing, and before it is final, the folder has to be what the plan
/// said it would be — if it is not, everything is put back.
fn commit_verified(store: &FolderStore, planned: &Plan) -> Result<(), CommitError> {
    let verify = |s: &FolderStore| -> Result<(), String> {
        if same_links(&planned.expected, &build_graph(s)) {
            Ok(())
        } else {
            Err("the folder after writing does not match the plan".to_string())
        }
    };
    store.commit_checked(&planned.changes, &verify).map(|_| ())
}

fn report_commit_error(e: CommitError) -> ExitCode {
    match e {
        CommitError::Invalid(why) => usage(why),
        CommitError::Stale(paths) => {
            for p in paths {
                eprintln!("stale: {p}");
            }
            eprintln!("the folder changed since the plan was made; nothing was written");
            ExitCode::from(1)
        }
        CommitError::Conflict(paths) => {
            for p in paths {
                eprintln!("refused [I5] {p}: already exists");
            }
            ExitCode::from(1)
        }
        CommitError::Failed { path, message } => {
            if path.is_empty() {
                eprintln!("error: {message}");
            } else {
                eprintln!("failed at {path}: {message}");
            }
            ExitCode::from(1)
        }
    }
}

fn same_links(a: &Graph, b: &Graph) -> bool {
    let project = |g: &Graph| -> Vec<(String, String, Option<String>)> {
        let mut v: Vec<_> = g
            .links
            .iter()
            .map(|l| (l.from.clone(), l.link.clone(), l.to.clone()))
            .collect();
        v.sort();
        v
    };
    project(a) == project(b) && a.errors.is_empty() == b.errors.is_empty()
}

fn applied_line(changes: &ChangeSet) -> String {
    let mut parts = Vec::new();
    let n = changes.moves.len();
    if n > 0 {
        parts.push(format!("{n} {}", if n == 1 { "move" } else { "moves" }));
    }
    let n = changes.creates.len();
    if n > 0 {
        parts.push(format!("{n} created"));
    }
    let n = changes.rewrites.len();
    if n > 0 {
        parts.push(format!(
            "{n} {} rewritten",
            if n == 1 { "link" } else { "links" }
        ));
    }
    let n = changes.deletes.len();
    if n > 0 {
        parts.push(format!("{n} deleted"));
    }
    if parts.is_empty() && !changes.writes.is_empty() {
        let n = changes.writes.len();
        parts.push(format!(
            "{n} {} written",
            if n == 1 { "document" } else { "documents" }
        ));
    }
    format!("Applied: {}.", parts.join(", "))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plan, as it is shown: what moves and goes, then which links are written differently.
fn print_plan(changes: &ChangeSet, op: &Op) {
    for m in &changes.moves {
        println!("move     {} -> {}", m.from, m.to);
    }
    // A write that only carries rewritten links is told by its rewrites; the others say what
    // they are made of.
    for w in changes.writes.iter().filter(|w| w.note.is_some()) {
        println!(
            "write    {} ({})",
            w.path,
            w.note.as_deref().unwrap_or_default()
        );
    }
    for c in &changes.creates {
        match &c.note {
            Some(note) => println!("create   {} ({note})", c.path),
            None => println!("create   {}", c.path),
        }
    }
    for d in &changes.deletes {
        match d.lines {
            Some(n) => println!("delete   {} ({})", d.path, plural(n, "line", "lines")),
            None => println!("delete   {}", d.path),
        }
    }
    for f in &changes.remove_folders {
        println!("remove   {f} (empty folder)");
    }
    println!();
    if !changes.rewrites.is_empty() {
        let docs: BTreeSet<&str> = changes.rewrites.iter().map(|r| r.path.as_str()).collect();
        println!(
            "rewrite  {} in {}",
            plural(changes.rewrites.len(), "link", "links"),
            plural(docs.len(), "document", "documents")
        );
        let mut last: Option<(&str, usize)> = None;
        for r in &changes.rewrites {
            // Several rewrites on one line share one heading.
            if last != Some((r.path.as_str(), r.line)) {
                println!("  {}:{}", r.path, r.line);
                last = Some((r.path.as_str(), r.line));
            }
            println!("    - {}", one_line(&r.before));
            println!("    + {}", one_line(&r.after));
            if let Some(reason) = &r.reason {
                println!("    ({reason})");
            }
        }
        println!();
    }
    for b in &changes.new_broken {
        println!(
            "broken   {}:{}: {} (in the new text; allowed)",
            b.path,
            b.line,
            one_line(&b.link)
        );
    }
    for o in &changes.orphaned {
        println!("orphan   {o} (nothing links to it any more; allowed)");
    }
    if !changes.new_broken.is_empty() || !changes.orphaned.is_empty() {
        println!();
    }
    if matches!(op, Op::Split { .. } | Op::Merge { .. }) {
        println!("checks   nothing disappears · links intact · no name clashes");
    } else if matches!(op, Op::Add { .. } | Op::Replace { .. }) {
        println!("checks   links intact · no name clashes");
    } else {
        println!("checks   content kept · links intact · no empty folders · no name clashes");
    }
}

fn one_line(s: &str) -> String {
    s.replace("\r\n", "\\n").replace('\n', "\\n")
}
