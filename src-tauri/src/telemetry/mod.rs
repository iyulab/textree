//! Content-free, by-construction telemetry egress (D25). See
//! docs/superpowers/specs/2026-07-01-app-telemetry-emit-design.md.

pub mod config;
pub mod event;
pub mod envelope;

use config::TelemetryConfig;
use event::{EnvFacts, TelemetryEvent};
use std::sync::OnceLock;

/// Raw connection string from runtime env first (dev/E2E injection), then the compile-time
/// embedded value (official builds). Cached. `None`/blank ⇒ telemetry disabled everywhere.
fn raw_connection() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            std::env::var(config::ENV_VAR)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| option_env!("TEXTREE_TELEMETRY_CONNECTION").map(str::to_string))
                .filter(|s| !s.trim().is_empty())
        })
        .clone()
}

fn resolved_connection() -> Option<TelemetryConfig> {
    config::parse_connection(&raw_connection()?)
}

/// The raw connection string to forward to the .NET host on spawn, so a single embedded string
/// activates both egress paths. `None` ⇒ host telemetry also stays off.
pub fn host_connection() -> Option<String> {
    raw_connection()
}

/// Sends one content-free event. No-op when telemetry is disabled. Never blocks the caller and
/// never propagates errors — a failed send is dropped (no buffering). Transparency: the event name
/// is logged locally so the user can see what is sent (D25 "no silence").
pub fn emit(event: TelemetryEvent) {
    let Some(config) = resolved_connection() else {
        return;
    };
    log::info!("[telemetry] emit {}", event.name());
    std::thread::spawn(move || {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ts = envelope::format_rfc3339_utc(secs);
        let facts = EnvFacts::current();
        let body = envelope::build_envelope(&event, &config, &facts, &ts);
        let _ = ureq::post(&config.track_url)
            .set("Content-Type", "application/json")
            .send_json(serde_json::json!([body]));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::event::TelemetryEvent;

    /// With no connection configured (the default in tests), emit must be a silent no-op and never
    /// panic or block. This is the safety contract for dev/test/un-injected builds.
    #[test]
    fn emit_is_noop_without_connection() {
        std::env::remove_var(config::ENV_VAR);
        emit(TelemetryEvent::AppLaunch); // must return immediately, no panic
        emit(TelemetryEvent::VaultOpenFailed { os_error_code: Some(3) });
    }
}
