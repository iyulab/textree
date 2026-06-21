use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::path::Path;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HostStatus {
    Starting,
    Ready,
    Unavailable,
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(rename = "embedderReady")]
    pub embedder_ready: bool,
}

/// Bind 127.0.0.1:0 to let the OS pick a free port, then release it so the
/// host can claim it. Avoids fixed-port collisions (lesson from Filer).
pub fn alloc_loopback_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

pub fn parse_health(body: &str) -> Option<HealthResponse> {
    serde_json::from_str(body).ok()
}

// ---------------------------------------------------------------------------
// HostHandle and accessors
// ---------------------------------------------------------------------------

struct HostStatusCell(HostStatus);
impl Default for HostStatusCell {
    fn default() -> Self {
        HostStatusCell(HostStatus::Unavailable)
    }
}

#[derive(Default)]
pub struct HostHandle {
    status: Mutex<HostStatusCell>,
    port: Mutex<Option<u16>>,
    child: Mutex<Option<Child>>,
    generation: AtomicU64,
}

impl HostHandle {
    pub fn status(&self) -> HostStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).0.clone()
    }
    fn set_status(&self, s: HostStatus) {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).0 = s;
    }
    pub fn base_url(&self) -> Option<String> {
        let p = *self.port.lock().unwrap_or_else(|e| e.into_inner());
        p.map(|p| format!("http://127.0.0.1:{p}"))
    }
}

// ---------------------------------------------------------------------------
// Spawn + health poll
// ---------------------------------------------------------------------------

const HEALTH_CEILING: Duration = Duration::from_secs(15 * 60); // first-run model download
const FAST_PATH: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Spawn the host on a fresh loopback port and begin health polling on a
/// background thread. Never panics; on any failure the handle stays Unavailable.
pub fn spawn_host(handle: Arc<HostHandle>, exe: String, app_data: std::path::PathBuf) {
    // Idempotent: never double-spawn. A concurrent caller (mount auto-spawn + ? enable, or a
    // dev eager-spawn racing a manual trigger) must not orphan a child or clobber the port.
    if matches!(handle.status(), HostStatus::Starting | HostStatus::Ready) {
        return;
    }
    let port = match alloc_loopback_port() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[host] port alloc failed: {e}");
            handle.set_status(HostStatus::Unavailable);
            return;
        }
    };
    let url = format!("http://127.0.0.1:{port}");
    let mut cmd = Command::new(&exe);
    cmd.arg("--urls")
        .arg(&url)
        .env("ASPNETCORE_URLS", &url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // TODO(lifecycle): the published host is framework-dependent — caller must ensure
    // DOTNET_ROOT is set or publish self-contained, else the exe fails to launch.
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[host] spawn failed: {e}");
            handle.set_status(HostStatus::Unavailable);
            return;
        }
    };
    // UTF-8 async drain (Filer mojibake/deadlock lesson).
    drain_utf8(child.stdout.take(), app_data.join("logs/host-out.log"));
    drain_utf8(child.stderr.take(), app_data.join("logs/host-err.log"));
    *handle.port.lock().unwrap_or_else(|e| e.into_inner()) = Some(port);
    *handle.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    handle.set_status(HostStatus::Starting);

    let my_gen = handle.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let h = handle.clone();
    std::thread::spawn(move || poll_health(h, url, my_gen));
}

fn drain_utf8(stream: Option<impl std::io::Read + Send + 'static>, log: std::path::PathBuf) {
    let Some(stream) = stream else { return };
    std::thread::spawn(move || {
        if let Some(parent) = log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .ok();
        let reader = BufReader::new(stream);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(f) = file.as_mut() {
                let _ = writeln!(f, "{line}"); // bytes are UTF-8 from the .NET host
            }
        }
    });
}

fn poll_health(handle: Arc<HostHandle>, base: String, my_gen: u64) {
    let start = Instant::now();
    let mut announced = false;
    while start.elapsed() < HEALTH_CEILING {
        std::thread::sleep(POLL_INTERVAL);
        if let Ok(resp) = ureq::get(&format!("{base}/health"))
            .timeout(Duration::from_secs(5))
            .call()
        {
            if let Ok(body) = resp.into_string() {
                if let Some(h) = parse_health(&body) {
                    if h.embedder_ready {
                        if handle.generation.load(Ordering::SeqCst) != my_gen {
                            return;
                        }
                        handle.set_status(HostStatus::Ready);
                        return;
                    }
                }
            }
        }
        if !announced && start.elapsed() >= FAST_PATH {
            announced = true;
            eprintln!(
                "[host] still starting (likely first-run model download); will keep polling up to 15m"
            );
        }
    }
    if handle.generation.load(Ordering::SeqCst) != my_gen {
        return;
    }
    handle.set_status(HostStatus::Unavailable);
}

// ---------------------------------------------------------------------------
// Shutdown ladder
// ---------------------------------------------------------------------------

pub fn shutdown_host(handle: &HostHandle) {
    // Invalidate any in-flight poll thread before we proceed; this ensures that a
    // poll thread whose /health response arrives after we return cannot overwrite
    // the Unavailable status we set at the end of this function.
    handle.generation.fetch_add(1, Ordering::SeqCst);
    if let Some(base) = handle.base_url() {
        let _ = ureq::post(&format!("{base}/shutdown"))
            .timeout(Duration::from_millis(800))
            .call();
    }
    let mut guard = handle.child.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut child) = guard.take() {
        // give graceful shutdown a brief moment, then force.
        std::thread::sleep(Duration::from_millis(300));
        let _ = child.kill();
        #[cfg(windows)]
        {
            let pid = child.id();
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }
        let _ = child.wait();
    }
    handle.set_status(HostStatus::Unavailable);
}

// ---------------------------------------------------------------------------
// Host executable resolution (mechanism B — parallel to publish::canopy_from_resource_dir)
// ---------------------------------------------------------------------------

/// Resolves the bundled host exe from a Tauri resource directory: `<resource>/host/textree-host(.exe)`.
/// Returns the absolute path, or `None` if it is missing (e.g. an unbundled dev build).
pub fn host_exe_from_resource_dir(resource: &Path) -> Option<String> {
    let name = if cfg!(windows) { "textree-host.exe" } else { "textree-host" };
    let exe = resource.join("host").join(name);
    if exe.exists() {
        Some(exe.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// How to launch the host. Dev/E2E: `TEXTREE_HOST_EXE` env (auto-consent — keeps the existing
/// dev/test flow eager-spawning). Production: the bundled sidecar under `<resource>/host/`.
pub fn resolve_host(app: &AppHandle) -> Option<String> {
    if let Ok(exe) = std::env::var("TEXTREE_HOST_EXE") {
        return Some(exe);
    }
    let resource = app.path().resource_dir().ok()?;
    host_exe_from_resource_dir(&resource)
}

// ---------------------------------------------------------------------------
// DTOs, response parser, and IPC commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticHit {
    pub path: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Deserialize)]
struct SearchResponseDto {
    results: Vec<SemanticHit>,
    #[allow(dead_code)]
    status: String,
}

pub fn parse_search_response(body: &str) -> Option<Vec<SemanticHit>> {
    serde_json::from_str::<SearchResponseDto>(body).ok().map(|r| r.results)
}

#[tauri::command]
pub fn host_status(host: State<'_, Arc<HostHandle>>) -> HostStatus {
    host.status()
}

/// User-triggered: spawn the local-AI host now. Called by the frontend after the user consents
/// (or at mount when the device-local consent flag is set). No-op if already up; graceful (Ok with
/// no spawn) when no host exe is bundled/available — AI simply stays off.
#[tauri::command]
pub fn prepare_ai_model(app: AppHandle, host: State<'_, Arc<HostHandle>>) -> Result<(), String> {
    if matches!(host.status(), HostStatus::Starting | HostStatus::Ready) {
        return Ok(());
    }
    let Some(exe) = resolve_host(&app) else {
        return Ok(()); // no host available → graceful degradation
    };
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    spawn_host(host.inner().clone(), exe, app_data);
    Ok(())
}

#[tauri::command]
pub async fn semantic_search(
    vault: String,
    query: String,
    scope_path: Option<String>,
    limit: u32,
    host: State<'_, Arc<HostHandle>>,
) -> Result<Vec<SemanticHit>, String> {
    // Read cheap mutex fields before entering the blocking closure.
    let Some(base) = host.base_url() else {
        return Ok(Vec::new()); // host not up → empty, caller falls back / shows unavailable
    };
    if !matches!(host.status(), HostStatus::Ready) {
        return Ok(Vec::new());
    }
    // N3: reject scope_path that escapes the vault (constitution: security at the edge).
    // is_within uses canonicalize — both paths must exist; scope defaults to vault root.
    if let Some(ref sp) = scope_path {
        if !crate::pathsafe::is_within(
            std::path::Path::new(&vault),
            std::path::Path::new(sp),
        ) {
            return Err("scope is outside the vault".into());
        }
    }
    let scope = scope_path.unwrap_or_else(|| vault.clone()); // compute before moving `vault`
    tauri::async_runtime::spawn_blocking(move || {
        let payload = serde_json::json!({
            "vaultPath": vault,
            "query": query,
            "scopePath": scope,
            "limit": limit,
        });
        match ureq::post(&format!("{base}/search"))
            .timeout(Duration::from_secs(10))
            .send_json(payload)
        {
            Ok(resp) => {
                let body = resp.into_string().map_err(|e| e.to_string())?;
                Ok(parse_search_response(&body).unwrap_or_default())
            }
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Fire-and-forget indexing helpers
// ---------------------------------------------------------------------------

/// Fire-and-forget single-note index. Silent on any error (degrade, don't block).
/// Content-immutable background work — auto-allowed per D18.
pub fn index_note(handle: &HostHandle, vault: &str, path: &str) {
    let Some(base) = handle.base_url() else { return };
    if !matches!(handle.status(), HostStatus::Ready) {
        return;
    }
    let payload = serde_json::json!({ "vaultPath": vault, "path": path });
    let _ = ureq::post(&format!("{base}/index"))
        .timeout(Duration::from_secs(10))
        .send_json(payload);
}

/// Fire-and-forget full vault reindex. Silent on any error (degrade, don't block).
/// Content-immutable background work — auto-allowed per D18.
pub fn reindex_vault(handle: &HostHandle, vault: &str) {
    let Some(base) = handle.base_url() else { return };
    if !matches!(handle.status(), HostStatus::Ready) {
        return;
    }
    let payload = serde_json::json!({ "vaultPath": vault });
    let _ = ureq::post(&format!("{base}/reindex"))
        .timeout(Duration::from_secs(10))
        .send_json(payload);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_loopback_port_returns_nonzero() {
        let p = alloc_loopback_port().unwrap();
        assert!(p > 0);
    }

    #[test]
    fn parse_health_reads_embedder_ready() {
        let h = parse_health(r#"{"status":"ok","embedderReady":true}"#).unwrap();
        assert_eq!(h.status, "ok");
        assert!(h.embedder_ready);
    }

    #[test]
    fn parse_health_handles_not_ready() {
        let h = parse_health(r#"{"status":"loading","embedderReady":false}"#).unwrap();
        assert!(!h.embedder_ready);
    }

    #[test]
    fn parse_health_rejects_garbage() {
        assert!(parse_health("not json").is_none());
    }

    #[test]
    fn parse_search_response_maps_hits() {
        let body = r#"{"results":[{"path":"a.md","snippet":"hello","score":0.9}],"status":"ok"}"#;
        let hits = parse_search_response(body).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "a.md");
        assert!((hits[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn parse_search_response_empty_on_no_results() {
        let hits = parse_search_response(r#"{"results":[],"status":"ok"}"#).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn host_exe_from_resource_dir_finds_bundled_exe() {
        let tmp = std::env::temp_dir().join(format!("textree-host-resolve-{}", std::process::id()));
        let host_dir = tmp.join("host");
        std::fs::create_dir_all(&host_dir).unwrap();
        let name = if cfg!(windows) { "textree-host.exe" } else { "textree-host" };
        let exe = host_dir.join(name);
        std::fs::write(&exe, b"stub").unwrap();

        let found = host_exe_from_resource_dir(&tmp);
        assert_eq!(found.as_deref(), exe.to_str());

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn host_exe_from_resource_dir_none_when_absent() {
        let tmp = std::env::temp_dir().join(format!("textree-host-absent-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(host_exe_from_resource_dir(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn spawn_host_is_noop_when_already_up() {
        let handle = Arc::new(HostHandle::default());
        handle.set_status(HostStatus::Ready); // pretend the host is already running
        let tmp = std::env::temp_dir().join("textree-host-guard");
        // A bogus exe would fail to spawn and flip status to Unavailable WITHOUT the guard.
        spawn_host(handle.clone(), "this-exe-does-not-exist".into(), tmp);
        assert!(
            matches!(handle.status(), HostStatus::Ready),
            "guard must prevent re-spawn when already Starting/Ready"
        );
    }

    #[test]
    #[ignore = "requires TEXTREE_HOST_EXE pointing at a built host"]
    fn spawn_reaches_ready() {
        let exe = std::env::var("TEXTREE_HOST_EXE").expect("set TEXTREE_HOST_EXE");
        let handle = Arc::new(HostHandle::default());
        let tmp = std::env::temp_dir().join("textree-host-test");
        spawn_host(handle.clone(), exe, tmp);
        // poll up to 90s for readiness (model may download)
        for _ in 0..90 {
            if matches!(handle.status(), HostStatus::Ready) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        assert!(matches!(handle.status(), HostStatus::Ready));
        shutdown_host(&handle);
        assert!(
            matches!(handle.status(), HostStatus::Unavailable),
            "status must be Unavailable after shutdown (generation-gate fix)"
        );
    }
}
