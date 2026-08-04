//! OS credential storage for the cloud publish token (D16 — never written to disk in plaintext).
//! Mirrors `byo_secret.rs`; both sit on `secret_store` for store init and namespacing. Distinct
//! (SERVICE, ACCOUNT) entry so it never collides with the BYO key.

use crate::secret_store::{ensure_store, service_name};
use keyring::Entry;

const SERVICE: &str = "com.textree.publish";
const ACCOUNT: &str = "token";

/// The service name entries are opened under — namespaced away from the installed app's entry
/// whenever this is a test run (see `secret_store`).
fn service() -> String {
    service_name(SERVICE)
}

/// Store the publish token in the OS credential store.
pub fn set_token(token: &str) -> Result<(), String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).map_err(|e| e.to_string())?;
    entry.set_password(token).map_err(|e| e.to_string())
}

/// Delete the stored token, if any. No-op-safe when nothing is stored.
pub fn clear_token() -> Result<(), String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether a token is currently stored (the only thing the frontend ever learns — D16).
pub fn has_token() -> bool {
    get_token().is_some()
}

/// Retrieve the plaintext token for internal use only (the uploader). Not a Tauri command —
/// callers are Rust-side only (see cloud_publish / commands::publish_to_cloud).
pub fn get_token() -> Option<String> {
    ensure_store();
    let entry = Entry::new(&service(), ACCOUNT).ok()?;
    entry.get_password().ok()
}

#[tauri::command]
pub fn clear_publish_token() -> Result<(), String> {
    clear_token()
}

#[tauri::command]
pub fn has_publish_token() -> bool {
    has_token()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Touches the real OS credential store (keyring has no fake seam), sharing one fixed
    // (SERVICE, ACCOUNT) entry. cargo test runs threads in parallel, so serialize the two tests.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // See the matching guard in `byo_secret`: a test that writes to the installed app's own entry
    // destroys the user's publish token, and only a browser sign-in can mint another.
    #[test]
    fn tests_do_not_open_the_entry_the_installed_app_uses() {
        assert_ne!(service(), SERVICE);
    }

    #[test]
    fn set_then_get_then_clear_roundtrips() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = clear_token(); // clean slate
        set_token("tk_test_abc").expect("set should succeed");
        assert!(has_token());
        assert_eq!(get_token().as_deref(), Some("tk_test_abc"));
        clear_token().expect("clear should succeed");
        assert!(!has_token());
    }

    #[test]
    fn clear_is_noop_safe_when_nothing_stored() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = clear_token();
        clear_token().expect("clear on empty entry should not error");
        assert!(!has_token());
    }
}
