//! Discovery visibility is deliberately broader than runtime authorization:
//! a non-deny rule advertises a potential operation, even if filtered or shadowed.

use interpreter::stdlib::capabilities;
use submilli_blueprint::{Action, Blueprint, DefaultAction};

#[derive(Clone, Copy)]
pub struct LibraryVisibility {
    http: bool,
    fs: bool,
    code: bool,
    git: bool,
    llm: bool,
    session: bool,
}

impl LibraryVisibility {
    /// Unscoped discovery advertises the standard library except opt-in Git.
    pub fn unscoped() -> Self {
        Self {
            http: true,
            fs: true,
            code: true,
            git: false,
            llm: true,
            session: true,
        }
    }

    pub fn for_blueprint(blueprint: &Blueprint) -> Self {
        Self {
            http: module_has_grant(blueprint, "submilli:http"),
            fs: module_has_grant(blueprint, "submilli:fs"),
            code: ["fs.read", "fs.write", "fs.stat", "fs.list"]
                .iter()
                .any(|capability| has_grant(blueprint, capability)),
            git: blueprint.git.is_some(),
            llm: !blueprint.llm.models.is_empty() && has_grant(blueprint, "llm.call"),
            // A store the agent can only write, or only read, holds nothing it can use.
            session: ["session.read", "session.write"]
                .iter()
                .all(|capability| has_grant(blueprint, capability)),
        }
    }

    pub fn allows(self, name: &str) -> bool {
        match name {
            "submilli:http" => self.http,
            "submilli:fs" => self.fs,
            "submilli:code" => self.code,
            "submilli:git" => self.git,
            "submilli:llm" => self.llm,
            "submilli:session" => self.session,
            _ => true,
        }
    }
}

fn module_has_grant(blueprint: &Blueprint, module: &str) -> bool {
    capabilities::catalog()
        .iter()
        .filter(|group| group.module == module)
        .flat_map(|group| group.capabilities)
        .any(|capability| has_grant(blueprint, capability.name))
}

fn has_grant(blueprint: &Blueprint, capability: &str) -> bool {
    blueprint.default_action.unwrap_or_default() != DefaultAction::Deny
        || blueprint.permissions.get("main").is_some_and(|rules| {
            rules
                .iter()
                .any(|rule| rule.capability == capability && rule.action != Action::Deny)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_deny_rules_advertise_potential_grants() {
        for action in ["allow", "ask-human", "deny"] {
            for filter in ["", "      filter: host == \"example.com\"\n"] {
                let blueprint = submilli_blueprint::parse(&format!(
                    "name: test\npermissions:\n  main:\n    - capability: http.get\n      action: deny\n    - capability: http.get\n      action: {action}\n{filter}"
                )).unwrap();
                let visibility = LibraryVisibility::for_blueprint(&blueprint);
                assert_eq!(visibility.allows("submilli:http"), action != "deny");
                assert!(!visibility.allows("submilli:fs"));
                assert!(!visibility.allows("submilli:code"));
            }
        }
    }

    #[test]
    fn only_known_main_capabilities_count() {
        let blueprint = submilli_blueprint::parse(
            "name: test\npermissions:\n  main:\n    - capability: http.unknown\n      action: allow\n    - capability: code.read\n      action: allow\n  '@acme/tools':\n    - capability: http.get\n      action: allow\n    - capability: fs.read\n      action: allow\n"
        ).unwrap();
        let visibility = LibraryVisibility::for_blueprint(&blueprint);
        for name in ["submilli:http", "submilli:fs", "submilli:code"] {
            assert!(!visibility.allows(name));
        }
        assert!(visibility.allows("submilli:crypto"));
    }

    #[test]
    fn llm_needs_a_model_and_a_non_deny_main_policy() {
        for config in [
            "",
            "llm: {}\n",
            "llm:\n  providers:\n    test:\n      type: anthropic\n",
        ] {
            for default in ["deny", "allow", "ask-human"] {
                let blueprint =
                    submilli_blueprint::parse(&format!("name: test\ndefault: {default}\n{config}"))
                        .unwrap();
                assert!(!LibraryVisibility::for_blueprint(&blueprint).allows("submilli:llm"));
            }
        }
        let config = "llm:\n  providers:\n    test:\n      type: anthropic\n  models:\n    test-model:\n      provider: test\n";
        for (policy, visible) in [
            ("", false),
            ("default: deny\n", false),
            ("default: allow\n", true),
            ("default: ask-human\n", true),
            (
                "permissions:\n  '@acme/tools':\n    - capability: llm.call\n      action: allow\n",
                false,
            ),
        ] {
            let blueprint =
                submilli_blueprint::parse(&format!("name: test\n{config}{policy}")).unwrap();
            assert_eq!(
                LibraryVisibility::for_blueprint(&blueprint).allows("submilli:llm"),
                visible,
                "{policy}"
            );
        }
        for action in ["deny", "allow", "ask-human"] {
            for filter in ["", "      filter: model == \"test-model\"\n"] {
                let blueprint = submilli_blueprint::parse(&format!(
                    "name: test\n{config}permissions:\n  main:\n    - capability: llm.call\n      action: deny\n    - capability: llm.call\n      action: {action}\n{filter}"
                )).unwrap();
                assert_eq!(
                    LibraryVisibility::for_blueprint(&blueprint).allows("submilli:llm"),
                    action != "deny"
                );
            }
        }
        assert!(LibraryVisibility::unscoped().allows("submilli:llm"));
    }

    #[test]
    fn session_needs_both_read_and_write() {
        let rule = |capability: &str, action: &str| {
            format!("    - capability: {capability}\n      action: {action}\n")
        };
        for (policy, visible) in [
            (String::new(), false),
            ("default: deny\n".into(), false),
            ("default: allow\n".into(), true),
            ("default: ask-human\n".into(), true),
            (
                format!("permissions:\n  main:\n{}", rule("session.read", "allow")),
                false,
            ),
            (
                format!("permissions:\n  main:\n{}", rule("session.write", "allow")),
                false,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}{}",
                    rule("session.read", "allow"),
                    rule("session.write", "deny")
                ),
                false,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}{}{}",
                    rule("session.read", "allow"),
                    rule("session.remove", "allow"),
                    rule("session.list", "allow")
                ),
                false,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}{}",
                    rule("session.read", "ask-human"),
                    rule("session.write", "allow")
                ),
                true,
            ),
            (
                format!(
                    "default: deny\npermissions:\n  main:\n{}{}",
                    rule("session.read", "allow"),
                    rule("session.write", "allow")
                ),
                true,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}{}{}",
                    rule("session.read", "allow"),
                    rule("session.write", "deny"),
                    rule("session.write", "allow")
                ),
                true,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}      filter: key glob \"none/*\"\n{}      filter: key == \"none\"\n",
                    rule("session.read", "allow"),
                    rule("session.write", "allow")
                ),
                true,
            ),
            (
                format!(
                    "permissions:\n  main:\n{}  '@acme/tools':\n{}",
                    rule("session.read", "allow"),
                    rule("session.write", "allow")
                ),
                false,
            ),
            (
                format!(
                    "permissions:\n  '@acme/tools':\n{}{}",
                    rule("session.read", "allow"),
                    rule("session.write", "allow")
                ),
                false,
            ),
        ] {
            let blueprint = submilli_blueprint::parse(&format!("name: test\n{policy}")).unwrap();
            assert_eq!(
                LibraryVisibility::for_blueprint(&blueprint).allows("submilli:session"),
                visible,
                "{policy}"
            );
        }
        assert!(LibraryVisibility::unscoped().allows("submilli:session"));
    }
}
