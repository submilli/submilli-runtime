use std::collections::BTreeSet;

use submilli_blueprint::{Blueprint, FieldMatch, FilterExpr};
use submilli_build::CapabilitySchema;

const SECRETS_GET: &str = "secrets.get";

pub fn missing_package_secret_warnings(
    blueprint: &Blueprint,
    package: &str,
    capabilities: &CapabilitySchema,
) -> Vec<String> {
    required_secret_names(capabilities)
        .into_iter()
        .filter(|secret| !blueprint.secrets.contains_key(secret))
        .map(|secret| {
            format!(
                "package `{package}` requires secret `{secret}`, but `secrets:` does not declare it"
            )
        })
        .collect()
}

fn required_secret_names(capabilities: &CapabilitySchema) -> BTreeSet<String> {
    capabilities
        .requires
        .iter()
        .filter(|required| required.capability == SECRETS_GET)
        .filter_map(|required| required.filter.as_deref())
        .filter_map(parse_filter)
        .flat_map(|filter| {
            filter
                .field_matches("name")
                .into_iter()
                .filter_map(|field| {
                    let FieldMatch::Equals(secret) = field else {
                        return None;
                    };
                    Some(secret)
                })
        })
        .collect()
}

fn parse_filter(filter: &str) -> Option<FilterExpr> {
    // A Rust string is a supported YAML scalar; the in-memory writer cannot
    // fail on its contents. Filter syntax is checked by deserialization below.
    serde_yml::from_str(&serde_yml::to_string(filter).expect("filter string serializes")).ok()
}

#[cfg(test)]
mod tests {
    use submilli_build::RequiredCapability;

    use super::*;

    #[test]
    fn warns_for_missing_literal_secret_requirement() {
        let blueprint = submilli_blueprint::parse("name: test\n").expect("blueprint parses");
        let capabilities = CapabilitySchema {
            requires: vec![RequiredCapability {
                capability: SECRETS_GET.to_string(),
                filter: Some("name == \"STRIPE_API_KEY\"".to_string()),
            }],
            ..CapabilitySchema::default()
        };

        let warnings = missing_package_secret_warnings(&blueprint, "@acme/app", &capabilities);

        assert_eq!(
            warnings,
            vec![
                "package `@acme/app` requires secret `STRIPE_API_KEY`, but `secrets:` does not declare it"
            ]
        );
    }

    #[test]
    fn declared_secret_suppresses_warning() {
        let blueprint = submilli_blueprint::parse(
            "name: test\nsecrets:\n  STRIPE_API_KEY: { store: STRIPE_API_KEY }\n",
        )
        .expect("blueprint parses");
        let capabilities = CapabilitySchema {
            requires: vec![RequiredCapability {
                capability: SECRETS_GET.to_string(),
                filter: Some("name == \"STRIPE_API_KEY\"".to_string()),
            }],
            ..CapabilitySchema::default()
        };

        let warnings = missing_package_secret_warnings(&blueprint, "@acme/app", &capabilities);

        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn ignores_non_literal_secret_filters() {
        let capabilities = CapabilitySchema {
            requires: vec![
                RequiredCapability {
                    capability: SECRETS_GET.to_string(),
                    filter: None,
                },
                RequiredCapability {
                    capability: SECRETS_GET.to_string(),
                    filter: Some("name glob \"STRIPE_*\"".to_string()),
                },
            ],
            ..CapabilitySchema::default()
        };

        assert!(required_secret_names(&capabilities).is_empty());
    }
}
