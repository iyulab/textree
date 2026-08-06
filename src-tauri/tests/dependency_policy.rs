//! Guards the dependency surface of the git backend.
//!
//! The bundled libgit2 is compiled with a minimal feature set: no TLS backend and no SSH
//! transport. Enabling either pulls additional native libraries into the binary and changes
//! the set of licenses the distribution has to carry. This test fails loudly if that happens.

/// Crates that must never appear in the resolved dependency graph.
const FORBIDDEN: [&str; 2] = ["openssl-sys", "libssh2-sys"];

/// Returns the forbidden crate names that appear as packages in a Cargo lockfile.
fn forbidden_packages_in(lockfile: &str) -> Vec<&'static str> {
    FORBIDDEN
        .into_iter()
        .filter(|name| lockfile.contains(&format!("name = \"{name}\"")))
        .collect()
}

#[test]
fn detects_a_forbidden_package_in_a_lockfile() {
    let lockfile = "\
[[package]]
name = \"openssl-sys\"
version = \"0.9.0\"
";
    assert_eq!(forbidden_packages_in(lockfile), vec!["openssl-sys"]);
}

#[test]
fn ignores_unrelated_packages_and_longer_names_sharing_a_prefix() {
    let lockfile = "\
[[package]]
name = \"serde\"
version = \"1.0.0\"

[[package]]
name = \"openssl-sys-probe\"
version = \"0.1.0\"
";
    assert!(forbidden_packages_in(lockfile).is_empty());
}

#[test]
fn the_committed_lockfile_carries_no_tls_or_ssh_backend() {
    let found = forbidden_packages_in(include_str!("../Cargo.lock"));
    assert!(
        found.is_empty(),
        "{found:?} entered the dependency graph. The git backend is pinned to a minimal \
         libgit2 feature set; do not enable its `https` or `ssh` features."
    );
}
