//! `submilli github deauthenticate` — remove the stored GitHub token.

use anyhow::Result;

use super::token;

#[derive(clap::Args)]
pub struct Args {}

pub fn run(_args: &Args) -> Result<String> {
    let removed = token::remove_stored()?;
    let mut message = if removed {
        "removed the stored GitHub token".to_string()
    } else {
        "no GitHub token was stored".to_string()
    };
    let auth = token::resolve_auth();
    if auth.token().is_some() {
        message.push_str(&format!(
            "; package fetches still use {}",
            auth.source().describe()
        ));
    }
    Ok(message)
}
