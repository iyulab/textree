//! `textree`: look at what Textree reads from a folder of Markdown documents.
//!
//! * `textree graph <folder> [--json]` — documents, links, tags, broken links and orphans.
//! * `textree check <folder>` — list broken links; the exit code is 1 when there are any.
//!
//! This is an observation and test tool, not a published product.

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
