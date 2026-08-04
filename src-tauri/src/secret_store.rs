//! Shared plumbing for the OS credential store: store initialisation and the namespacing that
//! keeps non-shipping builds off a real user's secrets.
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

/// Work around a confirmed upstream bug in `keyring` 4.1.3's `v1` convenience shim: its
/// `Entry::new()` is supposed to lazily call an internal `set_credential_store()` on first use
/// (mirroring this exact init), but the guard that gates that call is
/// `AtomicBool::compare_exchange(false, true, ..) == Ok(true)` — which can never be `true`
/// (a successful swap returns `Ok(<previous value>)`, i.e. `Ok(false)` on the first call, and a
/// failed one returns `Err(true)` on every call after). So the branch is dead code and no
/// default store is ever installed: bare `keyring::Entry::new/get_password/set_password` fail
/// on every platform with `Error::NoDefaultStore` ("No default store has been set"). Confirmed
/// empirically in this environment (both round-trip tests failed with that exact message) and
/// by isolating the `compare_exchange` call in a standalone repro. Fixed keyring release: none
/// as of 4.1.3 (latest on the `4` line at the time of writing).
///
/// We replicate the shim's own (otherwise-correct) init body ourselves, once per process.
/// Windows-only: textree ships on Windows only (see the windows-gated deps elsewhere in this
/// crate, e.g. `tauri-plugin-updater` in `lib.rs`), and `windows-native-keyring-store` is a
/// `cfg(windows)`-only Cargo dependency. On a hypothetical non-Windows build this is a no-op,
/// leaving `keyring::Entry` to surface the same upstream `NoDefaultStore` error it does today.
pub(crate) fn ensure_store() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        #[cfg(windows)]
        if let Ok(store) = windows_native_keyring_store::Store::new() {
            keyring_core::set_default_store(store);
        }
    });
}

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
