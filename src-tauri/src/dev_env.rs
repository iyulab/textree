//! Environment overrides that only development and test builds honour.
//!
//! Several knobs let a run swap what the app launches or talks to — the AI helper's executable, the
//! canopy command, the sign-in and publish servers, where the default notes folder and the personal
//! settings live, how often a watch is probed. They exist for the end-to-end suite and for staging
//! checks. A shipped build must not read them: anything that can write a user environment variable
//! could then choose the program the app starts or the server that receives a sign-in and a publish.
//! Every such read goes through here, so "which variables does a shipped build look at?" has one
//! answer: none of these.

use std::ffi::OsString;

/// The value of `name` in a development or test build; `None` in a shipped one.
pub fn var_os(name: &str) -> Option<OsString> {
    if cfg!(debug_assertions) {
        std::env::var_os(name)
    } else {
        None
    }
}

/// Like [`var_os`], for a value that is valid text.
pub fn var(name: &str) -> Option<String> {
    var_os(name)?.into_string().ok()
}
