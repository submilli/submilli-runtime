//! `submilli github auth-status` — which GitHub token package fetches send,
//! and whose it is. Never prints the token.

use anyhow::Result;
use submilli_shared::github::{GithubError, token_identity};

use super::token;

#[derive(clap::Args)]
pub struct Args {}

pub fn run(_args: &Args) -> Result<String> {
    let auth = token::resolve_auth();
    let Some(github_token) = auth.token() else {
        return Ok(
            "no GitHub token: packages are fetched from public repositories only; run \
             `submilli github authenticate` to reach private ones"
                .to_string(),
        );
    };
    let source = auth.source().describe();
    match token_identity(github_token) {
        Ok(None) => Ok(format!(
            "using {source}; GitHub won't identify it (a GitHub App or Actions token), so its \
             owner and expiry are unknown"
        )),
        Ok(Some(identity)) => Ok(format!(
            "using {source}, for {} ({})",
            identity.login,
            identity.expiry()
        )),
        Err(GithubError::Access(reason)) => anyhow::bail!(
            "{source}: {reason}; {}",
            auth.source().how_to_authenticate(None)
        ),
        Err(err) => anyhow::bail!("using {source}, but couldn't check it with GitHub: {err}"),
    }
}
