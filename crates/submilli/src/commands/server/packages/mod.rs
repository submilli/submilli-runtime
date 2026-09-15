use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod install;
pub mod list;
pub mod uninstall;

#[derive(Subcommand)]
pub enum PackagesCmd {
    /// Install a GitHub package into the server's store.
    Install(install::Args),
    /// List the packages installed in the server's store.
    List(list::Args),
    /// Remove an installed package from the server's store.
    Uninstall(uninstall::Args),
}

pub fn execute(cmd: PackagesCmd) -> Result<ExitCode> {
    match cmd {
        PackagesCmd::Install(args) => install::execute(args),
        PackagesCmd::List(args) => list::execute(args),
        PackagesCmd::Uninstall(args) => uninstall::execute(args),
    }
}
