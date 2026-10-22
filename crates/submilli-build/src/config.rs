//! Explicit build inputs and the configuration recorded with an installation.
use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageConfig {
    #[serde(default)]
    pub overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub values: BTreeMap<String, interpreter::package_config::ConfigValue>,
}

#[derive(Clone, Debug, Default)]
pub struct ConfigInputs {
    pub values: BTreeMap<String, String>,
    pub reset: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigInputError(pub String);

impl fmt::Display for ConfigInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ConfigInputError {}

impl ConfigInputs {
    pub fn parse(
        values: &[String],
        reset: &[String],
        selected: bool,
    ) -> Result<Self, ConfigInputError> {
        let mut inputs = Self {
            values: BTreeMap::new(),
            reset: reset.to_vec(),
        };
        for value in values {
            let (name, value) = value
                .split_once('=')
                .ok_or_else(|| ConfigInputError("expected --config NAME=VALUE".into()))?;
            if name.is_empty()
                || inputs
                    .values
                    .insert(name.to_owned(), value.to_owned())
                    .is_some()
            {
                return Err(ConfigInputError(format!(
                    "empty or duplicate configuration parameter `{name}`"
                )));
            }
        }
        inputs.validate(selected)?;
        Ok(inputs)
    }

    pub fn validate(&self, selected: bool) -> Result<(), ConfigInputError> {
        if !selected && (!self.values.is_empty() || !self.reset.is_empty()) {
            return Err(ConfigInputError(
                "configuration inputs require an explicitly selected package".into(),
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for name in &self.reset {
            if name.is_empty() || !names.insert(name) || self.values.contains_key(name) {
                return Err(ConfigInputError(format!(
                    "duplicate, empty, or conflicting --reset-config parameter `{name}`"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_inputs_reject_duplicates_conflicts_and_unselected_packages() {
        for (values, reset, selected) in [
            (vec!["a=1", "a=2"], vec![], true),
            (vec!["a=1"], vec!["a"], true),
            (vec![], vec!["a", "a"], true),
            (vec!["a=1"], vec![], false),
            (vec!["=x"], vec![], true),
            (vec!["a"], vec![], true),
        ] {
            let values = values.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let reset = reset.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(ConfigInputs::parse(&values, &reset, selected).is_err());
        }
        let inputs = ConfigInputs::parse(
            &["url=https://test/?a=b".into(), "empty=".into()],
            &[],
            true,
        )
        .unwrap();
        assert_eq!(inputs.values["url"], "https://test/?a=b");
        assert_eq!(inputs.values["empty"], "");
    }
}
