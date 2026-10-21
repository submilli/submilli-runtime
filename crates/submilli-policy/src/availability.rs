//! [`RulesAvailability`]: what a caller could use under a rule set, for
//! documentation that shows a reader only that.

use interpreter::DocCapability;
use interpreter::docs_view::{Availability, DocAvailability, known_context};
use interpreter::stdlib::capabilities;

use crate::rules::{Action, PossibleActions, Rules};

/// The [`DocAvailability`] of `caller` under `rules`. A capability is
/// available when some call could proceed: a rule whose filter needs what the
/// call passes keeps it available, with a note naming the filter, and so does
/// one that routes the call to a person, with a note saying so. A capability
/// the runtime refuses to `main` whatever the rules say is never available to
/// it.
pub struct RulesAvailability<'a, R: ?Sized> {
    rules: &'a R,
    caller: &'a str,
}

impl<'a, R: Rules + ?Sized> RulesAvailability<'a, R> {
    pub fn new(rules: &'a R, caller: &'a str) -> Self {
        Self { rules, caller }
    }

    fn availability(&self, capability: &str, known: &serde_json::Value) -> Availability {
        if self.caller == interpreter::mangle::USER_PACKAGE
            && capabilities::find_any(capability).is_some_and(|entry| !entry.grantable_to_main())
        {
            return Availability::Denied;
        }
        let possible = self.rules.possible_actions(self.caller, capability, known);
        if !possible.is_possible() {
            return Availability::Denied;
        }
        Availability::Available {
            notes: notes(capability, &possible),
        }
    }
}

impl<R: Rules + ?Sized> DocAvailability for RulesAvailability<'_, R> {
    fn tag(&self, tag: &DocCapability) -> Availability {
        self.availability(&tag.capability, &known_context(tag))
    }

    fn capability(&self, name: &str) -> Availability {
        self.availability(name, &serde_json::Value::Object(serde_json::Map::new()))
    }
}

/// What limits calls to `capability`, as a sentence for the reader.
fn notes(capability: &str, possible: &PossibleActions) -> Vec<String> {
    if possible.conditions.is_empty() {
        return match possible.decided {
            Action::AskHuman => vec![format!(
                "Under this blueprint, `{capability}` needs a person's approval."
            )],
            Action::Allow | Action::Deny => Vec::new(),
        };
    }
    let conditions = possible
        .conditions
        .iter()
        .map(|condition| format!("{} when `{}`", outcome(condition.action), condition.filter))
        .collect::<Vec<_>>()
        .join("; ");
    vec![format!(
        "Under this blueprint, `{capability}` is {conditions}; otherwise {}.",
        outcome(possible.decided)
    )]
}

fn outcome(action: Action) -> &'static str {
    match action {
        Action::Allow => "allowed",
        Action::AskHuman => "allowed after a person approves",
        Action::Deny => "denied",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DefaultAction, PermissionRule, RuleSet};
    use std::collections::BTreeMap;

    fn rules(main: Vec<PermissionRule>, default_action: DefaultAction) -> RuleSet {
        RuleSet::new(BTreeMap::from([("main".to_string(), main)]), default_action)
            .expect("valid rules")
    }

    fn rule(capability: &str, filter: Option<&str>, action: Action) -> PermissionRule {
        PermissionRule {
            name: None,
            capability: capability.to_string(),
            filter: filter.map(|filter| filter.parse().expect("valid filter")),
            action,
        }
    }

    #[test]
    fn a_filtered_grant_is_available_with_its_condition() {
        let set = rules(
            vec![rule(
                "http.request",
                Some("method == \"GET\""),
                Action::Allow,
            )],
            DefaultAction::Deny,
        );
        let availability = RulesAvailability::new(&set, "main");
        assert_eq!(
            availability.capability("http.request"),
            Availability::Available {
                notes: vec![
                    "Under this blueprint, `http.request` is allowed when `method == \"GET\"`; otherwise denied."
                        .to_string()
                ]
            }
        );
        assert_eq!(availability.capability("http.post"), Availability::Denied);
    }

    #[test]
    fn ask_human_is_available_with_a_note() {
        let set = rules(
            vec![rule("fs.write", None, Action::AskHuman)],
            DefaultAction::Deny,
        );
        let availability = RulesAvailability::new(&set, "main");
        assert_eq!(
            availability.capability("fs.write"),
            Availability::Available {
                notes: vec![
                    "Under this blueprint, `fs.write` needs a person's approval.".to_string()
                ]
            }
        );
    }

    #[test]
    fn main_never_holds_what_the_runtime_refuses_it() {
        let set = rules(Vec::new(), DefaultAction::Allow);
        let availability = RulesAvailability::new(&set, "main");
        assert_eq!(availability.capability("secrets.get"), Availability::Denied);
        assert_eq!(availability.capability("fs.read"), Availability::OPEN);
        let package = RulesAvailability::new(&set, "@acme/pkg");
        assert_eq!(package.capability("secrets.get"), Availability::OPEN);
    }

    #[test]
    fn an_earlier_uncertain_deny_is_named_in_the_note() {
        let set = rules(
            vec![
                rule("http.request", Some("host == \"internal\""), Action::Deny),
                rule("http.request", Some("method == \"GET\""), Action::Allow),
            ],
            DefaultAction::Deny,
        );
        assert_eq!(
            RulesAvailability::new(&set, "main").capability("http.request"),
            Availability::Available {
                notes: vec![
                    "Under this blueprint, `http.request` is denied when `host == \"internal\"`; allowed when `method == \"GET\"`; otherwise denied."
                        .to_string()
                ]
            }
        );
    }

    #[test]
    fn a_variable_filter_stays_undecided() {
        let set = rules(
            vec![rule(
                "session.read",
                Some("key == ${vars.key}"),
                Action::Allow,
            )],
            DefaultAction::Deny,
        );
        let availability = RulesAvailability::new(&set, "main");
        assert!(availability.capability("session.read").is_available());
    }
}
