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

    match agent.post(&format!("{base}/v1/shutdown")).send_empty() {
        Ok(response) => {
            if ok_or_report(response).is_none() {
                return Ok(ExitCode::from(1));
            }
        }
        Err(ureq::Error::Io(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            println!("already stopped (no server at {base})");
            return Ok(ExitCode::SUCCESS);
        }
        // The server may close the connection as it starts draining, before
        // its reply is read. Whether it is stopping is settled by the polling
        // below, which only succeeds once a fresh connection is refused.
        Err(ureq::Error::Io(error)) if closed_mid_exchange(&error) => {}
        Err(error) => return Err(error.into()),
    }

    if wait_until_stopped(|| agent.get(&format!("{base}/healthz")).call().map(|_| ()))? {
        println!("stopped");
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("error: server did not stop within {POLL_ATTEMPTS} attempts");
    Ok(ExitCode::from(1))
}

fn wait_until_stopped(mut probe: impl FnMut() -> Result<(), ureq::Error>) -> anyhow::Result<bool> {
    // Any health response means the listener is still up. A reset can race with
    // listener shutdown, so retry until a fresh connection is refused.
    for _ in 0..POLL_ATTEMPTS {
        match probe() {
            Ok(()) => {}
            Err(ureq::Error::Io(error))
                if error.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                return Ok(true);
            }
            Err(ureq::Error::Io(error)) if closed_mid_exchange(&error) => {}
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Ok(false)
}

/// Whether the server closed an open connection mid-request, as a draining
/// listener can: reset, or closed before the response (`UnexpectedEof`).
fn closed_mid_exchange(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn a_reset_during_drain_requires_a_later_refused_connection() {
        let mut responses = [
            Err(ureq::Error::Io(ErrorKind::ConnectionReset.into())),
            Ok(()),
            Err(ureq::Error::Io(ErrorKind::ConnectionRefused.into())),
        ]
        .into_iter();
        let stopped = wait_until_stopped(|| responses.next().expect("unexpected extra poll"))
            .expect("shutdown polling");
        assert!(stopped);
        assert!(responses.next().is_none(), "reset must not report success");
    }

    #[test]
    fn a_connection_closed_before_the_response_keeps_polling() {
        let mut responses = [
            Err(ureq::Error::Io(ErrorKind::UnexpectedEof.into())),
            Err(ureq::Error::Io(ErrorKind::ConnectionRefused.into())),
        ]
        .into_iter();
        let stopped = wait_until_stopped(|| responses.next().expect("unexpected extra poll"))
            .expect("shutdown polling");
        assert!(stopped);
    }

    #[test]
    fn unrelated_poll_errors_are_reported() {
        let error = wait_until_stopped(|| Err(ureq::Error::Io(ErrorKind::PermissionDenied.into())))
            .expect_err("permission failure must not report a stopped server");
        assert!(matches!(
            error.downcast_ref::<ureq::Error>(),
            Some(ureq::Error::Io(error)) if error.kind() == ErrorKind::PermissionDenied
        ));
    }
}
