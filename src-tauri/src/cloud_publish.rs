//! Cloud publishing (slice 2b): render the vault locally (reusing `publish::run_publish`), zip the
//! output, and upload it to api.textree.me/publish (slice 2a contract). User-triggered, read-only
//! over the source. The publish token is read from the OS keychain (publish_secret), never
//! passed from the frontend.

use crate::publish::{run_publish, CanopyInvocation, PublishOptions};
use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishToCloudResult {
    pub url: String,
    pub page_count: usize,
}

const DEFAULT_API_BASE: &str = "https://api.textree.me";

/// Overall deadline for the publish upload (connect + send + response). Bounds a hung/stalled
/// server so a publish can never wedge forever. Mirrors the `ask` chat timeout (host.rs).
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// The publish API base — `TEXTREE_API_BASE` override (dev/E2E/staging) or the production default.
pub fn api_base() -> String {
    std::env::var("TEXTREE_API_BASE").unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
}

/// Builds a zip (Deflate) from (relative forward-slash path, bytes) entries. Pure — no filesystem.
pub fn build_zip(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (path, bytes) in entries {
            zip.start_file(path, options).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(cursor.into_inner())
}

/// Parses the slice-2a success body `{ url, pageCount, publishedUtc }`.
pub fn parse_publish_response(body: &str) -> Result<PublishToCloudResult, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        url: String,
        page_count: usize,
    }
    let raw: Raw = serde_json::from_str(body).map_err(|e| format!("bad server response: {e}"))?;
    Ok(PublishToCloudResult { url: raw.url, page_count: raw.page_count })
}

/// Maps a non-2xx publish response to a user-facing message. The token is never included.
pub fn map_upload_error(status: u16, body: &str) -> String {
    match status {
        401 => "the publish token was rejected — reissue it at app.textree.me and update it in Settings".to_string(),
        400 => format!("the server rejected the bundle: {}", body.trim()),
        _ => format!("publish failed (HTTP {status})"),
    }
}

/// Recursively collects every file under `dir` into (relative forward-slash path, bytes) and zips it.
pub fn zip_vault_output(dir: &Path) -> Result<Vec<u8>, String> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    collect(dir, dir, &mut entries)?;
    build_zip(&entries)
}

fn collect(root: &Path, cur: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    for entry in std::fs::read_dir(cur).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            // Forward-slash relative path (zip / blob key convention), regardless of OS separator.
            let name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            out.push((name, bytes));
        }
    }
    Ok(())
}

/// Uploads the zip to `{base}/publish` with the publish token. The token is sent as a header only,
/// never logged. `timeout` bounds the whole request (connect + send + response) so a stalled server
/// cannot hang the caller forever.
pub fn upload_bundle(
    base: &str,
    token: &str,
    zip: Vec<u8>,
    timeout: Duration,
) -> Result<PublishToCloudResult, String> {
    let url = format!("{}/publish", base.trim_end_matches('/'));
    match ureq::post(&url)
        .set("X-Publish-Token", token)
        .set("Content-Type", "application/zip")
        .timeout(timeout)
        .send_bytes(&zip)
    {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| e.to_string())?;
            parse_publish_response(&body)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Err(map_upload_error(code, &body))
        }
        // Transport failure: a timeout (stalled/hung server) or an unreachable host. Both are
        // retryable — the message covers both honestly rather than matching ureq internals.
        Err(e) => Err(format!(
            "the upload could not complete ({e}) — check your connection and try again"
        )),
    }
}

/// Renders the vault locally (temp output, read-only over the source), zips it, and uploads.
pub fn publish_to_cloud(
    vault: &Path,
    options: &PublishOptions,
    canopy: &CanopyInvocation,
    token: &str,
) -> Result<PublishToCloudResult, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let out = tmp.path().join("site"); // a fresh dir outside the vault (run_publish validates this)
    run_publish(vault, &out, options, canopy, crate::publish::RENDER_TIMEOUT)?;
    let zip = zip_vault_output(&out)?;
    upload_bundle(&api_base(), token, zip, UPLOAD_TIMEOUT)
    // tmp (and the rendered output) is removed when `tmp` drops here.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn build_zip_roundtrips_entries() {
        let entries = vec![
            ("index.html".to_string(), b"<h1>hi</h1>".to_vec()),
            ("assets/style.css".to_string(), b"body{}".to_vec()),
        ];
        let bytes = build_zip(&entries).expect("zip should build");

        // Read it back with the same crate — proves it's a valid, well-formed zip.
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");
        assert_eq!(archive.len(), 2);
        let mut f = archive.by_name("assets/style.css").expect("entry present");
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "body{}");
    }

    #[test]
    fn parse_publish_response_extracts_url_and_count() {
        let body = r#"{"url":"https://pub.textree.me","pageCount":3,"publishedUtc":"2026-07-08T00:00:00Z"}"#;
        let r = parse_publish_response(body).expect("valid response");
        assert_eq!(r.url, "https://pub.textree.me");
        assert_eq!(r.page_count, 3);
    }

    #[test]
    fn parse_publish_response_rejects_garbage() {
        assert!(parse_publish_response("not json").is_err());
    }

    #[test]
    fn map_upload_error_explains_401_and_400() {
        assert!(map_upload_error(401, "").contains("token"));
        assert!(map_upload_error(400, "empty bundle").contains("empty bundle"));
        assert!(map_upload_error(503, "").contains("503"));
    }

    #[test]
    fn api_base_defaults_when_env_absent() {
        // Note: this test reads process env; it asserts the default only when the override is unset.
        if std::env::var("TEXTREE_API_BASE").is_err() {
            assert_eq!(api_base(), "https://api.textree.me");
        }
    }

    #[test]
    fn upload_times_out_when_server_stalls() {
        use std::net::TcpListener;
        use std::time::{Duration, Instant};

        // A server that accepts the connection but never sends a response — this is the
        // mid-transfer stall / hung-ingress failure mode, NOT a connect failure.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().unwrap();
        // Discriminator: the stall (sleep 2s) must outlast the assert bound (1s), which in turn must
        // exceed the client timeout (300ms). If the server closed before the bound, a BROKEN (absent)
        // timeout could also return within the bound and the test would pass for the wrong reason.
        // The thread is intentionally detached (not joined) — it self-terminates after the sleep, and
        // joining would make this fast test wait out that sleep for no added coverage.
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_secs(2));
                drop(stream);
            }
        });

        let base = format!("http://{addr}");
        let start = Instant::now();
        let result = upload_bundle(&base, "tok", vec![1, 2, 3], Duration::from_millis(300));

        assert!(result.is_err(), "a stalled server must not hang forever");
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "must return near the 300ms timeout, not wait for the stalled server: took {:?}",
            start.elapsed()
        );
        let msg = result.unwrap_err();
        assert!(
            msg.to_lowercase().contains("try again") || msg.to_lowercase().contains("connection"),
            "timeout error should guide the user to retry, got: {msg}"
        );
    }

    #[test]
    fn zip_vault_output_walks_dir_into_zip() {
        use std::io::Read;
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("index.html"), "root").unwrap();
        let sub = tmp.path().join("notes");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.html"), "nested").unwrap();

        let bytes = zip_vault_output(tmp.path()).expect("should zip the dir");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        // Entry paths are relative + forward-slash, regardless of OS.
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["index.html".to_string(), "notes/a.html".to_string()]);
        let mut f = archive.by_name("notes/a.html").unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "nested");
    }
}
