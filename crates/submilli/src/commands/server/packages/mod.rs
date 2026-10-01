use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod install;
pub mod list;
pub mod ssh_key;
pub mod uninstall;

#[derive(Subcommand)]
pub enum PackagesCmd {
    /// Install a GitHub package into the server's store. Private repos are
    /// fetched over SSH with the server's own key (see `ssh-key`).
    Install(install::Args),
    /// List the packages the server can resolve, marking any read from a
    /// fallback store.
    List(list::Args),
    /// Remove an installed package from the server's store.
    Uninstall(uninstall::Args),
    /// Print the public SSH key the server installs private GitHub packages
    /// with; add it to the repository as a deploy key.
    SshKey(ssh_key::Args),
}

pub fn execute(cmd: PackagesCmd) -> Result<ExitCode> {
    match cmd {
        PackagesCmd::Install(args) => install::execute(args),
        PackagesCmd::List(args) => list::execute(args),
        PackagesCmd::Uninstall(args) => uninstall::execute(args),
        PackagesCmd::SshKey(args) => ssh_key::execute(args),
    }
}
