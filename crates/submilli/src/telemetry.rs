//! Sentry telemetry for the CLI: crash reporting + per-invocation metrics.
//!
//! Off by default. Opt in by setting `SUBMILLI_TELEMETRY` to a truthy value
//! (`1`/`true`/`yes`/`on`).
//! When disabled, no Sentry client is initialized, so every capture/metric call
//! elsewhere becomes a no-op.

const DSN: &str = "https://3de786dd0e1733e40a3e3425ab3e4ddc@o4511530557702144.ingest.us.sentry.io/4511530561110016";

/// Initialize Sentry only when the environment explicitly opts in. The returned guard must be
/// held for the process lifetime; `None` means telemetry is disabled.
pub fn init() -> Option<sentry::ClientInitGuard> {
    if !telemetry_enabled(std::env::var("SUBMILLI_TELEMETRY").ok().as_deref()) {
        return None;
    }
    Some(sentry::init((
        DSN,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // Never send the machine's IP or other personal data; nothing here
            // needs it.
            // https://docs.sentry.io/platforms/rust/data-management/data-collected
            send_default_pii: false,
            ..Default::default()
        },
    )))
}

fn telemetry_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::telemetry_enabled;

    #[test]
    fn telemetry_requires_explicit_opt_in() {
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("no"),
            Some("off"),
            Some("invalid"),
        ] {
            assert!(!telemetry_enabled(value), "value: {value:?}");
        }
        for value in ["1", "true", "yes", "on", " TRUE ", "On"] {
            assert!(telemetry_enabled(Some(value)), "value: {value:?}");
        }
    }
}
