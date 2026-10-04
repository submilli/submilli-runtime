//! File-backed secret stores: a backend for credential-shaped state Submilli
//! reads (the blueprint `store:` secret source) or writes (OAuth refresh
//! tokens). Values are opaque UTF-8 strings under a flat, slash-delimited key
//! namespace (e.g. `mcp_oauth/<blueprint>/<server>/credential`).
//!
//! Two file backends, sharing the one-file-per-key-on-disk layout:
//!
//! - [`FileSecretStore`] — encrypted at rest (XChaCha20-Poly1305). The
//!   `submilli-server` uses it; values are **decrypted on demand**, one sealed
//!   file read+opened per lookup, with nothing held decrypted in memory (only
//!   the key/cipher is resident). The `kubernetes` / `aws_secrets_manager`
//!   backends are still open.
//! - [`PlaintextFileSecretStore`] — values stored as raw UTF-8 in `0600` files
//!   under a `0700` directory, no encryption. The CLI's local store: a
//!   per-user-readable file is the honest threat model for a dev machine (an
//!   encryption key kept in an env var or beside the file protects nothing),
//!   matching `aws`/`gcloud`/`vault`/`git credential-store` defaults.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use crate::host::{SecretStore, SecretStoreError};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chacha20poly1305::aead::{Aead, OsRng};
use chacha20poly1305::{AeadCore, KeyInit, XChaCha20Poly1305, XNonce};

/// Length of the XChaCha20-Poly1305 key, in bytes.
const KEY_LEN: usize = 32;
/// Length of the XChaCha20 nonce prefixing each sealed blob, in bytes.
const NONCE_LEN: usize = 24;

/// Where the encryption key is read from. CLI/config name the source — never the
/// key itself. The referenced env var / file holds a base64-encoded 32-byte key.
#[derive(Debug, Clone)]
pub enum KeySource {
    Env(String),
    File(PathBuf),
}

/// Read and decode the 32-byte key the [`KeySource`] points at.
fn load_key(src: &KeySource) -> Result<[u8; KEY_LEN], SecretStoreError> {
    let encoded = match src {
        KeySource::Env(var) => std::env::var(var)
            .map_err(|_| SecretStoreError::KeyConfig(format!("env var `{var}` is not set")))?,
        KeySource::File(path) => fs::read_to_string(path).map_err(|e| {
            SecretStoreError::KeyConfig(format!("reading key file `{}`: {e}", path.display()))
        })?,
    };
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|e| SecretStoreError::Crypto(format!("key is not valid base64: {e}")))?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        SecretStoreError::KeyConfig(format!(
            "key must be {KEY_LEN} bytes (got {}); generate with `head -c32 /dev/urandom | base64`",
            v.len()
        ))
    })
}

/// Encode a (possibly slash-containing) key into a single path segment.
/// URL-safe base64 with no padding never emits `/` or `=`.
fn key_to_filename(key: &str) -> String {
    URL_SAFE_NO_PAD.encode(key.as_bytes())
}

/// Inverse of [`key_to_filename`]. `None` for dot-prefixed temp files and any
/// name that isn't valid URL-safe base64 of UTF-8.
fn filename_to_key(name: &str) -> Option<String> {
    if name.starts_with('.') {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD.decode(name).ok()?;
    String::from_utf8(bytes).ok()
}

/// Encrypted-at-rest, file-per-secret store. One XChaCha20-Poly1305-sealed file
/// per key in `dir`. Reads decrypt on demand; no plaintext is held resident.
pub struct FileSecretStore {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
}

impl FileSecretStore {
    /// Open the store, creating `dir` if absent. No secrets are read here — the
    /// store holds only the key (the cipher); values are decrypted per lookup.
    pub fn open(dir: PathBuf, key_src: &KeySource) -> Result<Self, SecretStoreError> {
        let cipher = cipher_for(key_src)?;
        fs::create_dir_all(&dir).map_err(io)?;
        Ok(Self { dir, cipher })
    }

    /// Read and decrypt the one sealed file for `key`. `Ok(None)` if absent.
    /// The returned plaintext is the caller's to drop; nothing is cached.
    fn read_decrypt(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        let blob = match fs::read(self.dir.join(key_to_filename(key))) {
            Ok(blob) => blob,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        let plain = open_sealed(&self.cipher, &blob)?;
        let value = String::from_utf8(plain)
            .map_err(|e| SecretStoreError::Crypto(format!("value is not utf-8: {e}")))?;
        Ok(Some(value))
    }

    /// Write `bytes` to `file_name` in `dir` via temp file + fsync + atomic
    /// rename, then fsync the directory so the rename is durable.
    fn atomic_write(&self, file_name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!(".{file_name}.tmp"));
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.dir.join(file_name))?;
        self.sync_dir()
    }

    fn sync_dir(&self) -> io::Result<()> {
        File::open(&self.dir)?.sync_all()
    }

    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| SecretStoreError::Crypto(e.to_string()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }
}

/// Split the nonce prefix and decrypt the rest.
fn open_sealed(cipher: &XChaCha20Poly1305, blob: &[u8]) -> Result<Vec<u8>, SecretStoreError> {
    if blob.len() < NONCE_LEN {
        return Err(SecretStoreError::Crypto("sealed blob too short".into()));
    }
    let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|e| SecretStoreError::Crypto(e.to_string()))
}

#[async_trait::async_trait]
impl SecretStore for FileSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        self.read_decrypt(key)
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        let blob = self.seal(value.as_bytes())?;
        self.atomic_write(&key_to_filename(key), &blob).map_err(io)
    }

    async fn delete(&self, key: &str) -> Result<bool, SecretStoreError> {
        match fs::remove_file(self.dir.join(key_to_filename(key))) {
            Ok(()) => self.sync_dir().map(|()| true).map_err(io),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io(e)),
        }
    }

    /// Keys are the (decoded) file names, so listing never decrypts anything.
    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError> {
        let mut keys = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(io)? {
            let name = entry.map_err(io)?.file_name();
            let Some(key) = name.to_str().and_then(filename_to_key) else {
                continue;
            };
            if prefix.is_none_or(|p| key.starts_with(p)) {
                keys.push(key);
            }
        }
        keys.sort();
        Ok(keys)
    }
}

/// Whether the key a [`KeySource`] points at can be read and used, without
/// touching any store. For a boot that must not change state before it knows
/// every setting is acceptable.
pub fn check_key(key_src: &KeySource) -> Result<(), SecretStoreError> {
    cipher_for(key_src).map(|_| ())
}

fn cipher_for(key_src: &KeySource) -> Result<XChaCha20Poly1305, SecretStoreError> {
    let key = load_key(key_src)?;
    XChaCha20Poly1305::new_from_slice(&key).map_err(|e| SecretStoreError::Crypto(e.to_string()))
}

fn io(e: io::Error) -> SecretStoreError {
    SecretStoreError::Io(e.to_string())
}

/// Plaintext, file-per-secret store for local CLI use. One raw-UTF-8 file per
/// key in `dir`, written `0600` under a `0700` directory. Shares the
/// URL-safe-base64 filename scheme with [`FileSecretStore`] so the key
/// namespace (and `list` semantics) match.
pub struct PlaintextFileSecretStore {
    dir: PathBuf,
}

impl PlaintextFileSecretStore {
    /// Open the store, creating `dir` (and parents) if absent. The directory is
    /// tightened to owner-only (`0700`) on unix.
    pub fn open(dir: PathBuf) -> Result<Self, SecretStoreError> {
        fs::create_dir_all(&dir).map_err(io)?;
        restrict_dir(&dir).map_err(io)?;
        Ok(Self { dir })
    }

    /// Write `bytes` to `file_name` via temp file (created `0600`) + fsync +
    /// atomic rename, then fsync the directory so the rename is durable.
    fn atomic_write(&self, file_name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!(".{file_name}.tmp"));
        let mut file = create_owner_only(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.dir.join(file_name))?;
        File::open(&self.dir)?.sync_all()
    }
}

#[async_trait::async_trait]
impl SecretStore for PlaintextFileSecretStore {
    async fn get(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        match fs::read_to_string(self.dir.join(key_to_filename(key))) {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(e)),
        }
    }

    async fn put(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.atomic_write(&key_to_filename(key), value.as_bytes())
            .map_err(io)
    }

    async fn delete(&self, key: &str) -> Result<bool, SecretStoreError> {
        match fs::remove_file(self.dir.join(key_to_filename(key))) {
            Ok(()) => File::open(&self.dir)
                .and_then(|d| d.sync_all())
                .map(|()| true)
                .map_err(io),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io(e)),
        }
    }

    async fn list(&self, prefix: Option<&str>) -> Result<Vec<String>, SecretStoreError> {
        let mut keys = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(io)? {
            let name = entry.map_err(io)?.file_name();
            let Some(key) = name.to_str().and_then(filename_to_key) else {
                continue;
            };
            if prefix.is_none_or(|p| key.starts_with(p)) {
                keys.push(key);
            }
        }
        keys.sort();
        Ok(keys)
    }
}

/// Create a file for writing, owner-read/write only (`0600`) on unix.
fn create_owner_only(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        File::create(path)
    }
}

/// Tighten a directory to owner-only (`0700`) on unix; best-effort elsewhere.
fn restrict_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed, non-secret test key (32 bytes of 0x07), base64-encoded.
    fn test_key_b64() -> String {
        STANDARD.encode([7u8; KEY_LEN])
    }

    /// A store over a fresh temp dir, keyed from a key file inside it.
    fn temp_store() -> (tempfile::TempDir, FileSecretStore) {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        fs::write(&key_path, test_key_b64()).unwrap();
        let store =
            FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn plaintext_round_trip_put_get_list_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let store = PlaintextFileSecretStore::open(tmp.path().join("secrets")).unwrap();
        let key = "mcp_oauth/blueprint/server/credential";

        store.put(key, "tok-123").await.unwrap();
        assert_eq!(store.get(key).await.unwrap().as_deref(), Some("tok-123"));
        store.put("other/key", "x").await.unwrap();

        let mcp_keys = store.list(Some("mcp_oauth/")).await.unwrap();
        assert_eq!(mcp_keys, vec![key.to_string()]);
        let all = store.list(None).await.unwrap();
        assert_eq!(all, vec![key.to_string(), "other/key".to_string()]);

        assert!(store.delete(key).await.unwrap());
        assert_eq!(store.get(key).await.unwrap(), None);
        // Deleting an absent key is a no-op.
        assert!(!store.delete(key).await.unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn plaintext_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("secrets");
        let store = PlaintextFileSecretStore::open(dir.clone()).unwrap();
        store.put("k", "v").await.unwrap();

        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700, "directory must be owner-only");
        let file = dir.join(key_to_filename("k"));
        let file_mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600, "secret file must be owner-only");
    }

    #[tokio::test]
    async fn round_trip_put_get_list_delete() {
        let (_tmp, store) = temp_store();
        let key = "mcp/blueprint/server/refresh_token";

        store.put(key, "tok-123").await.unwrap();
        assert_eq!(store.get(key).await.unwrap().as_deref(), Some("tok-123"));
        store.put("other/key", "x").await.unwrap();

        let mcp_keys = store.list(Some("mcp/")).await.unwrap();
        assert_eq!(mcp_keys, vec![key.to_string()]);
        let all = store.list(None).await.unwrap();
        assert_eq!(all, vec![key.to_string(), "other/key".to_string()]);

        assert!(store.delete(key).await.unwrap());
        assert_eq!(store.get(key).await.unwrap(), None);
        // Deleting an absent key is a no-op.
        assert!(!store.delete(key).await.unwrap());
    }

    #[tokio::test]
    async fn persists_across_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        fs::write(&key_path, test_key_b64()).unwrap();
        let dir = tmp.path().join("secrets");

        {
            let store =
                FileSecretStore::open(dir.clone(), &KeySource::File(key_path.clone())).unwrap();
            store.put("api/token", "sealed-value").await.unwrap();
        }
        let reopened = FileSecretStore::open(dir, &KeySource::File(key_path)).unwrap();
        assert_eq!(
            reopened.get("api/token").await.unwrap().as_deref(),
            Some("sealed-value")
        );
    }

    #[tokio::test]
    async fn get_reads_on_demand() {
        let (_tmp, store) = temp_store();
        store.put("k", "v").await.unwrap();
        assert_eq!(store.get("k").await.unwrap().as_deref(), Some("v"));
        assert_eq!(store.get("absent").await.unwrap(), None);
    }

    #[test]
    fn seal_open_round_trips() {
        let (_tmp, store) = temp_store();
        let blob = store.seal(b"hello").unwrap();
        assert_eq!(open_sealed(&store.cipher, &blob).unwrap(), b"hello");
    }

    #[test]
    fn open_rejects_truncated_and_tampered() {
        let (_tmp, store) = temp_store();
        assert!(matches!(
            open_sealed(&store.cipher, &[0u8; NONCE_LEN - 1]),
            Err(SecretStoreError::Crypto(_))
        ));
        let mut blob = store.seal(b"hello").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xff;
        assert!(matches!(
            open_sealed(&store.cipher, &blob),
            Err(SecretStoreError::Crypto(_))
        ));
    }

    #[test]
    fn filename_round_trips_without_slash() {
        let key = "mcp/bp/server/refresh_token";
        let name = key_to_filename(key);
        assert!(!name.contains('/'));
        assert!(!name.contains('='));
        assert_eq!(filename_to_key(&name).as_deref(), Some(key));
        assert_eq!(filename_to_key(".secrets.tmp"), None);
    }

    #[test]
    fn load_key_rejects_bad_keys() {
        let tmp = tempfile::tempdir().unwrap();

        let missing = KeySource::Env("SUB_SECRET_KEY_DEFINITELY_UNSET".into());
        assert!(matches!(
            load_key(&missing),
            Err(SecretStoreError::KeyConfig(_))
        ));

        let not_b64 = tmp.path().join("bad.b64");
        fs::write(&not_b64, "not base64!!!").unwrap();
        assert!(matches!(
            load_key(&KeySource::File(not_b64)),
            Err(SecretStoreError::Crypto(_))
        ));

        let short = tmp.path().join("short.b64");
        fs::write(&short, STANDARD.encode([1u8; 16])).unwrap();
        assert!(matches!(
            load_key(&KeySource::File(short)),
            Err(SecretStoreError::KeyConfig(_))
        ));

        let good = tmp.path().join("good.b64");
        fs::write(&good, test_key_b64()).unwrap();
        assert_eq!(load_key(&KeySource::File(good)).unwrap(), [7u8; KEY_LEN]);
    }
}
