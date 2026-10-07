//! Classifying a blueprint change: did it widen or narrow what programs may do?
//!
//! [`diff`] compares two parsed blueprints and lists each change with its
//! effect on access: widening, narrowing, mixed, or unknown. Permission rules are
//! compared per caller block under first-match-wins semantics, so rule order,
//! `default`, `packages`, variable declarations, and moves between caller blocks
//! all count. Only the shapes enumerated (and tested) here are classified; any
//! other change is unknown, never guessed.
//!
//! In an `allow` rule, a pin is a top-level conjunct `field == ${vars.X}`: it ties
//! the rule to the session's binding. Losing one, so that the filter no longer
//! references the variable at all, is a pin removal. A pin that stays a top-level
//! conjunct holds whatever else changes, so it raises no flag. Any other change to a
//! variable-bearing allow filter (a pin moved under an `or`, or a filter with
//! variables but no pin edited short of a provable tightening) is "pin possibly
//! weakened". The same edits to a `deny` rule are classified by their effect, with
//! no pin flag.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

use crate::{Action, Blueprint, DefaultAction, FilterExpr, PermissionRule};

/// How a change moves access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification {
    /// Something denied before may now be allowed.
    Widening,
    /// Something allowed before may now be denied.
    Narrowing,
    /// Some changes widen and others narrow.
    Mixed,
    /// A change whose effect is not one of the classified shapes.
    Unknown,
}

impl Classification {
    /// The classification of several changes together: unknown if any is unknown,
    /// mixed when they pull both ways. `None` for no changes.
    fn combine(items: impl IntoIterator<Item = Classification>) -> Option<Classification> {
        let mut combined = None;
        for item in items {
            combined = Some(match (combined, item) {
                (None, item) => item,
                (Some(Classification::Unknown), _) | (_, Classification::Unknown) => {
                    Classification::Unknown
                }
                (Some(current), item) if current == item => current,
                (Some(_), _) => Classification::Mixed,
            });
        }
        combined
    }

    fn word(self) -> &'static str {
        match self {
            Classification::Widening => "Widened",
            Classification::Narrowing => "Narrowed",
            Classification::Mixed => "Changed both ways",
            Classification::Unknown => "Changed",
        }
    }
}

impl fmt::Display for Classification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Classification::Widening => "widening",
            Classification::Narrowing => "narrowing",
            Classification::Mixed => "mixed",
            Classification::Unknown => "unknown",
        })
    }
}

/// A change to an allow rule's tie to a session variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum PinChange {
    /// The rule no longer references the variable it was pinned to: whoever the
    /// session is bound to, the rule now matches other values of the field.
    Removed { field: String, variable: String },
    /// The filter still references the variable, but the edit may no longer
    /// hold the field to it.
    PossiblyWeakened { variables: Vec<String> },
}

/// One change, with its effect and a plain-language sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub classification: Classification,
    /// The caller block a rule change belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    /// The rule a change concerns, by name or as `rule <n> (<capability>)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<PinChange>,
    pub summary: String,
}

/// Every change between two blueprints, and their effect together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlueprintDiff {
    /// `unknown` when nothing classified changed.
    pub classification: Classification,
    pub changes: Vec<Change>,
    /// Variables the new blueprint requires that the old one did not: sessions
    /// opened before the change have no binding for them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub new_required_variables: Vec<String>,
}

impl BlueprintDiff {
    /// The changes that removed a pin.
    pub fn pin_removals(&self) -> impl Iterator<Item = &Change> {
        self.changes
            .iter()
            .filter(|change| matches!(change.pin, Some(PinChange::Removed { .. })))
    }

    /// One line per change, pin removals first.
    pub fn summary(&self) -> String {
        if self.changes.is_empty() {
            return "No change to what programs may do.".to_owned();
        }
        let mut lines: Vec<&str> = self
            .pin_removals()
            .map(|change| change.summary.as_str())
            .collect();
        lines.extend(
            self.changes
                .iter()
                .filter(|change| !matches!(change.pin, Some(PinChange::Removed { .. })))
                .map(|change| change.summary.as_str()),
        );
        lines.join("\n")
    }
}

/// Classify the change from `old` to `new`.
pub fn diff(old: &Blueprint, new: &Blueprint) -> BlueprintDiff {
    let mut changes = Vec::new();
    diff_default(old, new, &mut changes);
    diff_permissions(old, new, &mut changes);
    diff_packages(old, new, &mut changes);
    let new_required_variables = diff_variables(old, new, &mut changes);
    diff_mcp(old, new, &mut changes);
    diff_insecure_http(old, new, &mut changes);
    diff_unclassified(old, new, &mut changes);
    BlueprintDiff {
        classification: Classification::combine(changes.iter().map(|c| c.classification))
            .unwrap_or(Classification::Unknown),
        changes,
        new_required_variables,
    }
}

fn plain(classification: Classification, what: String) -> Change {
    Change {
        summary: format!("{}: {what}.", classification.word()),
        classification,
        caller: None,
        rule: None,
        pin: None,
    }
}

/// How permissive an action is: deny, then ask-human, then allow.
fn rank(action: Action) -> u8 {
    match action {
        Action::Deny => 0,
        Action::AskHuman => 1,
        Action::Allow => 2,
    }
}

fn by_rank(before: Action, after: Action) -> Classification {
    match rank(after).cmp(&rank(before)) {
        std::cmp::Ordering::Greater => Classification::Widening,
        std::cmp::Ordering::Less => Classification::Narrowing,
        std::cmp::Ordering::Equal => Classification::Unknown,
    }
}

fn action_word(action: Action) -> &'static str {
    match action {
        Action::Allow => "allow",
        Action::Deny => "deny",
        Action::AskHuman => "ask-human",
    }
}

fn diff_default(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    let before: Action = old.default_action.unwrap_or(DefaultAction::Deny).into();
    let after: Action = new.default_action.unwrap_or(DefaultAction::Deny).into();
    if before != after {
        out.push(plain(
            by_rank(before, after),
            format!(
                "the default for calls no rule matches is now `{}` (was `{}`)",
                action_word(after),
                action_word(before)
            ),
        ));
    }
}

fn diff_packages(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    for added in new.packages.difference(&old.packages) {
        out.push(plain(
            Classification::Widening,
            format!("programs may now import `{added}`"),
        ));
    }
    for removed in old.packages.difference(&new.packages) {
        out.push(plain(
            Classification::Narrowing,
            format!("programs may no longer import `{removed}`"),
        ));
    }
}

/// Returns the variables that became required.
fn diff_variables(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) -> Vec<String> {
    let mut newly_required = Vec::new();
    for (name, decl) in &new.variables {
        let was_required = old.variables.get(name).is_some_and(|old| old.required);
        match old.variables.get(name) {
            Some(old_decl) if old_decl == decl => {}
            _ if decl.required && !was_required => {
                newly_required.push(name.clone());
                out.push(plain(
                    Classification::Narrowing,
                    format!(
                        "sessions must now bind variable `{name}`; sessions opened before this \
                         change have no binding for it"
                    ),
                ));
            }
            None => out.push(plain(
                Classification::Unknown,
                format!("variable `{name}` was declared"),
            )),
            Some(_) => out.push(plain(
                Classification::Unknown,
                format!("variable `{name}`'s declaration changed"),
            )),
        }
    }
    for name in old.variables.keys() {
        if !new.variables.contains_key(name) {
            out.push(plain(
                Classification::Unknown,
                format!("variable `{name}` is no longer declared"),
            ));
        }
    }
    newly_required
}

fn diff_mcp(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    for (name, server) in &new.mcp {
        match old.mcp.get(name) {
            None => out.push(plain(
                Classification::Widening,
                format!("programs may now import `@mcp/{name}`"),
            )),
            Some(before) if before != server => out.push(plain(
                Classification::Unknown,
                format!("MCP server `{name}`'s settings changed"),
            )),
            Some(_) => {}
        }
    }
    for name in old.mcp.keys() {
        if !new.mcp.contains_key(name) {
            out.push(plain(
                Classification::Narrowing,
                format!("programs may no longer import `@mcp/{name}`"),
            ));
        }
    }
}

fn diff_insecure_http(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    match (old.allow_insecure_http, new.allow_insecure_http) {
        (false, true) => out.push(plain(
            Classification::Widening,
            "programs may now use cleartext `http://`".to_owned(),
        )),
        (true, false) => out.push(plain(
            Classification::Narrowing,
            "programs may no longer use cleartext `http://`".to_owned(),
        )),
        _ => {}
    }
}

/// Sections whose changes are not classified: each one that changed is listed
/// as unknown.
fn diff_unclassified(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    let sections: [(&str, bool); 9] = [
        ("kind", old.kind != new.kind),
        ("name", old.name != new.name),
        ("idle_timeout", old.idle_timeout != new.idle_timeout),
        ("vfs", old.vfs != new.vfs),
        ("secrets", old.secrets != new.secrets),
        ("auth_proxy", old.auth_proxy != new.auth_proxy),
        ("git", old.git != new.git),
        ("llm", old.llm != new.llm),
        ("embedding", old.embedding != new.embedding),
    ];
    for (section, changed) in sections {
        if changed {
            out.push(plain(
                Classification::Unknown,
                format!("the `{section}` section changed"),
            ));
        }
    }
}

fn diff_permissions(old: &Blueprint, new: &Blueprint, out: &mut Vec<Change>) {
    let empty = Vec::new();
    let callers: BTreeSet<&String> = old
        .permissions
        .keys()
        .chain(new.permissions.keys())
        .collect();
    let mut removed: Vec<(String, usize, PermissionRule)> = Vec::new();
    let mut added: Vec<(String, usize, PermissionRule)> = Vec::new();
    for caller in callers {
        let before = old.permissions.get(caller).unwrap_or(&empty);
        let after = new.permissions.get(caller).unwrap_or(&empty);
        if before == after {
            continue;
        }
        let block = diff_block(caller, before, after, out);
        removed.extend(
            block
                .removed
                .into_iter()
                .map(|(i, r)| (caller.clone(), i, r)),
        );
        added.extend(block.added.into_iter().map(|(i, r)| (caller.clone(), i, r)));
    }
    // A rule removed from one caller block and added, unchanged, to another moved
    // between them: it narrows the first caller and widens the second.
    let mut taken = vec![false; added.len()];
    let mut left: Vec<(String, usize, PermissionRule)> = Vec::new();
    for (from, from_index, rule) in removed {
        let moved = added
            .iter()
            .enumerate()
            .find(|(i, (to, _, candidate))| !taken[*i] && *to != from && *candidate == rule)
            .map(|(i, _)| i);
        if let Some(i) = moved {
            taken[i] = true;
            let (to, to_index, _) = &added[i];
            out.push(moved_between_callers(
                &from, from_index, to, *to_index, &rule,
            ));
        } else {
            out.push(rule_removed(&from, from_index, &rule));
            left.push((from, from_index, rule));
        }
    }
    // An allow rule removed and an allow rule added in its place (an edit that
    // also moved past another rule for its capability) still carry the pin
    // analysis, so a pin lost that way is flagged.
    let mut replaced = vec![false; left.len()];
    for (i, (caller, index, rule)) in added.iter().enumerate() {
        if taken[i] {
            continue;
        }
        let mut change = rule_added(caller, *index, rule);
        if rule.action == Action::Allow
            && let Some((k, pin)) = replaced_pin(caller, rule, &left, &replaced)
        {
            replaced[k] = true;
            change
                .summary
                .push_str(&pin_sentence(&pin, &rule.capability));
            change.pin = Some(pin);
        }
        out.push(change);
    }
}

/// The removed allow rule in `caller` that `rule` most plausibly replaces (its
/// namesake, else one for the same capability) and the pin it lost doing so.
fn replaced_pin(
    caller: &str,
    rule: &PermissionRule,
    removed: &[(String, usize, PermissionRule)],
    used: &[bool],
) -> Option<(usize, PinChange)> {
    let candidates = |same_name: bool| {
        removed
            .iter()
            .enumerate()
            .filter(move |(k, (from, _, old))| {
                !used[*k]
                    && from == caller
                    && old.action == Action::Allow
                    && old.capability == rule.capability
                    && (!same_name || (old.name.is_some() && old.name == rule.name))
            })
    };
    let (k, (_, _, old)) = candidates(true)
        .next()
        .or_else(|| candidates(false).next())?;
    pin_change(
        old.filter.as_ref(),
        rule.filter.as_ref(),
        FilterChange::Unknown,
    )
    .map(|pin| (k, pin))
}

/// A sentence, after a change's summary, on the pin a replacement lost.
fn pin_sentence(pin: &PinChange, capability: &str) -> String {
    match pin {
        PinChange::Removed { field, variable } => format!(
            " It replaces one that pinned `{field}` to `${{vars.{variable}}}`, so `{capability}` \
             now matches whatever `{field}` the session is bound to or not."
        ),
        PinChange::PossiblyWeakened { variables } => format!(
            " It replaces one pinned to {}, which may be weakened.",
            variable_list(variables)
        ),
    }
}

fn variable_list(variables: &[String]) -> String {
    variables
        .iter()
        .map(|v| format!("`${{vars.{v}}}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn rule_label(index: usize, rule: &PermissionRule) -> String {
    match &rule.name {
        Some(name) => format!("`{name}`"),
        None => format!("rule {} (`{}`)", index.saturating_add(1), rule.capability),
    }
}

fn rule_change(
    classification: Classification,
    caller: &str,
    index: usize,
    rule: &PermissionRule,
    what: String,
) -> Change {
    Change {
        summary: format!("{}: {what}.", classification.word()),
        classification,
        caller: Some(caller.to_owned()),
        rule: Some(rule_label(index, rule)),
        pin: None,
    }
}

/// Adding an allow rule can only let through calls that later rules or the
/// default decided; adding a deny rule can only stop them. An ask-human rule can
/// do either.
fn rule_added(caller: &str, index: usize, rule: &PermissionRule) -> Change {
    let classification = match rule.action {
        Action::Allow => Classification::Widening,
        Action::Deny => Classification::Narrowing,
        Action::AskHuman => Classification::Unknown,
    };
    rule_change(
        classification,
        caller,
        index,
        rule,
        format!(
            "`{caller}` gained {} rule {} for `{}`",
            action_article(rule.action),
            rule_label(index, rule),
            rule.capability
        ),
    )
}

fn rule_removed(caller: &str, index: usize, rule: &PermissionRule) -> Change {
    let classification = match rule.action {
        Action::Allow => Classification::Narrowing,
        Action::Deny => Classification::Widening,
        Action::AskHuman => Classification::Unknown,
    };
    rule_change(
        classification,
        caller,
        index,
        rule,
        format!(
            "`{caller}` lost {} rule {} for `{}`",
            action_article(rule.action),
            rule_label(index, rule),
            rule.capability
        ),
    )
}

fn moved_between_callers(
    from: &str,
    from_index: usize,
    to: &str,
    to_index: usize,
    rule: &PermissionRule,
) -> Change {
    let classification = match rule.action {
        Action::AskHuman => Classification::Unknown,
        _ => Classification::Mixed,
    };
    Change {
        summary: format!(
            "{}: {} {} for `{}` moved from `{from}` to `{to}`, so `{from}` lost it and `{to}` \
             gained it.",
            classification.word(),
            action_article(rule.action),
            rule_label(from_index, rule),
            rule.capability
        ),
        classification,
        caller: Some(to.to_owned()),
        rule: Some(rule_label(to_index, rule)),
        pin: None,
    }
}

fn action_article(action: Action) -> &'static str {
    match action {
        Action::Allow => "an allow",
        Action::Deny => "a deny",
        Action::AskHuman => "an ask-human",
    }
}

/// Rules a caller block lost or gained outright, for matching moves between
/// caller blocks.
struct BlockDiff {
    removed: Vec<(usize, PermissionRule)>,
    added: Vec<(usize, PermissionRule)>,
}

/// The most entries the table that matches one caller block's changed rules may
/// hold: (rules changed before + 1) x (rules changed after + 1). A block whose
/// changed middle is larger is reported as changed, unclassified, rather than
/// matched rule by rule.
const MAX_MATCH_TABLE: usize = 1 << 16;

/// What became of each old rule.
#[derive(Clone, Copy)]
enum Fate {
    /// Unchanged and in the same relative order (the longest common subsequence).
    Kept(usize),
    /// Unchanged, but moved relative to the kept rules.
    Moved(usize),
    /// Edited in place: its order relative to every other surviving rule for its
    /// capability is unchanged.
    Edited(usize),
    Removed,
}

impl Fate {
    /// The rule's position in the new block, when it survived.
    fn new_index(self) -> Option<usize> {
        match self {
            Fate::Kept(j) | Fate::Moved(j) | Fate::Edited(j) => Some(j),
            Fate::Removed => None,
        }
    }
}

fn diff_block(
    caller: &str,
    before: &[PermissionRule],
    after: &[PermissionRule],
    out: &mut Vec<Change>,
) -> BlockDiff {
    let Some(kept) = longest_common_subsequence(before, after) else {
        out.push(Change {
            classification: Classification::Unknown,
            caller: Some(caller.to_owned()),
            rule: None,
            pin: None,
            summary: format!(
                "{}: `{caller}`'s rules changed in too many places to compare one by one \
                 ({} rules before, {} after).",
                Classification::Unknown.word(),
                before.len(),
                after.len()
            ),
        });
        return BlockDiff {
            removed: Vec::new(),
            added: Vec::new(),
        };
    };
    let mut fate: Vec<Fate> = vec![Fate::Removed; before.len()];
    let mut claimed = vec![false; after.len()];
    for (i, j) in kept {
        fate[i] = Fate::Kept(j);
        claimed[j] = true;
    }
    for i in 0..before.len() {
        if !matches!(fate[i], Fate::Removed) {
            continue;
        }
        if let Some(j) = (0..after.len()).find(|&j| !claimed[j] && after[j] == before[i]) {
            fate[i] = Fate::Moved(j);
            claimed[j] = true;
        }
    }
    // Edits pair a rule with its namesake first, then with an unclaimed rule for
    // the same capability (a rename, or an unnamed rule edited). An edit is judged
    // with the rules around it fixed, so it is paired only when it keeps its order
    // relative to every rule already paired for its capability; otherwise (two
    // edited rules that swapped, say) it is a rule removed and another added.
    let namesake = |i: usize, j: usize| match (&before[i].name, &after[j].name) {
        (Some(a), Some(b)) => a == b,
        (None, None) => before[i].capability == after[j].capability,
        _ => false,
    };
    let same_capability = |i: usize, j: usize| before[i].capability == after[j].capability;
    for pairs in [&namesake as &dyn Fn(usize, usize) -> bool, &same_capability] {
        for i in 0..before.len() {
            if !matches!(fate[i], Fate::Removed) {
                continue;
            }
            if let Some(j) = (0..after.len())
                .find(|&j| !claimed[j] && pairs(i, j) && keeps_order(before, after, &fate, i, j))
            {
                fate[i] = Fate::Edited(j);
                claimed[j] = true;
            }
        }
    }

    for i in 0..before.len() {
        match fate[i] {
            Fate::Moved(j) => out.push(classify_move(caller, before, after, i, j, &fate)),
            Fate::Edited(j) => out.push(classify_edit(caller, i, &before[i], &after[j])),
            Fate::Kept(_) | Fate::Removed => {}
        }
    }
    BlockDiff {
        removed: (0..before.len())
            .filter(|&i| matches!(fate[i], Fate::Removed))
            .map(|i| (i, before[i].clone()))
            .collect(),
        added: (0..after.len())
            .filter(|&j| !claimed[j])
            .map(|j| (j, after[j].clone()))
            .collect(),
    }
}

/// Whether reading old rule `i` as new rule `j` keeps it in the same order relative
/// to every rule already paired that governs either rule's capability. Rules for
/// other capabilities never decide the same call, so passing them does not count.
fn keeps_order(
    before: &[PermissionRule],
    after: &[PermissionRule],
    fate: &[Fate],
    i: usize,
    j: usize,
) -> bool {
    fate.iter().enumerate().all(|(k, fate)| {
        let Some(k_new) = fate.new_index() else {
            return true;
        };
        // `k_new` came from pairing with a rule of `after`, so it indexes it.
        let related = before[k].capability == before[i].capability
            || after[k_new].capability == after[j].capability;
        !related || (k < i) == (k_new < j)
    })
}

/// Index pairs `(old, new)` of a longest common subsequence of equal rules, or
/// `None` when the rules that differ are too many to match within
/// [`MAX_MATCH_TABLE`]. A common prefix and suffix are matched directly, so a
/// long block with a few edits stays well inside the bound.
fn longest_common_subsequence(
    a: &[PermissionRule],
    b: &[PermissionRule],
) -> Option<Vec<(usize, usize)>> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a_rest, b_rest) = (&a[prefix..], &b[prefix..]);
    let suffix = a_rest
        .iter()
        .rev()
        .zip(b_rest.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    // `suffix` is at most the shorter rest, so neither slice end underflows.
    let a_mid = &a_rest[..a_rest.len() - suffix];
    let b_mid = &b_rest[..b_rest.len() - suffix];
    let width = b_mid.len().checked_add(1)?;
    if a_mid.len().checked_add(1)?.checked_mul(width)? > MAX_MATCH_TABLE {
        return None;
    }
    // lengths[i][j]: the LCS of a_mid[i..] and b_mid[j..].
    let mut lengths = vec![vec![0_usize; width]; a_mid.len() + 1];
    for i in (0..a_mid.len()).rev() {
        for j in (0..b_mid.len()).rev() {
            lengths[i][j] = if a_mid[i] == b_mid[j] {
                lengths[i + 1][j + 1].saturating_add(1)
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let mut pairs: Vec<(usize, usize)> = (0..prefix).map(|i| (i, i)).collect();
    let (mut i, mut j) = (0, 0);
    while i < a_mid.len() && j < b_mid.len() {
        if a_mid[i] == b_mid[j] {
            pairs.push((prefix + i, prefix + j));
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let (a_tail, b_tail) = (a.len() - suffix, b.len() - suffix);
    pairs.extend((0..suffix).map(|k| (a_tail + k, b_tail + k)));
    Some(pairs)
}

/// A rule that moved within its caller block changes which rule decides a call
/// only against rules for the same capability whose action differs and whose
/// order relative to it flipped. Moving ahead of a less permissive rule widens;
/// moving behind one narrows.
fn classify_move(
    caller: &str,
    before: &[PermissionRule],
    after: &[PermissionRule],
    from: usize,
    to: usize,
    fate: &[Fate],
) -> Change {
    let rule = &before[from];
    let mut effects = Vec::new();
    for (other, other_rule) in before.iter().enumerate() {
        if other == from || other_rule.capability != rule.capability {
            continue;
        }
        // A pair of moved rules is weighed once, from the earlier one.
        if matches!(fate[other], Fate::Moved(_)) && other < from {
            continue;
        }
        let Some(other_to) = fate[other].new_index() else {
            continue;
        };
        let Some(other_after) = after.get(other_to) else {
            continue;
        };
        if other_after.capability != rule.capability {
            continue;
        }
        let was_ahead = from < other;
        let is_ahead = to < other_to;
        if was_ahead == is_ahead {
            continue;
        }
        let other_action = other_after.action;
        if other_action == rule.action {
            continue;
        }
        effects.push(if is_ahead {
            by_rank(other_action, rule.action)
        } else {
            by_rank(rule.action, other_action)
        });
    }
    let classification = Classification::combine(effects).unwrap_or(Classification::Unknown);
    let direction = if to < from { "up" } else { "down" };
    let consequence = match classification {
        Classification::Widening => "so it now decides calls a less permissive rule decided",
        Classification::Narrowing => "so a less permissive rule now decides calls it decided",
        Classification::Mixed => "past rules both more and less permissive than it",
        Classification::Unknown => "past no rule for the same capability with another action",
    };
    rule_change(
        classification,
        caller,
        to,
        rule,
        format!(
            "{} {} for `{}` under `{caller}` moved {direction}, {consequence}",
            capitalized(action_article(rule.action)),
            rule_label(from, rule),
            rule.capability
        ),
    )
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How an edited filter relates to the old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterChange {
    /// Matches every call the old filter matched, and maybe more.
    Looser,
    /// Matches only calls the old filter matched.
    Tighter,
    Unknown,
}

/// A missing filter matches every call, as an empty conjunction does. Conjuncts
/// dropped (or a filter removed) loosen; conjuncts added tighten. Disjuncts added
/// loosen; disjuncts dropped tighten. Anything else is unknown.
fn filter_change(before: Option<&FilterExpr>, after: Option<&FilterExpr>) -> FilterChange {
    let old_and = before.map_or_else(Vec::new, FilterExpr::conjuncts);
    let new_and = after.map_or_else(Vec::new, FilterExpr::conjuncts);
    if strict_subset(&new_and, &old_and) {
        return FilterChange::Looser;
    }
    if strict_subset(&old_and, &new_and) {
        return FilterChange::Tighter;
    }
    if let (Some(before), Some(after)) = (before, after) {
        let (old_or, new_or) = (before.disjuncts(), after.disjuncts());
        if strict_subset(&old_or, &new_or) {
            return FilterChange::Looser;
        }
        if strict_subset(&new_or, &old_or) {
            return FilterChange::Tighter;
        }
    }
    FilterChange::Unknown
}

/// Every item of `small` is in `large`, and `large` has one that `small` lacks.
fn strict_subset(small: &[&FilterExpr], large: &[&FilterExpr]) -> bool {
    small.iter().all(|item| large.contains(item)) && large.iter().any(|item| !small.contains(item))
}

fn classify_edit(
    caller: &str,
    index: usize,
    before: &PermissionRule,
    after: &PermissionRule,
) -> Change {
    let label = rule_label(index, after);
    let change = |classification: Classification, what: String| {
        rule_change(classification, caller, index, after, what)
    };
    if before.capability != after.capability {
        return change(
            Classification::Unknown,
            format!(
                "{label} under `{caller}` now governs `{}` instead of `{}`",
                after.capability, before.capability
            ),
        );
    }
    let filter_changed = before.filter != after.filter;
    if before.action != after.action {
        let classification = if filter_changed {
            Classification::Unknown
        } else {
            by_rank(before.action, after.action)
        };
        let mut result = change(
            classification,
            format!(
                "{label} under `{caller}` now says `{}` (was `{}`){}",
                action_word(after.action),
                action_word(before.action),
                if filter_changed {
                    ", and its filter changed"
                } else {
                    ""
                }
            ),
        );
        if after.action == Action::Allow && filter_changed {
            result.pin = pin_change(
                before.filter.as_ref(),
                after.filter.as_ref(),
                FilterChange::Unknown,
            );
        }
        return result;
    }
    if !filter_changed {
        return change(
            Classification::Unknown,
            format!("{label} under `{caller}` was renamed"),
        );
    }
    let relation = filter_change(before.filter.as_ref(), after.filter.as_ref());
    let classification = match (after.action, relation) {
        (_, FilterChange::Unknown) | (Action::AskHuman, _) => Classification::Unknown,
        (Action::Allow, FilterChange::Looser) | (Action::Deny, FilterChange::Tighter) => {
            Classification::Widening
        }
        (Action::Allow, FilterChange::Tighter) | (Action::Deny, FilterChange::Looser) => {
            Classification::Narrowing
        }
    };
    let pin = match after.action {
        Action::Allow => pin_change(before.filter.as_ref(), after.filter.as_ref(), relation),
        Action::Deny | Action::AskHuman => None,
    };
    let what = match &pin {
        Some(PinChange::Removed { field, variable }) => format!(
            "{label} under `{caller}` no longer pins `{field}` to `${{vars.{variable}}}`, so \
             `{}` now matches whatever `{field}` the session is bound to or not",
            after.capability
        ),
        Some(PinChange::PossiblyWeakened { variables }) => format!(
            "{label} under `{caller}` changed its filter; its pin to {} may be weakened",
            variable_list(variables)
        ),
        None => format!(
            "{label} under `{caller}` {} its filter",
            match relation {
                FilterChange::Looser => "loosened",
                FilterChange::Tighter => "tightened",
                FilterChange::Unknown => "changed",
            }
        ),
    };
    let mut result = change(classification, what);
    result.pin = pin;
    result
}

/// The pin analysis for an allow rule whose filter changed.
fn pin_change(
    before: Option<&FilterExpr>,
    after: Option<&FilterExpr>,
    relation: FilterChange,
) -> Option<PinChange> {
    let before = before?;
    let pins_of = |filter: Option<&FilterExpr>| -> Vec<(String, String)> {
        filter.map_or_else(Vec::new, |filter| {
            filter
                .conjuncts()
                .into_iter()
                .filter_map(FilterExpr::as_pin)
                .map(|(field, var)| (field, var.to_owned()))
                .collect()
        })
    };
    let old_pins = pins_of(Some(before));
    let new_pins = pins_of(after);
    let referenced: Vec<&str> = after.map_or_else(Vec::new, |f| f.var_refs());
    let mut weakened: BTreeSet<String> = BTreeSet::new();
    for (field, variable) in &old_pins {
        if new_pins.contains(&(field.clone(), variable.clone())) {
            continue;
        }
        if !referenced.contains(&variable.as_str()) {
            return Some(PinChange::Removed {
                field: field.clone(),
                variable: variable.clone(),
            });
        }
        weakened.insert(variable.clone());
    }
    // Every pin still a top-level conjunct holds whatever else changed: the filter
    // matches only calls that satisfy it. A variable-bearing filter with no pin to
    // keep may be loosened past its variables by any edit short of a tightening.
    if old_pins.is_empty() && relation != FilterChange::Tighter {
        for variable in before.var_refs().into_iter().chain(referenced) {
            weakened.insert(variable.to_owned());
        }
    }
    (!weakened.is_empty()).then(|| PinChange::PossiblyWeakened {
        variables: weakened.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bp(yaml: &str) -> Blueprint {
        crate::parse(yaml).unwrap_or_else(|error| panic!("{error}\n{yaml}"))
    }

    fn with_main(rules: &str) -> Blueprint {
        bp(&format!(
            "name: demo\nvariables:\n  customerId:\n    required: true\npermissions:\n  main:\n{rules}"
        ))
    }

    const PINNED: &str = "    - name: charges-for-signed-in-customer\n      capability: http.get\n      \
                          filter: customerId == ${vars.customerId} and amount < 500\n      action: allow\n";

    fn only(diff: &BlueprintDiff) -> &Change {
        assert_eq!(diff.changes.len(), 1, "{diff:#?}");
        &diff.changes[0]
    }

    #[test]
    fn identical_blueprints_have_no_changes() {
        let diff = diff(&with_main(PINNED), &with_main(PINNED));
        assert!(diff.changes.is_empty());
        assert_eq!(diff.classification, Classification::Unknown);
    }

    // Covers AE5.
    #[test]
    fn removing_the_pin_conjunct_widens_and_flags_a_pin_removal() {
        let after = PINNED.replace("customerId == ${vars.customerId} and ", "");
        let diff = diff(&with_main(PINNED), &with_main(&after));
        let change = only(&diff);
        assert_eq!(diff.classification, Classification::Widening);
        assert_eq!(
            change.pin,
            Some(PinChange::Removed {
                field: "customerId".into(),
                variable: "customerId".into()
            })
        );
        assert_eq!(
            change.rule.as_deref(),
            Some("`charges-for-signed-in-customer`")
        );
        assert_eq!(diff.pin_removals().count(), 1);
        assert!(
            diff.summary().contains("no longer pins `customerId`"),
            "{}",
            diff.summary()
        );
    }

    #[test]
    fn removing_the_whole_filter_of_a_pinned_rule_is_a_pin_removal() {
        let after = PINNED.replace(
            "      filter: customerId == ${vars.customerId} and amount < 500\n",
            "",
        );
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Widening);
        assert!(matches!(only(&diff).pin, Some(PinChange::Removed { .. })));
    }

    #[test]
    fn an_or_beside_the_pin_widens_and_may_weaken_it() {
        let before = "    - capability: http.get\n      filter: customerId == ${vars.customerId}\n      action: allow\n";
        let after = "    - capability: http.get\n      filter: customerId == ${vars.customerId} or amount > 0\n      action: allow\n";
        let diff = diff(&with_main(before), &with_main(after));
        let change = only(&diff);
        assert_eq!(diff.classification, Classification::Widening);
        assert_eq!(
            change.pin,
            Some(PinChange::PossiblyWeakened {
                variables: vec!["customerId".into()]
            })
        );
        assert_eq!(diff.pin_removals().count(), 0);
    }

    #[test]
    fn rewriting_a_pinned_filter_beyond_recognition_is_unknown_and_may_weaken_the_pin() {
        let after = PINNED.replace(
            "customerId == ${vars.customerId} and amount < 500",
            "customerId != ${vars.customerId}",
        );
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Unknown);
        assert!(matches!(
            only(&diff).pin,
            Some(PinChange::PossiblyWeakened { .. })
        ));
    }

    #[test]
    fn editing_beside_an_intact_pin_raises_no_pin_flag() {
        let after = PINNED.replace("amount < 500", "amount < 100");
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Unknown);
        assert_eq!(only(&diff).pin, None);
    }

    #[test]
    fn widening_a_variable_bearing_filter_with_no_pin_may_weaken_it() {
        let before = "    - capability: http.get\n      filter: customerId == ${vars.customerId} or amount < 5\n      action: allow\n";
        let after = "    - capability: http.get\n      filter: customerId == ${vars.customerId} or amount < 5 or amount > 100\n      action: allow\n";
        let diff = diff(&with_main(before), &with_main(after));
        assert_eq!(diff.classification, Classification::Widening);
        assert!(matches!(
            only(&diff).pin,
            Some(PinChange::PossiblyWeakened { .. })
        ));
    }

    #[test]
    fn tightening_a_pinned_filter_narrows_with_no_pin_flag() {
        let after = PINNED.replace("amount < 500", "amount < 500 and currency == \"usd\"");
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Narrowing);
        assert_eq!(only(&diff).pin, None);
    }

    #[test]
    fn loosening_beside_an_intact_pin_widens_with_no_pin_flag() {
        let after = PINNED.replace(" and amount < 500", "");
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Widening);
        assert_eq!(only(&diff).pin, None);
    }

    #[test]
    fn removing_a_variable_conjunct_from_a_deny_rule_narrows_with_no_pin_flag() {
        let before = "    - capability: http.get\n      filter: customerId == ${vars.customerId} and amount > 500\n      action: deny\n";
        let after = "    - capability: http.get\n      filter: amount > 500\n      action: deny\n";
        let diff = diff(&with_main(before), &with_main(after));
        assert_eq!(diff.classification, Classification::Narrowing);
        assert_eq!(only(&diff).pin, None);
    }

    const DENY: &str =
        "    - capability: http.get\n      filter: host == \"evil.example\"\n      action: deny\n";
    const ALLOW: &str = "    - capability: http.get\n      action: allow\n";

    #[test]
    fn moving_an_allow_rule_above_a_deny_rule_for_the_same_capability_widens() {
        let before = format!("{DENY}{ALLOW}");
        let after = format!("{ALLOW}{DENY}");
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Widening, "{diff:#?}");
        assert!(only(&diff).summary.contains("moved"), "{diff:#?}");
    }

    #[test]
    fn moving_an_allow_rule_below_a_deny_rule_narrows() {
        let before = format!("{ALLOW}{DENY}");
        let after = format!("{DENY}{ALLOW}");
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Narrowing, "{diff:#?}");
    }

    #[test]
    fn moving_past_rules_for_other_capabilities_is_unknown() {
        let other = "    - capability: http.post\n      action: deny\n";
        let before = format!("{other}{ALLOW}");
        let after = format!("{ALLOW}{other}");
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Unknown, "{diff:#?}");
    }

    #[test]
    fn adding_an_allow_rule_widens_and_removing_one_narrows() {
        let added = diff(&with_main(DENY), &with_main(&format!("{DENY}{ALLOW}")));
        assert_eq!(added.classification, Classification::Widening);
        assert!(only(&added).summary.contains("gained an allow rule"));
        let removed = diff(&with_main(&format!("{DENY}{ALLOW}")), &with_main(DENY));
        assert_eq!(removed.classification, Classification::Narrowing);
    }

    #[test]
    fn adding_a_deny_rule_narrows_and_removing_one_widens() {
        let added = diff(&with_main(ALLOW), &with_main(&format!("{DENY}{ALLOW}")));
        assert_eq!(added.classification, Classification::Narrowing);
        let removed = diff(&with_main(&format!("{DENY}{ALLOW}")), &with_main(ALLOW));
        assert_eq!(removed.classification, Classification::Widening);
    }

    #[test]
    fn an_ask_human_rule_added_is_unknown() {
        let ask = "    - capability: http.get\n      action: ask-human\n";
        let diff = diff(&with_main(ALLOW), &with_main(&format!("{ask}{ALLOW}")));
        assert_eq!(diff.classification, Classification::Unknown);
    }

    #[test]
    fn flipping_a_rules_action_follows_the_direction() {
        let deny = ALLOW.replace("allow", "deny");
        assert_eq!(
            diff(&with_main(&deny), &with_main(ALLOW)).classification,
            Classification::Widening
        );
        assert_eq!(
            diff(&with_main(ALLOW), &with_main(&deny)).classification,
            Classification::Narrowing
        );
    }

    #[test]
    fn default_deny_to_allow_widens_and_back_narrows() {
        let deny = bp("name: demo\ndefault: deny\n");
        let allow = bp("name: demo\ndefault: allow\n");
        let absent = bp("name: demo\n");
        assert_eq!(diff(&deny, &allow).classification, Classification::Widening);
        assert_eq!(
            diff(&absent, &allow).classification,
            Classification::Widening
        );
        assert_eq!(
            diff(&allow, &deny).classification,
            Classification::Narrowing
        );
        // An absent default already denies.
        assert!(diff(&absent, &deny).changes.is_empty());
    }

    #[test]
    fn adding_a_package_widens_and_removing_one_narrows() {
        let without = bp("name: demo\n");
        let with = bp("name: demo\npackages: ['@acme/billing']\n");
        let added = diff(&without, &with);
        assert_eq!(added.classification, Classification::Widening);
        assert!(only(&added).summary.contains("`@acme/billing`"));
        assert_eq!(
            diff(&with, &without).classification,
            Classification::Narrowing
        );
    }

    #[test]
    fn a_new_required_variable_narrows_and_is_listed() {
        let before = bp("name: demo\n");
        let after = bp("name: demo\nvariables:\n  tenant:\n    required: true\n");
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Narrowing);
        assert_eq!(diff.new_required_variables, ["tenant"]);
        assert!(only(&diff).summary.contains("opened before this change"));
    }

    #[test]
    fn an_optional_variable_declared_is_unknown_and_not_listed() {
        let before = bp("name: demo\n");
        let after = bp("name: demo\nvariables:\n  tenant:\n    default: acme\n");
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Unknown);
        assert!(diff.new_required_variables.is_empty());
    }

    #[test]
    fn a_rule_moved_between_caller_blocks_is_mixed() {
        let before = bp(
            "name: demo\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n",
        );
        let after = bp(
            "name: demo\npermissions:\n  '@acme/billing':\n    - capability: http.get\n      action: allow\n",
        );
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Mixed);
        assert!(
            only(&diff)
                .summary
                .contains("moved from `main` to `@acme/billing`")
        );
    }

    #[test]
    fn changes_pulling_both_ways_are_mixed() {
        let before = bp("name: demo\ndefault: deny\npackages: ['@acme/a']\n");
        let after = bp("name: demo\ndefault: allow\n");
        assert_eq!(diff(&before, &after).classification, Classification::Mixed);
    }

    #[test]
    fn unknown_beside_a_classified_change_is_unknown() {
        let before = bp("name: demo\n");
        let after = bp("name: demo\nidle_timeout: 5m\npackages: ['@acme/a']\n");
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Unknown);
        assert_eq!(diff.changes.len(), 2);
    }

    #[test]
    fn an_unlisted_section_changing_is_unknown() {
        let before = bp("name: demo\n");
        let after = bp("name: demo\nvfs:\n  mode: none\n");
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Unknown);
        assert!(only(&diff).summary.contains("`vfs`"));
    }

    #[test]
    fn allowing_cleartext_http_widens() {
        let before = bp("name: demo\n");
        let after = bp("name: demo\nallow_insecure_http: true\n");
        assert_eq!(
            diff(&before, &after).classification,
            Classification::Widening
        );
        assert_eq!(
            diff(&after, &before).classification,
            Classification::Narrowing
        );
    }

    #[test]
    fn renaming_a_rule_is_unknown() {
        let after = PINNED.replace("charges-for-signed-in-customer", "customer-charges");
        let diff = diff(&with_main(PINNED), &with_main(&after));
        assert_eq!(diff.classification, Classification::Unknown);
    }

    #[test]
    fn an_edited_rule_that_also_moved_is_not_an_in_place_edit() {
        // The allow rule moves ahead of the deny rule and its filter changes: it is
        // read as one rule removed and another added, never as a pin analysis of a
        // rule in a different position.
        let before = format!("{DENY}{PINNED}");
        let after = format!("{}{DENY}", PINNED.replace(" and amount < 500", ""));
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Mixed, "{diff:#?}");
        assert!(diff.changes.iter().all(|change| change.pin.is_none()));
    }

    fn rules(rules: &str) -> Blueprint {
        bp(&format!("name: demo\npermissions:\n  main:\n{rules}"))
    }

    #[test]
    fn two_edited_rules_that_swap_are_not_judged_in_place() {
        // Read rule by rule, each edit narrows (the deny rule loosens, the allow rule
        // tightens), but the swap lets through a call the deny rule decided.
        let before = rules(
            "    - name: block-big\n      capability: http.get\n      filter: amount > 100 and \
             region == \"eu\"\n      action: deny\n    - name: allow-charges\n      capability: \
             http.get\n      filter: amount > 0\n      action: allow\n",
        );
        let after = rules(
            "    - name: allow-charges\n      capability: http.get\n      filter: amount > 0 and \
             currency == \"usd\"\n      action: allow\n    - name: block-big\n      capability: \
             http.get\n      filter: amount > 100\n      action: deny\n",
        );
        let context = serde_json::json!({ "amount": 500, "region": "eu", "currency": "usd" });
        let vars = crate::VarBindings::default();
        assert_eq!(
            before.resolve_permission("main", "http.get", &context, &vars),
            Action::Deny
        );
        assert_eq!(
            after.resolve_permission("main", "http.get", &context, &vars),
            Action::Allow
        );
        let diff = diff(&before, &after);
        assert_eq!(diff.classification, Classification::Mixed, "{diff:#?}");
    }

    #[test]
    fn a_pin_removed_by_an_edit_that_moves_past_another_capabilitys_rule_is_flagged() {
        let other = "    - capability: http.post\n      action: deny\n";
        let before = format!("{PINNED}{other}");
        let after = format!(
            "{other}{}",
            PINNED.replace("customerId == ${vars.customerId} and ", "")
        );
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Widening, "{diff:#?}");
        assert_eq!(diff.pin_removals().count(), 1, "{diff:#?}");
    }

    #[test]
    fn a_pin_removed_by_an_edit_that_moves_past_a_rule_for_its_capability_is_flagged() {
        let before = format!("{DENY}{PINNED}");
        let after = format!(
            "{}{DENY}",
            PINNED.replace("customerId == ${vars.customerId} and ", "")
        );
        let diff = diff(&with_main(&before), &with_main(&after));
        assert_eq!(diff.classification, Classification::Mixed, "{diff:#?}");
        let removals: Vec<&Change> = diff.pin_removals().collect();
        assert_eq!(removals.len(), 1, "{diff:#?}");
        assert_eq!(
            removals[0].pin,
            Some(PinChange::Removed {
                field: "customerId".into(),
                variable: "customerId".into()
            })
        );
        assert!(
            diff.summary().starts_with(&removals[0].summary),
            "{}",
            diff.summary()
        );
        assert!(
            removals[0].summary.contains("pinned `customerId`"),
            "{}",
            removals[0].summary
        );
    }

    #[test]
    fn an_explicit_default_deny_is_the_same_policy_as_an_absent_default() {
        // The two texts parse to different blueprints, so the watcher logs a new
        // version; the classifier finds no change to what programs may do.
        let absent = bp("name: demo\n");
        let explicit = bp("name: demo\ndefault: deny\n");
        assert_ne!(absent, explicit);
        for (before, after) in [(&absent, &explicit), (&explicit, &absent)] {
            let diff = diff(before, after);
            assert!(diff.changes.is_empty(), "{diff:#?}");
            assert_eq!(diff.classification, Classification::Unknown);
            assert_eq!(diff.summary(), "No change to what programs may do.");
        }
    }

    fn numbered_rules(count: usize, edited: Option<usize>) -> Blueprint {
        let mut text = String::new();
        for n in 0..count {
            let bound = if Some(n) == edited { n + 1 } else { n };
            text.push_str(&format!(
                "    - name: r{n}\n      capability: http.get\n      filter: amount > {bound}\n      \
                 action: deny\n"
            ));
        }
        rules(&text)
    }

    #[test]
    fn a_long_block_with_one_edit_is_matched_through_its_common_ends() {
        let diff = diff(&numbered_rules(600, None), &numbered_rules(600, Some(300)));
        let change = only(&diff);
        assert_eq!(change.rule.as_deref(), Some("`r300`"));
    }

    #[test]
    fn a_block_too_changed_to_match_is_unknown_without_a_large_table() {
        let before = numbered_rules(600, None);
        let mut reversed = before.clone();
        if let Some(rules) = reversed.permissions.get_mut("main") {
            rules.reverse();
        }
        let diff = diff(&before, &reversed);
        let change = only(&diff);
        assert_eq!(change.classification, Classification::Unknown);
        assert!(change.summary.contains("too many places"), "{change:#?}");
    }

    /// A small deterministic generator, so the property test needs no dependency.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    const ATOMS: &[&str] = &[
        "amount > 100",
        "amount > 0",
        "amount < 50",
        "region == \"eu\"",
        "currency == \"usd\"",
        "customerId == ${vars.customerId}",
    ];

    #[derive(Clone)]
    struct GenRule {
        name: Option<usize>,
        capability: &'static str,
        filter: Vec<(usize, bool)>,
        action: &'static str,
    }

    impl GenRule {
        fn random(rng: &mut Rng, next_name: &mut usize) -> Self {
            let name = (rng.below(3) > 0).then(|| {
                *next_name += 1;
                *next_name
            });
            let mut rule = GenRule {
                name,
                capability: ["http.get", "http.get", "http.post"][rng.below(3)],
                filter: Vec::new(),
                action: "allow",
            };
            rule.randomize(rng);
            rule
        }

        fn randomize(&mut self, rng: &mut Rng) {
            self.action = ["allow", "deny", "ask-human"][rng.below(3)];
            // Each atom joined to the previous by `and` (false) or `or` (true).
            self.filter = (0..rng.below(3))
                .map(|_| (rng.below(ATOMS.len()), rng.below(2) == 0))
                .collect();
        }

        fn edit_filter(&mut self, rng: &mut Rng) {
            match rng.below(3) {
                0 if !self.filter.is_empty() => {
                    let at = rng.below(self.filter.len());
                    self.filter.remove(at);
                }
                _ => self
                    .filter
                    .push((rng.below(ATOMS.len()), rng.below(2) == 0)),
            }
        }

        fn yaml(&self) -> String {
            let mut text = String::new();
            if let Some(name) = self.name {
                text.push_str(&format!("    - name: r{name}\n      "));
            } else {
                text.push_str("    - ");
            }
            text.push_str(&format!("capability: {}\n", self.capability));
            if !self.filter.is_empty() {
                let mut filter = String::new();
                for (k, (atom, or)) in self.filter.iter().enumerate() {
                    if k > 0 {
                        filter.push_str(if *or { " or " } else { " and " });
                    }
                    filter.push_str(ATOMS[*atom]);
                }
                text.push_str(&format!("      filter: {filter}\n"));
            }
            text.push_str(&format!("      action: {}\n", self.action));
            text
        }
    }

    fn generated(rules: &[GenRule], default: &str) -> Blueprint {
        let body: String = rules.iter().map(GenRule::yaml).collect();
        let permissions = if rules.is_empty() {
            String::new()
        } else {
            format!("permissions:\n  main:\n{body}")
        };
        bp(&format!(
            "name: demo\n{default}variables:\n  customerId:\n    required: true\n{permissions}"
        ))
    }

    fn mutate(rules: &mut Vec<GenRule>, rng: &mut Rng, next_name: &mut usize) {
        let len = rules.len();
        match rng.below(7) {
            0 if len >= 2 => {
                let (a, b) = (rng.below(len), rng.below(len));
                rules.swap(a, b);
            }
            1 if len >= 1 => {
                let at = rng.below(len);
                rules.remove(at);
            }
            2 if len >= 1 => {
                let at = rng.below(len);
                rules[at].edit_filter(rng);
            }
            3 if len >= 1 => {
                let at = rng.below(len);
                rules[at].action = ["allow", "deny", "ask-human"][rng.below(3)];
            }
            5 if len >= 2 => {
                // Two rules swap and both change, as when one edit reorders a block.
                let (a, b) = (rng.below(len), rng.below(len));
                rules.swap(a, b);
                rules[a].edit_filter(rng);
                rules[b].edit_filter(rng);
            }
            4 if len >= 1 => {
                let rule = rules.remove(rng.below(len));
                let at = rng.below(rules.len() + 1);
                rules.insert(at, rule);
            }
            _ => {
                let at = rng.below(len + 1);
                rules.insert(at, GenRule::random(rng, next_name));
            }
        }
    }

    // Guards the classifier as a whole: whenever it calls a change narrowing (or
    // finds none), no call is allowed or asked about that was decided less
    // permissively before; whenever it calls one widening, none is decided less
    // permissively after.
    #[test]
    fn a_classified_direction_holds_for_every_call() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut contexts = Vec::new();
        for amount in [0, 20, 70, 150] {
            for region in ["eu", "us"] {
                for currency in ["usd", "eur"] {
                    for customer in ["c1", "c2"] {
                        contexts.push(serde_json::json!({
                            "amount": amount, "region": region,
                            "currency": currency, "customerId": customer,
                        }));
                    }
                }
            }
        }
        let vars: crate::VarBindings = [("customerId".to_owned(), "c1".to_owned())]
            .into_iter()
            .collect();
        let defaults = [
            "",
            "default: deny\n",
            "default: allow\n",
            "default: ask-human\n",
        ];
        for _ in 0..6000 {
            let mut next_name = 0;
            let mut old: Vec<GenRule> = (0..rng.below(5))
                .map(|_| GenRule::random(&mut rng, &mut next_name))
                .collect();
            let old_default = defaults[rng.below(defaults.len())];
            let mut new = old.clone();
            for _ in 0..=rng.below(3) {
                mutate(&mut new, &mut rng, &mut next_name);
            }
            let new_default = if rng.below(4) == 0 {
                defaults[rng.below(defaults.len())]
            } else {
                old_default
            };
            if rng.below(2) == 0 {
                std::mem::swap(&mut old, &mut new);
            }
            let (before, after) = (generated(&old, old_default), generated(&new, new_default));
            let diff = diff(&before, &after);
            let narrowing =
                diff.changes.is_empty() || diff.classification == Classification::Narrowing;
            let widening =
                diff.changes.is_empty() || diff.classification == Classification::Widening;
            for capability in ["http.get", "http.post"] {
                for context in &contexts {
                    let was = before.resolve_permission("main", capability, context, &vars);
                    let now = after.resolve_permission("main", capability, context, &vars);
                    let wider = rank(now) > rank(was);
                    let narrower = rank(now) < rank(was);
                    assert!(
                        !(narrowing && wider) && !(widening && narrower),
                        "{capability} {context}: {was:?} -> {now:?} under {}\nbefore:\n{}\nafter:\n{}\n{diff:#?}",
                        diff.classification,
                        crate::to_yaml(&before),
                        crate::to_yaml(&after),
                    );
                }
            }
        }
    }

    #[test]
    fn the_diff_serializes_with_kebab_case_classifications() {
        let after = PINNED.replace("customerId == ${vars.customerId} and ", "");
        let value = serde_json::to_value(diff(&with_main(PINNED), &with_main(&after))).unwrap();
        assert_eq!(value["classification"], "widening");
        assert_eq!(value["changes"][0]["pin"]["kind"], "removed");
        assert_eq!(value["changes"][0]["caller"], "main");
    }
}
