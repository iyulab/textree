//! In-app publishing: spawn the canopy renderer (a separate executable) to turn the open vault
//! into a static site. Read-only over the source (constitution D13 — publish is one-directional):
//! canopy only *reads* the vault; the source `.md` is never mutated. User-triggered only — there is
//! no autonomous publish (Filer boundary).
//!
//! Layering: this module owns the validation + spawn orchestration; `commands::publish_site` is the
//! thin IPC wrapper that resolves how to invoke canopy and forwards here.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishOptions {
    /// Overrides the site title (defaults to the vault folder name inside canopy).
    pub site_title: Option<String>,
    /// The host's design-token CSS **content** (not a path). Written to a temp file and passed to
    /// canopy via `--tokens-css` so the published site matches the app. None = canopy built-in tokens.
    pub tokens_css: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub page_count: usize,
    pub out_dir: String,
}

/// How to invoke canopy: a program plus any fixed leading args. In production the program is the
/// bundled single-exe sidecar (empty prefix); in dev/E2E it is `node` with the CLI script path.
pub struct CanopyInvocation {
    pub program: OsString,
    pub prefix_args: Vec<OsString>,
}

/// Resolves the bundled canopy sidecar (mechanism B) from a Tauri resource directory. The payload
/// lives under `<resource>/canopy/`: a pinned `node` runtime plus canopy's `cli.js`. Returns the
/// invocation `node cli.js`, or `None` if either piece is missing (e.g. an unbundled dev build).
pub fn canopy_from_resource_dir(resource: &Path) -> Option<CanopyInvocation> {
    let dir = resource.join("canopy");
    let node = dir.join(if cfg!(windows) { "node.exe" } else { "node" });
    let cli = dir.join("cli.js");
    if node.exists() && cli.exists() {
        Some(CanopyInvocation {
            program: node.into_os_string(),
            prefix_args: vec![cli.into_os_string()],
        })
    } else {
        None
    }
}

/// Canonicalizes the longest existing ancestor of `p` and re-appends the non-existing tail, so a
/// not-yet-created output directory can still be compared against the vault for containment.
fn resolve_existing_prefix(p: &Path) -> Result<PathBuf, String> {
    let mut tail: Vec<OsString> = Vec::new();
    let mut current = p.to_path_buf();
    loop {
        if current.exists() {
            let mut base = current.canonicalize().map_err(|e| e.to_string())?;
            for part in tail.iter().rev() {
                base.push(part);
            }
            return Ok(base);
        }
        let name = current.file_name().map(|s| s.to_os_string());
        let parent = current.parent().map(|p| p.to_path_buf());
        match (parent, name) {
            (Some(parent), Some(n)) if parent != current => {
                tail.push(n);
                current = parent;
            }
            _ => return Err("the output path has no existing parent directory".into()),
        }
    }
}

/// Validates the publish source/destination. The vault must be a real directory, and the output
/// must live entirely **outside** the vault — otherwise canopy would re-ingest its own output and
/// the published site could clobber (or be polluted by) the source of truth.
pub fn validate_publish_paths(vault: &Path, out: &Path) -> Result<(), String> {
    if !vault.is_dir() {
        return Err("the vault path is not a directory".into());
    }
    let vault_c = vault.canonicalize().map_err(|e| e.to_string())?;
    let out_c = resolve_existing_prefix(out)?;
    if out_c == vault_c || out_c.starts_with(&vault_c) {
        return Err("the output directory must be outside the vault".into());
    }
    if vault_c.starts_with(&out_c) {
        return Err("the output directory must not contain the vault".into());
    }
    Ok(())
}

/// Counts `.html` files in the output tree — the number of published pages reported back.
pub fn count_html_pages(out: &Path) -> usize {
    fn walk(dir: &Path, acc: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, acc);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("html"))
            {
                *acc += 1;
            }
        }
    }
    let mut count = 0;
    walk(out, &mut count);
    count
}

/// Validates, then spawns canopy to build the site. Returns the page count and output directory.
/// The source vault is read-only throughout (canopy never writes into it).
pub fn run_publish(
    vault: &Path,
    out: &Path,
    options: &PublishOptions,
    canopy: &CanopyInvocation,
) -> Result<PublishResult, String> {
    validate_publish_paths(vault, out)?;

    // Materialize the injected tokens CSS to a temp file (canopy reads it via --tokens-css). Held in
    // scope until canopy finishes so the path stays valid; auto-removed on drop.
    let tokens_file = match &options.tokens_css {
        Some(css) => {
            let mut f = tempfile::Builder::new()
                .suffix(".css")
                .tempfile()
                .map_err(|e| e.to_string())?;
            f.write_all(css.as_bytes()).map_err(|e| e.to_string())?;
            Some(f)
        }
        None => None,
    };

    let mut cmd = Command::new(&canopy.program);
    cmd.args(&canopy.prefix_args);
    cmd.arg("build").arg(vault).arg(out);
    if let Some(title) = &options.site_title {
        cmd.arg("--site-title").arg(title);
    }
    if let Some(f) = &tokens_file {
        cmd.arg("--tokens-css").arg(f.path());
    }

    let output = cmd
        .output()
        .map_err(|e| format!("failed to start canopy: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("canopy failed: {}", stderr.trim()));
    }

    Ok(PublishResult {
        page_count: count_html_pages(out),
        out_dir: out.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_rejects_non_directory_vault() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("not-a-dir.md");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_publish_paths(&file, &tmp.path().join("site")).is_err());
    }

    #[test]
    fn validate_rejects_output_inside_vault() {
        let tmp = TempDir::new().unwrap();
        // out = <vault>/site — canopy would re-read its own output / write into the source.
        assert!(validate_publish_paths(tmp.path(), &tmp.path().join("site")).is_err());
    }

    #[test]
    fn validate_rejects_output_equal_to_vault() {
        let tmp = TempDir::new().unwrap();
        assert!(validate_publish_paths(tmp.path(), tmp.path()).is_err());
    }

    #[test]
    fn validate_rejects_output_containing_vault() {
        let parent = TempDir::new().unwrap();
        let vault = parent.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        // out = the parent that contains the vault → publishing there could clobber the vault.
        assert!(validate_publish_paths(&vault, parent.path()).is_err());
    }

    #[test]
    fn validate_accepts_sibling_output_dir() {
        let parent = TempDir::new().unwrap();
        let vault = parent.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        // out = a sibling that does not exist yet (parent exists) → allowed.
        assert!(validate_publish_paths(&vault, &parent.path().join("site")).is_ok());
    }

    #[test]
    fn count_html_pages_counts_recursively_and_ignores_others() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("index.html"), "").unwrap();
        std::fs::write(tmp.path().join("tokens.css"), "").unwrap();
        let sub = tmp.path().join("notes");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("idea.html"), "").unwrap();
        std::fs::write(sub.join("image.png"), "").unwrap();
        assert_eq!(count_html_pages(tmp.path()), 2);
    }

    #[test]
    fn canopy_from_resource_dir_finds_bundled_node_and_cli() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("canopy");
        std::fs::create_dir(&dir).unwrap();
        let node_name = if cfg!(windows) { "node.exe" } else { "node" };
        std::fs::write(dir.join(node_name), "").unwrap();
        std::fs::write(dir.join("cli.js"), "").unwrap();

        let inv = canopy_from_resource_dir(tmp.path()).expect("should resolve");
        assert_eq!(inv.program, dir.join(node_name).into_os_string());
        assert_eq!(inv.prefix_args, vec![dir.join("cli.js").into_os_string()]);
    }

    #[test]
    fn canopy_from_resource_dir_none_when_incomplete() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("canopy");
        std::fs::create_dir(&dir).unwrap();
        // cli.js present but node missing -> not resolvable.
        std::fs::write(dir.join("cli.js"), "").unwrap();
        assert!(canopy_from_resource_dir(tmp.path()).is_none());
    }

    /// Production-path guard: resolve canopy from the *assembled* sidecar payload (node + cli.js +
    /// node_modules under `src-tauri/resources/canopy/`) and actually publish a vault through it,
    /// proving the bundled payload renders AND leaves the source `.md` byte-unchanged (D13). Ignored
    /// by default because it requires the payload — run `scripts/assemble-canopy-sidecar.ps1` first,
    /// then `cargo test -- --ignored run_publish_via_assembled_sidecar`. CI does both (release.yml).
    #[test]
    #[ignore = "requires assembled canopy sidecar payload (run scripts/assemble-canopy-sidecar.ps1)"]
    fn run_publish_via_assembled_sidecar() {
        // resource dir = src-tauri/resources (the helper appends `canopy/`).
        let resource = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let canopy = canopy_from_resource_dir(&resource)
            .expect("assembled payload missing — run scripts/assemble-canopy-sidecar.ps1");

        let tmp = TempDir::new().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        let note = vault.join("hello.md");
        let source = "# Hello\n\nworld\n";
        std::fs::write(&note, source).unwrap();
        let out = tmp.path().join("site");

        let result = run_publish(&vault, &out, &PublishOptions { site_title: None, tokens_css: None }, &canopy)
            .expect("publish should succeed via the assembled sidecar");

        assert!(result.page_count >= 1, "expected at least one published page");
        assert!(out.join("hello.html").exists(), "expected hello.html in the output");
        // D13: the source vault note is untouched.
        assert_eq!(std::fs::read_to_string(&note).unwrap(), source);
    }

    /// Runtime integration: actually spawn canopy (via node) and prove the Rust wiring end-to-end —
    /// the arg vector, the temp-file -> subprocess handoff (a Windows file-sharing risk), exit
    /// status, AND that the source vault is byte-unchanged (D13 read-only). Ignored by default
    /// because it needs node + a built canopy; run with `cargo test -- --ignored`. The permanent
    /// guard is the C44 E2E; this is the empirical de-risk before building UI on top.
    #[test]
    #[ignore = "requires node + a built canopy CLI"]
    fn run_publish_spawns_canopy_and_keeps_source_unchanged() {
        // Note: do NOT canonicalize — on Windows that yields a `\\?\` verbatim path that node's
        // module resolver chokes on. Production never canonicalizes the paths it hands canopy.
        let cli = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../canopy/dist/cli.js");
        assert!(cli.exists(), "build canopy first (npm run build in canopy/)");

        let parent = TempDir::new().unwrap();
        let vault = parent.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        let note = vault.join("note.md");
        let source = "# Hello\r\n\r\nworld\r\n"; // CRLF too: the source must come back byte-identical
        std::fs::write(&note, source).unwrap();
        let out = parent.path().join("site");

        let options = PublishOptions {
            site_title: Some("Spawn Test".into()),
            tokens_css: Some(":root{--probe:INJECTED}\n".into()),
        };
        let canopy = CanopyInvocation {
            program: "node".into(),
            prefix_args: vec![cli.into_os_string()],
        };

        let result = run_publish(&vault, &out, &options, &canopy).expect("publish should succeed");

        assert!(result.page_count > 0, "at least one page emitted");
        assert!(out.join("note.html").is_file(), "the note rendered to html");
        let tokens = std::fs::read_to_string(out.join("tokens.css")).unwrap();
        assert!(tokens.contains("INJECTED"), "injected tokens reached the site");
        // D13: the source `.md` is read-only — its bytes (CRLF included) are untouched by publish.
        assert_eq!(std::fs::read_to_string(&note).unwrap(), source);
    }
}
