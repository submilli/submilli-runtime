//! Shared opt-in for the expensive conformance suite bodies.

pub fn requested() -> bool {
    std::env::var("SUBMILLI_CONFORMANCE_TEST").is_ok_and(|value| enabled(&value))
}

fn enabled(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::enabled;

    #[test]
    fn requires_explicit_opt_in() {
        for value in ["", "0", "false", "no", "off", "typo", "2"] {
            assert!(!enabled(value), "unexpected opt-in: {value}");
        }
    }

    #[test]
    fn accepts_documented_true_values() {
        for value in ["1", "true", "yes", "on", "TRUE", "On"] {
            assert!(enabled(value), "missing opt-in: {value}");
        }
    }
}
