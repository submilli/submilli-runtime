# submilli-policy

The capability policy of [Submilli](https://github.com/submilli/submilli-runtime):
the filter language, permission rules, and the check that evaluates them. The
CLI, the server and blueprints decide capability calls with this crate, so an
embedder of `submilli-engine` that uses it decides them the same way.

Every effect a Submilli program has goes through a capability check, which
reports a JSON context. Rules are kept per caller (`main` for the program, a
package name for library code) and walked in order: the first rule that names
the capability and whose filter matches decides, else the default does.

```rust
use submilli_policy::{Action, DefaultAction, PermissionRule, RuleSet, Rules, VarBindings};

let rules = RuleSet::new(
    [(
        "main".to_string(),
        vec![PermissionRule {
            name: Some("docs-only".to_string()),
            capability: "http.get".to_string(),
            filter: Some(r#"host == "docs.example""#.parse().expect("valid filter")),
            action: Action::Allow,
        }],
    )]
    .into(),
    DefaultAction::Deny,
)
.expect("valid rules");
let decision = rules.resolve(
    "main",
    "http.get",
    &serde_json::json!({ "host": "docs.example" }),
    &VarBindings::new(),
);
assert_eq!(decision.action, Action::Allow);
```

`Rules::explain` gives the reasoning: the deciding rule or the default, and the
rules whose filters rejected the call, with each comparison that failed.
`Rules::possible_actions` answers before a call, when only part of its context
is known: which actions it could still resolve to.

With the `engine` feature, `PermissionCheck` is the `SecurityCheck` to install
on a `submilli-engine` store, built from a rule set and the session's
`${vars.NAME}` bindings, and `RulesAvailability` tells `submilli-engine`'s
documentation view what a caller could use, so docs show only that.

Licensed under Apache-2.0.
