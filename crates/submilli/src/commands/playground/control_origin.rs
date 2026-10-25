//! The project's browser origin survives shutdown independently of the instance record.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use super::state;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedOrigin {
    version: u32,
    port: u16,
}

/// Called while the host holds the project's instance lock. A saved port is never
/// silently replaced: that would strand open tabs and their origin-scoped sign-in.
pub(crate) async fn bind(state_root: &Path) -> Result<TcpListener> {
    let path = state_root.join("control-origin.json");
    let saved = state::read_private_bounded(&path, 1024)
        .with_context(|| format!("reading the saved Playground URL in {}", path.display()))?;
    let port = match saved.as_deref() {
        Some(text) => {
            let origin: SavedOrigin = serde_json::from_str(text).with_context(|| {
                format!("{} is not a valid saved Playground URL; restore this record while the Playground is stopped", path.display())
            })?;
            if origin.version != 1 || origin.port == 0 {
                bail!(
                    "{} has an unsupported or invalid saved Playground URL; restore this record while the Playground is stopped",
                    path.display()
                );
            }
            origin.port
        }
        None => 0,
    };
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
        .await
        .with_context(|| {
            if port == 0 {
                "binding the Playground page on loopback".to_owned()
            } else {
                format!("cannot bind the saved Playground URL http://127.0.0.1:{port}/; if another process is using this port, stop it and try again; the Playground URL has not changed")
            }
        })?;
    if saved.is_none() {
        let origin = SavedOrigin {
            version: 1,
            port: listener.local_addr()?.port(),
        };
        state::write_private(&path, &serde_json::to_vec(&origin)?)
            .with_context(|| format!("saving the Playground URL in {}", path.display()))?;
        // Sync the rename before announcing a URL that a crash must not change.
        std::fs::File::open(state_root)?.sync_all()?;
    }
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[tokio::test]
    async fn saved_origin_is_private_stable_and_refuses_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let first = bind(dir.path()).await.unwrap();
        let address = first.local_addr().unwrap();
        assert_eq!(address.ip(), Ipv4Addr::LOCALHOST);
        let path = dir.path().join("control-origin.json");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let original = std::fs::read(&path).unwrap();
        let error = bind(dir.path()).await.unwrap_err();
        assert!(error.to_string().contains("stop it and try again"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        drop(first);
        let restarted = bind(dir.path()).await.unwrap();
        assert_eq!(restarted.local_addr().unwrap(), address);
        let other = tempfile::tempdir().unwrap();
        let separate = bind(other.path()).await.unwrap();
        assert_ne!(separate.local_addr().unwrap(), address);
    }

    #[tokio::test]
    async fn invalid_or_unsafe_records_do_not_choose_another_origin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("control-origin.json");
        for text in [
            "broken",
            r#"{"version":1,"port":0}"#,
            r#"{"version":2,"port":12345}"#,
            r#"{"version":1,"port":65536}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(bind(dir.path()).await.is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        std::fs::write(&path, "x".repeat(1025)).unwrap();
        assert!(bind(dir.path()).await.is_err());
        std::fs::remove_file(&path).unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, r#"{"version":1,"port":12345}"#).unwrap();
        symlink(&target, &path).unwrap();
        assert!(bind(dir.path()).await.is_err());
    }
}
