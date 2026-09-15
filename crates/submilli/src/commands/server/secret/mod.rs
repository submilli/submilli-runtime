use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod delete;
pub mod list;
pub mod put;

#[derive(Subcommand)]
pub enum SecretCmd {
    /// Store a secret; the value is read from stdin.
    Put(put::Args),
    /// Delete a secret by key.
    Delete(delete::Args),
    /// List secret keys, optionally filtered by prefix.
    List(list::Args),
}

pub fn execute(cmd: SecretCmd) -> Result<ExitCode> {
    match cmd {
        SecretCmd::Put(args) => put::execute(args),
        SecretCmd::Delete(args) => delete::execute(args),
        SecretCmd::List(args) => list::execute(args),
    }
}
