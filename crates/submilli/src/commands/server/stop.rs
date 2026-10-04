//! `submilli server stop` — ask a running server to drain and exit, then wait
//! until it stops answering.

use std::process::ExitCode;
use std::time::Duration;

use crate::commands::http::{ServerTarget, ok_or_report};

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    target: ServerTarget,
}

/// How long to wait for the server to finish draining before giving up.
const POLL_ATTEMPTS: u32 = 50;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let base = args.target.base();
    let agent = match args.target.agent() {
        Ok(agent) => agent,
        Err(error) if crate::commands::http::connection_refused(&error) => {
            println!("already stopped (no server at {base})");
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => return Err(error),
    };

    let resp = match agent.post(&format!("{base}/v1/shutdown")).send_empty() {
        Ok(response) => response,
        Err(ureq::Error::Io(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            println!("already stopped (no server at {base})");
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => return Err(error.into()),
    };
    if ok_or_report(resp).is_none() {
        return Ok(ExitCode::from(1));
    }

    // Poll until the listener stops accepting — then it's down. `/healthz`
    // rather than `/v1/status`: any answer at all means "still up", and this
    // one needs no token.
    for _ in 0..POLL_ATTEMPTS {
        match agent.get(&format!("{base}/healthz")).call() {
            Ok(_) => {}
            Err(ureq::Error::Io(error))
                if error.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                println!("stopped");
                return Ok(ExitCode::SUCCESS);
            }
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    eprintln!("error: server did not stop within {POLL_ATTEMPTS} attempts");
    Ok(ExitCode::from(1))
}
