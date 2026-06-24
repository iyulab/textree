//! Console-window suppression for spawned child processes.
//!
//! The app is built with `windows_subsystem = "windows"` (see `main.rs`), which
//! suppresses a console only for the app process itself. Console-subsystem
//! children we spawn — the .NET host sidecar, the canopy Node CLI, `taskkill` —
//! still get their own console window on Windows unless `CREATE_NO_WINDOW` is
//! set. This helper centralizes that flag so the magic constant lives in one
//! place. No-op on non-Windows platforms.

use std::process::Command;

/// Suppress the console window for a child process spawned from this app.
pub(crate) fn no_console_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW — run the child without allocating a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}
