//! The SSH transport: git protocol over SSH through libgit2 (git2 with
//! vendored libgit2/libssh2), so no `ssh` or `git` executable is needed.
//!
//! Every connection offers exactly one identity. libgit2 calls the credentials
//! callback again after a rejection; the callback refuses, and the next
//! identity gets a fresh connection. That keeps a fatal libssh2 error for one
//! identity (an agent with no keys, a wrong passphrase) from ending the search,
//! and makes the attempt count bounded by the identity list.
//!
//! Host keys are verified by [`KnownHosts`] in the `certificate_check`
//! callback; there is no trust-on-first-use and no prompt. libgit2 never reads
//! anyone's `~/.ssh`: see [`Libgit2Guard`].

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use git2::{
    Binding, CertificateCheckStatus, Cred, CredentialType, Direction, ErrorClass, FetchOptions,
    ObjectType, Oid, RemoteCallbacks, Repository,
};
use tempfile::TempDir;
use zeroize::Zeroizing;

use super::keys::{self, LocalIdentity};
use super::{
    FetchAuth, GithubError, KnownHosts, LocalIdentities, MAX_SOURCE_BYTES, MAX_SOURCE_ENTRIES,
    Result, ServerSshKey,
};

const CONNECT_TIMEOUT_MS: i32 = 30_000;
const IO_TIMEOUT_MS: i32 = 60_000;
/// Overall bound on one resolve or fetch, enforced from progress callbacks.
const OPERATION_DEADLINE: Duration = Duration::from_secs(600);
/// How to give a server an SSH identity for package installs.
pub const SSH_NOT_CONFIGURED_HINT: &str = "set `package_ssh_key_file` in the server config (the \
     Helm chart sets it), then add the public key that `submilli server packages ssh-key` prints \
     as a deploy key on the repository";

/// Longest file path and file name a built tarball may hold; longer ones
/// could not be extracted on any supported file system.
const MAX_PATH_BYTES: usize = 4096;
const MAX_NAME_BYTES: usize = 255;
/// Bounds the tree objects one walk inflates. Real trees take tens of bytes
/// per entry, so this is far beyond any repository within the entry cap.
const MAX_TREE_BYTES: u64 = 64 * 1024 * 1024;
/// Bounds the commit object read before its tree.
const MAX_COMMIT_BYTES: u64 = 16 * 1024 * 1024;
const GITHUB_SSH_USER: &str = "git";
const GITHUB_SSH_HOST: &str = "github.com";

/// Where an SSH operation connects, and for how long it may run. Production
/// always targets `git@github.com:22` with [`OPERATION_DEADLINE`]; tests point
/// it at a local `sshd`.
#[derive(Clone, Debug)]
pub(crate) struct Endpoint {
    pub(crate) user: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
    pub(crate) deadline: Duration,
}

impl Endpoint {
    fn github(org: &str, repo: &str) -> Self {
        Self {
            user: GITHUB_SSH_USER.to_string(),
            host: GITHUB_SSH_HOST.to_string(),
            port: 22,
            path: format!("{org}/{repo}.git"),
            deadline: OPERATION_DEADLINE,
        }
    }

    fn url(&self) -> String {
        format!(
            "ssh://{}@{}:{}/{}",
            self.user, self.host, self.port, self.path
        )
    }

    /// The name `known_hosts` files record for this host.
    fn known_hosts_name(&self) -> String {
        if self.port == 22 {
            self.host.clone()
        } else {
            format!("[{}]:{}", self.host, self.port)
        }
    }

    fn display(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.path)
    }
}

/// Resolve `git_ref` (default branch when `None`) to a commit SHA by listing
/// the remote's refs.
pub(super) fn resolve_ref(
    org: &str,
    repo: &str,
    git_ref: Option<&str>,
    auth: &FetchAuth<'_>,
) -> Result<String> {
    resolve_ref_at(&Endpoint::github(org, repo), git_ref, auth)
}

/// Fetch the commit at `sha` and return a deterministic, uncompressed tarball
/// of its tree under a `<repo>-<sha>/` prefix, matching codeload's layout.
pub(super) fn download_tarball(
    org: &str,
    repo: &str,
    sha: &str,
    auth: &FetchAuth<'_>,
) -> Result<Vec<u8>> {
    download_tarball_at(
        &Endpoint::github(org, repo),
        &format!("{repo}-{sha}"),
        sha,
        auth,
    )
}

pub(crate) fn resolve_ref_at(
    endpoint: &Endpoint,
    git_ref: Option<&str>,
    auth: &FetchAuth<'_>,
) -> Result<String> {
    let identities = Identities::of(auth, endpoint)?;
    let _libgit2 = Libgit2Guard::acquire(identities.known_hosts, endpoint, Stage::Resolve)?;
    let scratch = scratch_repo()?;
    let heads = with_identities(endpoint, &identities, Stage::Resolve, |callbacks| {
        let mut remote = scratch.repo.remote_anonymous(&endpoint.url())?;
        let mut connection = remote.connect_auth(Direction::Fetch, Some(callbacks), None)?;
        advertised_refs(&mut connection)
    })?;
    pick_ref(&heads, git_ref, &endpoint.display())
}

pub(crate) fn download_tarball_at(
    endpoint: &Endpoint,
    prefix: &str,
    sha: &str,
    auth: &FetchAuth<'_>,
) -> Result<Vec<u8>> {
    let oid = Oid::from_str(sha)
        .map_err(|err| GithubError::Download(format!("`{sha}` is not a commit SHA: {err}")))?;
    let identities = Identities::of(auth, endpoint)?;
    let _libgit2 = Libgit2Guard::acquire(identities.known_hosts, endpoint, Stage::Download)?;
    let scratch = scratch_repo()?;
    with_identities(endpoint, &identities, Stage::Download, |callbacks| {
        let mut remote = scratch.repo.remote_anonymous(&endpoint.url())?;
        let mut options = FetchOptions::new();
        options
            .remote_callbacks(callbacks)
            .depth(1)
            .download_tags(git2::AutotagOption::None);
        remote.fetch(&[sha], Some(&mut options), None)
    })?;
    let commit = fetched_commit(&scratch.repo, oid, endpoint)?;
    tree_to_tar(&scratch.repo, &commit, prefix)
}

/// The fetched commit `oid`, refused past [`MAX_COMMIT_BYTES`] before it is
/// inflated.
fn fetched_commit<'r>(
    repo: &'r Repository,
    oid: Oid,
    endpoint: &Endpoint,
) -> Result<git2::Commit<'r>> {
    let missing = |err: git2::Error| {
        GithubError::Download(format!(
            "{} did not send commit {oid}: {}",
            endpoint.display(),
            err.message()
        ))
    };
    let odb = repo.odb().map_err(|err| {
        GithubError::Download(format!("opening object database: {}", err.message()))
    })?;
    let size = match object_size(&odb, oid, ObjectType::Commit) {
        Ok(size) => size,
        // An annotated tag's SHA names the tag object, not its commit.
        Err(ObjectHeaderError::WrongType { .. }) => {
            return Err(GithubError::Download(format!(
                "{oid} is not a commit; pin the commit SHA itself (for an annotated tag, the \
                 commit it points to)"
            )));
        }
        Err(ObjectHeaderError::Unreadable(err)) => return Err(missing(err)),
    };
    if size > MAX_COMMIT_BYTES {
        return Err(GithubError::Download(format!(
            "commit {oid} is larger than {} MiB",
            MAX_COMMIT_BYTES / (1024 * 1024)
        )));
    }
    repo.find_commit(oid).map_err(missing)
}

/// The refs the remote advertised, as `(name, commit)`. Names that are not
/// UTF-8 are skipped: they can never match a requested ref, and git2's
/// `RemoteHead::name` would panic on them, so the list is read through
/// libgit2 directly.
fn advertised_refs(
    connection: &mut git2::RemoteConnection<'_, '_, '_>,
) -> std::result::Result<Vec<(String, Oid)>, git2::Error> {
    let remote = connection.remote().raw();
    let mut heads: *mut *const libgit2_sys::git_remote_head = std::ptr::null_mut();
    let mut count: usize = 0;
    // SAFETY: `remote` is connected, and libgit2 keeps the returned array and
    // its entries alive until the remote disconnects or lists again; nothing
    // touches the remote until this function returns.
    let status = unsafe { libgit2_sys::git_remote_ls(&mut heads, &mut count, remote) };
    if status < 0 {
        return Err(git2::Error::last_error(status));
    }
    if heads.is_null() || count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: as above; libgit2 reported `count` entries at `heads`.
    let heads = unsafe { std::slice::from_raw_parts(heads, count) };
    let mut refs = Vec::with_capacity(heads.len());
    for &head in heads {
        // SAFETY: each non-null entry is a live `git_remote_head`.
        let Some(head) = (unsafe { head.as_ref() }) else {
            continue;
        };
        if head.name.is_null() {
            continue;
        }
        // SAFETY: `name` is a NUL-terminated string owned by the head.
        let name = unsafe { std::ffi::CStr::from_ptr(head.name) };
        let Ok(name) = name.to_str() else {
            continue;
        };
        refs.push((name.to_string(), Oid::from_bytes(&head.oid.id)?));
    }
    Ok(refs)
}

/// The identities one operation may offer, the host keys it trusts, and whose
/// they are for messages.
struct Identities<'a> {
    known_hosts: &'a KnownHosts,
    candidates: Vec<Candidate<'a>>,
    owner: Owner,
}

impl<'a> Identities<'a> {
    fn of(auth: &FetchAuth<'a>, endpoint: &Endpoint) -> Result<Self> {
        match *auth {
            FetchAuth::Unconfigured => Err(GithubError::SshNotConfigured(format!(
                "cannot fetch {} over SSH: this server has no SSH key for package installs; \
                 {SSH_NOT_CONFIGURED_HINT}",
                endpoint.display()
            ))),
            FetchAuth::Server { key, known_hosts } => Ok(Self {
                known_hosts,
                candidates: vec![Candidate::Server(key)],
                owner: Owner::Server,
            }),
            FetchAuth::Local {
                identities,
                known_hosts,
            } => Ok(Self {
                known_hosts,
                candidates: identities
                    .candidates()
                    .into_iter()
                    .map(|identity| Candidate::Local {
                        identity,
                        identities,
                    })
                    .collect(),
                owner: Owner::Local,
            }),
        }
    }
}

/// Whose SSH identity an operation uses, which decides what its errors tell
/// the reader to fix.
#[derive(Clone, Copy)]
enum Owner {
    Server,
    Local,
}

/// Run `op` once per identity until one is accepted. `op` must open its own
/// connection with the callbacks it is given.
fn with_identities<T>(
    endpoint: &Endpoint,
    identities: &Identities<'_>,
    stage: Stage,
    mut op: impl FnMut(RemoteCallbacks<'_>) -> std::result::Result<T, git2::Error>,
) -> Result<T> {
    let connector = Connector {
        endpoint,
        known_hosts: identities.known_hosts,
        owner: identities.owner,
        stage,
        // One bound for the whole operation, however many identities it tries.
        deadline: Instant::now().checked_add(endpoint.deadline),
    };
    let mut tried: Vec<String> = Vec::new();
    for candidate in &identities.candidates {
        for retry_passphrase in [false, true] {
            let credential = match candidate.credential(retry_passphrase) {
                Ok(credential) => credential,
                Err(skipped) => {
                    tried.push(skipped);
                    break;
                }
            };
            match connector.attempt(&credential, &mut op) {
                Attempt::Accepted(value) => {
                    candidate.accepted();
                    return Ok(value);
                }
                Attempt::Failed(err) => return Err(err),
                Attempt::Rejected {
                    wrong_passphrase: true,
                    ..
                } if !retry_passphrase && candidate.can_prompt() => {
                    // Ask for the passphrase once more, then try this identity again.
                    continue;
                }
                Attempt::Rejected { reason, .. } => {
                    tried.push(format!("{} ({reason})", candidate.describe()));
                    break;
                }
            }
        }
    }
    Err(GithubError::Auth(auth_failure_message(
        endpoint,
        identities.owner,
        &tried,
    )))
}

/// How one connection attempt ended.
enum Attempt<T> {
    Accepted(T),
    /// The identity was refused or unusable; the next one may still work.
    Rejected {
        reason: String,
        wrong_passphrase: bool,
    },
    /// A failure no other identity can fix: the host key, a limit, or
    /// anything after a successful login.
    Failed(GithubError),
}

/// Everything one operation's connection attempts share.
struct Connector<'a> {
    endpoint: &'a Endpoint,
    known_hosts: &'a KnownHosts,
    owner: Owner,
    stage: Stage,
    deadline: Option<Instant>,
}

impl Connector<'_> {
    fn attempt<T>(
        &self,
        credential: &Credential<'_>,
        op: &mut impl FnMut(RemoteCallbacks<'_>) -> std::result::Result<T, git2::Error>,
    ) -> Attempt<T> {
        let state = AttemptState::default();
        let outcome = op(self.callbacks(credential, &state));
        if let Some(message) = state.host_key_error.take() {
            return Attempt::Failed(GithubError::HostKey(message));
        }
        if state.timed_out.get() {
            return Attempt::Failed(self.stage.error(format!(
                "{} took longer than {}s; check network access to {} on port {}",
                self.endpoint.display(),
                self.endpoint.deadline.as_secs(),
                self.endpoint.host,
                self.endpoint.port
            )));
        }
        if state.too_large.get() {
            return Attempt::Failed(self.stage.error(format!(
                "{} sent more than {} MiB",
                self.endpoint.display(),
                MAX_SOURCE_BYTES / (1024 * 1024)
            )));
        }
        let err = match outcome {
            Ok(value) => return Attempt::Accepted(value),
            Err(err) => err,
        };
        if exceeded_pack_object_limit(&err) {
            return Attempt::Failed(self.stage.error(format!(
                "{} is too large to install: its source exceeds {MAX_SOURCE_ENTRIES} entries",
                self.endpoint.display()
            )));
        }
        if !is_authentication_failure(&err, &state) {
            return Attempt::Failed(self.stage.error(remote_failure_message(
                self.endpoint,
                &err,
                self.owner,
            )));
        }
        Attempt::Rejected {
            wrong_passphrase: credential.has_passphrase() && err.message().contains("passphrase"),
            reason: rejection_reason(&err),
        }
    }

    fn callbacks<'c>(
        &'c self,
        credential: &'c Credential<'_>,
        state: &'c AttemptState,
    ) -> RemoteCallbacks<'c> {
        let endpoint = self.endpoint;
        let known_hosts = self.known_hosts;
        let deadline = self.deadline;
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(move |_url, username, allowed| {
            let user = username.unwrap_or(&endpoint.user);
            if !allowed.contains(CredentialType::SSH_KEY) {
                if allowed.contains(CredentialType::USERNAME) {
                    return Cred::username(user);
                }
                return Err(git2::Error::from_str("the server offers no SSH key login"));
            }
            let calls = state.key_requests.get().saturating_add(1);
            state.key_requests.set(calls);
            // A second request means the server refused the identity offered.
            if calls > 1 {
                return Err(git2::Error::from_str(IDENTITY_REJECTED));
            }
            credential.to_cred(user)
        });
        callbacks.certificate_check(move |cert, host| {
            let verdict = if host == endpoint.host {
                match cert.as_hostkey().and_then(|hostkey| hostkey.hostkey()) {
                    Some(presented) => known_hosts.verify(&endpoint.known_hosts_name(), presented),
                    None => Err(format!("{host} did not present an SSH host key")),
                }
            } else {
                Err(format!(
                    "refusing SSH host `{host}`: package fetches only connect to {}",
                    endpoint.host
                ))
            };
            match verdict {
                Ok(()) => Ok(CertificateCheckStatus::CertificateOk),
                Err(message) => {
                    let err = git2::Error::from_str(&message);
                    state.host_key_error.replace(Some(message));
                    Err(err)
                }
            }
        });
        callbacks.transfer_progress(move |progress| {
            let received = u64::try_from(progress.received_bytes()).unwrap_or(u64::MAX);
            if received > MAX_SOURCE_BYTES {
                state.too_large.set(true);
                return false;
            }
            within_deadline(deadline, state)
        });
        callbacks.sideband_progress(move |_| within_deadline(deadline, state));
        callbacks
    }
}

/// What the callbacks observed during one connection attempt.
#[derive(Default)]
struct AttemptState {
    key_requests: Cell<u32>,
    host_key_error: RefCell<Option<String>>,
    timed_out: Cell<bool>,
    too_large: Cell<bool>,
}

/// libgit2's indexer refusing a pack past `GIT_OPT_SET_PACK_MAX_OBJECTS`.
fn exceeded_pack_object_limit(err: &git2::Error) -> bool {
    err.class() == ErrorClass::Indexer && err.message().contains("too many objects")
}

const IDENTITY_REJECTED: &str = "SSH identity not accepted";

/// How libgit2 and libssh2 word failures of the authentication step itself.
/// Errors after a successful login are SSH-class too and may mention
/// authentication (GitHub's SAML SSO refusal links an "authenticating" page),
/// so the class alone cannot tell them apart, and the phrases are libgit2's
/// own.
const AUTHENTICATION_ERRORS: [&str; 4] = [
    "failed to authenticate SSH session",
    "error authenticating",
    "Wrong passphrase",
    "private key file",
];

fn is_authentication_failure(err: &git2::Error, state: &AttemptState) -> bool {
    match state.key_requests.get() {
        0 => false,
        1 => {
            err.code() == git2::ErrorCode::Auth
                || (err.class() == ErrorClass::Ssh
                    && AUTHENTICATION_ERRORS
                        .iter()
                        .any(|phrase| err.message().contains(phrase)))
        }
        _ => true,
    }
}

/// libgit2 only calls the progress callbacks while objects are transferring;
/// the connect and ref listing before that are bounded by the IO timeout.
fn within_deadline(deadline: Option<Instant>, state: &AttemptState) -> bool {
    let within = deadline.is_none_or(|deadline| Instant::now() < deadline);
    if !within {
        state.timed_out.set(true);
    }
    within
}

fn rejection_reason(err: &git2::Error) -> String {
    let message = err.message();
    if message.contains(IDENTITY_REJECTED) {
        "not accepted".to_string()
    } else if message.contains("passphrase") {
        "could not be decrypted; wrong passphrase?".to_string()
    } else {
        message.trim().to_string()
    }
}

static LIBGIT2: Mutex<()> = Mutex::new(());

/// Exclusive use of libgit2 for one SSH operation.
///
/// libgit2 picks the host key type to ask for from `$HOME/.ssh/known_hosts`,
/// apart from the `certificate_check` callback, and reads the user's and the
/// system's git config. Left alone, a line libssh2 cannot parse or a
/// `url.*.insteadOf` rule would fail or redirect every fetch, and it could ask
/// for a host key type [`KnownHosts`] has no key for. So each operation gets a
/// fresh home holding only a `known_hosts` with exactly the keys that will be
/// accepted, and no config search path.
///
/// Those are libgit2 process globals. Rule for every libgit2 use in this
/// crate: hold this guard. This module is the only one, and the lock
/// serializes SSH operations in a process; a fetch is bounded by the deadline
/// and IO timeouts, and package installs are infrequent.
struct Libgit2Guard {
    // Dropped first, so the home is removed while the lock is still held.
    _home: TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl Libgit2Guard {
    fn acquire(known_hosts: &KnownHosts, endpoint: &Endpoint, stage: Stage) -> Result<Self> {
        let lock = LIBGIT2.lock().unwrap_or_else(PoisonError::into_inner);
        let home = private_home(known_hosts, &endpoint.known_hosts_name())
            .map_err(|message| stage.error(message))?;
        configure_libgit2(&home).map_err(|message| stage.error(message))?;
        Ok(Self {
            _home: home,
            _lock: lock,
        })
    }
}

/// A home directory whose `.ssh/known_hosts` trusts exactly what `known_hosts`
/// does for `host`.
fn private_home(known_hosts: &KnownHosts, host: &str) -> std::result::Result<TempDir, String> {
    let home = tempfile::tempdir().map_err(|err| format!("creating libgit2 home: {err}"))?;
    let ssh_dir = home.path().join(".ssh");
    std::fs::create_dir(&ssh_dir).map_err(|err| format!("creating libgit2 home: {err}"))?;
    let path = ssh_dir.join("known_hosts");
    std::fs::write(&path, known_hosts.trusted_lines(host))
        .map_err(|err| format!("writing {}: {err}", path.display()))?;
    Ok(home)
}

/// Point libgit2 at `home`, clear its config search paths, and set its
/// timeouts and pack object limit.
fn configure_libgit2(home: &TempDir) -> std::result::Result<(), String> {
    let path = libgit2_home_path(home.path())?;
    let failed =
        |what: &str, err: git2::Error| format!("configuring libgit2 {what}: {}", err.message());
    // SAFETY: these calls write libgit2 process globals. The caller holds
    // `Libgit2Guard`'s lock, which every libgit2 use in this crate holds, so
    // nothing reads them concurrently. libgit2 copies `path`.
    unsafe {
        git2::opts::set_server_connect_timeout_in_milliseconds(CONNECT_TIMEOUT_MS)
            .map_err(|err| failed("connect timeout", err))?;
        git2::opts::set_server_timeout_in_milliseconds(IO_TIMEOUT_MS)
            .map_err(|err| failed("IO timeout", err))?;
        for level in [
            git2::ConfigLevel::ProgramData,
            git2::ConfigLevel::System,
            git2::ConfigLevel::XDG,
            git2::ConfigLevel::Global,
        ] {
            git2::opts::set_search_path(level, "")
                .map_err(|err| failed("config search path", err))?;
        }
        let status = libgit2_sys::git_libgit2_opts(
            libgit2_sys::GIT_OPT_SET_HOMEDIR as std::ffi::c_int,
            path.as_ptr(),
        );
        if status < 0 {
            return Err(format!("setting libgit2 home directory failed ({status})"));
        }
        // A depth-1 fetch of one commit holds at most the commit, its root
        // tree, and the entries the tarball walk accepts, so a pack claiming
        // more is refused before libgit2 sizes its index for it.
        let max_objects =
            usize::try_from(MAX_SOURCE_ENTRIES.saturating_add(2)).unwrap_or(usize::MAX);
        let status = libgit2_sys::git_libgit2_opts(
            libgit2_sys::GIT_OPT_SET_PACK_MAX_OBJECTS as std::ffi::c_int,
            max_objects,
        );
        if status < 0 {
            return Err(format!(
                "setting libgit2 pack object limit failed ({status})"
            ));
        }
    }
    Ok(())
}

/// `home` as libgit2 takes it: UTF-8 on every platform, and read as a search
/// list split on the platform's path separator, so it may not contain one.
fn libgit2_home_path(home: &Path) -> std::result::Result<CString, String> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    home.to_str()
        // libgit2 also splices its previous value in for a literal `$PATH`.
        .filter(|path| !path.contains(separator) && !path.contains("$PATH"))
        .and_then(|path| CString::new(path).ok())
        .ok_or_else(|| {
            format!(
                "the temp directory {} cannot hold libgit2's home: the path must be UTF-8 \
                 without `{separator}` or `$PATH`; point TMPDIR elsewhere",
                home.display()
            )
        })
}

/// One identity to offer GitHub. A local identity carries the set it came
/// from, so accepting it and asking for its passphrase need nothing else.
enum Candidate<'a> {
    Server(&'a ServerSshKey),
    Local {
        identity: LocalIdentity,
        identities: &'a LocalIdentities<'a>,
    },
}

impl<'a> Candidate<'a> {
    fn describe(&self) -> String {
        match self {
            Candidate::Server(_) => "the server's package SSH key".to_string(),
            Candidate::Local { identity, .. } => identity.describe(),
        }
    }

    /// Build the credential, asking for a passphrase when the key file needs
    /// one. `Err` carries why the candidate was skipped.
    fn credential(&self, retry_passphrase: bool) -> std::result::Result<Credential<'a>, String> {
        let (path, identities) = match self {
            Candidate::Server(key) => return Ok(Credential::ServerKey(key)),
            Candidate::Local {
                identity: LocalIdentity::Agent,
                ..
            } => return Ok(Credential::Agent),
            Candidate::Local {
                identity: LocalIdentity::KeyFile(path),
                identities,
            } => (path, identities),
        };
        let passphrase = passphrase_for(path, identities, retry_passphrase)?;
        // Without the `.pub`, libssh2 must decrypt the private key before
        // offering it, so a wrong passphrase fails locally and distinctly
        // instead of looking like a key the server rejected.
        let public = if passphrase.is_some() {
            None
        } else {
            public_key_path(path)
        };
        Ok(Credential::KeyFile {
            path: path.clone(),
            public,
            passphrase,
        })
    }

    fn can_prompt(&self) -> bool {
        match self {
            Candidate::Server(_) => false,
            Candidate::Local { identities, .. } => identities.can_prompt(),
        }
    }

    fn accepted(&self) {
        if let Candidate::Local {
            identity,
            identities,
        } = self
        {
            identities.remember_accepted(identity);
        }
    }
}

/// The passphrase an encrypted key file needs, or `None` for an unencrypted
/// one. `Err` explains why the key is skipped.
fn passphrase_for(
    path: &Path,
    identities: &LocalIdentities<'_>,
    retry: bool,
) -> std::result::Result<Option<Zeroizing<String>>, String> {
    let text =
        keys::read_key(path).map_err(|err| format!("{} (unreadable: {err})", path.display()))?;
    if !keys::key_file_is_encrypted(&text) {
        return Ok(None);
    }
    if let Some(entered) = identities.passphrase(path, retry) {
        return Ok(Some(entered));
    }
    let why = if identities.can_prompt() {
        "no passphrase entered"
    } else {
        "passphrase-protected and no terminal to ask; load it into ssh-agent with `ssh-add`"
    };
    Err(format!("{} ({why})", path.display()))
}

fn public_key_path(private: &Path) -> Option<PathBuf> {
    let mut name = private.file_name()?.to_os_string();
    name.push(".pub");
    let public = private.with_file_name(name);
    public.is_file().then_some(public)
}

/// One identity, ready to hand to libgit2.
enum Credential<'k> {
    ServerKey(&'k ServerSshKey),
    Agent,
    KeyFile {
        path: PathBuf,
        public: Option<PathBuf>,
        passphrase: Option<Zeroizing<String>>,
    },
}

impl Credential<'_> {
    fn to_cred(&self, user: &str) -> std::result::Result<Cred, git2::Error> {
        match self {
            Credential::ServerKey(key) => {
                Cred::ssh_key_from_memory(user, None, key.private_key(), None)
            }
            Credential::Agent => Cred::ssh_key_from_agent(user),
            Credential::KeyFile {
                path,
                public,
                passphrase,
            } => Cred::ssh_key(
                user,
                public.as_deref(),
                path,
                passphrase.as_deref().map(String::as_str),
            ),
        }
    }

    fn has_passphrase(&self) -> bool {
        matches!(
            self,
            Credential::KeyFile {
                passphrase: Some(_),
                ..
            }
        )
    }
}

fn auth_failure_message(endpoint: &Endpoint, owner: Owner, tried: &[String]) -> String {
    let tried = if tried.is_empty() {
        "no SSH identity was available (no ssh-agent and no ~/.ssh/id_ed25519, id_ecdsa, or \
         id_rsa)"
            .to_string()
    } else {
        format!("tried {}", tried.join("; "))
    };
    let fix = match owner {
        Owner::Server => {
            "add the public key that `submilli server packages ssh-key` prints as a deploy key \
             on the repository"
        }
        Owner::Local => {
            "add your public key to a GitHub account that can read the repository \
             (https://github.com/settings/keys), or load the right key into ssh-agent with \
             `ssh-add`"
        }
    };
    format!(
        "SSH authentication to {} failed for {}: {tried}. To fix, {fix}",
        endpoint.host,
        endpoint.display()
    )
}

fn remote_failure_message(endpoint: &Endpoint, err: &git2::Error, owner: Owner) -> String {
    let message = err.message().trim();
    if !message
        .to_ascii_lowercase()
        .contains("repository not found")
    {
        return format!("fetching {} over SSH: {message}", endpoint.display());
    }
    let (whose, fix) = match owner {
        Owner::Server => (
            "the server's package SSH key",
            "add the key as a deploy key on the repository",
        ),
        Owner::Local => (
            "your SSH identity",
            "add your key to a GitHub account that can read the repository",
        ),
    };
    format!(
        "{} was not found, or {whose} has no access to it (GitHub reports both the same way). \
         Check the org/repo name, then {fix}",
        endpoint.display()
    )
}

/// A throwaway bare repository the fetch writes objects into.
struct Scratch {
    repo: Repository,
    _dir: TempDir,
}

fn scratch_repo() -> Result<Scratch> {
    let dir = tempfile::tempdir()
        .map_err(|err| GithubError::Download(format!("creating temp dir: {err}")))?;
    let repo = Repository::init_bare(dir.path()).map_err(|err| {
        GithubError::Download(format!("creating scratch repository: {}", err.message()))
    })?;
    Ok(Scratch { repo, _dir: dir })
}

#[derive(Clone, Copy)]
enum Stage {
    Resolve,
    Download,
}

impl Stage {
    fn error(self, message: String) -> GithubError {
        match self {
            Stage::Resolve => GithubError::Resolve(message),
            Stage::Download => GithubError::Download(message),
        }
    }
}

/// Pick the SHA for `git_ref` from an advertised ref list: an explicit
/// `refs/…` name, `HEAD` (the default), then a tag, then a branch, peeled to
/// its commit. Tags before branches is git's own order (`git rev-parse`).
fn pick_ref(heads: &[(String, Oid)], git_ref: Option<&str>, display: &str) -> Result<String> {
    let lookup = |name: &str| {
        let peeled = format!("{name}^{{}}");
        heads
            .iter()
            .find(|(head, _)| *head == peeled)
            .or_else(|| heads.iter().find(|(head, _)| head == name))
            .map(|(_, oid)| oid.to_string())
    };
    let wanted = git_ref.unwrap_or("HEAD");
    let names = if wanted == "HEAD" || wanted.starts_with("refs/") {
        vec![wanted.to_string()]
    } else {
        vec![
            format!("refs/tags/{wanted}"),
            format!("refs/heads/{wanted}"),
        ]
    };
    if let Some(sha) = names.iter().find_map(|name| lookup(name)) {
        return Ok(sha);
    }
    if (4..40).contains(&wanted.len()) && wanted.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GithubError::Resolve(format!(
            "`{wanted}` looks like an abbreviated commit SHA; over SSH pass the full \
             40-character SHA"
        )));
    }
    Err(GithubError::Resolve(format!(
        "{display} has no branch or tag `{wanted}`"
    )))
}

/// Serialize the commit's tree as an uncompressed tar under `prefix/`: each
/// directory's files in git tree order before its subdirectories, fixed
/// ownership, and the commit time as mtime, so the bytes — and the recorded
/// source hash — depend only on the commit.
/// Submodules (gitlinks) are skipped, as GitHub's tarballs do.
fn tree_to_tar(repo: &Repository, commit: &git2::Commit<'_>, prefix: &str) -> Result<Vec<u8>> {
    let download = |message: String| GithubError::Download(message);
    let mtime = u64::try_from(commit.time().seconds()).unwrap_or(0);
    let odb = repo
        .odb()
        .map_err(|err| download(format!("opening object database: {}", err.message())))?;
    let mut builder = tar::Builder::new(Vec::new());
    // Shared subtrees cost almost nothing in a pack but are walked once per
    // path, so entries are capped as well as bytes.
    let mut entries: u64 = 0;
    // Directories are kept as names in an arena rather than full paths, and
    // names and path lengths are capped as each one is reached, so a tiny
    // pack of deep, wide trees cannot make the walk hold gigabytes of text.
    let mut dirs = vec![TarDir {
        parent: None,
        name: prefix.to_string(),
        path_len: prefix.len(),
    }];
    // libgit2 inflates and parses a whole tree before its entries can be
    // counted, so tree sizes are charged from their headers first; the root is
    // reached by id too, never through `Commit::tree`.
    let mut tree_bytes: u64 = 0;
    // An explicit stack bounds memory rather than call depth; subdirectories
    // are pushed in reverse so they pop in tree order.
    let mut stack: Vec<(Oid, usize)> = vec![(commit.tree_id(), 0)];
    while let Some((tree_id, dir)) = stack.pop() {
        let read_tree_err =
            |err: &dyn std::fmt::Display| download(format!("reading tree {tree_id}: {err}"));
        let size =
            object_size(&odb, tree_id, ObjectType::Tree).map_err(|err| read_tree_err(&err))?;
        tree_bytes = tree_bytes.saturating_add(size);
        if tree_bytes > MAX_TREE_BYTES {
            return Err(download(format!(
                "repository trees exceed {} MiB",
                MAX_TREE_BYTES / (1024 * 1024)
            )));
        }
        let tree = repo
            .find_tree(tree_id)
            .map_err(|err| read_tree_err(&err.message()))?;
        let mut subtrees = Vec::new();
        for entry in &tree {
            entries = entries.saturating_add(1);
            if entries > MAX_SOURCE_ENTRIES {
                return Err(super::source_too_large());
            }
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    let name = entry_name(&entry, &dirs, dir)?;
                    let path_len = child_path_len(&dirs, dir, &name)?;
                    let index = dirs.len();
                    dirs.push(TarDir {
                        parent: Some(dir),
                        name,
                        path_len,
                    });
                    subtrees.push((entry.id(), index));
                }
                Some(ObjectType::Blob) => {
                    let name = entry_name(&entry, &dirs, dir)?;
                    let path = file_path(&dirs, dir, &name)?;
                    append_tree_blob(repo, &odb, &mut builder, &entry, &path, mtime)?;
                }
                // Gitlinks (submodules) and anything else carry no content
                // and are never extracted, so their names are not checked.
                _ => {}
            }
        }
        stack.extend(subtrees.into_iter().rev());
    }
    builder
        .into_inner()
        .map_err(|err| download(format!("building source tarball: {err}")))
}

/// Add the blob `entry` to the tarball at `path`, refusing it before it is
/// inflated if it would take the tarball past [`MAX_SOURCE_BYTES`], and after
/// it is written if padding and long-name blocks did.
fn append_tree_blob(
    repo: &Repository,
    odb: &git2::Odb<'_>,
    builder: &mut tar::Builder<Vec<u8>>,
    entry: &git2::TreeEntry<'_>,
    path: &Path,
    mtime: u64,
) -> Result<()> {
    let read_err = |err: &dyn std::fmt::Display| {
        GithubError::Download(format!("reading {}: {err}", path.display()))
    };
    let size = object_size(odb, entry.id(), ObjectType::Blob).map_err(|err| read_err(&err))?;
    if tarball_len(builder).saturating_add(size) > MAX_SOURCE_BYTES {
        return Err(super::source_too_large());
    }
    let blob = repo
        .find_blob(entry.id())
        .map_err(|err| read_err(&err.message()))?;
    append_blob(builder, path, entry.filemode(), blob.content(), mtime)?;
    if tarball_len(builder) > MAX_SOURCE_BYTES {
        return Err(super::source_too_large());
    }
    Ok(())
}

/// The size of object `oid` from its header, without inflating it, refused
/// unless it is of the `expected` type. Read through libgit2 directly: git2's
/// `Odb::read_header` unwraps the type and panics on the codes 0 and 5 that a
/// pack may carry.
fn object_size(
    odb: &git2::Odb<'_>,
    oid: Oid,
    expected: ObjectType,
) -> std::result::Result<u64, ObjectHeaderError> {
    let mut size: usize = 0;
    let mut kind = libgit2_sys::GIT_OBJECT_INVALID;
    // SAFETY: `odb` and `oid` are live for the call, and libgit2 writes only
    // the two out-parameters.
    let status =
        unsafe { libgit2_sys::git_odb_read_header(&mut size, &mut kind, odb.raw(), oid.raw()) };
    if status < 0 {
        return Err(ObjectHeaderError::Unreadable(git2::Error::last_error(
            status,
        )));
    }
    if ObjectType::from_raw(kind) != Some(expected) {
        return Err(ObjectHeaderError::WrongType { oid, expected });
    }
    Ok(u64::try_from(size).unwrap_or(u64::MAX))
}

/// Why [`object_size`] refused an object.
#[derive(Debug)]
enum ObjectHeaderError {
    /// libgit2 could not read the header: missing, corrupt, or unreadable.
    Unreadable(git2::Error),
    /// The object is not of the type its referrer named, or of no known type.
    WrongType { oid: Oid, expected: ObjectType },
}

impl std::fmt::Display for ObjectHeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObjectHeaderError::Unreadable(err) => f.write_str(err.message()),
            ObjectHeaderError::WrongType { oid, expected } => {
                write!(f, "object {oid} is not a {expected}")
            }
        }
    }
}

/// A directory reached by [`tree_to_tar`]'s walk: its parent's index in the
/// arena, its own name, and the byte length of its full path.
struct TarDir {
    parent: Option<usize>,
    name: String,
    path_len: usize,
}

/// The full path of file `name` in directory `dir` of the walk's arena.
fn file_path(dirs: &[TarDir], dir: usize, name: &str) -> Result<PathBuf> {
    child_path_len(dirs, dir, name)?;
    let mut parts = vec![name];
    let mut next = Some(dir);
    while let Some(index) = next {
        let entry = arena_dir(dirs, index)?;
        parts.push(&entry.name);
        next = entry.parent;
    }
    Ok(parts.iter().rev().collect())
}

/// The byte length of `name`'s full path in directory `dir`, refused past
/// [`MAX_PATH_BYTES`], which no file system could extract.
fn child_path_len(dirs: &[TarDir], dir: usize, name: &str) -> Result<usize> {
    let path_len = arena_dir(dirs, dir)?
        .path_len
        .saturating_add(1)
        .saturating_add(name.len());
    if path_len > MAX_PATH_BYTES {
        return Err(GithubError::Download(format!(
            "`{}` makes a path longer than {MAX_PATH_BYTES} bytes",
            display_entry(dirs, dir, name)
        )));
    }
    Ok(path_len)
}

fn arena_dir(dirs: &[TarDir], index: usize) -> Result<&TarDir> {
    dirs.get(index)
        .ok_or_else(|| GithubError::Download("tree walk lost track of a directory".to_string()))
}

/// `name` in directory `dir`, below the `<repo>-<sha>` prefix, for messages.
fn display_entry(dirs: &[TarDir], dir: usize, name: &str) -> String {
    let dir = dir_path(dirs, dir);
    let name = truncated(name);
    if dir.is_empty() {
        name
    } else {
        format!("{dir}/{name}")
    }
}

/// Directory `dir`'s path below the `<repo>-<sha>` prefix, for messages.
fn dir_path(dirs: &[TarDir], dir: usize) -> String {
    let mut names = Vec::new();
    let mut next = Some(dir);
    while let Some(entry) = next.and_then(|index| dirs.get(index)) {
        if entry.parent.is_some() {
            names.push(truncated(&entry.name));
        }
        next = entry.parent;
    }
    names.reverse();
    names.join("/")
}

/// At most 64 characters of `name`, for messages.
fn truncated(name: &str) -> String {
    match name.char_indices().nth(64) {
        Some((end, _)) => format!("{}…", name.get(..end).unwrap_or(name)),
        None => name.to_string(),
    }
}

fn tarball_len(builder: &tar::Builder<Vec<u8>>) -> u64 {
    u64::try_from(builder.get_ref().len()).unwrap_or(u64::MAX)
}

/// The name of `entry` in directory `dir`, refused if it is not a plain,
/// UTF-8 file name a file system could hold.
fn entry_name(entry: &git2::TreeEntry<'_>, dirs: &[TarDir], dir: usize) -> Result<String> {
    let lossy = String::from_utf8_lossy(entry.name_bytes());
    let refuse =
        |why: &str| GithubError::Download(format!("`{}` {why}", display_entry(dirs, dir, &lossy)));
    let name = std::str::from_utf8(entry.name_bytes()).map_err(|_| refuse("is not valid UTF-8"))?;
    if name.len() > MAX_NAME_BYTES {
        return Err(refuse(&format!(
            "has a name longer than {MAX_NAME_BYTES} bytes"
        )));
    }
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(refuse("is not a safe file name"));
    }
    Ok(name.to_string())
}

fn append_blob(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &Path,
    filemode: i32,
    content: &[u8],
    mtime: u64,
) -> Result<()> {
    let write_err = |err: std::io::Error| {
        GithubError::Download(format!("adding {} to tarball: {err}", path.display()))
    };
    let mut header = tar::Header::new_gnu();
    header.set_mtime(mtime);
    header.set_uid(0);
    header.set_gid(0);
    if filemode == 0o120000 {
        let target = std::str::from_utf8(content).map_err(|_| {
            GithubError::Download(format!("symlink {} has a non-UTF-8 target", path.display()))
        })?;
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_size(0);
        return builder
            .append_link(&mut header, path, target)
            .map_err(write_err);
    }
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(if filemode == 0o100755 { 0o755 } else { 0o644 });
    header.set_size(content.len() as u64);
    builder
        .append_data(&mut header, path, content)
        .map_err(write_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        Oid::from_bytes(&[byte; 20]).unwrap()
    }

    fn heads() -> Vec<(String, Oid)> {
        vec![
            ("HEAD".to_string(), oid(1)),
            ("refs/heads/main".to_string(), oid(1)),
            ("refs/heads/v1".to_string(), oid(2)),
            ("refs/tags/v1".to_string(), oid(3)),
            ("refs/tags/v1^{}".to_string(), oid(4)),
            ("refs/tags/v2".to_string(), oid(5)),
            ("refs/tags/v2^{}".to_string(), oid(6)),
        ]
    }

    #[test]
    fn picks_head_tag_then_branch_peeled() {
        let heads = heads();
        assert_eq!(pick_ref(&heads, None, "r").unwrap(), oid(1).to_string());
        assert_eq!(
            pick_ref(&heads, Some("main"), "r").unwrap(),
            oid(1).to_string()
        );
        // A tag wins over a branch with the same name, as in git.
        assert_eq!(
            pick_ref(&heads, Some("v1"), "r").unwrap(),
            oid(4).to_string()
        );
        assert_eq!(
            pick_ref(&heads, Some("v2"), "r").unwrap(),
            oid(6).to_string()
        );
        assert_eq!(
            pick_ref(&heads, Some("refs/tags/v1"), "r").unwrap(),
            oid(4).to_string()
        );
    }

    #[test]
    fn unknown_and_abbreviated_refs_explain_themselves() {
        let heads = heads();
        let missing = pick_ref(&heads, Some("nope"), "git@github.com:o/r.git").unwrap_err();
        assert!(missing.to_string().contains("no branch or tag `nope`"));
        let short = pick_ref(&heads, Some("abc1234"), "r").unwrap_err();
        assert!(short.to_string().contains("full 40-character SHA"));
    }

    fn state_after(key_requests: u32) -> AttemptState {
        let state = AttemptState::default();
        state.key_requests.set(key_requests);
        state
    }

    #[test]
    fn only_authentication_step_errors_count_as_rejections() {
        let ssh = |message: &str| {
            git2::Error::new(git2::ErrorCode::GenericError, ErrorClass::Ssh, message)
        };
        let after_login = [
            "ERROR: Repository not found.",
            "SSH could not read data: Timeout waiting on socket",
            // GitHub's SAML SSO refusal links a page about authenticating.
            "ERROR: The 'acme' organization has enabled or enforced SAML SSO. Visit \
             https://docs.github.com/articles/authenticating-to-a-github-organization-with-saml-single-sign-on/",
        ];
        for message in after_login {
            assert!(
                !is_authentication_failure(&ssh(message), &state_after(1)),
                "{message}"
            );
        }
        let during_login = [
            "failed to authenticate SSH session: Unable to extract public key from private key \
             file: Wrong passphrase or invalid/unrecognized private key file format",
            "error authenticating: failed connecting with agent",
        ];
        for message in during_login {
            assert!(
                is_authentication_failure(&ssh(message), &state_after(1)),
                "{message}"
            );
        }
        let refused = git2::Error::from_str(IDENTITY_REJECTED);
        assert!(is_authentication_failure(&refused, &state_after(2)));
        assert!(!is_authentication_failure(&refused, &state_after(0)));
    }

    #[cfg(not(windows))]
    #[test]
    fn libgit2_home_refuses_the_search_list_separator() {
        assert!(libgit2_home_path(Path::new("/tmp/abc")).is_ok());
        let err = libgit2_home_path(Path::new("/tmp/a:b")).unwrap_err();
        assert!(err.contains("TMPDIR"), "{err}");
        assert!(libgit2_home_path(Path::new("/tmp/$PATHx")).is_err());
    }

    #[test]
    fn file_paths_are_capped_at_4096_bytes() {
        let prefix = "repo-sha".to_string();
        let dirs = vec![
            TarDir {
                parent: None,
                path_len: prefix.len(),
                name: prefix,
            },
            TarDir {
                parent: Some(0),
                name: "d".repeat(200),
                path_len: 8 + 1 + 200,
            },
        ];
        assert_eq!(
            file_path(&dirs, 1, "f").unwrap(),
            PathBuf::from("repo-sha").join("d".repeat(200)).join("f")
        );
        // 209 + "/" + name: exactly 4096 bytes is accepted, one more is not.
        assert!(file_path(&dirs, 1, &"f".repeat(4096 - 210)).is_ok());
        let err = file_path(&dirs, 1, &"f".repeat(4096 - 209)).unwrap_err();
        assert!(err.to_string().contains("longer than 4096"), "{err}");
    }

    #[test]
    fn messages_truncate_names_on_a_character_boundary() {
        assert_eq!(truncated("short"), "short");
        let long = "é".repeat(100);
        assert_eq!(truncated(&long), format!("{}…", "é".repeat(64)));
    }

    #[test]
    fn only_the_indexer_object_limit_counts_as_too_large() {
        let indexer = |message: &str| {
            git2::Error::new(git2::ErrorCode::GenericError, ErrorClass::Indexer, message)
        };
        assert!(exceeded_pack_object_limit(&indexer("too many objects")));
        assert!(!exceeded_pack_object_limit(&indexer(
            "unexpected end of pack"
        )));
        let other = git2::Error::new(
            git2::ErrorCode::GenericError,
            ErrorClass::Ssh,
            "too many objects",
        );
        assert!(!exceeded_pack_object_limit(&other));
    }

    #[test]
    fn endpoint_names_hosts_like_known_hosts() {
        let mut endpoint = Endpoint::github("org", "repo");
        assert_eq!(endpoint.known_hosts_name(), "github.com");
        assert_eq!(endpoint.url(), "ssh://git@github.com:22/org/repo.git");
        endpoint.host = "127.0.0.1".into();
        endpoint.port = 2222;
        assert_eq!(endpoint.known_hosts_name(), "[127.0.0.1]:2222");
    }

    #[test]
    fn server_without_a_key_fails_with_the_setting_name() {
        let err = resolve_ref("org", "repo", None, &FetchAuth::Unconfigured).unwrap_err();
        assert!(
            matches!(&err, GithubError::SshNotConfigured(message) if message.contains("package_ssh_key_file"))
        );
    }

    #[test]
    fn no_local_identity_fails_without_connecting() {
        let identities = LocalIdentities::new(false, Vec::new(), None);
        let known_hosts = KnownHosts::github_builtin();
        let auth = FetchAuth::Local {
            identities: &identities,
            known_hosts: &known_hosts,
        };
        let err = resolve_ref("org", "repo", None, &auth).unwrap_err();
        assert!(
            matches!(&err, GithubError::Auth(message) if message.contains("no SSH identity was available")),
            "{err}"
        );
    }

    /// Directory names and paths are refused as the walk reaches them, so a
    /// tree with no files cannot grow the walk's memory unchecked.
    #[test]
    fn tree_tarball_refuses_long_names_before_any_file() {
        let _libgit2 = LIBGIT2.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init_bare(dir.path()).unwrap();
        let empty = repo.treebuilder(None).unwrap().write().unwrap();
        let sig =
            git2::Signature::new("t", "t@example.com", &git2::Time::new(1_700_000_000, 0)).unwrap();
        let commit_with = |tree_id: Oid| {
            let tree = repo.find_tree(tree_id).unwrap();
            let id = repo.commit(None, &sig, &sig, "c", &tree, &[]).unwrap();
            repo.find_commit(id).unwrap()
        };

        let mut long_name = repo.treebuilder(None).unwrap();
        long_name.insert("n".repeat(256), empty, 0o040000).unwrap();
        let mut src = repo.treebuilder(None).unwrap();
        src.insert("src", long_name.write().unwrap(), 0o040000)
            .unwrap();
        let commit = commit_with(src.write().unwrap());
        let err = tree_to_tar(&repo, &commit, "repo-sha")
            .unwrap_err()
            .to_string();
        assert!(err.contains("longer than 255"), "{err}");
        assert!(err.contains("`src/nnnn"), "names the entry: {err}");

        // A submodule is never extracted, so its name is not held to the rule.
        let mut gitlink = repo.treebuilder(None).unwrap();
        gitlink
            .insert(
                "m".repeat(256),
                Oid::from_bytes(&[9; 20]).unwrap(),
                0o160000,
            )
            .unwrap();
        let tar = tree_to_tar(&repo, &commit_with(gitlink.write().unwrap()), "repo-sha").unwrap();
        assert_eq!(
            tar::Archive::new(tar.as_slice()).entries().unwrap().count(),
            0
        );

        // 17 nested 255-byte directories pass 4096 bytes with no file at all.
        let mut tree = empty;
        for _ in 0..17 {
            let mut parent = repo.treebuilder(None).unwrap();
            parent.insert("d".repeat(255), tree, 0o040000).unwrap();
            tree = parent.write().unwrap();
        }
        let err = tree_to_tar(&repo, &commit_with(tree), "repo-sha").unwrap_err();
        assert!(err.to_string().contains("longer than 4096"), "{err}");
    }

    /// An object of another (or an unknown) type is an error, never a panic.
    #[test]
    fn object_size_refuses_an_unexpected_type() {
        let _libgit2 = LIBGIT2.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init_bare(dir.path()).unwrap();
        let blob = repo.blob(b"hello\n").unwrap();
        let odb = repo.odb().unwrap();
        assert_eq!(object_size(&odb, blob, ObjectType::Blob).unwrap(), 6);
        let Err(err) = object_size(&odb, blob, ObjectType::Tree) else {
            panic!("a blob is not a tree");
        };
        assert!(err.to_string().contains("is not a tree"), "{err}");
    }

    #[test]
    fn tree_tarball_is_deterministic_and_keeps_modes() {
        let _libgit2 = LIBGIT2.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init_bare(dir.path()).unwrap();
        let blob = repo.blob(b"hello\n").unwrap();
        let script = repo.blob(b"#!/bin/sh\n").unwrap();
        let link = repo.blob(b"a.txt").unwrap();
        let mut sub = repo.treebuilder(None).unwrap();
        sub.insert("deep.txt", blob, 0o100644).unwrap();
        let sub = sub.write().unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        root.insert("a.txt", blob, 0o100644).unwrap();
        root.insert("run.sh", script, 0o100755).unwrap();
        root.insert("link", link, 0o120000).unwrap();
        root.insert("dir", sub, 0o040000).unwrap();
        root.insert("module", oid(9), 0o160000).unwrap();
        let tree = repo.find_tree(root.write().unwrap()).unwrap();
        let sig =
            git2::Signature::new("t", "t@example.com", &git2::Time::new(1_700_000_000, 0)).unwrap();
        let commit_id = repo.commit(None, &sig, &sig, "c", &tree, &[]).unwrap();
        let commit = repo.find_commit(commit_id).unwrap();

        let first = tree_to_tar(&repo, &commit, "repo-sha").unwrap();
        let second = tree_to_tar(&repo, &commit, "repo-sha").unwrap();
        assert_eq!(first, second, "same commit, same bytes");

        let mut archive = tar::Archive::new(first.as_slice());
        let entries: Vec<(String, u32, tar::EntryType)> = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let header = entry.header();
                (
                    entry.path().unwrap().display().to_string(),
                    header.mode().unwrap(),
                    header.entry_type(),
                )
            })
            .collect();
        assert_eq!(
            entries,
            vec![
                ("repo-sha/a.txt".to_string(), 0o644, tar::EntryType::Regular),
                ("repo-sha/link".to_string(), 0o777, tar::EntryType::Symlink),
                (
                    "repo-sha/run.sh".to_string(),
                    0o755,
                    tar::EntryType::Regular
                ),
                (
                    "repo-sha/dir/deep.txt".to_string(),
                    0o644,
                    tar::EntryType::Regular
                ),
            ]
        );
    }

    /// End-to-end transport tests against a throwaway, unprivileged `sshd`
    /// serving `git-upload-pack` for a local bare repository. They need
    /// OpenSSH's `sshd` and `ssh-keygen` plus `git`, so they run only with
    /// `SUBMILLI_TEST_SSHD=1`.
    mod sshd {
        use super::*;
        use std::net::{TcpListener, TcpStream};
        use std::process::{Child, Command, Stdio};

        struct Fixture {
            dir: tempfile::TempDir,
            sshd: Child,
            port: u16,
            main_sha: String,
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = self.sshd.kill();
                let _ = self.sshd.wait();
            }
        }

        fn enabled() -> bool {
            std::env::var("SUBMILLI_TEST_SSHD").is_ok_and(|value| value == "1")
        }

        fn run(command: &mut Command) -> String {
            let output = command.output().expect("spawn");
            assert!(output.status.success(), "{command:?}: {output:?}");
            String::from_utf8(output.stdout)
                .expect("utf-8")
                .trim()
                .to_string()
        }

        fn keygen(path: &Path) {
            run(Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "", "-f"])
                .arg(path));
        }

        fn start() -> Fixture {
            start_serving(None)
        }

        /// `force_command` replaces `git-upload-pack` as what every login runs.
        fn start_serving(force_command: Option<&str>) -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            keygen(&root.join("host_key"));
            keygen(&root.join("client_key"));
            keygen(&root.join("stranger_key"));
            run(Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "correct horse", "-C", "", "-f"])
                .arg(root.join("protected_key")));
            let authorized = [
                std::fs::read_to_string(root.join("client_key.pub")).unwrap(),
                std::fs::read_to_string(root.join("protected_key.pub")).unwrap(),
            ];
            std::fs::write(root.join("authorized_keys"), authorized.concat()).unwrap();

            let work = root.join("work");
            let bare = root.join("repo.git");
            run(Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&work));
            std::fs::create_dir_all(work.join("src")).unwrap();
            std::fs::write(work.join("submilli.toml"), "# manifest\n").unwrap();
            std::fs::write(work.join("src/lib.ts"), "export const x = 1;\n").unwrap();
            let git = |args: &[&str]| {
                run(Command::new("git")
                    .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                    .arg("-C")
                    .arg(&work)
                    .args(args))
            };
            git(&["add", "."]);
            git(&["commit", "-qm", "one"]);
            git(&["tag", "-a", "v1", "-m", "v1"]);
            run(Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&bare));
            run(Command::new("git").arg("-C").arg(&bare).args([
                "config",
                "uploadpack.allowReachableSHA1InWant",
                "true",
            ]));
            git(&["push", "-q", bare.to_str().unwrap(), "main", "v1"]);
            let main_sha = git(&["rev-parse", "HEAD"]);

            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let config = format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {host}\nPidFile {pid}\n\
                 AuthorizedKeysFile {auth}\nPasswordAuthentication no\n\
                 KbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n\
                 ForceCommand {command}\n",
                host = root.join("host_key").display(),
                pid = root.join("sshd.pid").display(),
                auth = root.join("authorized_keys").display(),
                command = force_command.map_or_else(
                    || format!("git-upload-pack {}", bare.display()),
                    str::to_string
                ),
            );
            std::fs::write(root.join("sshd_config"), config).unwrap();
            let child = spawn_sshd(&root.join("sshd_config"), port);
            Fixture {
                dir,
                sshd: child,
                port,
                main_sha,
            }
        }

        /// Stop the fixture's sshd and start it again also offering `host_key`,
        /// listed first so it is the server's own preference.
        fn restart_with_extra_host_key(mut fixture: Fixture, host_key: &Path) -> Fixture {
            let _ = fixture.sshd.kill();
            let _ = fixture.sshd.wait();
            let config_path = fixture.dir.path().join("sshd_config");
            let config = std::fs::read_to_string(&config_path).unwrap();
            std::fs::write(
                &config_path,
                format!("HostKey {}\n{config}", host_key.display()),
            )
            .unwrap();
            fixture.sshd = spawn_sshd(&config_path, fixture.port);
            fixture
        }

        fn spawn_sshd(config_path: &Path, port: u16) -> Child {
            let sshd = ["/usr/sbin/sshd", "/usr/bin/sshd"]
                .into_iter()
                .find(|path| Path::new(path).exists())
                .expect("sshd installed");
            let child = Command::new(sshd)
                .arg("-D")
                .arg("-f")
                .arg(config_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start sshd");
            let started = Instant::now();
            while TcpStream::connect(("127.0.0.1", port)).is_err() {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "sshd did not start"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            child
        }

        impl Fixture {
            fn endpoint(&self) -> Endpoint {
                Endpoint {
                    user: run(&mut Command::new("whoami")),
                    host: "127.0.0.1".to_string(),
                    port: self.port,
                    path: "repo.git".to_string(),
                    deadline: OPERATION_DEADLINE,
                }
            }

            fn known_hosts(&self) -> KnownHosts {
                let public = std::fs::read_to_string(self.dir.path().join("host_key.pub")).unwrap();
                KnownHosts::from_text(&format!("[127.0.0.1]:{} {public}", self.port), "test")
            }

            fn key(&self, name: &str) -> ServerSshKey {
                ServerSshKey::load(&self.dir.path().join(name)).unwrap()
            }
        }

        #[test]
        fn resolves_and_downloads_with_an_authorized_key() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let (key, known_hosts) = (fixture.key("client_key"), fixture.known_hosts());
            let auth = FetchAuth::Server {
                key: &key,
                known_hosts: &known_hosts,
            };
            let endpoint = fixture.endpoint();
            assert_eq!(
                resolve_ref_at(&endpoint, None, &auth).unwrap(),
                fixture.main_sha
            );
            assert_eq!(
                resolve_ref_at(&endpoint, Some("v1"), &auth).unwrap(),
                fixture.main_sha,
                "annotated tag peels to its commit"
            );

            // An annotated tag's own SHA names the tag object, not a commit.
            let tag_sha = run(Command::new("git")
                .arg("-C")
                .arg(fixture.dir.path().join("repo.git"))
                .args(["rev-parse", "v1"]));
            let err = download_tarball_at(&endpoint, "repo-x", &tag_sha, &auth).unwrap_err();
            assert!(err.to_string().contains("is not a commit"), "{err}");

            let first = download_tarball_at(&endpoint, "repo-x", &fixture.main_sha, &auth).unwrap();
            let second =
                download_tarball_at(&endpoint, "repo-x", &fixture.main_sha, &auth).unwrap();
            assert_eq!(first, second, "the source hash is stable across fetches");
            let tree = super::super::super::extract_tarball(first.as_slice()).unwrap();
            assert_eq!(
                std::fs::read_to_string(tree.path().join("src/lib.ts")).unwrap(),
                "export const x = 1;\n"
            );
        }

        #[test]
        fn a_pkcs8_key_like_the_chart_generates_authenticates() {
            use base64::Engine;
            if !enabled() {
                return;
            }
            let fixture = start();
            let openssh = std::fs::read_to_string(fixture.dir.path().join("client_key")).unwrap();
            let seed = ssh_key::PrivateKey::from_openssh(&openssh)
                .unwrap()
                .key_data()
                .ed25519()
                .unwrap()
                .private
                .to_bytes();
            let mut der = vec![
                0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
                0x04, 0x20,
            ];
            der.extend_from_slice(&seed);
            let pem = format!(
                "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
                base64::engine::general_purpose::STANDARD.encode(der)
            );
            std::fs::write(fixture.dir.path().join("pkcs8_key"), pem).unwrap();
            let (key, known_hosts) = (fixture.key("pkcs8_key"), fixture.known_hosts());
            let auth = FetchAuth::Server {
                key: &key,
                known_hosts: &known_hosts,
            };
            assert_eq!(
                resolve_ref_at(&fixture.endpoint(), None, &auth).unwrap(),
                fixture.main_sha
            );
        }

        /// GitHub reports a missing repository, or one the key cannot read,
        /// after the login succeeded. That is not an authentication failure,
        /// so no other identity is tried and the message says what happened.
        #[test]
        fn a_missing_repository_after_login_is_not_an_auth_failure() {
            if !enabled() {
                return;
            }
            let fixture = start_serving(Some("echo 'ERROR: Repository not found.' >&2; exit 1"));
            let known_hosts = fixture.known_hosts();
            let identities = LocalIdentities::new(
                false,
                vec![
                    fixture.dir.path().join("client_key"),
                    fixture.dir.path().join("stranger_key"),
                ],
                None,
            );
            let auth = FetchAuth::Local {
                identities: &identities,
                known_hosts: &known_hosts,
            };
            let err = resolve_ref_at(&fixture.endpoint(), None, &auth).unwrap_err();
            assert!(
                matches!(&err, GithubError::Resolve(message) if message.contains("was not found")),
                "{err}"
            );
        }

        /// libgit2 asks for a host key type before checking it. With the
        /// server offering several and the trusted set holding only one, the
        /// pinned preference must still make the server present that one.
        #[test]
        fn the_host_key_type_asked_for_is_one_that_is_trusted() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let ecdsa = fixture.dir.path().join("host_key_ecdsa");
            run(Command::new("ssh-keygen")
                .args(["-q", "-t", "ecdsa", "-N", "", "-C", "", "-f"])
                .arg(&ecdsa));
            let fixture = restart_with_extra_host_key(fixture, &ecdsa);
            let (key, known_hosts) = (fixture.key("client_key"), fixture.known_hosts());
            let auth = FetchAuth::Server {
                key: &key,
                known_hosts: &known_hosts,
            };
            assert_eq!(
                resolve_ref_at(&fixture.endpoint(), None, &auth).unwrap(),
                fixture.main_sha
            );
        }

        /// git allows ref names that are not UTF-8; one advertised anywhere in
        /// the repository must not stop a resolve.
        #[test]
        fn a_ref_name_that_is_not_utf8_is_skipped() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let packed = fixture.dir.path().join("repo.git/packed-refs");
            let mut refs = std::fs::read(&packed).unwrap_or_default();
            refs.extend_from_slice(fixture.main_sha.as_bytes());
            refs.extend_from_slice(b" refs/heads/caf\xe9\n");
            std::fs::write(&packed, refs).unwrap();
            let (key, known_hosts) = (fixture.key("client_key"), fixture.known_hosts());
            let auth = FetchAuth::Server {
                key: &key,
                known_hosts: &known_hosts,
            };
            assert_eq!(
                resolve_ref_at(&fixture.endpoint(), None, &auth).unwrap(),
                fixture.main_sha
            );
        }

        #[test]
        fn local_identities_fall_through_to_an_authorized_key_file() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let known_hosts = fixture.known_hosts();
            let identities = LocalIdentities::new(
                false,
                vec![
                    fixture.dir.path().join("stranger_key"),
                    fixture.dir.path().join("client_key"),
                ],
                None,
            );
            let auth = FetchAuth::Local {
                identities: &identities,
                known_hosts: &known_hosts,
            };
            let endpoint = fixture.endpoint();
            assert_eq!(
                resolve_ref_at(&endpoint, None, &auth).unwrap(),
                fixture.main_sha
            );
            assert_eq!(
                identities.candidates()[0],
                LocalIdentity::KeyFile(fixture.dir.path().join("client_key")),
                "the accepted key is tried first next time"
            );
        }

        /// Answers wrong first, then right on the re-prompt.
        struct ForgetfulPrompt(std::sync::atomic::AtomicUsize);

        impl keys::PassphrasePrompt for ForgetfulPrompt {
            fn passphrase(&self, _key_path: &Path, retry: bool) -> Option<Zeroizing<String>> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let answer = if retry { "correct horse" } else { "wrong" };
                Some(Zeroizing::new(answer.to_string()))
            }
        }

        #[test]
        fn protected_key_file_reprompts_after_a_wrong_passphrase() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let known_hosts = fixture.known_hosts();
            let prompt = ForgetfulPrompt(std::sync::atomic::AtomicUsize::new(0));
            let identities = LocalIdentities::new(
                false,
                vec![fixture.dir.path().join("protected_key")],
                Some(&prompt),
            );
            let auth = FetchAuth::Local {
                identities: &identities,
                known_hosts: &known_hosts,
            };
            let endpoint = fixture.endpoint();
            assert_eq!(
                resolve_ref_at(&endpoint, None, &auth).unwrap(),
                fixture.main_sha
            );
            download_tarball_at(&endpoint, "repo-x", &fixture.main_sha, &auth).unwrap();
            assert_eq!(
                prompt.0.load(std::sync::atomic::Ordering::SeqCst),
                2,
                "one wrong answer, one re-prompt, then remembered for the download"
            );

            let silent =
                LocalIdentities::new(false, vec![fixture.dir.path().join("protected_key")], None);
            let auth = FetchAuth::Local {
                identities: &silent,
                known_hosts: &known_hosts,
            };
            let err = resolve_ref_at(&endpoint, None, &auth).unwrap_err();
            assert!(
                matches!(&err, GithubError::Auth(message) if message.contains("no terminal to ask")),
                "{err}"
            );
        }

        #[test]
        fn unauthorized_key_unknown_host_and_deadline_fail_clearly() {
            if !enabled() {
                return;
            }
            let fixture = start();
            let endpoint = fixture.endpoint();
            let known_hosts = fixture.known_hosts();

            let stranger = fixture.key("stranger_key");
            let auth = FetchAuth::Server {
                key: &stranger,
                known_hosts: &known_hosts,
            };
            let err = resolve_ref_at(&endpoint, None, &auth).unwrap_err();
            assert!(
                matches!(&err, GithubError::Auth(message) if message.contains("deploy key")),
                "{err}"
            );

            let client = fixture.key("client_key");
            let github_only = KnownHosts::github_builtin();
            let auth = FetchAuth::Server {
                key: &client,
                known_hosts: &github_only,
            };
            let err = resolve_ref_at(&endpoint, None, &auth).unwrap_err();
            assert!(
                matches!(&err, GithubError::HostKey(message) if message.contains("not a known SSH host")),
                "{err}"
            );

            let auth = FetchAuth::Server {
                key: &client,
                known_hosts: &known_hosts,
            };
            let hurried = Endpoint {
                deadline: Duration::ZERO,
                ..endpoint
            };
            let err =
                download_tarball_at(&hurried, "repo-x", &fixture.main_sha, &auth).unwrap_err();
            assert!(
                matches!(&err, GithubError::Download(message) if message.contains("took longer")),
                "{err}"
            );
        }
    }
}
