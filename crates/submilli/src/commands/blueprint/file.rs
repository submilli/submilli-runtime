//! Shared load/save for commands that edit a local `blueprint.yaml`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const DEFAULT_FILE: &str = "blueprint.yaml";

pub(super) fn blueprint_path(arg: &Option<PathBuf>) -> PathBuf {
    arg.clone().unwrap_or_else(|| PathBuf::from(DEFAULT_FILE))
}

pub(super) fn load(path: &Path) -> Result<submilli_blueprint::Blueprint> {
    let yaml = fs::read_to_string(path).with_context(|| {
        format!(
            "reading {} (run `submilli blueprint init` first?)",
            path.display()
        )
    })?;
    submilli_blueprint::parse(&yaml).with_context(|| format!("parsing {}", path.display()))
}

pub(super) fn write(path: &Path, blueprint: &submilli_blueprint::Blueprint) -> Result<()> {
    let updated = submilli_blueprint::to_yaml(blueprint);
    // Re-validate before writing so we never leave an unparseable blueprint on disk.
    submilli_blueprint::parse(&updated)
        .context("the resulting blueprint is invalid — not written")?;
    fs::write(path, &updated).with_context(|| format!("writing {}", path.display()))
}

/// Whether `caller` has any rule for `capability`, whatever its filter or action.
pub(super) fn has_capability_rule(
    blueprint: &submilli_blueprint::Blueprint,
    caller: &str,
    capability: &str,
) -> bool {
    blueprint
        .permissions
        .get(caller)
        .is_some_and(|rules| rules.iter().any(|rule| rule.capability == capability))
}

/// Whether `caller` has a rule for `capability` with no filter: one that
/// decides every call earlier rules leave, so none reaches `default:`.
pub(super) fn has_unfiltered_capability_rule(
    blueprint: &submilli_blueprint::Blueprint,
    caller: &str,
    capability: &str,
) -> bool {
    blueprint.permissions.get(caller).is_some_and(|rules| {
        rules
            .iter()
            .any(|rule| rule.capability == capability && rule.filter.is_none())
    })
}
