//! The local user's SSH identity for fetching private GitHub packages:
//! ssh-agent, then `~/.ssh/id_ed25519`, `id_ecdsa`, `id_rsa`, verified against
//! `~/.ssh/known_hosts`. Passphrase-protected keys are asked for at the
//! terminal; without one they are skipped.

use std::io::IsTerminal;
use std::path::Path;

use submilli_shared::github::{FetchAuth, KnownHosts, LocalIdentities, PassphrasePrompt};
use zeroize::Zeroizing;

pub(crate) struct LocalSsh {
    prompt: Option<TerminalPrompt>,
    known_hosts: KnownHosts,
}

impl LocalSsh {
    pub(crate) fn new() -> Self {
        let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        Self {
            prompt: interactive.then_some(TerminalPrompt),
            known_hosts: KnownHosts::user_default(),
        }
    }

    pub(crate) fn identities(&self) -> LocalIdentities<'_> {
        LocalIdentities::user_default(
            self.prompt
                .as_ref()
                .map(|prompt| prompt as &dyn PassphrasePrompt),
        )
    }

    pub(crate) fn auth<'a>(&'a self, identities: &'a LocalIdentities<'a>) -> FetchAuth<'a> {
        FetchAuth::Local {
            identities,
            known_hosts: &self.known_hosts,
        }
    }
}

struct TerminalPrompt;

impl PassphrasePrompt for TerminalPrompt {
    fn passphrase(&self, key_path: &Path, retry: bool) -> Option<Zeroizing<String>> {
        let prompt = if retry {
            format!("Wrong passphrase. Passphrase for {}", key_path.display())
        } else {
            format!("Passphrase for {}", key_path.display())
        };
        dialoguer::Password::new()
            .with_prompt(prompt)
            .allow_empty_password(true)
            .interact()
            .ok()
            .filter(|entered| !entered.is_empty())
            .map(Zeroizing::new)
    }
}
