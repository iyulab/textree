//! Cloud publishing (slice 2b): render the vault locally (reusing `publish::run_publish`), zip the
//! output, and upload it to api.textree.me/publish (slice 2a contract). User-triggered, read-only
//! over the source (D13). The publish token is read from the OS keychain (publish_secret), never
//! passed from the frontend.

use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishToCloudResult {
    pub url: String,
    pub page_count: usize,
}

const DEFAULT_API_BASE: &str = "https://api.textree.me";

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
}
