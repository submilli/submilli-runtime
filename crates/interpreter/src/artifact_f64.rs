//! serde bridge for an `f64` that reaches a persisted artifact.
//!
//! JSON has no non-finite number, and `serde_json` silently writes `null` for
//! one — which then fails to deserialize, so a package builds and installs
//! cleanly and is unloadable forever after. `Infinity` / `-Infinity` / `NaN` are
//! all reachable in a source position that lands here (a parameter default, a
//! literal type over `1e400`), so they round-trip as their source spelling.

use serde::{Deserialize, Deserializer, Serializer, de};

pub(crate) fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    if v.is_finite() {
        s.serialize_f64(*v)
    } else if v.is_nan() {
        s.serialize_str("NaN")
    } else if v.is_sign_positive() {
        s.serialize_str("Infinity")
    } else {
        s.serialize_str("-Infinity")
    }
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    // `expecting` replaces serde's default "data did not match any variant of
    // untagged enum Repr", which leaks a private name at the one caller that
    // matters: an artifact written before this bridge existed holds `null` here.
    #[derive(Deserialize)]
    #[serde(
        untagged,
        expecting = "a number, or \"Infinity\" / \"-Infinity\" / \"NaN\" (a `null` here is \
                     an artifact written by an older compiler — rebuild the package)"
    )]
    enum Repr {
        Finite(f64),
        NonFinite(String),
    }
    match Repr::deserialize(d)? {
        Repr::Finite(n) => Ok(n),
        Repr::NonFinite(text) => match text.as_str() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            other => Err(de::Error::custom(format!(
                "expected a number or a non-finite spelling, got `{other}`"
            ))),
        },
    }
}

/// The same bridge for a number enum's `(name, value)` table, where the
/// attribute has nowhere to sit on the `f64` inside the tuple. `Variant`
/// serializes as a two-element sequence, exactly as the bare tuple does, so
/// the wire format is unchanged for finite values.
pub(crate) mod pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Variant(String, #[serde(with = "super")] f64);

    pub(crate) fn serialize<S: Serializer>(
        variants: &[(String, f64)],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        variants
            .iter()
            .map(|(name, value)| Variant(name.clone(), *value))
            .collect::<Vec<_>>()
            .serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<(String, f64)>, D::Error> {
        Ok(Vec::<Variant>::deserialize(d)?
            .into_iter()
            .map(|Variant(name, value)| (name, value))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Probe(#[serde(with = "super")] f64);

    /// The message an artifact written before this bridge existed produces. It
    /// has to name the fix, not serde's private `Repr`.
    #[test]
    fn legacy_null_names_the_fix() {
        let err = serde_json::from_str::<Probe>("null")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("Repr"), "leaks a private type name: {err}");
        assert!(
            err.contains("rebuild the package"),
            "does not name a fix: {err}"
        );
    }

    #[test]
    fn accepts_the_pre_bridge_number_forms() {
        assert_eq!(serde_json::from_str::<Probe>("1.5").unwrap().0, 1.5);
        assert_eq!(serde_json::from_str::<Probe>("3").unwrap().0, 3.0);
        assert!(
            serde_json::from_str::<Probe>("-0.0")
                .unwrap()
                .0
                .is_sign_negative()
        );
    }
}
