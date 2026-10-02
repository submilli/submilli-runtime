//! `submilli github …` — the GitHub token `submilli install` and `submilli
//! build` send when fetching packages, which lets them reach private
//! repositories. A remote `submilli server packages install` never sends it:
//! the server fetches with its own token (`github_token_file`).

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

mod auth_status;
mod authenticate;
mod deauthenticate;
pub mod retry;
pub mod token;

#[derive(Subcommand)]
pub enum GithubCmd {
    /// Store a GitHub token for installing packages from private repositories.
    /// Prompts for it, or reads it from piped stdin. It needs Repository
    /// permissions → Contents: Read-only on the package repositories (a
    /// fine-grained token), or the `repo` scope (a classic token).
    Authenticate(authenticate::Args),
    /// Remove the stored GitHub token.
    Deauthenticate(deauthenticate::Args),
    /// Show which GitHub token package fetches use, and whose it is.
    #[command(name = "auth-status")]
    AuthStatus(auth_status::Args),
}

pub fn execute(cmd: GithubCmd) -> Result<ExitCode> {
    let result = match cmd {
        GithubCmd::Authenticate(args) => authenticate::run(&args),
        GithubCmd::Deauthenticate(args) => deauthenticate::run(&args),
        GithubCmd::AuthStatus(args) => auth_status::run(&args),
    };
    match result {
        Ok(message) => {
            println!("✓ {message}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(ExitCode::from(1))
        }
    }
}
