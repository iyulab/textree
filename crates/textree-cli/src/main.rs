//! `textree`: look at what Textree reads from a folder of Markdown documents.
//!
//! * `textree graph <folder> [--json]` — documents, links, tags, broken links and orphans.
//! * `textree check <folder>` — list broken links; the exit code is 1 when there are any.
//! * `textree move <folder> <from> <to>` and `textree delete <folder> <path>` — plan a change
//!   (and with `--apply` make it); links are rewritten so that they keep meaning what they meant.
//! * `textree split <folder> <doc>` and `textree merge <folder> <into> <from>...` — turn one
//!   document into a folder of documents, one per section, or the other way round.
//! * `textree add <folder> <path> <file>` and `textree replace <folder> <path> <file>` — add a
//!   document, or replace the text of one, with the text of a file (`-` reads standard input).
//! * `textree apply <folder> <plan>` — apply a plan that was saved with `--plan`.
//!
//! This is an observation and test tool, not a published product.

mod edit;

use std::collections::BTreeMap;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use textree::path::dir_of;
use textree::{build_graph, Graph};
use textree_folder::FolderStore;

#[derive(Parser)]
#[command(
    name = "textree",
    version,
    about = "Look at what Textree reads from a folder of Markdown documents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the documents, links, tags, broken links and orphans of a folder
    Graph {
        /// The folder to read
        folder: String,
        /// Print the full relation graph, with positions, as JSON
        #[arg(long)]
        json: bool,
    },
    /// List broken links; exits with 1 when there are any
    Check {
        /// The folder to read
        folder: String,
    },
    /// Move or rename a document, an asset or a folder; shows the plan, writes with --apply
    Move {
        /// The folder to work in
        folder: String,
        /// What to move, relative to the folder
        from: String,
        /// Its new full path, relative to the folder
        to: String,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Delete a document, an asset or a folder; shows the plan, writes with --apply
    Delete {
        /// The folder to work in
        folder: String,
        /// What to delete, relative to the folder
        path: String,
        /// Allow links to the deleted things to be left pointing at nothing
        #[arg(long)]
        break_links: bool,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Split a document into a folder of documents, one per section; shows the plan, writes with --apply
    Split {
        /// The folder to work in
        folder: String,
        /// The document to split, relative to the folder
        path: String,
        /// The heading level to split at
        #[arg(long, default_value_t = 2, value_name = "N")]
        level: u8,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Merge documents into another one as sections; shows the plan, writes with --apply
    Merge {
        /// The folder to work in
        folder: String,
        /// The document that receives the others
        into: String,
        /// The documents to merge in, in the order of their sections
        #[arg(required = true)]
        from: Vec<String>,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Add a document with the text of a file; shows the plan, writes with --apply
    Add {
        /// The folder to work in
        folder: String,
        /// The new document, relative to the folder
        path: String,
        /// A file with its text (`-` reads standard input)
        #[arg(value_name = "FILE")]
        content: String,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Replace the text of a document with the text of a file; shows the plan, writes with --apply
    Replace {
        /// The folder to work in
        folder: String,
        /// The document, relative to the folder
        path: String,
        /// A file with its new text (`-` reads standard input)
        #[arg(value_name = "FILE")]
        content: String,
        #[command(flatten)]
        opts: edit::Options,
    },
    /// Apply a plan saved with --plan; refuses when the folder changed since
    Apply {
        /// The folder the plan was made for
        folder: String,
        /// The saved plan
        plan: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Graph { folder, json } => {
            let graph = match load(&folder) {
                Ok(graph) => graph,
                Err(code) => return code,
            };
            warn_errors(&graph);
            if json {
                match serde_json::to_string_pretty(&graph) {
                    Ok(text) => println!("{text}"),
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::from(2);
                    }
                }
            } else {
                print_summary(&folder, &graph);
            }
            ExitCode::SUCCESS
        }
        Command::Check { folder } => {
            let graph = match load(&folder) {
                Ok(graph) => graph,
                Err(code) => return code,
            };
            warn_errors(&graph);
            for b in &graph.broken {
                println!("{}:{}: broken link {}", b.from, b.line, b.link);
            }
            if graph.broken.is_empty() {
                println!("no broken links");
            }
            if graph.broken.is_empty() && graph.errors.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Command::Move {
            folder,
            from,
            to,
            opts,
        } => edit::run(&folder, textree::Op::Move { from, to }, false, &opts),
        Command::Delete {
            folder,
            path,
            break_links,
            opts,
        } => edit::run(&folder, textree::Op::Delete { path }, break_links, &opts),
        Command::Split {
            folder,
            path,
            level,
            opts,
        } => edit::run(&folder, textree::Op::Split { path, level }, false, &opts),
        Command::Merge {
            folder,
            into,
            from,
            opts,
        } => edit::run(&folder, textree::Op::Merge { into, from }, false, &opts),
        Command::Add {
            folder,
            path,
            content,
            opts,
        } => match edit::read_text(&content) {
            Ok(content) => edit::run(&folder, textree::Op::Add { path, content }, false, &opts),
            Err(code) => code,
        },
        Command::Replace {
            folder,
            path,
            content,
            opts,
        } => match edit::read_text(&content) {
            Ok(content) => edit::run(
                &folder,
                textree::Op::Replace { path, content },
                false,
                &opts,
            ),
            Err(code) => code,
        },
        Command::Apply { folder, plan } => edit::apply_saved(&folder, &plan),
    }
}

fn load(folder: &str) -> Result<Graph, ExitCode> {
    match FolderStore::open(folder) {
        Ok(store) => Ok(build_graph(&store)),
        Err(message) => {
            eprintln!("error: {message}");
            Err(ExitCode::from(2))
        }
    }
}

fn warn_errors(graph: &Graph) {
    for e in &graph.errors {
        eprintln!("warning: {}: {}", e.path, e.message);
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn print_summary(folder: &str, graph: &Graph) {
    let shown = if folder.ends_with('/') {
        folder.to_string()
    } else {
        format!("{folder}/")
    };
    let mut head = format!(
        "{shown}  {}",
        plural(graph.documents.len(), "document", "documents")
    );
    if !graph.assets.is_empty() {
        head.push_str(&format!(
            " ({})",
            plural(graph.assets.len(), "asset", "assets")
        ));
    }
    println!("{head}\n");

    let resolved = graph.links.iter().filter(|l| l.to.is_some()).count();
    println!(
        "links    {resolved} resolved · {} broken",
        graph.broken.len()
    );

    if graph.tags.is_empty() {
        println!("tags     none");
    } else {
        let mut tags: Vec<(&String, usize)> =
            graph.tags.iter().map(|(t, d)| (t, d.len())).collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let mut parts: Vec<String> = tags
            .iter()
            .take(8)
            .map(|(t, n)| format!("{t} ({n})"))
            .collect();
        if tags.len() > 8 {
            parts.push(format!("… {} more", tags.len() - 8));
        }
        println!("tags     {}", parts.join(" · "));
    }

    if !graph.broken.is_empty() {
        print_list(
            "broken   ",
            graph
                .broken
                .iter()
                .map(|b| format!("{}:{}  {}", b.from, b.line, b.link))
                .collect(),
            10,
        );
    }
    print_list("orphans  ", graph.orphans.clone(), 10);

    // Names that more than one document carry: say which one each link was given.
    let mut notes: BTreeMap<(String, usize), (String, String, String)> = BTreeMap::new();
    for l in graph.links.iter().filter(|l| !l.alternatives.is_empty()) {
        if let Some(to) = &l.to {
            let reason = if dir_of(&l.from) == dir_of(to) {
                "same folder"
            } else {
                "nearest"
            };
            notes.insert(
                (l.from.clone(), l.range.start),
                (l.link.clone(), to.clone(), reason.to_string()),
            );
        }
    }
    let total = notes.len();
    for ((from, _), (link, to, reason)) in notes.into_iter().take(5) {
        let name = name_of_link(&link);
        let count = graph
            .links
            .iter()
            .find(|l| l.from == from && l.link == link)
            .map_or(2, |l| l.alternatives.len() + 1);
        println!("\nnote: {count} documents are named \"{name}\".\n  {link} in {from}\n  -> {to} ({reason})");
    }
    if total > 5 {
        println!("\n… and {} more notes", total - 5);
    }
}

fn print_list(label: &str, items: Vec<String>, limit: usize) {
    if items.is_empty() {
        println!("{label}none");
        return;
    }
    let pad = " ".repeat(label.len());
    for (i, item) in items.iter().take(limit).enumerate() {
        println!("{}{item}", if i == 0 { label } else { pad.as_str() });
    }
    if items.len() > limit {
        println!("{pad}… and {} more", items.len() - limit);
    }
}

/// The name a wiki link asks for, from the link as written.
fn name_of_link(link: &str) -> String {
    let inner = link
        .trim_start_matches('!')
        .trim_start_matches("[[")
        .trim_end_matches("]]");
    let end = inner.find(['|', '#']).unwrap_or(inner.len());
    let name = inner[..end].trim();
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .trim_end_matches(".md")
        .to_string()
}
