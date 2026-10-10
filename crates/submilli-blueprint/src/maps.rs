use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, de};

/// Name-keyed blueprint maps must not silently replace an earlier declaration.
pub(crate) fn deserialize<'de, D, V>(deserializer: D) -> Result<BTreeMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    deserializer.deserialize_map(UniqueNames(PhantomData))
}

struct UniqueNames<V>(PhantomData<V>);

impl<'de, V: Deserialize<'de>> de::Visitor<'de> for UniqueNames<V> {
    type Value = BTreeMap<String, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map with unique names")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut names = BTreeMap::new();
        while let Some(name) = map.next_key::<String>()? {
            if names.contains_key(&name) {
                map.next_value_seed(crate::Reject::<V>::new(format!(
                    "duplicate key `{name}`: remove or rename the repeated declaration"
                )))?;
                continue;
            }
            let value = map.next_value()?;
            names.insert(name, value);
        }
        Ok(names)
    }
}
