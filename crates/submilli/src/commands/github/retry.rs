//! The retry that offers to store a GitHub token when a fetch finds no public
//! repository, and the lazily looked-up token install and build fetch with.

use std::io::IsTerminal;
use std::sync::OnceLock;

use submilli_build::{FetchError, FetchedRepo, RepoFetcher};
use submilli_shared::github::{GithubAuth, GithubRepoFetcher, TOKEN_PERMISSIONS, TokenSource};

use super::{authenticate, token};

/// Run `attempt` with the CLI's token. If it failed on a repository that may
/// be private and, at a terminal, the user stores a token when offered, run
/// it once more with that token. Warns when GitHub rejected the token in use.
///
/// Whether to offer is read from the token's state, not `T`: a repository not
/// found without a token is always a failure for install and build.
pub fn with_authentication_retry<T>(mut attempt: impl FnMut(&LazyAuth) -> T) -> T {
    let auth = LazyAuth::default();
    let outcome = attempt(&auth);
    let stored_new_token = auth.resolved().is_some_and(offer_authentication);
    if !stored_new_token {
        auth.warn_if_rejected();
        return outcome;
    }
    let retry = LazyAuth::default();
    let outcome = attempt(&retry);
    retry.warn_if_rejected();
    outcome
}

/// After a fetch failed because a repository wasn't found without a token,
/// offer, at a terminal, to store a token now. `true` if one was stored, so
/// the caller should retry. The failure itself has already been reported,
/// with how to authenticate.
fn offer_authentication(auth: &GithubAuth) -> bool {
    let Some(owner) = auth.unauthenticated_not_found() else {
        return false;
    };
    // A token in the environment wins over a stored one, so storing another
    // wouldn't change the retry.
    if matches!(auth.source(), TokenSource::Env(_)) {
        return false;
    }
    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        return false;
    }
    let question = format!(
        "The repository may be private. Store a GitHub token with {TOKEN_PERMISSIONS} on {owner}'s \
         repositories now?"
    );
    let accepted = dialoguer::Confirm::new()
        .with_prompt(question)
        .default(false)
        .interact()
        .unwrap_or(false);
    if !accepted {
        return false;
    }
    match authenticate::authenticate(Some(owner)) {
        Ok(message) => {
            eprintln!("✓ {message}");
            true
        }
        Err(err) => {
            eprintln!("error: {err:#}");
            false
        }
    }
}

/// The CLI's token, looked up on first use, so a build whose dependencies are
/// all locked and installed never runs `gh` or reads the stored token.
#[derive(Default)]
pub struct LazyAuth(OnceLock<GithubAuth>);

impl LazyAuth {
    pub fn get(&self) -> &GithubAuth {
        self.0.get_or_init(token::resolve_auth)
    }

    /// A dependency fetcher that looks the token up when it first fetches.
    pub fn fetcher(&self) -> LazyFetcher<'_> {
        LazyFetcher(self)
    }

    fn resolved(&self) -> Option<&GithubAuth> {
        self.0.get()
    }

    fn warn_if_rejected(&self) {
        // A repository not found after the rejection already said so.
        let Some(auth) = self
            .resolved()
            .filter(|auth| auth.token_rejected() && auth.unauthenticated_not_found().is_none())
        else {
            return;
        };
        eprintln!(
            "warning: GitHub rejected {} (expired or revoked), so packages were fetched without \
             it; {}",
            auth.source().describe(),
            auth.source().how_to_authenticate(None)
        );
    }
}

/// [`GithubRepoFetcher`] with a token looked up on the first fetch.
pub struct LazyFetcher<'a>(&'a LazyAuth);

impl RepoFetcher for LazyFetcher<'_> {
    fn fetch(&self, url: &str, sha: &str) -> Result<FetchedRepo, FetchError> {
        GithubRepoFetcher::new(self.0.get()).fetch(url, sha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing is looked up until a fetch needs the token.
    #[test]
    fn a_lazy_token_is_not_resolved_until_used() {
        let auth = LazyAuth::default();
        let _fetcher = auth.fetcher();
        assert!(auth.resolved().is_none());
    }
}
