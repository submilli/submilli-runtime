//! `submilli server secret put <key> [--server <url>]` — store a secret in the
//! server's secret store. The value is prompted for, or read from stdin, so it
//! never appears in the process's argument list.

use std::process::ExitCode;

use anyhow::Result;

use crate::commands::local;

use crate::commands::http::{ServerTarget, error_message};

#[derive(clap::Args)]
pub struct Args {
    key: String,

    #[command(flatten)]
    target: ServerTarget,
}

pub fn execute(args: Args) -> Result<ExitCode> {
    let value = local::read_secret_value(&args.key)?;

    let base = args.target.base();
    let url = format!("{base}/v1/secrets");

    let agent = args.target.agent()?;
    let resp = match agent
        .post(&url)
        .send_json(serde_json::json!({ "key": args.key, "value": value }))
    {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(1));
        }
    };

    let status = resp.status().as_u16();
    if status == 200 {
        println!("Stored secret '{}'", args.key);
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: {}", error_message(resp));
        Ok(ExitCode::from(1))
    }
}
