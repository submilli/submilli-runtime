//! SSH identities for package fetches: the server's configured key file, and
//! the local user's ssh-agent plus default key files.

use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine;
use ssh_key::private::Ed25519Keypair;
use ssh_key::public::KeyData;
use ssh_key::{Cipher, HashAlg, PrivateKey, PublicKey};
use zeroize::Zeroizing;

/// Bounds a key file read; real keys are a few kilobytes.
const MAX_KEY_BYTES: u64 = 64 * 1024;

/// DER prefix of a PKCS#8 v1 ed25519 private key — the form Helm's
/// `genPrivateKey "ed25519"` and `openssl genpkey -algorithm ed25519` emit —
/// followed by the 32-byte seed.
const PKCS8_ED25519_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

const KEYGEN_HINT: &str = "use an unencrypted OpenSSH key (`ssh-keygen -t ed25519 -N \"\" -f \
     <file>`), or rewrite an existing key without a passphrase with `ssh-keygen -p -N \"\" -f \
     <file>`";

/// An SSH key or known_hosts file that could not be used, with the reason and
/// a fix.
#[derive(Debug)]
pub struct SshFileError(String);

impl SshFileError {
    pub(crate) fn new(message: String) -> Self {
        Self(message)
    }
}

impl fmt::Display for SshFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SshFileError {}

/// The server's package-install SSH key, loaded once at boot. The private key
/// is only ever handed to libgit2; [`Self::public_key`] is what operators add
/// to GitHub as a deploy key.
pub struct ServerSshKey {
    path: PathBuf,
    private_key: Zeroizing<String>,
    public_key: String,
    fingerprint: String,
}

impl fmt::Debug for ServerSshKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerSshKey")
            .field("path", &self.path)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl ServerSshKey {
    /// Read and validate an unencrypted private key: an OpenSSH key of any
    /// type, or a PKCS#8 ed25519 key (what the Helm chart generates).
    pub fn load(path: &Path) -> Result<Self, SshFileError> {
        let private_key = read_key(path).map_err(|err| {
            SshFileError::new(format!("reading SSH key {}: {err}", path.display()))
        })?;
        let public = public_key_of(&private_key)
            .map_err(|reason| SshFileError::new(format!("SSH key {}: {reason}", path.display())))?;
        let public_key = public.to_openssh().map_err(|err| {
            SshFileError::new(format!(
                "SSH key {}: encoding its public key: {err}",
                path.display()
            ))
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            private_key,
            public_key,
            fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The OpenSSH public-key line (`ssh-ed25519 AAAA…`).
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    /// `SHA256:…`, as GitHub shows deploy keys.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn private_key(&self) -> &str {
        &self.private_key
    }
}

fn public_key_of(text: &str) -> Result<PublicKey, String> {
    let text = text.trim();
    if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
        let key = PrivateKey::from_openssh(text)
            .map_err(|err| format!("not a valid OpenSSH private key ({err}); {KEYGEN_HINT}"))?;
        if key.cipher() != Cipher::None {
            return Err(format!("the key is passphrase-protected; {KEYGEN_HINT}"));
        }
        return Ok(key.public_key().clone());
    }
    if text.starts_with("-----BEGIN PRIVATE KEY-----") {
        let seed = pkcs8_ed25519_seed(text)?;
        let keypair = Ed25519Keypair::from_seed(&seed);
        return Ok(PublicKey::new(KeyData::Ed25519(keypair.public), ""));
    }
    if is_encrypted_pem(text) {
        return Err(format!("the key is passphrase-protected; {KEYGEN_HINT}"));
    }
    Err(format!(
        "unsupported key format (expected an OpenSSH or PKCS#8 ed25519 private key); {KEYGEN_HINT}"
    ))
}

fn pkcs8_ed25519_seed(pem: &str) -> Result<Zeroizing<[u8; 32]>, String> {
    let body: Zeroizing<String> = Zeroizing::new(
        pem.lines()
            .filter(|line| !line.starts_with("-----"))
            .map(str::trim)
            .collect(),
    );
    let der = Zeroizing::new(
        base64::engine::general_purpose::STANDARD
            .decode(body.as_bytes())
            .map_err(|err| format!("not a valid PKCS#8 key ({err}); {KEYGEN_HINT}"))?,
    );
    let seed_bytes = der
        .strip_prefix(&PKCS8_ED25519_PREFIX[..])
        .filter(|seed| seed.len() == 32)
        .ok_or_else(|| format!("only ed25519 keys are supported in PKCS#8 form; {KEYGEN_HINT}"))?;
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(seed_bytes);
    Ok(seed)
}

/// Whether a private key file needs a passphrase. Unrecognized formats read as
/// unencrypted; libssh2 then reports the real problem.
pub(crate) fn key_file_is_encrypted(text: &str) -> bool {
    let text = text.trim();
    if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
        return PrivateKey::from_openssh(text).is_ok_and(|key| key.cipher() != Cipher::None);
    }
    is_encrypted_pem(text)
}

fn is_encrypted_pem(text: &str) -> bool {
    text.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----")
        || text
            .lines()
            .take(3)
            .any(|line| line.starts_with("Proc-Type:") && line.contains("ENCRYPTED"))
}

pub(crate) fn read_key(path: &Path) -> std::io::Result<Zeroizing<String>> {
    read_bounded(path, MAX_KEY_BYTES)
}

/// Read at most `limit` bytes of a text file, zeroized on drop.
pub(crate) fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Zeroizing<String>> {
    let mut text = Zeroizing::new(String::new());
    std::fs::File::open(path)?
        .take(limit)
        .read_to_string(&mut text)?;
    Ok(text)
}

/// Asks the person at the terminal for a key's passphrase.
pub trait PassphrasePrompt: Sync {
    /// Ask for the passphrase protecting `key_path`; `retry` is set after a
    /// wrong one. `None` skips the key.
    fn passphrase(&self, key_path: &Path, retry: bool) -> Option<Zeroizing<String>>;
}

/// One local identity to offer GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalIdentity {
    Agent,
    KeyFile(PathBuf),
}

impl LocalIdentity {
    pub(crate) fn describe(&self) -> String {
        match self {
            LocalIdentity::Agent => "ssh-agent".to_string(),
            LocalIdentity::KeyFile(path) => path.display().to_string(),
        }
    }
}

/// The local user's SSH identities, tried in order: ssh-agent, then
/// `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`. Remembers the identity GitHub
/// accepted and any passphrase entered, so one install prompts at most once
/// per key even across recursive dependency fetches.
pub struct LocalIdentities<'a> {
    use_agent: bool,
    key_files: Vec<PathBuf>,
    prompt: Option<&'a dyn PassphrasePrompt>,
    state: Mutex<LocalState>,
}

#[derive(Default)]
struct LocalState {
    accepted: Option<LocalIdentity>,
    passphrases: HashMap<PathBuf, Zeroizing<String>>,
}

impl<'a> LocalIdentities<'a> {
    /// The agent when one is reachable (`SSH_AUTH_SOCK`, or Pageant/the
    /// OpenSSH agent pipe on Windows) and the default key files.
    pub fn user_default(prompt: Option<&'a dyn PassphrasePrompt>) -> Self {
        let use_agent =
            cfg!(windows) || std::env::var_os("SSH_AUTH_SOCK").is_some_and(|sock| !sock.is_empty());
        let ssh_dir = std::env::home_dir().map(|home| home.join(".ssh"));
        let key_files = ssh_dir
            .map(|dir| {
                ["id_ed25519", "id_ecdsa", "id_rsa"]
                    .iter()
                    .map(|name| dir.join(name))
                    .collect()
            })
            .unwrap_or_default();
        Self::new(use_agent, key_files, prompt)
    }

    pub(crate) fn new(
        use_agent: bool,
        key_files: Vec<PathBuf>,
        prompt: Option<&'a dyn PassphrasePrompt>,
    ) -> Self {
        Self {
            use_agent,
            key_files,
            prompt,
            state: Mutex::new(LocalState::default()),
        }
    }

    /// Identities to try, the previously accepted one first. Key files that
    /// don't exist are left out.
    pub(crate) fn candidates(&self) -> Vec<LocalIdentity> {
        let mut candidates = Vec::new();
        if self.use_agent {
            candidates.push(LocalIdentity::Agent);
        }
        candidates.extend(
            self.key_files
                .iter()
                .filter(|path| path.is_file())
                .cloned()
                .map(LocalIdentity::KeyFile),
        );
        if let Some(accepted) = self.state().accepted.clone()
            && let Some(index) = candidates.iter().position(|c| *c == accepted)
        {
            let accepted = candidates.remove(index);
            candidates.insert(0, accepted);
        }
        candidates
    }

    pub(crate) fn remember_accepted(&self, identity: &LocalIdentity) {
        self.state().accepted = Some(identity.clone());
    }

    /// The passphrase for an encrypted key file: remembered, or asked for.
    /// `retry` forgets a remembered one and asks again.
    pub(crate) fn passphrase(&self, path: &Path, retry: bool) -> Option<Zeroizing<String>> {
        if !retry && let Some(known) = self.state().passphrases.get(path) {
            return Some(known.clone());
        }
        let entered = self.prompt?.passphrase(path, retry)?;
        self.state()
            .passphrases
            .insert(path.to_path_buf(), entered.clone());
        Some(entered)
    }

    pub(crate) fn can_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    fn state(&self) -> MutexGuard<'_, LocalState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENSSH_ED25519: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDTHcatq+9yqfpryRguKL1sVfxXJ1/upvYEc/WmhCZ7qAAAAJAuGBB+LhgQ
fgAAAAtzc2gtZWQyNTUxOQAAACDTHcatq+9yqfpryRguKL1sVfxXJ1/upvYEc/WmhCZ7qA
AAAEAr/vTBoF3h62GstXv8iBWf7O4dw4NPGkFHERQt5ZCNydMdxq2r73Kp+mvJGC4ovWxV
/FcnX+6m9gRz9aaEJnuoAAAAB2ZpeHR1cmUBAgMEBQY=
-----END OPENSSH PRIVATE KEY-----
";
    const OPENSSH_ED25519_FINGERPRINT: &str = "SHA256:aZ1HE6mrJoO1t7RlHQovnKkofKLpoXO5kfcLh+VGs3A";
    const OPENSSH_ED25519_ENCRYPTED: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABDGp5Sl88
GD9nNJ/uWidNpVAAAAGAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAICO4o1XGQVi1ddDc
ZJAw/NAzPukXW2URrJNb5DcvzHT5AAAAkDmzyankaQf4dSNZg7TC+GXE9d6rr44GY6o8xM
/jMw7aAM3iIqb+xt7aw89iaLiWhM0jXdNo7ABXsjsb8c7urwj/hMvo35mEQ6GRc9pw/YMZ
mq/Ci0SBE0xhpkXUpWF6qMUsWay8WZDMgpZZe6Wu6kGSNx1xMQ7J0XQlHC3/3396Iu1nmD
cpuQW8Jb5GNpYZvA==
-----END OPENSSH PRIVATE KEY-----
";

    fn write_key(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn loads_openssh_and_pkcs8_ed25519_keys() {
        let dir = tempfile::tempdir().unwrap();
        let key = ServerSshKey::load(&write_key(dir.path(), "id", OPENSSH_ED25519)).unwrap();
        assert!(
            key.public_key()
                .starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5")
        );
        assert!(key.fingerprint().starts_with("SHA256:"));

        assert_eq!(key.fingerprint(), OPENSSH_ED25519_FINGERPRINT);

        // A Helm-style PKCS#8 encoding of the same seed must derive the same
        // public key.
        let openssh = PrivateKey::from_openssh(OPENSSH_ED25519).unwrap();
        let seed = openssh.key_data().ed25519().unwrap().private.to_bytes();
        let mut der = PKCS8_ED25519_PREFIX.to_vec();
        der.extend_from_slice(&seed);
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            base64::engine::general_purpose::STANDARD.encode(der)
        );
        let pkcs8 = ServerSshKey::load(&write_key(dir.path(), "pkcs8", &pem)).unwrap();
        assert_eq!(pkcs8.fingerprint(), key.fingerprint());
        assert_eq!(
            pkcs8.public_key().split(' ').nth(1),
            key.public_key().split(' ').nth(1)
        );
        assert!(
            !format!("{pkcs8:?}").contains("PRIVATE"),
            "Debug redacts the key"
        );
    }

    #[test]
    fn rejects_encrypted_unsupported_and_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let encrypted = "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\nDEK-Info: AES-128-CBC,00\n\nAAAA\n-----END RSA PRIVATE KEY-----\n";
        let err = ServerSshKey::load(&write_key(dir.path(), "enc", encrypted)).unwrap_err();
        assert!(err.to_string().contains("passphrase-protected"), "{err}");
        assert!(key_file_is_encrypted(encrypted));
        assert!(!key_file_is_encrypted(OPENSSH_ED25519));
        assert!(key_file_is_encrypted(OPENSSH_ED25519_ENCRYPTED));
        let err = ServerSshKey::load(&write_key(dir.path(), "enc2", OPENSSH_ED25519_ENCRYPTED))
            .unwrap_err();
        assert!(err.to_string().contains("passphrase-protected"), "{err}");

        let err = ServerSshKey::load(&write_key(dir.path(), "junk", "garbage")).unwrap_err();
        assert!(err.to_string().contains("unsupported key format"), "{err}");

        let err = ServerSshKey::load(&dir.path().join("missing")).unwrap_err();
        assert!(err.to_string().contains("reading SSH key"), "{err}");
    }

    #[test]
    fn candidates_put_the_accepted_identity_first_and_skip_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let ed = write_key(dir.path(), "id_ed25519", OPENSSH_ED25519);
        let rsa = write_key(dir.path(), "id_rsa", OPENSSH_ED25519);
        let identities = LocalIdentities::new(
            true,
            vec![ed.clone(), dir.path().join("id_ecdsa"), rsa.clone()],
            None,
        );
        assert_eq!(
            identities.candidates(),
            vec![
                LocalIdentity::Agent,
                LocalIdentity::KeyFile(ed.clone()),
                LocalIdentity::KeyFile(rsa.clone()),
            ]
        );
        identities.remember_accepted(&LocalIdentity::KeyFile(rsa.clone()));
        assert_eq!(identities.candidates()[0], LocalIdentity::KeyFile(rsa));
    }

    struct CountingPrompt(std::sync::atomic::AtomicUsize);

    impl PassphrasePrompt for CountingPrompt {
        fn passphrase(&self, _key_path: &Path, _retry: bool) -> Option<Zeroizing<String>> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(Zeroizing::new("secret".to_string()))
        }
    }

    #[test]
    fn passphrases_are_asked_once_unless_retrying() {
        let prompt = CountingPrompt(std::sync::atomic::AtomicUsize::new(0));
        let identities = LocalIdentities::new(false, Vec::new(), Some(&prompt));
        let path = Path::new("/home/me/.ssh/id_ed25519");
        assert_eq!(
            identities
                .passphrase(path, false)
                .as_deref()
                .map(String::as_str),
            Some("secret")
        );
        identities.passphrase(path, false);
        assert_eq!(prompt.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        identities.passphrase(path, true);
        assert_eq!(prompt.0.load(std::sync::atomic::Ordering::SeqCst), 2);

        let silent = LocalIdentities::new(false, Vec::new(), None);
        assert!(silent.passphrase(path, false).is_none());
    }
}
