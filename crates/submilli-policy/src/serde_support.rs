//! Serde helpers shared by the policy types and the formats that embed them.
//! Not part of the policy API.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, de};

/// Name-keyed maps must not silently replace an earlier declaration.
pub fn unique_map<'de, D, V>(deserializer: D) -> Result<BTreeMap<String, V>, D::Error>
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
                map.next_value_seed(Reject::<V>::new(format!(
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

/// A map value whose deserialization always fails, standing in for a value of
/// type `T` so it can replace any field's seed.
///
/// Raising the error from *inside* the value is what anchors the diagnostic at
/// that key: an error returned from the enclosing map's visitor carries only the
/// map's path, which is too coarse for an editor to squiggle the offending line.
/// Failing from within the visitor rather than after consuming the node also
/// keeps the parser's mark on the value, so the reported line is the offending
/// one and not the first line of the block.
pub struct Reject<T>(String, PhantomData<T>);

impl<T> Reject<T> {
    pub fn new(message: impl Into<String>) -> Self {
        Reject(message.into(), PhantomData)
    }

    fn fail<E: de::Error>(self) -> Result<T, E> {
        Err(de::Error::custom(self.0))
    }
}

impl<'de, T> de::DeserializeSeed<'de> for Reject<T> {
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

// Every shape of value gets the same message; the default `Visitor` methods
// would replace it with serde's own `invalid type` wording.
impl<'de, T> de::Visitor<'de> for Reject<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<T, E> {
        self.fail()
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<T, E> {
        self.fail()
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<T, E> {
        self.fail()
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<T, E> {
        self.fail()
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<T, E> {
        self.fail()
    }

    fn visit_bytes<E: de::Error>(self, _: &[u8]) -> Result<T, E> {
        self.fail()
    }

    fn visit_unit<E: de::Error>(self) -> Result<T, E> {
        self.fail()
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }

    fn visit_map<A: de::MapAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, _: A) -> Result<T, A::Error> {
        self.fail()
    }
}
