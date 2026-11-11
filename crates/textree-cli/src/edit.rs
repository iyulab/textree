//! `move`, `delete` and `apply`: plan a change, show it, and write it when asked.

use std::collections::BTreeSet;
use std::fs;
use std::process::ExitCode;

use clap::Args;
use textree::{
    build_graph, plan, validate_paths, ChangeSet, CommitError, Graph, Op, PlanError, Policy,
    Snapshot, WritePort,
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
    print_plan(changes);
    if !opts.apply {
        println!("Dry run: nothing was written. Use --apply to write.");
        return ExitCode::SUCCESS;
    }
    match store.commit(changes) {
        Ok(_) => {}
        Err(e) => return report_commit_error(e),
    }
    // Read the folder again: it has to be what the plan said it would be.
    let after = match FolderStore::open(folder) {
        Ok(s) => build_graph(&s),
        Err(message) => return usage(message),
    };
    if !same_links(&planned.expected, &after) {
        eprintln!("error: the folder after writing does not match the plan; check it with git or a backup");
        return ExitCode::from(1);
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
    match store.commit(&changes) {
        Ok(_) => {
            println!("{}", applied_line(&changes));
            ExitCode::SUCCESS
        }
        Err(e) => report_commit_error(e),
    }
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
            eprintln!("failed at {path}: {message}");
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
    format!("Applied: {}.", parts.join(", "))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The plan, as it is shown: what moves and goes, then which links are written differently.
fn print_plan(changes: &ChangeSet) {
    for m in &changes.moves {
        println!("move     {} -> {}", m.from, m.to);
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
        for r in &changes.rewrites {
            println!("  {}:{}", r.path, r.line);
            println!("    - {}", one_line(&r.before));
            println!("    + {}", one_line(&r.after));
            if let Some(reason) = &r.reason {
                println!("    ({reason})");
            }
        }
        println!();
    }
    println!("checks   content kept · links intact · no empty folders · no name clashes");
}

fn one_line(s: &str) -> String {
    s.replace("\r\n", "\\n").replace('\n', "\\n")
}
