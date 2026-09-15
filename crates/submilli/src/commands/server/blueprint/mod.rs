use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;

pub mod add;
pub mod apply;
pub mod list;
pub mod remove;
pub mod show;

#[derive(Subcommand)]
pub enum BlueprintCmd {
    /// Register a new blueprint file with the server; fails if the name is taken.
    Add(add::Args),
    /// Register a blueprint file, replacing any existing one with the same name.
    Apply(apply::Args),
    /// List blueprints registered on the server.
    List(list::Args),
    /// Print a registered blueprint's YAML.
    Show(show::Args),
    /// Unregister a blueprint by name.
    Remove(remove::Args),
}

pub fn execute(cmd: BlueprintCmd) -> Result<ExitCode> {
    match cmd {
        BlueprintCmd::Add(args) => add::execute(args),
        BlueprintCmd::Apply(args) => apply::execute(args),
        BlueprintCmd::List(args) => list::execute(args),
        BlueprintCmd::Show(args) => show::execute(args),
        BlueprintCmd::Remove(args) => remove::execute(args),
    }
}
