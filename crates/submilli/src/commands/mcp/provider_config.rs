//! The local MCP OAuth provider config — `$SUBMILLI_HOME/mcp_oauth.yaml`.
//!
//! It mirrors the server's `mcp_oauth.providers` block: per-authorization-host
//! `client_id` / `client_secret` / `scopes`, each a literal or `${env.X}` /
//! `${secrets.X}` reference resolved from the local secret store at use. It
//! lives outside the blueprint on purpose — a `client_secret` is a machine
//! credential, and blueprints are meant to be shared and committed.
//!
//! This is what lets the local `submilli mcp authenticate` complete the
//! confidential-client OAuth flow (e.g. GitHub OAuth apps) that would otherwise
//! only work through a running server.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use submilli_shared::OAuthProvider;

/// One `providers:` entry. `match` is the OAuth host — matched against the
/// token endpoint's host (the same as the authorization server for typical
/// providers), not the MCP URL's host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    #[serde(rename = "match")]
    pub match_host: String,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvidersFile {
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
}

impl From<&ProviderEntry> for OAuthProvider {
    fn from(entry: &ProviderEntry) -> Self {
        OAuthProvider {
            match_host: entry.match_host.clone(),
            client_id: entry.client_id.clone(),
            client_secret: entry.client_secret.clone(),
            scopes: entry.scopes.clone(),
        }
    }
}

/// `$SUBMILLI_HOME/mcp_oauth.yaml` (default `~/.submilli/mcp_oauth.yaml`) — the
/// same data root the local secret store lives under.
pub fn config_path() -> PathBuf {
    submilli_build::default_data_root().join("mcp_oauth.yaml")
}

/// Read the config file, returning an empty set when it doesn't exist.
pub fn load_file() -> Result<ProvidersFile> {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_yml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ProvidersFile::default()),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// The configured providers as the shared [`OAuthProvider`] type, ready for
/// `match_provider`. Empty when no config exists.
pub fn load() -> Result<Vec<OAuthProvider>> {
    Ok(load_file()?
        .providers
        .iter()
        .map(OAuthProvider::from)
        .collect())
}

/// Write the config file, creating the data root if needed. The file is
/// owner-only (0600) under a 0700 directory, matching the co-located secret
/// store — the config holds only secret *references*, but the surrounding dir
/// is shared with real credentials.
pub fn save_file(file: &ProvidersFile) -> Result<()> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        restrict_dir(dir)?;
    }
    let text = serde_yml::to_string(file).context("serializing provider config")?;
    write_owner_only(&path, text.as_bytes()).with_context(|| format!("writing {}", path.display()))
}

#[cfg(unix)]
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    // `.mode()` only applies when creating; enforce 0600 on an existing file too.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("restricting {}", dir.display()))
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_provider_entry() {
        let yaml = "providers:\n  - match: github.com\n    client_id: ${env.GH_ID}\n    client_secret: ${secrets.GH_SECRET}\n    scopes: [repo, read:org]\n";
        let file: ProvidersFile = serde_yml::from_str(yaml).unwrap();
        assert_eq!(file.providers.len(), 1);
        let p = &file.providers[0];
        assert_eq!(p.match_host, "github.com");
        assert_eq!(p.client_id, "${env.GH_ID}");
        assert_eq!(p.client_secret.as_deref(), Some("${secrets.GH_SECRET}"));
        assert_eq!(p.scopes, vec!["repo", "read:org"]);
    }

    #[test]
    fn public_client_entry_has_no_secret() {
        let yaml = "providers:\n  - match: example.com\n    client_id: pinned-id\n";
        let file: ProvidersFile = serde_yml::from_str(yaml).unwrap();
        assert!(file.providers[0].client_secret.is_none());
        assert!(file.providers[0].scopes.is_empty());
    }

    #[test]
    fn round_trips_and_omits_empty_fields() {
        let file = ProvidersFile {
            providers: vec![ProviderEntry {
                match_host: "github.com".into(),
                client_id: "id".into(),
                client_secret: None,
                scopes: Vec::new(),
            }],
        };
        let text = serde_yml::to_string(&file).unwrap();
        assert!(!text.contains("client_secret"));
        assert!(!text.contains("scopes"));
        let back: ProvidersFile = serde_yml::from_str(&text).unwrap();
        assert_eq!(back.providers[0].match_host, "github.com");
    }

    #[cfg(unix)]
    #[test]
    fn write_owner_only_tightens_existing_file_to_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mcp_oauth.yaml");
        // Pre-create world-readable to prove an existing file is tightened too.
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_owner_only(&path, b"new").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = serde_yml::from_str::<ProvidersFile>(
            "providers:\n  - match: x\n    client_id: y\n    bogus: 1\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("bogus"), "got: {err}");
    }
}
