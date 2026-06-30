//! By-construction telemetry events. Every variant carries only structured, content-free
//! fields — never a raw error message, path, or panic payload. The type system is the guard.

/// Structured environment facts. All values are compile-time/runtime constants, never user input.
#[derive(Clone, Debug)]
pub struct EnvFacts {
    pub app_version: String,
    pub os: String,
    pub arch: String,
}

impl EnvFacts {
    pub fn current() -> Self {
        EnvFacts {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

/// The telemetry allowlist's event dimension. Adding a variant is the only way to send a new event.
#[derive(Clone, Debug)]
pub enum TelemetryEvent {
    AppLaunch,
    /// `os_error_code` is `std::io::Error::raw_os_error()` — an integer, never the message string.
    VaultOpenFailed { os_error_code: Option<i32> },
    /// `location` is our own source `file:line` (a code identifier, not a user path). The panic
    /// payload/message is never captured.
    AppPanic { location: String },
}

impl TelemetryEvent {
    pub fn name(&self) -> &'static str {
        match self {
            TelemetryEvent::AppLaunch => "app.launch",
            TelemetryEvent::VaultOpenFailed { .. } => "vault.open.failed",
            TelemetryEvent::AppPanic { .. } => "app.panic",
        }
    }

    /// Event-specific properties, all content-free. Keys are fixed; values are integers/identifiers.
    pub fn properties(&self) -> Vec<(&'static str, String)> {
        match self {
            TelemetryEvent::AppLaunch => vec![],
            TelemetryEvent::VaultOpenFailed { os_error_code } => vec![(
                "osErrorCode",
                os_error_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()),
            )],
            TelemetryEvent::AppPanic { location } => vec![("location", location.clone())],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_names_are_stable() {
        assert_eq!(TelemetryEvent::AppLaunch.name(), "app.launch");
        assert_eq!(TelemetryEvent::VaultOpenFailed { os_error_code: Some(3) }.name(), "vault.open.failed");
        assert_eq!(TelemetryEvent::AppPanic { location: "x".into() }.name(), "app.panic");
    }

    #[test]
    fn app_launch_has_no_extra_properties() {
        assert!(TelemetryEvent::AppLaunch.properties().is_empty());
    }

    #[test]
    fn vault_failure_carries_only_an_integer_code() {
        let p = TelemetryEvent::VaultOpenFailed { os_error_code: Some(3) }.properties();
        assert_eq!(p, vec![("osErrorCode", "3".to_string())]);
        let unknown = TelemetryEvent::VaultOpenFailed { os_error_code: None }.properties();
        assert_eq!(unknown, vec![("osErrorCode", "unknown".to_string())]);
    }

    #[test]
    fn panic_carries_only_location_never_message() {
        let p = TelemetryEvent::AppPanic { location: "src-tauri/src/lib.rs:42".into() }.properties();
        assert_eq!(p, vec![("location", "src-tauri/src/lib.rs:42".to_string())]);
    }

    #[test]
    fn env_facts_are_non_empty() {
        let f = EnvFacts::current();
        assert!(!f.app_version.is_empty());
        assert!(!f.os.is_empty());
        assert!(!f.arch.is_empty());
    }
}
