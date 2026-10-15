//! The one usage event, `app.launch`, and the facts every event carries. Failures are not events:
//! they are diagnostics reports (see the module above).

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

/// `app.launch`: the app started. It carries the environment facts and nothing else.
#[derive(Clone, Copy, Debug)]
pub struct LaunchEvent;

impl LaunchEvent {
    pub const NAME: &'static str = "app.launch";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_name_is_stable() {
        assert_eq!(LaunchEvent::NAME, "app.launch");
    }

    #[test]
    fn env_facts_are_non_empty() {
        let f = EnvFacts::current();
        assert!(!f.app_version.is_empty());
        assert!(!f.os.is_empty());
        assert!(!f.arch.is_empty());
    }
}
