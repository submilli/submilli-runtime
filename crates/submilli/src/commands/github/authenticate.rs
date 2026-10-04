//! `submilli github authenticate` — check a GitHub token with GitHub and store
//! it for package fetches. Re-run to replace it.

use std::io::IsTerminal;

use anyhow::{Result, bail};
use submilli_shared::github::{GithubToken, is_valid_owner, token_identity, token_link};

use super::token;
use crate::commands::local;

#[derive(clap::Args)]
pub struct Args {
    /// The user or organization whose repositories the token is for; fills it
    /// in on the token-creation link.
    #[arg(long)]
    owner: Option<String>,
}

pub fn run(args: &Args) -> Result<String> {
    if let Some(owner) = args.owner.as_deref()
        && !is_valid_owner(owner)
    {
        bail!("`{owner}` is not a GitHub user or organization name");
    }
    authenticate(args.owner.as_deref())
}

/// Read a token at a terminal or from piped stdin, check it with GitHub, and
/// store it. Returns a summary: whose token it is and when it expires.
pub fn authenticate(owner: Option<&str>) -> Result<String> {
    if std::io::stdin().is_terminal() {
        eprintln!("{}", instructions(owner));
    }
    let raw = local::read_hidden("GitHub token", "the GitHub token")?;
    let github_token = GithubToken::parse(&raw)?;
    let Some(identity) = token_identity(&github_token)? else {
        bail!(
            "GitHub won't identify this token, as for a GitHub App or Actions token; set it in \
             `GH_TOKEN` or `GITHUB_TOKEN` instead of storing it"
        );
    };
    token::store(&github_token)?;
    let mut message = format!(
        "stored a GitHub token for {} ({}) in {}",
        identity.login,
        identity.expiry(),
        token::stored_path().display()
    );
    if let Some(name) = token::env_token_var() {
        message.push_str(&format!(
            "; `{name}` is set and is used instead until you unset it"
        ));
    }
    Ok(message)
}

/// How to create a token, for `owner`'s repositories when known.
fn instructions(owner: Option<&str>) -> String {
    format!(
        "Create a fine-grained token at {link}\n  \
         Repository access: Only select repositories, choosing the package repositories\n  \
         Repository permissions: Contents → Read-only (GitHub adds Metadata → Read-only)\n\
         A classic token with the `repo` scope also works, but can do far more.",
        link = token_link(owner)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_name_the_exact_permission() {
        let text = instructions(Some("acme"));
        assert!(text.contains("Contents → Read-only"), "{text}");
        assert!(text.contains("target_name=acme"), "{text}");
    }
}
