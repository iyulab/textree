//! Shared plumbing for the OS credential store: the namespacing that keeps non-shipping builds off
//! a real user's secrets.
//!
//! Secrets live under one fixed entry per kind, so anything that writes or clears an entry writes
//! or clears *the* entry — the one the installed app uses. That is a data-safety problem well
//! beyond tests: a unit test that ends by clearing the entry destroys the user's stored token, and
//! a development build shares credentials with the copy of the app the user actually relies on.
//!
//! Each kind of build therefore gets its own entry, decided **at compile time**:
//!
//! | build | entry |
//! |---|---|
//! | `cargo test` | `<service>.test` |
//! | `cargo build` / `tauri dev` | `<service>.dev` |
//! | shipped release | `<service>` |
//!
//! Compile time, not an environment variable, for two reasons: nothing can be forgotten at launch
//! (a harness that failed to set a variable would silently fall back to the user's real entry), and
//! a shipped binary offers no way to redirect where its credentials are read from or written to.

/// Namespace the unit suite compiles against. Checked first — test builds are debug builds too, and
/// a `cargo test` run must not clobber a signed-in development session either.
const TEST_NAMESPACE: &str = "test";

/// Namespace every non-shipping build compiles against, `tauri dev` included.
const DEV_NAMESPACE: &str = "dev";

/// Appends a namespace to a service name. `None` and blank namespaces leave the name untouched,
/// so the shipped app keeps using the entry it has always used.
fn namespaced(base: &str, namespace: Option<&str>) -> String {
    match namespace {
        Some(ns) if !ns.trim().is_empty() => format!("{base}.{}", ns.trim()),
        _ => base.to_string(),
    }
}

/// The namespace this build compiles against, or `None` for a shipped release.
fn build_namespace() -> Option<&'static str> {
    if cfg!(test) {
        Some(TEST_NAMESPACE)
    } else if cfg!(debug_assertions) {
        Some(DEV_NAMESPACE)
    } else {
        None
    }
}

/// The service name to open entries under.
pub(crate) fn service_name(base: &str) -> String {
    namespaced(base, build_namespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_or_blank_namespace_leaves_the_production_name_alone() {
        assert_eq!(namespaced("com.example.secret", None), "com.example.secret");
        assert_eq!(namespaced("com.example.secret", Some("")), "com.example.secret");
        assert_eq!(namespaced("com.example.secret", Some("   ")), "com.example.secret");
    }

    #[test]
    fn a_namespace_yields_an_entry_distinct_from_the_production_one() {
        let base = "com.example.secret";
        let ns = namespaced(base, Some("harness"));
        assert_eq!(ns, "com.example.secret.harness");
        assert_ne!(ns, base);
    }

    #[test]
    fn surrounding_whitespace_does_not_fork_the_namespace() {
        assert_eq!(
            namespaced("com.example.secret", Some(" harness ")),
            namespaced("com.example.secret", Some("harness")),
        );
    }

    #[test]
    fn the_unit_suite_resolves_to_its_own_namespace() {
        let base = "com.example.secret";
        assert_eq!(service_name(base), "com.example.secret.test");
        assert_ne!(
            service_name(base),
            base,
            "unit tests must never open the entry the installed app uses",
        );
    }

    #[test]
    fn a_test_run_is_kept_separate_from_a_development_session() {
        // Test builds are debug builds, so the test namespace has to win — otherwise running the
        // merge gate would clear the token a `tauri dev` session had signed in with.
        assert_ne!(
            namespaced("com.example.secret", Some(TEST_NAMESPACE)),
            namespaced("com.example.secret", Some(DEV_NAMESPACE)),
        );
        assert_eq!(build_namespace(), Some(TEST_NAMESPACE));
    }
}
