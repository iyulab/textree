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
    #[serde(rename = "generatorReady", default)]
    pub generator_ready: bool,
    #[serde(rename = "generatorError", default)]
    pub generator_error: Option<String>,
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
    /// Separate single-flight counter for `ask` streams.
    /// Must NOT share `generation` — that counter drives the health-poll thread
    /// (spawn_host increments it); mixing them would cancel in-flight asks on
    /// every host respawn, and a new ask would invalidate the health poller.
    ask_generation: AtomicU64,
    current_vault: Mutex<Option<String>>,
    /// Read-only tracking of the host's generatorReady flag (from /health).
    /// Does NOT gate the Ready transition — embedder_ready controls that.
    generator_ready: Mutex<bool>,
    /// Last-known generator load error from /health (None = no error). Read-only tracking,
    /// surfaced via host_status so the frontend can show a failure instead of hanging on
    /// "preparing". Cleared optimistically by prepare_generation, refreshed every poll.
    generator_error: Mutex<Option<String>>,
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
    pub fn set_current_vault(&self, vault: String) {
        *self.current_vault.lock().unwrap_or_else(|e| e.into_inner()) = Some(vault);
    }
    pub fn current_vault(&self) -> Option<String> {
        self.current_vault.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// Bump the ask-stream generation and return the new value.
    /// Used by `ask` for single-flight cancellation (independent of health-poll `generation`).
    pub fn bump_ask_generation(&self) -> u64 {
        self.ask_generation.fetch_add(1, Ordering::SeqCst) + 1
    }
    /// Read the current ask generation (used inside spawn_blocking to detect cancellation).
    pub fn ask_generation(&self) -> u64 {
        self.ask_generation.load(Ordering::SeqCst)
    }
    /// Returns the last-known value of the host's generatorReady flag.
    /// Updated on each successful /health poll; does not gate the Ready transition.
    pub fn generator_ready(&self) -> bool {
        *self.generator_ready.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn set_generator_ready(&self, v: bool) {
        *self.generator_ready.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }
    /// Returns the last-known generator load error (None = healthy / still preparing).
    pub fn generator_error(&self) -> Option<String> {
        self.generator_error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn set_generator_error(&self, v: Option<String>) {
        *self.generator_error.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }
}

// ---------------------------------------------------------------------------
// Spawn + health poll
// ---------------------------------------------------------------------------

const HEALTH_CEILING: Duration = Duration::from_secs(15 * 60); // first-run model download
const FAST_PATH: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// After the host is Ready we keep polling for its lifetime only to observe the lazily-loaded
/// generator flip generatorReady; back off so steady-state polling does not flood the host log
/// (Kestrel logs ~2 lines per /health request). ≤10s gate latency is dwarfed by model load.
const READY_POLL_INTERVAL: Duration = Duration::from_secs(10);

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
    // The bundled host is published self-contained single-file (assemble-host-sidecar.ps1),
    // so it carries its own runtime — no DOTNET_ROOT needed. In dev, TEXTREE_HOST_EXE may point
    // at a framework-dependent build, which then relies on the ambient .NET runtime.
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

/// Per-poll decision for the health loop. Pure (no I/O) so it is unit-tested.
/// `already_ready` = the host previously reached Ready this spawn; once true we keep polling
/// for the host's lifetime to refresh generatorReady (it loads lazily, long after
/// embedder_ready) and the startup ceiling no longer applies.
#[derive(Debug, PartialEq, Eq)]
enum PollAction {
    /// First observation of embedder_ready: transition to Ready (+ reindex) once, then keep polling.
    BecomeReady,
    /// Stay in the loop and poll again (still starting within the ceiling, or already Ready).
    KeepPolling,
    /// Never reached Ready and the startup ceiling elapsed: mark Unavailable and stop.
    GiveUp,
}

fn poll_action(embedder_ready: bool, already_ready: bool, ceiling_exceeded: bool) -> PollAction {
    if already_ready {
        // Host is up; keep refreshing generatorReady indefinitely (ceiling no longer applies).
        PollAction::KeepPolling
    } else if embedder_ready {
        PollAction::BecomeReady
    } else if ceiling_exceeded {
        PollAction::GiveUp
    } else {
        PollAction::KeepPolling
    }
}

fn poll_health(handle: Arc<HostHandle>, base: String, my_gen: u64) {
    let start = Instant::now();
    let mut announced = false;
    let mut ready = false;
    loop {
        std::thread::sleep(if ready { READY_POLL_INTERVAL } else { POLL_INTERVAL });
        // A newer spawn or a shutdown invalidates this poll thread.
        if handle.generation.load(Ordering::SeqCst) != my_gen {
            return;
        }
        let health = ureq::get(&format!("{base}/health"))
            .timeout(Duration::from_secs(5))
            .call()
            .ok()
            .and_then(|resp| resp.into_string().ok())
            .and_then(|body| parse_health(&body));

        if let Some(h) = &health {
            // Refresh generatorReady on EVERY poll for the host's lifetime: the generator loads
            // lazily (first /chat or /prepare-generation), long after embedder_ready. Previously
            // the loop returned on embedder_ready, freezing generatorReady=false forever, so the
            // frontend chat/ask gate never advanced past "preparing".
            handle.set_generator_ready(h.generator_ready);
            handle.set_generator_error(h.generator_error.clone());
        }
        let embedder_ready = health.as_ref().map(|h| h.embedder_ready).unwrap_or(false);
        match poll_action(embedder_ready, ready, start.elapsed() >= HEALTH_CEILING) {
            PollAction::BecomeReady => {
                ready = true;
                handle.set_status(HostStatus::Ready);
                // Re-trigger the reindex the open vault missed while the host was Starting
                // (lazy spawn means the host is never Ready at the first open_vault).
                if let Some(vault) = handle.current_vault() {
                    reindex_vault(&handle, &vault);
                }
            }
            PollAction::GiveUp => {
                handle.set_status(HostStatus::Unavailable);
                return;
            }
            PollAction::KeepPolling => {}
        }
        if !ready && !announced && start.elapsed() >= FAST_PATH {
            announced = true;
            eprintln!(
                "[host] still starting (likely first-run model download); will keep polling up to 15m"
            );
        }
    }
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

/// Payload returned by the `host_status` IPC command.
/// `rename_all = "camelCase"` is load-bearing: the frontend expects `generatorReady` (camelCase).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatusPayload {
    pub status: HostStatus,
    pub generator_ready: bool,
    pub generator_error: Option<String>,
}

#[tauri::command]
pub fn host_status(host: State<'_, Arc<HostHandle>>) -> HostStatusPayload {
    HostStatusPayload {
        status: host.status(),
        generator_ready: host.generator_ready(),
        generator_error: host.generator_error(),
    }
}

/// Fire-and-forget: bump ask_generation so any in-flight `ask` stream detects the mismatch,
/// drops its reader, and closes the TCP connection — causing the host to see RequestAborted
/// and stop generating. Called by the frontend on panel close / note switch.
/// No params; no return value needed — bumping the counter is the entire effect.
#[tauri::command]
pub fn cancel_ask(host: State<'_, Arc<HostHandle>>) {
    host.bump_ask_generation();
}

/// Fire-and-forget: ask the host to warm its generator (model loading, KV cache pre-fill, etc.).
/// Called by the frontend when the user opens the Ask panel so the first token arrives faster.
/// Graceful: no-op when the host is not up or not Ready.
#[tauri::command]
pub async fn prepare_generation(host: State<'_, Arc<HostHandle>>) -> Result<(), String> {
    let Some(base) = host.base_url() else { return Ok(()) };
    if !matches!(host.status(), HostStatus::Ready) {
        return Ok(());
    }
    // A (re)prepare attempt clears the last generator error optimistically: otherwise a stale
    // error from before this attempt lingers in host_status until the next health poll (up to
    // READY_POLL_INTERVAL), bouncing the frontend retry loop straight back to 'error'.
    host.set_generator_error(None);
    tauri::async_runtime::spawn_blocking(move || {
        let _ = ureq::post(&format!("{base}/prepare-generation"))
            .timeout(Duration::from_secs(10))
            .call();
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
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
// DTOs and helpers for `ask` (streaming /chat)
// ---------------------------------------------------------------------------

/// A single message in a conversation, forwarded from the frontend to the host's /chat endpoint.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Events emitted by the `ask` command over its `Channel<AskEvent>`.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AskEvent {
    /// A streamed text token from the model.
    Token { text: String },
    /// The semantic-search hits that were used to ground the answer (sent once, after Done).
    Citations { hits: Vec<SemanticHit> },
    /// Stream complete — no more events will follow on this channel.
    Done,
    /// A non-fatal error; the frontend should display the message to the user.
    Error { message: String },
}

// Private SSE-parsing DTOs.
#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChatChoice>,
}
#[derive(Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
}
#[derive(Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: String,
}

/// Parse one SSE line (`"data: {...}"`) into a token string.
///
/// Returns `Some(text)` when the line carries a non-empty content delta.
/// Returns `None` for:
/// - `data: [DONE]` (stream terminator)
/// - blank lines (SSE field separator)
/// - non-`data:` lines (event/id/comment fields)
/// - JSON parse failures (malformed chunk)
/// - empty `content` field (role-only or heartbeat deltas)
pub fn parse_chat_chunk(line: &str) -> Option<String> {
    let payload = line.strip_prefix("data: ")?;
    if payload.trim() == "[DONE]" {
        return None;
    }
    let chunk: ChatChunk = serde_json::from_str(payload).ok()?;
    let text = chunk.choices.first()?.delta.content.clone();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Stream a conversational response from the local-AI host's `/chat` endpoint.
///
/// Events are pushed over `on_event`:
/// 1. `Token { text }` — one per SSE delta, in order.
/// 2. `Citations { hits }` — the pre-fetched semantic hits used to ground the answer.
/// 3. `Done` — stream complete.
///
/// On any error (host not up, connection failure, I/O) an `Error { message }` event is sent
/// instead of Token/Citations/Done, and the command returns `Ok(())` so the frontend does
/// not receive a Tauri IPC error.
///
/// Single-flight cancellation: a new `ask` call bumps `ask_generation`; in-flight streams
/// detect the mismatch on each line and abort (dropping the reader closes the TCP connection,
/// causing the host to see `RequestAborted` and stop generating — freeing CPU/memory).
///
/// Scope validation mirrors `semantic_search`: `scope_path`, when provided, must be within
/// the vault directory, or the command returns `Err` (an IPC-level error, not an event).
#[tauri::command]
pub async fn ask(
    vault: String,
    messages: Vec<ChatMessage>,
    citation_hits: Vec<SemanticHit>,
    scope_path: Option<String>,
    on_event: tauri::ipc::Channel<AskEvent>,
    host: State<'_, Arc<HostHandle>>,
) -> Result<(), String> {
    // Graceful degradation: host not up → inform the user, do not hard-error.
    let Some(base) = host.base_url() else {
        let _ = on_event.send(AskEvent::Error {
            message: "Local AI is not available".into(),
        });
        return Ok(());
    };
    if !matches!(host.status(), HostStatus::Ready) {
        let _ = on_event.send(AskEvent::Error {
            message: "Local AI is preparing".into(),
        });
        return Ok(());
    }
    // Security at the edge: reject scope_path that escapes the vault (mirrors semantic_search N3).
    if let Some(ref sp) = scope_path {
        if !crate::pathsafe::is_within(Path::new(&vault), Path::new(sp)) {
            return Err("scope is outside the vault".into());
        }
    }

    // Single-flight cancellation counter (separate from health-poll `generation`).
    let my_gen = host.bump_ask_generation();
    let handle = host.inner().clone();

    // Clone the channel so on_event is available both inside and after spawn_blocking.
    // Channel<T> is Clone in Tauri 2; both halves push to the same frontend listener.
    let on_event_inner = on_event.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        let payload = serde_json::json!({ "messages": messages, "stream": true });
        let resp = ureq::post(&format!("{base}/chat"))
            .timeout(Duration::from_secs(120))
            .send_json(payload)
            .map_err(|e| e.to_string())?;
        let reader = BufReader::new(resp.into_reader());
        for line in reader.lines() {
            // A newer `ask` has started — abandon this stream.
            // Dropping `reader` closes the TCP connection; the host sees RequestAborted
            // and stops generating, freeing its CPU/memory budget.
            if handle.ask_generation() != my_gen {
                return Ok(());
            }
            let line = line.map_err(|e| e.to_string())?;
            if let Some(text) = parse_chat_chunk(&line) {
                let _ = on_event_inner.send(AskEvent::Token { text });
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(()) => {
            // Guard against sending Citations/Done on a superseded channel (cancelled ask).
            // If another ask has since started, our channel is stale — skip the epilogue.
            if host.ask_generation() == my_gen {
                let _ = on_event.send(AskEvent::Citations { hits: citation_hits });
                let _ = on_event.send(AskEvent::Done);
            }
        }
        Err(e) => {
            let _ = on_event.send(AskEvent::Error { message: e });
        }
    }
    Ok(())
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
    fn current_vault_round_trips() {
        let handle = HostHandle::default();
        assert!(handle.current_vault().is_none());
        handle.set_current_vault("D:/vault".into());
        assert_eq!(handle.current_vault().as_deref(), Some("D:/vault"));
    }

    #[test]
    #[ignore = "requires assemble-host-sidecar.ps1 to have staged the exe under src-tauri/resources/host/"]
    fn run_host_present_via_assembled_sidecar() {
        // CARGO_MANIFEST_DIR = src-tauri/. The assembled payload lives under resources/host/.
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let resource = manifest.join("resources");
        let found = host_exe_from_resource_dir(&resource);
        assert!(
            found.is_some(),
            "assembled host exe not found under {}/host/ — run scripts/assemble-host-sidecar.ps1",
            resource.display()
        );
    }

    #[test]
    fn poll_action_becomes_ready_on_embedder_ready() {
        assert_eq!(poll_action(true, false, false), PollAction::BecomeReady);
    }

    #[test]
    fn poll_action_embedder_ready_beats_ceiling() {
        // Embedder up right at the ceiling still transitions to Ready (not GiveUp).
        assert_eq!(poll_action(true, false, true), PollAction::BecomeReady);
    }

    #[test]
    fn poll_action_keeps_polling_while_starting_within_ceiling() {
        assert_eq!(poll_action(false, false, false), PollAction::KeepPolling);
    }

    #[test]
    fn poll_action_gives_up_when_never_ready_past_ceiling() {
        assert_eq!(poll_action(false, false, true), PollAction::GiveUp);
    }

    #[test]
    fn poll_action_keeps_polling_forever_once_ready() {
        // Once Ready we keep refreshing generatorReady for the host's lifetime regardless of
        // embedder blips or the startup ceiling — never GiveUp (that would mark a healthy host
        // dead). This is the invariant that fixes the frozen-generatorReady bug: the loop must
        // keep observing /health after embedder_ready so the lazily-loaded generator is seen.
        assert_eq!(poll_action(true, true, true), PollAction::KeepPolling);
        assert_eq!(poll_action(false, true, true), PollAction::KeepPolling);
        assert_eq!(poll_action(false, true, false), PollAction::KeepPolling);
    }

    #[test]
    fn parse_health_reads_generator_ready() {
        let body = r#"{"status":"ok","embedderReady":true,"generatorReady":false}"#;
        let h = parse_health(body).unwrap();
        assert!(h.embedder_ready);
        assert!(!h.generator_ready);
    }

    #[test]
    fn parse_health_reads_generator_error() {
        let body = r#"{"status":"ok","embedderReady":true,"generatorReady":false,"generatorError":"model download failed"}"#;
        let h = parse_health(body).unwrap();
        assert_eq!(h.generator_error.as_deref(), Some("model download failed"));
    }

    #[test]
    fn parse_health_generator_error_defaults_none() {
        let body = r#"{"status":"ok","embedderReady":true,"generatorReady":false}"#;
        let h = parse_health(body).unwrap();
        assert!(h.generator_error.is_none());
    }

    #[test]
    fn generator_error_round_trips() {
        let handle = HostHandle::default();
        assert!(handle.generator_error().is_none());
        handle.set_generator_error(Some("boom".into()));
        assert_eq!(handle.generator_error().as_deref(), Some("boom"));
        handle.set_generator_error(None);
        assert!(handle.generator_error().is_none());
    }

    #[test]
    fn parse_chat_chunk_extracts_delta_content() {
        // Valid delta with text.
        let line = r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#;
        assert_eq!(parse_chat_chunk(line), Some("Hel".to_string()));
        // Stream terminator — must return None.
        assert_eq!(parse_chat_chunk("data: [DONE]"), None);
        // Blank line (SSE separator) — must return None.
        assert_eq!(parse_chat_chunk(""), None);
        // Empty content field — model emitted an empty delta, not a real token.
        let empty_delta = r#"data: {"choices":[{"delta":{"content":""}}]}"#;
        assert_eq!(parse_chat_chunk(empty_delta), None);
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
