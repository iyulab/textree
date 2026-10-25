//! Guards the one rule about environment overrides: a shipped build reads none of them.
//!
//! The `TEXTREE_*` knobs (the AI helper's executable, the canopy command, the sign-in and publish
//! servers, the default-folder and settings bases, the watch interval) let an end-to-end run swap
//! what the app launches or talks to. They are read through `dev_env`, which answers `None` in a
//! shipped build. This test fails if a new read bypasses it.

use std::fs;
use std::path::{Path, PathBuf};

/// Reads that are held back by a `cfg` of their own (development builds only) rather than by
/// `dev_env`: the end-to-end stall flags.
const READ_UNDER_OWN_CFG: [&str; 2] = ["TEXTREE_E2E_STALL_FLAG", "TEXTREE_E2E_OPEN_STALL_FLAG"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The part of a source file a shipped build compiles: everything before its test module.
fn shipped_part(source: &str) -> &str {
    match source.find("#[cfg(test)]\nmod ").or_else(|| source.find("#[cfg(test)]\r\nmod ")) {
        Some(at) => &source[..at],
        None => source,
    }
}

/// Names of `TEXTREE_*` variables read straight from the process environment.
fn direct_reads(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in source.lines() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        for read in ["env::var(\"TEXTREE_", "env::var_os(\"TEXTREE_"] {
            if let Some(at) = code.find(read) {
                if code[..at].ends_with("dev_") {
                    continue; // `dev_env::var(…)` — the sanctioned way
                }
                let rest = &code[at + read.len() - "TEXTREE_".len()..];
                let name = rest.split('"').next().unwrap_or_default();
                found.push(name.to_string());
            }
        }
    }
    found
}

#[test]
fn detects_a_direct_read_and_skips_a_test_module_and_comments() {
    let source = "\
fn a() { let _ = std::env::var(\"TEXTREE_HOST_EXE\"); }
// std::env::var(\"TEXTREE_APP_BASE\")
#[cfg(test)]
mod tests { fn b() { let _ = std::env::var(\"TEXTREE_API_BASE\"); } }
";
    assert_eq!(direct_reads(shipped_part(source)), vec!["TEXTREE_HOST_EXE"]);
}

#[test]
fn no_shipped_code_reads_a_textree_variable_without_dev_env() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no sources found under {}", src.display());

    let mut offenders = Vec::new();
    for file in files {
        if file.file_name().is_some_and(|n| n == "dev_env.rs") {
            continue;
        }
        let source = fs::read_to_string(&file).unwrap();
        for name in direct_reads(shipped_part(&source)) {
            if !READ_UNDER_OWN_CFG.contains(&name.as_str()) {
                offenders.push(format!("{}: {name}", file.strip_prefix(&src).unwrap().display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "{offenders:?} read the environment directly. Go through `dev_env::var`/`var_os` so a \
         shipped build ignores the override."
    );
}
