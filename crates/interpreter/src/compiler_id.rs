//! Identity of this compiler, taken from the workspace `Cargo.toml` and
//! `Cargo.lock` packaged beside this crate.
//!
//! Installed package artifacts record [`compiler_id`]. A missing or different
//! stamp means `pkg.wasm` was produced by another compiler and must be rebuilt
//! from the sources retained beside it, not executed.
//!
//! The copies are the files `cargo publish` can ship. A workspace build refuses
//! to proceed when they drift from the repository manifests. A compiler change
//! that does not touch those manifests does not move the stamp.

use std::sync::OnceLock;

const PACKAGED_CARGO_TOML: &str = include_str!("../compiler-identity/workspace-Cargo.toml");
const PACKAGED_CARGO_LOCK: &str = include_str!("../compiler-identity/workspace-Cargo.lock");

/// `{workspace package version}+{hash of the packaged manifests}`.
pub fn compiler_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(stamp_packaged_manifests)
}

fn stamp_packaged_manifests() -> String {
    use sha2::{Digest, Sha256};

    let version = workspace_package_version(PACKAGED_CARGO_TOML);
    let mut hasher = Sha256::new();
    hash_text(&mut hasher, PACKAGED_CARGO_TOML);
    hash_text(&mut hasher, PACKAGED_CARGO_LOCK);
    let fingerprint: String = hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{version}+{fingerprint}")
}

fn hash_text(hasher: &mut sha2::Sha256, text: &str) {
    use sha2::Digest;
    hasher.update((text.len() as u64).to_le_bytes());
    hasher.update(text.as_bytes());
}

/// `[workspace.package] version` from the packaged workspace manifest.
///
/// `build.rs` rejects a workspace build unless this file is the repository
/// `Cargo.toml`, which has that table. The published crate ships those same
/// bytes, so a missing version means the compiler package itself is corrupt.
fn workspace_package_version(toml: &str) -> &str {
    let Some((_, after)) = toml.split_once("[workspace.package]") else {
        panic!("packaged workspace Cargo.toml has no [workspace.package] table");
    };
    let body = after.split("\n[").next().unwrap_or(after);
    for line in body.lines() {
        let Some(rest) = line.trim().strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim().strip_prefix('=') else {
            continue;
        };
        let version = rest.trim().trim_matches('"');
        if !version.is_empty() {
            return version;
        }
    }
    panic!("packaged workspace Cargo.toml has no package version");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_uses_the_packaged_workspace_version() {
        let version = workspace_package_version(PACKAGED_CARGO_TOML);
        let id = compiler_id();
        assert_eq!(version, engine_version(PACKAGED_CARGO_LOCK));
        assert!(id.starts_with(&format!("{version}+")), "{id}");
        assert_eq!(id.len(), version.len() + 1 + 16);
        assert_eq!(id, compiler_id());
    }

    fn engine_version(lock: &str) -> &str {
        for block in lock.split("[[package]]").skip(1) {
            let mut name = None;
            let mut version = None;
            for line in block.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    break;
                }
                if let Some(value) = line.strip_prefix("name = ") {
                    name = Some(value.trim_matches('"'));
                } else if let Some(value) = line.strip_prefix("version = ") {
                    version = Some(value.trim_matches('"'));
                }
            }
            if name == Some("submilli-engine") {
                return version.unwrap_or_else(|| {
                    panic!("packaged Cargo.lock submilli-engine entry has no version")
                });
            }
        }
        panic!("packaged Cargo.lock has no submilli-engine package");
    }
}
