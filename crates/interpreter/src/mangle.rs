//! Fully-qualified symbol naming: `<package>#<symbol>` for public/top-level
//! package symbols. Internal module symbols use a separate `mod:` namespace so
//! they cannot collide with package-public names. Treat [`MangledName`] as
//! opaque; use the constructors rather than depending on the textual format.

use std::fmt;

use serde::{Deserialize, Serialize};

pub const PRELUDE_PACKAGE: &str = "submilli:prelude";
pub const USER_PACKAGE: &str = "main";
pub const SEP: char = '#';
const MODULE_PREFIX: &str = "mod:";

/// Opaque newtype; construct via the module builders to keep the format centralised.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MangledName(String);

impl MangledName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MangledName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for MangledName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Canonical public package symbol.
pub fn package_symbol(package: &str, symbol: &str) -> MangledName {
    MangledName(format!("{package}{SEP}{symbol}"))
}

/// Internal module symbol for a declaration in a multi-file package. `module` is
/// the canonicalized package-relative path (`util`, `internal/math`) — no
/// leading `./`, no `.subm` suffix. Root-exported (public) symbols use
/// [`package_symbol`] instead.
pub fn package_module_symbol(package: &str, module: &str, symbol: &str) -> MangledName {
    MangledName(format!(
        "{MODULE_PREFIX}{package}{SEP}{module}{SEP}{symbol}"
    ))
}

pub fn prelude(symbol: &str) -> MangledName {
    package_symbol(PRELUDE_PACKAGE, symbol)
}

/// Whether a symbol is declared by Submilli itself (the prelude or a
/// `submilli:` standard-library module) rather than by a program or package.
pub fn is_builtin(name: &MangledName) -> bool {
    let unprefixed = name
        .as_str()
        .strip_prefix(MODULE_PREFIX)
        .unwrap_or(name.as_str());
    unprefixed.starts_with("submilli:")
}

pub fn extend(parent: &MangledName, suffix: &str) -> MangledName {
    MangledName(format!("{}{}{}", parent.as_str(), SEP, suffix))
}

/// Dispatch key for a static class member: `Class#static#name`. The extra
/// `static` segment keeps statics disjoint from instance-method export keys
/// (`Class#name`) — a static and an instance method may share a name.
pub fn static_member(class: &MangledName, member: &str) -> MangledName {
    extend(&extend(class, "static"), member)
}

pub fn host(host_module: &str, symbol: &str) -> MangledName {
    package_symbol(host_module, symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn package_symbol_shape() {
        assert_eq!(package_symbol("main", "foo").as_str(), "main#foo");
        assert_eq!(
            package_symbol("submilli:uuid", "v4").as_str(),
            "submilli:uuid#v4"
        );
    }

    #[test]
    fn package_module_symbol_shape() {
        assert_eq!(
            package_module_symbol("main", "util", "foo").as_str(),
            "mod:main#util#foo"
        );
        assert_eq!(
            package_module_symbol("@acme/stripe", "internal/math", "helper").as_str(),
            "mod:@acme/stripe#internal/math#helper",
        );
    }

    #[test]
    fn package_and_module_symbols_cannot_collide() {
        assert_ne!(
            package_symbol("main#util", "foo"),
            package_module_symbol("main", "util", "foo"),
        );
    }

    #[test]
    fn prelude_shape() {
        assert_eq!(
            prelude("string_concat").as_str(),
            "submilli:prelude#string_concat"
        );
    }

    #[test]
    fn extend_composes_three_segments() {
        let iface = prelude("String");
        assert_eq!(
            extend(&iface, "concat").as_str(),
            "submilli:prelude#String#concat",
        );
    }

    #[test]
    fn host_shape() {
        assert_eq!(
            host("submilli:console", "log").as_str(),
            "submilli:console#log"
        );
    }

    #[test]
    fn newtype_keyed_lookup() {
        let mut map: BTreeMap<MangledName, u32> = BTreeMap::new();
        map.insert(package_symbol("main", "foo"), 42);
        assert_eq!(map.get(&package_symbol("main", "foo")), Some(&42));
        assert_eq!(map.get(&package_symbol("main", "absent")), None);
    }
}
