//! Explicit approval of server public keys, scoped to a hostname and port.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use submilli_shared::tls::{CertificateInfo, Verifier, client_config};
use url::Url;

const MAX_STORE_BYTES: u64 = 1024 * 1024;

#[derive(Subcommand)]
pub enum TrustCmd {
    /// Inspect and approve a server public key. No token is sent.
    Add(AddArgs),
    /// List approved server public keys in this Submilli home.
    List,
    /// Remove a saved public key before approving a verified replacement.
    Remove(RemoveArgs),
}

#[derive(clap::Args)]
pub struct AddArgs {
    /// HTTPS server URL whose public key should be approved.
    #[arg(long, env = "SUBMILLI_SERVER_URL", value_name = "URL")]
    server: String,
    /// Independently obtained SHA-256 SPKI fingerprint (sha256:<64 hex digits>).
    /// Matching it approves trust without an interactive prompt.
    #[arg(long, value_name = "SHA256")]
    fingerprint: Option<String>,
}

#[derive(clap::Args)]
pub struct RemoveArgs {
    /// HTTPS server URL whose saved key should be removed.
    #[arg(long, env = "SUBMILLI_SERVER_URL", value_name = "URL")]
    server: String,
}

pub fn execute(cmd: TrustCmd) -> Result<ExitCode> {
    let store = Store::default();
    match cmd {
        TrustCmd::Add(args) => {
            let url = https_url(&args.server)?;
            let authority = authority(&url)?;
            let existing = store.read()?.pins.get(&authority).cloned();
            let expected = args
                .fingerprint
                .as_deref()
                .map(normalize_fingerprint)
                .transpose()?;
            let info = discover(&url)?;
            if let Some(existing) = &existing
                && existing != &info.fingerprint
            {
                bail!(
                    "saved key for {authority} differs; verify the change, then run `submilli server trust remove --server {url}` first"
                );
            }
            approve(&url, &info, expected.as_deref())?;
            store.add(&authority, &info.fingerprint)?;
            println!("Trusted {authority} {}", info.fingerprint);
        }
        TrustCmd::List => {
            for (authority, fingerprint) in store.read()?.pins {
                println!("{authority} {fingerprint}");
            }
        }
        TrustCmd::Remove(args) => {
            let authority = authority(&https_url(&args.server)?)?;
            store.remove(&authority)?;
            println!("Removed trust for {authority}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Called before constructing an authenticated client. The discovery handshake
/// carries neither an HTTP request nor a token; actual requests verify again.
pub fn prepare(base: &str) -> Result<Option<Arc<rustls::ClientConfig>>> {
    let url = Url::parse(base).context("invalid server URL")?;
    if url.scheme() == "http" {
        return Ok(None);
    }
    let url = https_url(base)?;
    let authority = authority(&url)?;
    let store = Store::default();
    let pin = store.read()?.pins.get(&authority).cloned();
    if let Some(pin) = pin {
        return Ok(Some(client_config(Verifier::new(Some(pin), false)?)?));
    }
    let info = discover(&url)?;
    if info.publicly_trusted {
        return Ok(Some(client_config(Verifier::new(None, false)?)?));
    }
    approve(&url, &info, None)?;
    store.add(&authority, &info.fingerprint)?;
    Ok(Some(client_config(Verifier::new(
        Some(info.fingerprint),
        false,
    )?)?))
}

fn approve(url: &Url, info: &CertificateInfo, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if expected != info.fingerprint {
            bail!(
                "server fingerprint mismatch: expected {expected}, received {}",
                info.fingerprint
            );
        }
        return Ok(());
    }
    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        bail!(
            "server certificate is not approved; verify its public-key fingerprint independently, then run `submilli server trust add --server {url} --fingerprint {}`",
            info.fingerprint
        );
    }
    eprintln!(
        "Server: {}\nCertificate names: {}\nValid: {}\nPublic-key fingerprint: {}\nVerify this fingerprint with the server operator before approving.",
        authority(url)?,
        info.names,
        info.validity,
        info.fingerprint
    );
    if !dialoguer::Confirm::new()
        .with_prompt("Trust this server public key?")
        .default(false)
        .interact()?
    {
        bail!("server certificate trust declined");
    }
    Ok(())
}

fn discover(url: &Url) -> Result<CertificateInfo> {
    let verifier = Verifier::new(None, true)?;
    submilli_shared::tls::discover(url.as_str(), verifier).with_context(|| {
        format!(
            "verifying TLS certificate for {}",
            url.host_str().unwrap_or("server")
        )
    })
}

fn https_url(base: &str) -> Result<Url> {
    let url = Url::parse(base).context("invalid server URL")?;
    if url.scheme() != "https" {
        bail!("certificate trust requires an https server URL");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("server URL must not contain credentials");
    }
    authority(&url)?;
    Ok(url)
}

fn authority(url: &Url) -> Result<String> {
    Ok(format!(
        "{}:{}",
        url.host_str().context("server URL has no host")?,
        url.port_or_known_default()
            .context("server URL has no port")?
    ))
}

fn normalize_fingerprint(value: &str) -> Result<String> {
    let value = value.to_ascii_lowercase();
    let digest = value
        .strip_prefix("sha256:")
        .context("fingerprint must start with sha256:")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("fingerprint must contain 64 hexadecimal digits after sha256:");
    }
    Ok(value)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustFile {
    version: u32,
    pins: BTreeMap<String, String>,
}

impl Default for TrustFile {
    fn default() -> Self {
        Self {
            version: 1,
            pins: BTreeMap::new(),
        }
    }
}

struct Store {
    path: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            path: submilli_build::default_data_root().join("server-trust.json"),
        }
    }
}

impl Store {
    fn read(&self) -> Result<TrustFile> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TrustFile::default());
            }
            Err(error) => return Err(error).context("opening server trust store"),
        };
        let mut bytes = Vec::new();
        file.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            bail!("server trust store exceeds 1 MiB");
        }
        let mut trust: TrustFile = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "reading trust store {}; repair it before connecting",
                self.path.display()
            )
        })?;
        if trust.version != 1 {
            bail!("unsupported server trust store version {}", trust.version);
        }
        for (host, pin) in &mut trust.pins {
            *pin = normalize_fingerprint(pin)
                .with_context(|| format!("invalid saved fingerprint for {host}"))?;
        }
        Ok(trust)
    }

    fn add(&self, authority: &str, pin: &str) -> Result<()> {
        self.update(|trust| {
            if let Some(existing) = trust.pins.get(authority) && existing != pin {
                bail!("saved public key for {authority} changed; remove its trust before adding a verified replacement");
            }
            trust.pins.insert(authority.to_owned(), pin.to_owned());
            Ok(())
        })
    }

    fn remove(&self, authority: &str) -> Result<()> {
        self.update(|trust| {
            if trust.pins.remove(authority).is_none() {
                bail!("no saved trust for {authority}");
            }
            Ok(())
        })
    }

    fn update(&self, change: impl FnOnce(&mut TrustFile) -> Result<()>) -> Result<()> {
        let directory = self
            .path
            .parent()
            .context("trust store has no parent directory")?;
        std::fs::create_dir_all(directory)?;
        let lock = open_lock(&directory.join("server-trust.lock"))?;
        lock.lock().context("locking server trust store")?;
        let mut trust = self.read()?;
        change(&mut trust)?;
        let bytes = serde_json::to_vec_pretty(&trust)?;
        if bytes.len() as u64 > MAX_STORE_BYTES {
            bail!("server trust store exceeds 1 MiB");
        }
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .context("saving server trust store")?;
        // Closing the lock releases it even on a failed read, change, or write.
        drop(lock);
        Ok(())
    }
}

fn open_lock(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).context("opening server trust lock")
}

#[cfg(test)]
mod tests;
