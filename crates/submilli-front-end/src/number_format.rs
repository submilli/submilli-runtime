//! JavaScript's `Number#toString()` text, as type rendering prints number
//! literal types.

/// ECMAScript ToString for f64 — spells NaN/Infinity/-Infinity and
/// normalizes -0 to "0" (Rust's default prints "inf" and keeps the sign).
pub fn format_number_js(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if n == 0.0 {
        "0".to_string()
    } else if n.abs() >= 1e21 || n.abs() < 1e-6 {
        jsify_exponent(&format!("{n:e}"))
    } else {
        n.to_string()
    }
}

/// JS-style exponent spelling: `1.5e3` → `1.5e+3` (negative exponents
/// already carry their sign).
pub fn jsify_exponent(s: &str) -> String {
    match s.split_once('e') {
        Some((mantissa, exponent)) if !exponent.starts_with('-') => {
            format!("{mantissa}e+{exponent}")
        }
        _ => s.to_string(),
    }
}
