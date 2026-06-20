use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
// Step 1: HostHandle and accessors
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
// Step 2: spawn + health poll
// ---------------------------------------------------------------------------

const HEALTH_CEILING: Duration = Duration::from_secs(15 * 60); // first-run model download
const FAST_PATH: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Spawn the host on a fresh loopback port and begin health polling on a
/// background thread. Never panics; on any failure the handle stays Unavailable.
pub fn spawn_host(handle: Arc<HostHandle>, exe: String, app_data: std::path::PathBuf) {
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
    // .exe vs a node-style launcher: if exe ends in .dll, prepend `dotnet`.
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

    let h = handle.clone();
    std::thread::spawn(move || poll_health(h, url));
}

fn drain_utf8(stream: Option<impl std::io::Read + Send + 'static>, log: std::path::PathBuf) {
    let Some(stream) = stream else { return };
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(log.parent().unwrap());
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

fn poll_health(handle: Arc<HostHandle>, base: String) {
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
    handle.set_status(HostStatus::Unavailable);
}

// ---------------------------------------------------------------------------
// Step 3: shutdown ladder
// ---------------------------------------------------------------------------

pub fn shutdown_host(handle: &HostHandle) {
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
    }
}
