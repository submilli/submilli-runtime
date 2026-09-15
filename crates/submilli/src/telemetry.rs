//! Sentry telemetry for the CLI: crash reporting + per-invocation metrics.
//!
//! On by default. Opt out by setting `SUBMILLI_TELEMETRY` to a falsy value
//! (`0`/`false`/`no`/`off`) or the cross-tool `DO_NOT_TRACK` standard. When
//! opted out, no Sentry client is initialized, so every capture/metric call
//! elsewhere becomes a no-op.

const DSN: &str = "https://3de786dd0e1733e40a3e3425ab3e4ddc@o4511530557702144.ingest.us.sentry.io/4511530561110016";

/// Initialize Sentry unless the environment opts out. The returned guard must be
/// held for the process lifetime; `None` means telemetry is disabled.
pub fn init() -> Option<sentry::ClientInitGuard> {
    if opted_out_via_env() {
        return None;
    }
    Some(sentry::init((
        DSN,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // Capture user IPs and potentially sensitive headers via the HTTP integration.
            // https://docs.sentry.io/platforms/rust/data-management/data-collected
            send_default_pii: true,
            ..Default::default()
        },
    )))
}

/// True when the user opted out via `DO_NOT_TRACK` (set & truthy) or
/// `SUBMILLI_TELEMETRY` (set & falsy).
fn opted_out_via_env() -> bool {
    env_truthy("DO_NOT_TRACK") || env_falsy("SUBMILLI_TELEMETRY")
}

fn env_truthy(name: &str) -> bool {
    matches!(
        env_token(name).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

fn env_falsy(name: &str) -> bool {
    matches!(
        env_token(name).as_deref(),
        Some("0" | "false" | "no" | "off")
    )
}

fn env_token(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
}
