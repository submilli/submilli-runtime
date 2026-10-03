//! Session-bound guest paths and volume-relative directory selections.
use crate::{Blueprint, BlueprintError, Fault, VarBindings, VfsConfig, YamlPath, yaml_path};

const MAX_PATH_BYTES: usize = 4096;

impl VfsConfig {
    pub fn cwd(&self) -> &str {
        match self {
            Self::None => "/",
            Self::Ephemeral { cwd, .. }
            | Self::PerSession { cwd, .. }
            | Self::Named { cwd, .. } => cwd.as_deref().unwrap_or("/"),
        }
    }

    /// Bind paths before opening directories. Values are substituted once, never
    /// interpreted as another template or permitted to introduce path separators.
    pub fn resolve(&self, variables: &VarBindings) -> Result<Self, BlueprintError> {
        let mut resolved = self.clone();
        visit_paths(&mut resolved, |path, absolute, field| {
            *path = expand(path, absolute, &field, |name| {
                variables.get(name).cloned().ok_or_else(|| {
                    fault(&field, format!("variable '${{vars.{name}}}' has no value"))
                })
            })?;
            Ok(())
        })?;
        Ok(resolved)
    }
}

pub(crate) fn validate(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    visit_paths(&mut blueprint.vfs.clone(), |path, absolute, field| {
        expand(path, absolute, &field, |name| {
            if !blueprint.variables.contains_key(name) {
                return Err(fault(
                    &field,
                    format!("references undeclared variable '${{vars.{name}}}'"),
                ));
            }
            Ok("component".into())
        })?;
        Ok(())
    })
}

fn visit_paths(
    config: &mut VfsConfig,
    mut visit: impl FnMut(&mut String, bool, YamlPath) -> Result<(), BlueprintError>,
) -> Result<(), BlueprintError> {
    if let VfsConfig::Named {
        sub_path: Some(path),
        ..
    } = config
    {
        visit(path, false, yaml_path!["vfs", "subPath"])?;
    }
    let (cwd, mounts) = match config {
        VfsConfig::None => return Ok(()),
        VfsConfig::Ephemeral { cwd, mounts, .. }
        | VfsConfig::PerSession { cwd, mounts, .. }
        | VfsConfig::Named { cwd, mounts, .. } => (cwd, mounts),
    };
    if let Some(cwd) = cwd {
        visit(cwd, true, yaml_path!["vfs", "cwd"])?;
    }
    for (guest, mount) in mounts {
        if let Some(path) = &mut mount.sub_path {
            visit(path, false, yaml_path!["vfs", "mounts", guest, "subPath"])?;
        }
    }
    Ok(())
}

fn expand(
    path: &str,
    absolute: bool,
    field: &YamlPath,
    mut lookup: impl FnMut(&str) -> Result<String, BlueprintError>,
) -> Result<String, BlueprintError> {
    if path.len() > MAX_PATH_BYTES || path.starts_with('/') != absolute {
        return Err(fault(
            field,
            if absolute {
                "must be an absolute guest path of at most 4096 bytes"
            } else {
                "must be a relative volume path of at most 4096 bytes"
            },
        ));
    }
    if absolute && path == "/" {
        return Ok(path.into());
    }
    let body = if absolute {
        path.strip_prefix('/').unwrap_or(path)
    } else {
        path
    };
    let mut output = if absolute {
        String::from("/")
    } else {
        String::new()
    };
    for (index, component) in body.split('/').enumerate() {
        let value = if component.contains("${") {
            let name = component
                .strip_prefix("${vars.")
                .and_then(|s| s.strip_suffix('}'))
                .filter(|name| !name.is_empty() && !name.contains(['{', '}']))
                .ok_or_else(|| fault(field, "use ${vars.NAME} as a whole path component"))?;
            lookup(name)?
        } else {
            component.into()
        };
        if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\', '\0']) {
            return Err(fault(
                field,
                "each path component must be nonempty and contain no separator, NUL, '.' or '..' component",
            ));
        }
        if index > 0 {
            output.push('/');
        }
        if value.len() > MAX_PATH_BYTES.saturating_sub(output.len()) {
            return Err(fault(field, "resolved path exceeds 4096 bytes"));
        }
        output.push_str(&value);
    }
    Ok(output)
}

fn fault(field: &YamlPath, message: impl Into<String>) -> BlueprintError {
    BlueprintError::InvalidVfs(Fault::at(field.clone(), message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_volume_and_guest_paths_and_round_trips() {
        let blueprint = crate::parse("name: test\nvariables:\n  user: {required: true}\nvfs:\n  mode: named\n  volume: notes\n  subPath: users/${vars.user}\n  cwd: /${vars.user}\n  mounts:\n    /shared: {mode: named, volume: notes, subPath: shared}\n").unwrap();
        assert_eq!(
            crate::parse(&crate::to_yaml(&blueprint)).unwrap().vfs,
            blueprint.vfs
        );
        let bindings = VarBindings::from([("user".into(), "ada.test".into())]);
        let resolved = blueprint.vfs.resolve(&bindings).unwrap();
        assert_eq!(resolved.cwd(), "/ada.test");
        assert!(
            matches!(resolved, VfsConfig::Named { sub_path: Some(path), .. } if path == "users/ada.test")
        );
        for value in ["", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(
                blueprint
                    .vfs
                    .resolve(&VarBindings::from([("user".into(), value.into())]))
                    .is_err()
            );
        }
        assert!(blueprint.vfs.resolve(&VarBindings::new()).is_err());
    }

    #[test]
    fn invalid_paths_fail_at_parse() {
        for field in [
            "subPath: /users",
            "subPath: ''",
            "subPath: a//b",
            "subPath: a/../b",
            "subPath: prefix${vars.user}",
            "subPath: ${vars.missing}",
            "cwd: relative",
            "cwd: /a/",
            "cwd: /a/./b",
        ] {
            let yaml = format!(
                "name: test\nvariables:\n  user: {{required: true}}\nvfs:\n  mode: named\n  volume: notes\n  {field}\n"
            );
            assert!(crate::parse(&yaml).is_err(), "{field}");
        }
    }
}
