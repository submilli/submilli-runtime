//! `submilli server stop` — ask a running server to drain and exit, then wait
//! until it stops answering.

use std::process::ExitCode;
use std::time::Duration;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8128")]
    server: String,
}

/// How long to wait for the server to finish draining before giving up.
const POLL_ATTEMPTS: u32 = 50;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn execute(args: Args) -> anyhow::Result<ExitCode> {
    let base = args.server.trim_end_matches('/');
    let agent: ureq::Agent = ureq::Agent::config_builder().build().into();

    if agent
        .post(&format!("{base}/v1/shutdown"))
        .send_empty()
        .is_err()
    {
        println!("already stopped (no server at {base})");
        return Ok(ExitCode::SUCCESS);
    }

    // Poll /v1/status until the listener stops accepting — then it's down.
    for _ in 0..POLL_ATTEMPTS {
        if agent.get(&format!("{base}/v1/status")).call().is_err() {
            println!("stopped");
            return Ok(ExitCode::SUCCESS);
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    eprintln!("error: server did not stop within {POLL_ATTEMPTS} attempts");
    Ok(ExitCode::from(1))
}
