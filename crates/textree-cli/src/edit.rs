//! `move`, `delete` and `apply`: plan a change, show it, and write it when asked.

use std::collections::BTreeSet;
use std::fs;
use std::process::ExitCode;

use clap::Args;
use textree::{ChangeSet, Op, Policy};
use textree_folder::edit::{applied_line, apply_outside, commit_planned, plan_change, Failure};
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

fn fail(f: Failure) -> ExitCode {
    eprint!("{}", f.text());
    ExitCode::from(f.exit_code())
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
    let planned = match plan_change(&store, &op, &policy) {
        Ok(planned) => planned,
        Err(f) => return fail(f),
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
    print_plan(changes, &op);
    if !opts.apply {
        println!("Dry run: nothing was written. Use --apply to write.");
        return ExitCode::SUCCESS;
    }
    if let Err(f) = commit_planned(&store, &planned) {
        return fail(f);
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
    match apply_outside(&store, &changes) {
        Ok(_) => {
            println!("{}", applied_line(&changes));
            ExitCode::SUCCESS
        }
        Err(f) => fail(f),
    }
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
