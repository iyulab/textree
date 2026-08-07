use crate::secret_store::{ensure_store, service_name};
use keyring::Entry;

const SERVICE: &str = "com.textree.byo";
const ACCOUNT: &str = "api-key";

/// The service name entries are opened under — namespaced away from the installed app's entry
/// whenever this is a test run (see `secret_store`).
fn service() -> String {
    service_name(SERVICE)
}

/// Store the BYO API key in the OS credential store. Never touches disk in plaintext (
/// secrets are not written to `.md`/sidecar JSON/localStorage).
pub fn set_api_key(key: &str) -> Result<(), String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(key).map_err(|e| e.to_string())
}

/// Delete the stored key, if any. No-op-safe when nothing is stored.
pub fn clear_api_key() -> Result<(), String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether a key is currently stored. Never returns the plaintext value to callers that don't
/// need it — the frontend only ever sees this boolean (no plaintext key round-trips to JS).
pub fn has_api_key() -> bool {
    get_api_key().is_some()
}

/// Retrieve the plaintext key for internal use only (spawning the host with it as an env var).
/// Not exposed as a Tauri command — callers are Rust-side only (see host.rs restart/prepare).
pub fn get_api_key() -> Option<String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).ok()?;
    entry.get_password().ok()
}

#[tauri::command]
pub fn set_byo_api_key(key: String) -> Result<(), String> {
    set_api_key(&key)
}

#[tauri::command]
pub fn clear_byo_api_key() -> Result<(), String> {
    clear_api_key()
}

#[tauri::command]
pub fn has_byo_api_key() -> bool {
    has_api_key()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // These tests touch the real Windows Credential Manager (no fake injected — keyring's API
    // has no seam for one). They share one fixed (SERVICE, ACCOUNT) entry, and `cargo test` runs
    // tests on parallel threads by default — without serialization, one test's `clear_api_key()`
    // can delete the other's key mid-flight. TEST_LOCK forces the two to run one at a time.
    // pub(crate): host.rs's test_byo_connection fallback test also touches this same credential
    // store entry and must serialize against these tests too, or the two modules' tests can
    // race each other (the mutex only protects tests that actually acquire it).
    pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // These tests write to and clear a credential entry. If that entry is the one the installed
    // app uses, running the merge gate silently destroys the user's stored key — and can leave a
    // test literal behind for the app to send to a provider as if it were the real key.
    #[test]
    fn tests_do_not_open_the_entry_the_installed_app_uses() {
        assert_ne!(service(), SERVICE);
    }

    #[test]
    fn set_then_get_then_clear_roundtrips() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = clear_api_key(); // clean slate in case a prior aborted run left an entry
        set_api_key("test-key-123").expect("set should succeed");
        assert!(has_api_key());
        assert_eq!(get_api_key().as_deref(), Some("test-key-123"));
        clear_api_key().expect("clear should succeed");
        assert!(!has_api_key());
    }

    #[test]
    fn clear_is_noop_safe_when_nothing_stored() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = clear_api_key(); // ensure clean slate regardless of prior test order
        clear_api_key().expect("clear on empty entry should not error");
        assert!(!has_api_key());
    }
}
