//! Structured diagnostic payloads for [`crate::BlueprintError`]: the message
//! plus, when known, the YAML path (as segments, so keys containing `.`/`/`
//! stay unambiguous) and the 1-based line/column of the offending text. HTTP
//! handlers surface these so editors can anchor the error in the source.

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSeg {
    Key(String),
    Index(usize),
}

impl Serialize for PathSeg {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PathSeg::Key(key) => serializer.serialize_str(key),
            PathSeg::Index(index) => serializer.serialize_u64(*index as u64),
        }
    }
}

impl From<&str> for PathSeg {
    fn from(key: &str) -> Self {
        PathSeg::Key(key.to_string())
    }
}

impl From<&String> for PathSeg {
    fn from(key: &String) -> Self {
        PathSeg::Key(key.clone())
    }
}

impl From<String> for PathSeg {
    fn from(key: String) -> Self {
        PathSeg::Key(key)
    }
}

impl From<usize> for PathSeg {
    fn from(index: usize) -> Self {
        PathSeg::Index(index)
    }
}

pub type YamlPath = Vec<PathSeg>;

/// Build a [`YamlPath`] from mixed key/index segments:
/// `yaml_path!["permissions", caller, i, "capability"]`.
#[macro_export]
macro_rules! yaml_path {
    ($($seg:expr),* $(,)?) => {
        vec![$($crate::PathSeg::from($seg)),*]
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The un-prefixed leaf message; `BlueprintError`'s `Display` adds the
    /// per-variant prefix around it.
    pub message: String,
    pub path: Option<YamlPath>,
    /// 1-based `(line, column)`; present only when the YAML parser reported one.
    pub location: Option<(usize, usize)>,
}

impl Fault {
    pub fn at(path: YamlPath, message: impl Into<String>) -> Self {
        Fault {
            message: message.into(),
            path: Some(path),
            location: None,
        }
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Self {
        Fault {
            message,
            path: None,
            location: None,
        }
    }
}

impl From<&str> for Fault {
    fn from(message: &str) -> Self {
        Fault::from(message.to_string())
    }
}
