//! Warning enforcement shared by CLI and server entry points.

pub fn deny_warnings_from_env() -> bool {
    std::env::var_os("SUBMILLI_DENY_WARNINGS").is_some_and(|value| value == "1")
}

pub fn warning_denial_message(count: usize) -> String {
    format!("{count} warning(s) treated as errors (--deny-warnings)")
}
