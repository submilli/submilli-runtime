//! Package names: the scoped `@org/leaf` form packages are published under,
//! and that caller blocks name library code by.

/// Validate a scoped package name `@org/leaf` and return its parts.
///
/// Package namespaces are GitHub-org-rooted: the scope must be a GitHub-org
/// token (ASCII alphanumeric with single internal hyphens, no leading/trailing
/// hyphen, no dots, 1–39 chars). The leaf is permissive (`[A-Za-z0-9._-]`, not
/// `.`/`..`). The `Err` is an LLM-actionable message naming the fix. This is the
/// single source of truth for package-name shape — blueprints, the package
/// store, and the build manifest all route through it.
pub fn validate_scoped_name(name: &str) -> Result<(&str, &str), String> {
    let Some(rest) = name.strip_prefix('@') else {
        return Err(format!(
            "package `{name}` must use scoped form `@org/name` (GitHub org as the scope)"
        ));
    };
    let mut parts = rest.split('/');
    let org = parts.next().unwrap_or_default();
    let leaf = parts.next().unwrap_or_default();
    if leaf.is_empty() || parts.next().is_some() {
        return Err(format!(
            "package `{name}` must use scoped form `@org/name` (exactly one `/`)"
        ));
    }
    if !is_valid_org(org) {
        return Err(format!(
            "package `{name}` scope `@{org}` must be a GitHub org: ASCII letters, digits, and single \
             internal hyphens only (no dots), 1–39 characters"
        ));
    }
    if !is_valid_leaf(leaf) {
        return Err(format!(
            "package `{name}` leaf `{leaf}` contains characters outside [A-Za-z0-9._-]"
        ));
    }
    Ok((org, leaf))
}

/// GitHub org/user rules: 1–39 chars, ASCII alphanumeric or single internal
/// hyphens, no leading/trailing hyphen.
fn is_valid_org(org: &str) -> bool {
    if org.is_empty() || org.len() > 39 {
        return false;
    }
    if org.starts_with('-') || org.ends_with('-') || org.contains("--") {
        return false;
    }
    org.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_valid_leaf(leaf: &str) -> bool {
    if leaf.is_empty() || leaf == "." || leaf == ".." {
        return false;
    }
    leaf.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_names_need_a_github_org_and_a_plain_leaf() {
        assert_eq!(
            validate_scoped_name("@pied-piper/sdk.v2_x-y"),
            Ok(("pied-piper", "sdk.v2_x-y"))
        );
        assert!(validate_scoped_name(&format!("@{}/x", "a".repeat(39))).is_ok());
        for name in [
            "pied-piper/sdk",
            "@pied-piper",
            "@pied-piper/",
            "@/sdk",
            "@pied-piper/sdk/extra",
            "@-pied-piper/sdk",
            "@pied-piper-/sdk",
            "@ac--me/sdk",
            "@ac.me/sdk",
            "@pied-piper/.",
            "@pied-piper/..",
            "@pied-piper/s dk",
        ] {
            assert!(validate_scoped_name(name).is_err(), "{name}");
        }
        assert!(validate_scoped_name(&format!("@{}/x", "a".repeat(40))).is_err());
    }
}
