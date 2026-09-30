//! `submilli server blueprint remove <name> [--server <url>]` — unregister
//! a blueprint by name.

use std::process::ExitCode;

use anyhow::Result;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    name: String,

    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let base = args.target.base();
    let url = format!("{base}/v1/blueprints/{}", args.name);

    let agent = args.target.agent()?;
    let resp = match agent.delete(&url).call() {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        println!("Removed blueprint '{}'", args.name);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
