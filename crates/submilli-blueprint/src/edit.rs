//! Drafting an `allow` rule from a refused call, as a checked splice into the
//! blueprint's YAML text.
//!
//! The draft allows exactly the observed call: its filter compares every
//! top-level scalar field of the call's context for equality. No per-capability
//! context schema exists in this crate, so fields holding arrays or objects are
//! not compared (the filter language has no whole-value equality for them). A
//! string equal to a bound, declared session variable is written as
//! `${vars.NAME}`; every other value is written through the filter language's
//! quoting. A value holding a newline or control character is refused, and so
//! is an integer past 2^53 - 1, which the filter language's f64 comparison
//! cannot tell from its neighbours.
//!
//! The rule goes directly above the rule that decided, or at the end of the
//! caller's block when the default decided. The text is edited line by line,
//! never re-serialized, so comments and layout survive. The result is then
//! checked: it parses, every pre-existing line and rule is unchanged, the call
//! now resolves to `allow` through the drafted rule, and a call differing in
//! any compared field is not matched by it. The draft is returned for review;
//! writing it is the caller's step.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use serde_json::Value;

use crate::filter::{self, VarBindings};
use crate::permissions::{Action, PermissionRule, ResolutionCause, RuleRef};
use crate::{Blueprint, BlueprintError, VariableDecl};

/// The refused call a rule is drafted from.
#[derive(Debug, Clone, Copy)]
pub struct DraftCall<'a> {
    pub caller: &'a str,
    pub capability: &'a str,
    /// The context the policy tested, as passed to
    /// [`Blueprint::explain_permission`].
    pub context: &'a Value,
    /// The session's variable bindings.
    pub vars: &'a VarBindings,
    /// The name the drafted rule carries, written as its first key; `None`
    /// drafts an unnamed rule. It must be unique within the caller's block.
    pub name: Option<&'a str>,
    /// The blueprint the refusal was decided under. The draft is refused when
    /// the current text decides the call through a different rule (rules are
    /// compared by content, not position, so rules added around it do not
    /// count), or through a rule where the default decided, or the reverse.
    pub decided_under: &'a Blueprint,
}

/// A drafted rule and the blueprint text that contains it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The whole blueprint with the rule inserted, in the file's line endings.
    pub text: String,
    /// The inserted rule's lines, joined with `\n`.
    pub rule: String,
    /// The rule's first and last line in `text`, 1-based.
    pub lines: RangeInclusive<usize>,
    /// The rule that decided the refusal, which the draft now precedes and
    /// overrides for this call. `None` when the default decided.
    pub overrides: Option<RuleRef>,
}

/// Why a rule could not be drafted.
#[derive(Debug)]
pub enum DraftError {
    /// The current blueprint text does not parse.
    CurrentInvalid(BlueprintError),
    /// The current blueprint already allows the call.
    AlreadyAllowed,
    /// The current blueprint decides the call differently than the refusal
    /// being drafted from; the text changed since.
    DecisionChanged { current: ResolutionCause },
    /// A value holds a newline or control character, which a rule cannot
    /// carry safely. `field` is the context field, `caller`, or `capability`.
    UnsafeValue { field: String, character: char },
    /// A string value holds `${vars.`, which the filter language always reads
    /// as a variable placeholder.
    UnquotableValue { field: String },
    /// An integer field too large for an exact comparison: filters compare
    /// numbers as f64, which cannot tell integers past 2^53 - 1 apart.
    InexactNumber { field: String },
    /// A context field name the filter language cannot address as a single
    /// field (a dot, a dash, or a keyword such as `not`).
    UnaddressableField { field: String },
    /// The context is neither an object nor null.
    UnsupportedContext,
    /// The requested rule name is empty.
    EmptyName,
    /// The requested rule name holds a newline or control character, which a rule
    /// cannot carry safely.
    UnsafeName { character: char },
    /// Another rule in the caller's block already has the requested name.
    /// `position` is that rule's 1-based place in the block.
    DuplicateName {
        caller: String,
        name: String,
        position: usize,
    },
    /// The blueprint has rules for the caller, but no block for it was found
    /// in the text.
    CallerBlockNotFound { caller: String },
    /// More than one block in the text names the caller.
    CallerBlockAmbiguous { caller: String },
    /// The text under `permissions:` (or the line endings of the whole text)
    /// uses YAML the splice does not edit: anchors, aliases, tags, flow
    /// collections, or irregular indentation. `line` is 1-based; `caller` is
    /// the block the line is in, when that block's key could be read, which
    /// may be another caller's than the one drafted for.
    UnsupportedLayout {
        line: usize,
        caller: Option<String>,
        reason: &'static str,
    },
    /// The drafted text does not parse.
    DraftInvalid(BlueprintError),
    /// The drafted text changes a pre-existing line or rule.
    ExistingChanged,
    /// The drafted call does not resolve to `allow` through the drafted rule.
    DoesNotAllow,
    /// The drafted rule matches a call whose `field` differs.
    TooBroad { field: String },
    /// The drafted rule does not read back with the requested name.
    NameNotKept,
}

impl fmt::Display for DraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DraftError::CurrentInvalid(err) => write!(f, "the blueprint does not parse: {err}"),
            DraftError::AlreadyAllowed => f.write_str("the blueprint already allows this call"),
            DraftError::DecisionChanged { .. } => f.write_str(
                "the blueprint no longer decides this call the way the refusal reported; \
                 it changed since",
            ),
            DraftError::UnsafeValue { field, character } => write!(
                f,
                "`{field}` holds {} ({}), which a rule cannot match safely",
                character_kind(*character),
                character.escape_unicode()
            ),
            DraftError::UnquotableValue { field } => write!(
                f,
                "`{field}` holds `${{vars.`, which a filter string always reads as a variable"
            ),
            DraftError::InexactNumber { field } => write!(
                f,
                "`{field}` is a number too large for a filter to compare exactly; a rule \
                 comparing it would also allow its neighbours"
            ),
            DraftError::UnaddressableField { field } => {
                write!(f, "context field `{field}` cannot be named in a filter")
            }
            DraftError::UnsupportedContext => {
                f.write_str("the call's context is neither an object nor null")
            }
            DraftError::EmptyName => f.write_str("a rule name cannot be empty"),
            DraftError::UnsafeName { character } => write!(
                f,
                "the rule name holds {} ({}), which a rule cannot carry safely",
                character_kind(*character),
                character.escape_unicode()
            ),
            DraftError::DuplicateName {
                caller,
                name,
                position,
            } => write!(
                f,
                "rule {position} of `{caller}` is already named `{name}`; rule names must be \
                 unique within a caller block"
            ),
            DraftError::CallerBlockNotFound { caller } => {
                write!(f, "no `{caller}` block was found under `permissions:`")
            }
            DraftError::CallerBlockAmbiguous { caller } => {
                write!(
                    f,
                    "more than one `{caller}` block appears under `permissions:`"
                )
            }
            DraftError::UnsupportedLayout {
                line,
                caller,
                reason,
            } => {
                write!(f, "line {line}")?;
                if let Some(caller) = caller {
                    write!(f, ", in the `{caller}` block")?;
                }
                write!(
                    f,
                    ": {reason}; add the rule by hand or rewrite the block in plain block style"
                )
            }
            DraftError::DraftInvalid(err) => {
                write!(f, "the drafted blueprint does not parse: {err}")
            }
            DraftError::ExistingChanged => {
                f.write_str("the draft would change an existing line or rule")
            }
            DraftError::DoesNotAllow => f.write_str("the drafted rule would not allow the call"),
            DraftError::TooBroad { field } => write!(
                f,
                "the drafted rule would also allow a call with a different `{field}`"
            ),
            DraftError::NameNotKept => {
                f.write_str("the drafted rule does not read back with the name it was given")
            }
        }
    }
}

impl std::error::Error for DraftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DraftError::CurrentInvalid(err) | DraftError::DraftInvalid(err) => Some(err),
            _ => None,
        }
    }
}

/// Draft a rule allowing exactly `call` into the blueprint `text`, checked as
/// described in the module documentation. `text` is not modified.
pub fn draft_allow(text: &str, call: &DraftCall<'_>) -> Result<Draft, DraftError> {
    require_printable("caller", call.caller)?;
    require_printable("capability", call.capability)?;
    let current = crate::parse(text).map_err(DraftError::CurrentInvalid)?;
    if let Some(name) = call.name {
        require_new_name(&current, call.caller, name)?;
    }
    let resolution =
        current.explain_permission(call.caller, call.capability, call.context, call.vars);
    if resolution.action == Action::Allow {
        return Err(DraftError::AlreadyAllowed);
    }
    let decided = call
        .decided_under
        .explain_permission(call.caller, call.capability, call.context, call.vars)
        .cause;
    if !same_decider(call.decided_under, &decided, &current, &resolution.cause) {
        return Err(DraftError::DecisionChanged {
            current: resolution.cause,
        });
    }
    let filter = draft_filter(call.context, call.vars, &current.variables)?;
    let existing = current.permissions.get(call.caller).map_or(0, Vec::len);
    let (index, overrides) = match resolution.cause {
        ResolutionCause::Rule(rule) => (rule.index, Some(rule)),
        ResolutionCause::Default { .. } => (existing, None),
    };

    let lines = split_lines(text)?;
    let rule = RuleText {
        name: call.name.map(yaml_scalar),
        capability: yaml_scalar(call.capability),
        filter: filter.text.as_deref().map(yaml_scalar),
    };
    let splice = plan(&lines, call.caller, index, existing, &rule)?;
    let new_text = apply_checked(&current, &lines, &splice, call, index, &filter.compared)?;

    let inserted = splice.rule_lines();
    let first = splice.at + splice.rule_start + 1;
    Ok(Draft {
        text: new_text,
        rule: inserted.join("\n"),
        lines: first..=first + inserted.len().saturating_sub(1),
        overrides,
    })
}

/// Whether `was`, under `before`, and `now`, under `after`, are the same
/// decider: rules with the same content, or the default both times (whether or
/// not the caller has a block, since no rule in it matched).
fn same_decider(
    before: &Blueprint,
    was: &ResolutionCause,
    after: &Blueprint,
    now: &ResolutionCause,
) -> bool {
    match (was, now) {
        (ResolutionCause::Rule(was), ResolutionCause::Rule(now)) => {
            match (rule_at(before, was), rule_at(after, now)) {
                (Some(was), Some(now)) => was == now,
                _ => false,
            }
        }
        (ResolutionCause::Default { .. }, ResolutionCause::Default { .. }) => true,
        _ => false,
    }
}

fn rule_at<'b>(blueprint: &'b Blueprint, at: &RuleRef) -> Option<&'b PermissionRule> {
    blueprint
        .permissions
        .get(&at.caller)
        .and_then(|rules| rules.get(at.index))
}

// ---------------------------------------------------------------------------
// The filter.

struct DraftedFilter {
    /// `None` when the context has no scalar field: the rule then matches every
    /// call to the capability, which is every call this context describes.
    text: Option<String>,
    /// The fields the filter compares, in context order.
    compared: Vec<String>,
}

fn draft_filter(
    context: &Value,
    vars: &VarBindings,
    declared: &BTreeMap<String, VariableDecl>,
) -> Result<DraftedFilter, DraftError> {
    let fields = match context {
        Value::Object(fields) => fields,
        Value::Null => {
            return Ok(DraftedFilter {
                text: None,
                compared: Vec::new(),
            });
        }
        _ => return Err(DraftError::UnsupportedContext),
    };
    let mut comparisons = Vec::new();
    let mut compared = Vec::new();
    for (field, value) in fields {
        let Some(operand) = operand(field, value, vars, declared)? else {
            continue;
        };
        if !filter::is_field_name(field) {
            return Err(DraftError::UnaddressableField {
                field: field.clone(),
            });
        }
        comparisons.push(format!("{field} == {operand}"));
        compared.push(field.clone());
    }
    Ok(DraftedFilter {
        text: (!comparisons.is_empty()).then(|| comparisons.join(" and ")),
        compared,
    })
}

/// The filter operand `value` is compared to, or `None` for an array or
/// object, which the draft does not compare.
fn operand(
    field: &str,
    value: &Value,
    vars: &VarBindings,
    declared: &BTreeMap<String, VariableDecl>,
) -> Result<Option<String>, DraftError> {
    let operand = match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            require_exact_number(field, n)?;
            n.to_string()
        }
        Value::String(s) => {
            require_printable(field, s)?;
            match bound_variable(s, vars, declared) {
                Some(name) => format!("${{vars.{name}}}"),
                None => filter::quote_literal(s).ok_or_else(|| DraftError::UnquotableValue {
                    field: field.to_string(),
                })?,
            }
        }
        Value::Array(_) | Value::Object(_) => return Ok(None),
    };
    Ok(Some(operand))
}

/// The largest integer an f64 holds with no other integer rounding to it.
const MAX_EXACT_INTEGER: u64 = (1 << 53) - 1;

/// [`MAX_EXACT_INTEGER`] as an f64, which holds it exactly.
const MAX_EXACT_FLOAT: f64 = 9_007_199_254_740_991.0;

/// Refuses a number that a filter, comparing numbers as f64, would find equal
/// to its integer neighbours: any integer, or float, past 2^53 - 1 in
/// magnitude. A smaller float compares exactly, as no other integer rounds to
/// it and a float only equals itself.
fn require_exact_number(field: &str, n: &serde_json::Number) -> Result<(), DraftError> {
    let inexact = match n.as_u64().or_else(|| n.as_i64().map(i64::unsigned_abs)) {
        Some(magnitude) => magnitude > MAX_EXACT_INTEGER,
        None => n.as_f64().is_some_and(|x| x.abs() > MAX_EXACT_FLOAT),
    };
    if inexact {
        return Err(DraftError::InexactNumber {
            field: field.to_string(),
        });
    }
    Ok(())
}

/// The first (by name) declared variable the session bound to `value`. Only
/// strings are matched: a variable is text, and pinning a number or flag to
/// whichever variable happens to share its spelling would mislead.
fn bound_variable<'a>(
    value: &str,
    vars: &'a VarBindings,
    declared: &BTreeMap<String, VariableDecl>,
) -> Option<&'a str> {
    vars.iter()
        .find(|(name, bound)| {
            bound.as_str() == value
                && declared.contains_key(name.as_str())
                && filter::is_valid_var_name(name)
        })
        .map(|(name, _)| name.as_str())
}

/// A name the drafted rule can carry: what the parser and `blueprint lint` accept
/// (non-empty, unique within the caller's block), and printable, as every value
/// the splice writes must be.
fn require_new_name(current: &Blueprint, caller: &str, name: &str) -> Result<(), DraftError> {
    if name.is_empty() {
        return Err(DraftError::EmptyName);
    }
    if let Some(character) = name.chars().find(|&c| is_unsafe_char(c)) {
        return Err(DraftError::UnsafeName { character });
    }
    let taken = current
        .permissions
        .get(caller)
        .into_iter()
        .flatten()
        .position(|rule| rule.name.as_deref() == Some(name));
    match taken {
        Some(index) => Err(DraftError::DuplicateName {
            caller: caller.to_string(),
            name: name.to_string(),
            position: index.saturating_add(1),
        }),
        None => Ok(()),
    }
}

fn require_printable(field: &str, value: &str) -> Result<(), DraftError> {
    match value.chars().find(|&c| is_unsafe_char(c)) {
        Some(character) => Err(DraftError::UnsafeValue {
            field: field.to_string(),
            character,
        }),
        None => Ok(()),
    }
}

/// Control characters (C0, DEL, C1 — which covers NEL), the Unicode line and
/// paragraph separators the YAML parser also treats as line breaks, and the
/// noncharacters YAML refuses in a stream.
fn is_unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{FFFE}' | '\u{FFFF}')
}

/// What an unsafe character is, in words.
fn character_kind(c: char) -> &'static str {
    if is_line_break(c) {
        "a newline"
    } else {
        "a control character"
    }
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// A YAML scalar for `value`: plain when that reads back as the same string,
/// single-quoted otherwise. Callers have refused unsafe characters.
fn yaml_scalar(value: &str) -> String {
    if is_plain_safe(value) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

/// Conservative: a plain scalar starting with a letter, `_`, or `(`, with no
/// `: ` or ` #` and no trailing `:` or space, that YAML 1.1 does not read as a
/// boolean or null.
fn is_plain_safe(value: &str) -> bool {
    const SPECIAL: [&str; 9] = ["y", "n", "yes", "no", "true", "false", "on", "off", "null"];
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '(')
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.ends_with([':', ' '])
        && !SPECIAL
            .iter()
            .any(|special| value.eq_ignore_ascii_case(special))
}

// ---------------------------------------------------------------------------
// Reading the text.

/// One line of the source, without its line ending.
#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    content: &'a str,
    /// `\n`, `\r\n`, or empty for a last line without one.
    eol: &'a str,
}

impl Line<'_> {
    fn indent(&self) -> usize {
        self.content.len() - self.content.trim_start_matches(' ').len()
    }

    fn body(&self) -> &str {
        self.content.trim_start_matches(' ')
    }

    /// Neither blank nor a comment.
    fn has_content(&self) -> bool {
        let body = self.body().trim_start();
        !body.is_empty() && !body.starts_with('#')
    }

    fn is_comment(&self) -> bool {
        self.body().trim_start().starts_with('#')
    }

    fn is_item(&self) -> bool {
        let body = self.body();
        body == "-" || body.starts_with("- ")
    }
}

fn split_lines(text: &str) -> Result<Vec<Line<'_>>, DraftError> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (line, eol, next) = match rest.find('\n') {
            Some(end) => {
                let (line, eol) = match rest[..end].strip_suffix('\r') {
                    Some(line) => (line, &rest[end - 1..=end]),
                    None => (&rest[..end], &rest[end..=end]),
                };
                (line, eol, &rest[end + 1..])
            }
            None => (rest, "", ""),
        };
        // YAML also breaks lines at a lone carriage return, NEL, LS, and PS;
        // this scan would not, so it would misjudge the structure.
        if line.contains(is_line_break) {
            return Err(DraftError::UnsupportedLayout {
                line: lines.len() + 1,
                caller: None,
                reason: "a line ends with a break other than a line feed",
            });
        }
        lines.push(Line { content: line, eol });
        rest = next;
    }
    Ok(lines)
}

/// The `permissions:` mapping as laid out in the text.
struct PermissionsLayout {
    /// Index just past the last content line of the mapping. Comments that
    /// follow it belong to whatever comes next.
    end: usize,
    /// Indentation of the caller keys, when there are any.
    caller_indent: Option<usize>,
    callers: Vec<CallerLayout>,
}

struct CallerLayout {
    key: String,
    header: usize,
    /// The header reads `key: []`.
    flow_empty: bool,
    /// Index just past the block's last content line.
    end: usize,
    /// The first line of each rule, in order.
    items: Vec<usize>,
    dash: Option<DashStyle>,
}

/// How a block writes its list items: the dash's indentation and the spaces
/// between the dash and the item's first key.
#[derive(Debug, Clone, Copy)]
struct DashStyle {
    indent: usize,
    gap: usize,
}

const DEFAULT_CALLER_INDENT: usize = 2;
const DEFAULT_DASH_OFFSET: usize = 2;

fn unsupported(index: usize, reason: &'static str) -> DraftError {
    DraftError::UnsupportedLayout {
        line: index + 1,
        caller: None,
        reason,
    }
}

/// `error`, naming the caller block its line is in.
fn in_block(error: DraftError, key: &str) -> DraftError {
    match error {
        DraftError::UnsupportedLayout {
            line,
            caller: None,
            reason,
        } => DraftError::UnsupportedLayout {
            line,
            caller: Some(key.to_string()),
            reason,
        },
        other => other,
    }
}

fn scan_permissions(lines: &[Line<'_>]) -> Result<Option<PermissionsLayout>, DraftError> {
    let mut header = None;
    for (index, line) in lines.iter().enumerate() {
        if line.indent() != 0 || !line.has_content() {
            continue;
        }
        if mapping_key(line.content).is_some_and(|(key, _)| key == "permissions") {
            if header.is_some() {
                return Err(unsupported(index, "`permissions:` appears twice"));
            }
            header = Some((index, line));
        }
    }
    let Some((header, header_line)) = header else {
        return Ok(None);
    };
    let value = mapping_key(header_line.content).map_or("", |(_, rest)| value_text(rest));
    if !value.is_empty() {
        return Err(unsupported(
            header,
            "`permissions:` is not written as a block mapping",
        ));
    }

    let mut layout = PermissionsLayout {
        end: header + 1,
        caller_indent: None,
        callers: Vec::new(),
    };
    for (index, line) in lines.iter().enumerate().skip(header + 1) {
        if !line.has_content() {
            continue;
        }
        let indent = line.indent();
        if indent == 0 {
            break;
        }
        layout.end = index + 1;
        let caller_indent = *layout.caller_indent.get_or_insert(indent);
        if indent < caller_indent {
            return Err(unsupported(index, "caller keys are not evenly indented"));
        }
        if indent == caller_indent && !line.is_item() {
            layout.callers.push(scan_caller_header(index, line)?);
            continue;
        }
        let Some(caller) = layout.callers.last_mut() else {
            return Err(unsupported(index, "a list item has no caller key above it"));
        };
        scan_block_line(index, line, caller).map_err(|error| in_block(error, &caller.key))?;
    }
    Ok(Some(layout))
}

/// Adds line `index`, under `caller`'s header, to the block's layout.
fn scan_block_line(
    index: usize,
    line: &Line<'_>,
    caller: &mut CallerLayout,
) -> Result<(), DraftError> {
    if caller.flow_empty {
        return Err(unsupported(index, "a `[]` caller block has lines under it"));
    }
    refuse_node_properties(index, line)?;
    let indent = line.indent();
    let dash = match caller.dash {
        Some(dash) => dash,
        None if line.is_item() => *caller.dash.insert(DashStyle {
            indent,
            gap: item_gap(line),
        }),
        None => return Err(unsupported(index, "a caller block is not a list of rules")),
    };
    if indent < dash.indent {
        return Err(unsupported(index, "rules are not evenly indented"));
    }
    if indent == dash.indent {
        if !line.is_item() {
            return Err(unsupported(index, "a caller block is not a list of rules"));
        }
        caller.items.push(index);
    }
    caller.end = index + 1;
    Ok(())
}

fn scan_caller_header(index: usize, line: &Line<'_>) -> Result<CallerLayout, DraftError> {
    let Some((key, rest)) = mapping_key(line.body()) else {
        return Err(unsupported(
            index,
            "a caller key is not a plain or quoted key",
        ));
    };
    let flow_empty = match value_text(rest) {
        "" => false,
        "[]" => true,
        _ => {
            return Err(in_block(
                unsupported(index, "a caller's rules are not written as a block list"),
                &key,
            ));
        }
    };
    Ok(CallerLayout {
        key,
        header: index,
        flow_empty,
        end: index + 1,
        items: Vec::new(),
        dash: None,
    })
}

/// Refuse anchors, aliases, tags, merge keys, complex keys, and flow
/// collections on a rule line: editing beside them could change what they
/// mean elsewhere.
fn refuse_node_properties(index: usize, line: &Line<'_>) -> Result<(), DraftError> {
    let mut node = line.body();
    while let Some(rest) = node.strip_prefix('-') {
        if !(rest.is_empty() || rest.starts_with(' ')) {
            break;
        }
        node = rest.trim_start();
    }
    let value = match mapping_key(node) {
        Some((key, rest)) if key != "<<" => value_text(rest),
        Some(_) => return Err(unsupported(index, "a merge key (`<<`) is used")),
        None => node,
    };
    for text in [node, value] {
        match text.chars().next() {
            Some('&') => return Err(unsupported(index, "an anchor (`&`) is used")),
            Some('*') => return Err(unsupported(index, "an alias (`*`) is used")),
            Some('!') => return Err(unsupported(index, "a tag (`!`) is used")),
            Some('{' | '[') => return Err(unsupported(index, "a flow collection is used")),
            Some('?') => return Err(unsupported(index, "a complex key (`?`) is used")),
            _ => {}
        }
    }
    Ok(())
}

/// Spaces between an item's dash and its content.
fn item_gap(line: &Line<'_>) -> usize {
    let after = line.body().get(1..).unwrap_or("");
    let gap = after.len() - after.trim_start_matches(' ').len();
    if after.trim().is_empty() {
        1
    } else {
        gap.max(1)
    }
}

/// The key of a `key: value` line and the text after its colon. Plain,
/// single-quoted, and double-quoted keys (with `\"` and `\\` escapes) are
/// read; anything else is `None`.
fn mapping_key(node: &str) -> Option<(String, &str)> {
    let (key, after) = match node.chars().next()? {
        '\'' => {
            let mut key = String::new();
            let mut rest = node.get(1..)?;
            loop {
                let end = rest.find('\'')?;
                key.push_str(&rest[..end]);
                rest = &rest[end + 1..];
                match rest.strip_prefix('\'') {
                    Some(more) => {
                        key.push('\'');
                        rest = more;
                    }
                    None => break,
                }
            }
            (key, rest)
        }
        '"' => {
            let mut key = String::new();
            let mut chars = node.char_indices().skip(1);
            let end = loop {
                let (at, c) = chars.next()?;
                match c {
                    '"' => break at + 1,
                    '\\' => match chars.next()?.1 {
                        escaped @ ('"' | '\\') => key.push(escaped),
                        _ => return None,
                    },
                    c => key.push(c),
                }
            };
            (key, node.get(end..)?)
        }
        '-' | '?' | ':' | ',' | '[' | ']' | '{' | '}' | '#' | '&' | '*' | '!' | '|' | '>' | '%'
        | '@' | '`' => return None,
        _ => {
            let colon = node
                .char_indices()
                .find(|&(at, c)| c == ':' && starts_value(&node[at + 1..]))?
                .0;
            if node[..colon].contains(" #") {
                return None;
            }
            return Some((node[..colon].trim_end().to_string(), &node[colon + 1..]));
        }
    };
    let rest = after.strip_prefix(':')?;
    starts_value(rest).then_some((key, rest))
}

fn starts_value(after_colon: &str) -> bool {
    after_colon.is_empty() || after_colon.starts_with([' ', '\t'])
}

/// A value with surrounding space and any trailing comment removed.
fn value_text(rest: &str) -> &str {
    let value = rest.trim_start();
    if value.starts_with('#') {
        return "";
    }
    match value.find(" #") {
        Some(comment) => value[..comment].trim_end(),
        None => value.trim_end(),
    }
}

// ---------------------------------------------------------------------------
// Placing the rule.

struct RuleText {
    /// Written first, when the rule is named.
    name: Option<String>,
    capability: String,
    filter: Option<String>,
}

impl RuleText {
    fn lines(&self, dash: DashStyle) -> Vec<String> {
        let pad = " ".repeat(dash.indent);
        let gap = " ".repeat(dash.gap);
        let inner = " ".repeat(dash.indent + 1 + dash.gap);
        let capability = format!("capability: {}", self.capability);
        let mut lines = match &self.name {
            Some(name) => vec![
                format!("{pad}-{gap}name: {name}"),
                format!("{inner}{capability}"),
            ],
            None => vec![format!("{pad}-{gap}{capability}")],
        };
        if let Some(filter) = &self.filter {
            lines.push(format!("{inner}filter: {filter}"));
        }
        lines.push(format!("{inner}action: allow"));
        lines
    }
}

/// Lines inserted before original line `at`, plus an optional rewrite of one
/// original line (a `key: []` header losing its `[]`).
struct Splice {
    at: usize,
    inserted: Vec<String>,
    /// Where the rule starts within `inserted`; earlier lines create its
    /// caller block or `permissions:`.
    rule_start: usize,
    header: Option<(usize, String)>,
}

impl Splice {
    fn rule_lines(&self) -> &[String] {
        self.inserted.get(self.rule_start..).unwrap_or(&[])
    }

    fn apply(&self, lines: &[Line<'_>]) -> String {
        let eol = lines
            .iter()
            .map(|line| line.eol)
            .find(|eol| !eol.is_empty())
            .unwrap_or("\n");
        let mut out = String::new();
        let push_inserted = |out: &mut String| {
            for line in &self.inserted {
                out.push_str(line);
                out.push_str(eol);
            }
        };
        for (index, line) in lines.iter().enumerate() {
            if index == self.at {
                push_inserted(&mut out);
            }
            match &self.header {
                Some((header, rewritten)) if *header == index => out.push_str(rewritten),
                _ => out.push_str(line.content),
            }
            out.push_str(line.eol);
        }
        if self.at >= lines.len() {
            let ends_with_newline = lines.last().is_none_or(|line| !line.eol.is_empty());
            if !ends_with_newline {
                out.push_str(eol);
            }
            push_inserted(&mut out);
            if !ends_with_newline {
                out.truncate(out.len() - eol.len());
            }
        }
        out
    }
}

fn plan(
    lines: &[Line<'_>],
    caller: &str,
    index: usize,
    existing: usize,
    rule: &RuleText,
) -> Result<Splice, DraftError> {
    let Some(layout) = scan_permissions(lines)? else {
        if existing > 0 {
            return Err(DraftError::CallerBlockNotFound {
                caller: caller.to_string(),
            });
        }
        let caller_indent = DEFAULT_CALLER_INDENT;
        let mut inserted = vec![
            "permissions:".to_string(),
            format!("{}{}:", " ".repeat(caller_indent), yaml_scalar(caller)),
        ];
        inserted.extend(rule.lines(DashStyle {
            indent: caller_indent + DEFAULT_DASH_OFFSET,
            gap: 1,
        }));
        return Ok(Splice {
            at: lines.len(),
            inserted,
            rule_start: 2,
            header: None,
        });
    };

    let caller_indent = layout.caller_indent.unwrap_or(DEFAULT_CALLER_INDENT);
    // A block without rules takes its dash style from a sibling block.
    let (dash_offset, gap) = layout
        .callers
        .iter()
        .find_map(|block| {
            let dash = block.dash?;
            Some((dash.indent.checked_sub(caller_indent)?, dash.gap))
        })
        .unwrap_or((DEFAULT_DASH_OFFSET, 1));
    let sibling_dash = DashStyle {
        indent: caller_indent + dash_offset,
        gap,
    };

    let mut matching = layout.callers.iter().filter(|block| block.key == caller);
    let block = matching.next();
    if matching.next().is_some() {
        return Err(DraftError::CallerBlockAmbiguous {
            caller: caller.to_string(),
        });
    }
    let Some(block) = block else {
        if existing > 0 {
            return Err(DraftError::CallerBlockNotFound {
                caller: caller.to_string(),
            });
        }
        let mut inserted = vec![format!(
            "{}{}:",
            " ".repeat(caller_indent),
            yaml_scalar(caller)
        )];
        inserted.extend(rule.lines(sibling_dash));
        return Ok(Splice {
            at: layout.end,
            inserted,
            rule_start: 1,
            header: None,
        });
    };

    if block.items.len() != existing {
        return Err(unsupported(
            block.header,
            "the caller block's list items do not line up with its rules",
        ));
    }
    let header = match lines.get(block.header) {
        Some(line) if block.flow_empty => Some((block.header, without_empty_flow(line.content))),
        _ => None,
    };
    let at = match block.items.get(index) {
        Some(&item) => with_leading_comments(lines, block.header, item),
        None => block.end,
    };
    Ok(Splice {
        at,
        inserted: rule.lines(block.dash.unwrap_or(sibling_dash)),
        rule_start: 0,
        header,
    })
}

/// `key: []` (with any trailing comment) as `key:`.
fn without_empty_flow(header: &str) -> String {
    let body = header.trim_start_matches(' ');
    let value_start = mapping_key(body).map_or(0, |(_, rest)| header.len() - rest.len());
    match header[value_start..].find("[]") {
        Some(at) => {
            let at = value_start + at;
            format!("{}{}", header[..at].trim_end(), &header[at + 2..])
        }
        None => header.to_string(),
    }
}

/// The first line of `item` counting the comment lines directly above it, so
/// a rule's comment stays with its rule.
fn with_leading_comments(lines: &[Line<'_>], header: usize, item: usize) -> usize {
    let mut start = item;
    while start > header + 1 && lines.get(start - 1).is_some_and(Line::is_comment) {
        start -= 1;
    }
    start
}

// ---------------------------------------------------------------------------
// Checking the result.

/// Apply `splice` and run every check on the result, which places the drafted
/// rule at `index` of the caller's block.
fn apply_checked(
    current: &Blueprint,
    lines: &[Line<'_>],
    splice: &Splice,
    call: &DraftCall<'_>,
    index: usize,
    compared: &[String],
) -> Result<String, DraftError> {
    let new_text = splice.apply(lines);
    let drafted = crate::parse(&new_text).map_err(DraftError::DraftInvalid)?;
    require_lines_kept(lines, splice, &new_text)?;
    require_rules_kept(current, &drafted, call.caller, index)?;
    require_allows(&drafted, call, index)?;
    require_exact(&drafted, call, index, compared)?;
    Ok(new_text)
}

/// Every original line survives, in order, apart from the inserted lines and
/// the one rewritten `[]` header.
fn require_lines_kept(
    lines: &[Line<'_>],
    splice: &Splice,
    new_text: &str,
) -> Result<(), DraftError> {
    let drafted = split_lines(new_text)?;
    let inserted = splice.at..splice.at + splice.inserted.len();
    let kept: Vec<&Line<'_>> = drafted
        .iter()
        .enumerate()
        .filter(|(index, _)| !inserted.contains(index))
        .map(|(_, line)| line)
        .collect();
    let inserted_match = drafted.get(inserted).is_some_and(|got| {
        got.iter()
            .map(|line| line.content)
            .eq(splice.inserted.iter().map(String::as_str))
    });
    let last = lines.len().saturating_sub(1);
    let unchanged = kept.len() == lines.len()
        && kept
            .iter()
            .zip(lines)
            .enumerate()
            .all(|(index, (got, original))| {
                let content = match &splice.header {
                    Some((header, rewritten)) if *header == index => rewritten.as_str(),
                    _ => original.content,
                };
                // A last line without a newline gains one when lines follow it.
                let eol_kept = got.eol == original.eol
                    || (index == last && original.eol.is_empty() && splice.at > last);
                got.content == content && eol_kept
            });
    if inserted_match && unchanged {
        Ok(())
    } else {
        Err(DraftError::ExistingChanged)
    }
}

/// Removing the drafted rule gives back the current blueprint exactly.
fn require_rules_kept(
    current: &Blueprint,
    drafted: &Blueprint,
    caller: &str,
    index: usize,
) -> Result<(), DraftError> {
    let mut without = drafted.clone();
    let Some(rules) = without.permissions.get_mut(caller) else {
        return Err(DraftError::ExistingChanged);
    };
    if index >= rules.len() {
        return Err(DraftError::ExistingChanged);
    }
    rules.remove(index);
    if rules.is_empty() && !current.permissions.contains_key(caller) {
        without.permissions.remove(caller);
    }
    if without == *current {
        Ok(())
    } else {
        Err(DraftError::ExistingChanged)
    }
}

fn require_allows(
    drafted: &Blueprint,
    call: &DraftCall<'_>,
    index: usize,
) -> Result<(), DraftError> {
    let decision =
        drafted.resolve_permission_with_rule(call.caller, call.capability, call.context, call.vars);
    if decision == (Action::Allow, Some(index)) {
        Ok(())
    } else {
        Err(DraftError::DoesNotAllow)
    }
}

/// Changing any compared field takes the call outside the drafted rule.
fn require_exact(
    drafted: &Blueprint,
    call: &DraftCall<'_>,
    index: usize,
    compared: &[String],
) -> Result<(), DraftError> {
    let Some(rule) = drafted
        .permissions
        .get(call.caller)
        .and_then(|rules| rules.get(index))
    else {
        return Err(DraftError::DoesNotAllow);
    };
    if rule.name.as_deref() != call.name {
        return Err(DraftError::NameNotKept);
    }
    for field in compared {
        let mut altered = call.context.clone();
        if let Some(value) = altered.get_mut(field.as_str()) {
            *value = altered_value(value);
        }
        let matched = rule
            .filter
            .as_ref()
            .is_none_or(|filter| filter.matches_with(&altered, call.vars));
        if matched {
            return Err(DraftError::TooBroad {
                field: field.clone(),
            });
        }
    }
    Ok(())
}

/// A value of the same kind (or, for null, any non-null) that differs.
fn altered_value(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(format!("{s}~")),
        Value::Bool(b) => Value::Bool(!b),
        Value::Number(n) => match n.as_f64() {
            Some(x) if x != 0.0 => serde_json::Number::from_f64(-x)
                .map_or_else(|| Value::String("altered".into()), Value::Number),
            _ => Value::from(1),
        },
        Value::Null | Value::Array(_) | Value::Object(_) => Value::String("altered".into()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> VarBindings {
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    /// Draft for the call as the current text decides it.
    fn draft(
        text: &str,
        caller: &str,
        capability: &str,
        context: Value,
        bindings: &VarBindings,
    ) -> Result<Draft, DraftError> {
        draft_named(text, caller, capability, context, bindings, None)
    }

    /// [`draft`], with the drafted rule named `name`.
    fn draft_named(
        text: &str,
        caller: &str,
        capability: &str,
        context: Value,
        bindings: &VarBindings,
        name: Option<&str>,
    ) -> Result<Draft, DraftError> {
        let blueprint = crate::parse(text).expect("fixture parses");
        draft_allow(
            text,
            &DraftCall {
                caller,
                capability,
                context: &context,
                vars: bindings,
                name,
                decided_under: &blueprint,
            },
        )
    }

    /// Draft into `text` for a refusal decided under the blueprint `under`.
    fn draft_under(
        text: &str,
        under: &str,
        capability: &str,
        context: Value,
    ) -> Result<Draft, DraftError> {
        let under = crate::parse(under).expect("fixture parses");
        draft_allow(
            text,
            &DraftCall {
                caller: "main",
                capability,
                context: &context,
                vars: &VarBindings::new(),
                name: None,
                decided_under: &under,
            },
        )
    }

    /// The new text minus the rule's lines and the `created` lines just above
    /// them (a new caller block or `permissions:`) equals `original`.
    fn assert_only_inserted(original: &str, draft: &Draft, created: usize) {
        let first = draft.lines.start() - 1 - created;
        let last = *draft.lines.end();
        let kept: String = draft
            .text
            .split_inclusive('\n')
            .enumerate()
            .filter(|(index, _)| *index < first || *index >= last)
            .map(|(_, line)| line)
            .collect();
        assert_eq!(kept, original, "draft:\n{}", draft.text);
    }

    fn rule_lines(draft: &Draft) -> Vec<&str> {
        draft
            .text
            .lines()
            .skip(draft.lines.start() - 1)
            .take(draft.lines.end() - draft.lines.start() + 1)
            .collect()
    }

    const ODD_LAYOUT: &str = "\
# Billing service blueprint.
kind: blueprint
name:   odd     # trailing comment

variables:
    customerId:
        required: true

permissions:   # who may do what

     # The generated program.
     main:
         -   capability: acme.com/charges.list   # listing
             filter: customerId == ${vars.customerId}
             action: allow


         # Refunds always need a person.
         -   capability: acme.com/refunds.create
             action: ask-human
     # Package code.
     billing:
         -   capability: secrets.get
             action: allow

# Trailing notes stay last.
default: deny
";

    #[test]
    fn appends_at_end_of_block_keeping_every_line() {
        let context = json!({ "host": "api.acme.com", "port": 443 });
        let draft =
            draft(ODD_LAYOUT, "main", "http.get", context, &VarBindings::new()).expect("drafts");
        assert_only_inserted(ODD_LAYOUT, &draft, 0);
        assert_eq!(draft.overrides, None);
        assert_eq!(
            rule_lines(&draft),
            [
                "         -   capability: http.get",
                "             filter: host == \"api.acme.com\" and port == 443",
                "             action: allow",
            ]
        );
        // Directly after the last rule of `main`, before `billing`'s comment.
        let before: Vec<&str> = draft.text.lines().take(draft.lines.start() - 1).collect();
        assert_eq!(before.last(), Some(&"             action: ask-human"));
        assert_eq!(draft.rule, rule_lines(&draft).join("\n"));
    }

    #[test]
    fn creates_a_missing_caller_block_at_the_end_of_permissions() {
        let draft = draft(
            ODD_LAYOUT,
            "@acme/reports",
            "fs.read",
            json!({ "path": "/data/a.csv" }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_only_inserted(ODD_LAYOUT, &draft, 1);
        let start = draft.lines.start() - 2;
        let created: Vec<&str> = draft.text.lines().skip(start).take(4).collect();
        assert_eq!(
            created,
            [
                "     '@acme/reports':",
                "         -   capability: fs.read",
                "             filter: path == \"/data/a.csv\"",
                "             action: allow",
            ]
        );
        let after: Vec<&str> = draft.text.lines().skip(start + 4).take(2).collect();
        assert_eq!(after, ["", "# Trailing notes stay last."]);
    }

    #[test]
    fn creates_permissions_when_absent_keeping_line_endings() {
        let original = "kind: blueprint\r\nname: bare";
        let draft =
            draft(original, "main", "fs.list", json!({}), &VarBindings::new()).expect("drafts");
        assert_eq!(
            draft.text,
            "kind: blueprint\r\nname: bare\r\npermissions:\r\n  main:\r\n    \
             - capability: fs.list\r\n      action: allow"
        );
        assert_eq!(draft.lines, 5..=6);
    }

    #[test]
    fn fills_an_empty_flow_block() {
        let original = "name: q\npermissions:\n  main:\n  - capability: a\n    action: allow\n  \
                        '@acme/billing': []  # nothing yet\n";
        let draft = draft(
            original,
            "@acme/billing",
            "secrets.get",
            json!({ "name": "KEY" }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_eq!(
            draft.text,
            "name: q\npermissions:\n  main:\n  - capability: a\n    action: allow\n  \
             '@acme/billing':  # nothing yet\n  - capability: secrets.get\n    \
             filter: name == \"KEY\"\n    action: allow\n"
        );
        assert_eq!(draft.lines, 7..=9);
    }

    #[test]
    fn a_bound_variable_value_is_written_as_the_variable() {
        let bindings = vars(&[("customerId", "cus_42"), ("region", "eu")]);
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "acme.com/charges.create",
            json!({ "customerId": "cus_42", "currency": "eur" }),
            &bindings,
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft)[1],
            "             filter: currency == \"eur\" and customerId == ${vars.customerId}"
        );
    }

    #[test]
    fn an_undeclared_variable_is_not_referenced() {
        let bindings = vars(&[("undeclared", "eur")]);
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "x.y",
            json!({ "currency": "eur" }),
            &bindings,
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft)[1],
            "             filter: currency == \"eur\""
        );
    }

    const DENIES_STAGING: &str = "\
name: deny
default: allow
permissions:
  main:
    - capability: fs.read
      filter: path == \"/public\"
      action: allow
    # Staging data stays put.
    - name: no-staging
      capability: http.get
      filter: host glob \"*.staging.acme.com\"
      action: deny
    - capability: http.get
      action: allow
";

    #[test]
    fn a_denied_call_is_drafted_above_the_deny_rule() {
        let context = json!({ "host": "api.staging.acme.com" });
        let draft = draft(
            DENIES_STAGING,
            "main",
            "http.get",
            context.clone(),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_only_inserted(DENIES_STAGING, &draft, 0);
        assert_eq!(
            draft.overrides,
            Some(RuleRef {
                caller: "main".into(),
                index: 1,
                name: Some("no-staging".into()),
            })
        );
        // Above the deny rule and the comment that belongs to it.
        let after: Vec<&str> = draft
            .text
            .lines()
            .skip(*draft.lines.end())
            .take(2)
            .collect();
        assert_eq!(
            after,
            ["    # Staging data stays put.", "    - name: no-staging"]
        );
        let drafted = crate::parse(&draft.text).expect("parses");
        assert_eq!(
            drafted.resolve_permission_with_rule("main", "http.get", &context, &VarBindings::new()),
            (Action::Allow, Some(1))
        );
        // Another staging host is still denied by the rule the draft overrides.
        assert_eq!(
            drafted.resolve_permission_with_rule(
                "main",
                "http.get",
                &json!({ "host": "db.staging.acme.com" }),
                &VarBindings::new()
            ),
            (Action::Deny, Some(2))
        );
    }

    #[test]
    fn default_decided_appends_after_the_last_rule() {
        let original = "name: d\npermissions:\n  main:\n    - capability: a\n      action: allow\n\
                        \n# Notes.\ndefault: deny\n";
        let draft = draft(
            original,
            "main",
            "b",
            json!({ "n": 1 }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_only_inserted(original, &draft, 0);
        assert_eq!(draft.lines, 6..=8);
        assert_eq!(draft.overrides, None);
    }

    #[test]
    fn a_hostile_value_matches_only_itself() {
        let hostile = "x\" || true";
        let context = json!({ "customerId": hostile });
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "acme.com/charges.create",
            context.clone(),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft)[1],
            r#"             filter: customerId == "x\" || true""#
        );
        let drafted = crate::parse(&draft.text).expect("parses");
        let filter = drafted.permissions["main"][2]
            .filter
            .as_ref()
            .expect("filter");
        assert!(filter.matches(&context));
        for other in ["x", "x\"", "", "y", "x\" || true ", "anything"] {
            assert!(!filter.matches(&json!({ "customerId": other })), "{other}");
        }
    }

    #[test]
    fn yaml_significant_values_are_quoted() {
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "search",
            json!({ "q": "a: b #c 'd'" }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft)[1],
            r#"             filter: 'q == "a: b #c ''d''"'"#
        );
    }

    #[test]
    fn a_newline_is_refused_naming_the_field() {
        let err = draft(
            ODD_LAYOUT,
            "main",
            "fs.write",
            json!({ "path": "/a\n  action: allow" }),
            &VarBindings::new(),
        )
        .expect_err("refused");
        assert!(
            matches!(&err, DraftError::UnsafeValue { field, character: '\n' } if field == "path")
        );
        assert!(err.to_string().contains("`path` holds a newline"), "{err}");
        let err = draft(
            ODD_LAYOUT,
            "main",
            "fs.write",
            json!({ "path": "a\u{1b}b" }),
            &VarBindings::new(),
        )
        .expect_err("refused");
        assert!(
            err.to_string().contains("`path` holds a control character"),
            "{err}"
        );
    }

    #[test]
    fn a_named_draft_writes_the_name_first_and_reads_back_with_it() {
        let context = json!({ "amount": 1250 });
        let draft = draft_named(
            ODD_LAYOUT,
            "main",
            "acme.com/refunds.create",
            context,
            &VarBindings::new(),
            Some("small-refunds"),
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft),
            [
                "         -   name: small-refunds",
                "             capability: acme.com/refunds.create",
                "             filter: amount == 1250",
                "             action: allow",
            ]
        );
        assert_only_inserted(ODD_LAYOUT, &draft, 0);
        let parsed = crate::parse(&draft.text).expect("parses");
        let rule = &parsed.permissions["main"][1];
        assert_eq!(rule.name.as_deref(), Some("small-refunds"));

        // A name YAML would read as another type is quoted, and reads back as text.
        let draft = draft_named(
            ODD_LAYOUT,
            "billing",
            "fs.read",
            json!({ "path": "/a" }),
            &VarBindings::new(),
            Some("true"),
        )
        .expect("drafts");
        assert_eq!(rule_lines(&draft)[0], "         -   name: 'true'");
        let parsed = crate::parse(&draft.text).expect("parses");
        assert_eq!(
            parsed.permissions["billing"][1].name.as_deref(),
            Some("true")
        );
    }

    #[test]
    fn a_rule_name_must_be_new_to_its_block_non_empty_and_printable() {
        let named = "name: a\npermissions:\n  main:\n  - name: reads\n    capability: a\n    action: allow\n  other:\n  - name: writes\n    capability: b\n    action: allow\n";
        let refused = |name: &str| {
            draft_named(
                named,
                "main",
                "b",
                json!({}),
                &VarBindings::new(),
                Some(name),
            )
            .expect_err("refused")
        };
        let taken = refused("reads");
        assert!(
            matches!(&taken, DraftError::DuplicateName { caller, name, position: 1 }
                if caller == "main" && name == "reads"),
            "{taken:?}"
        );
        assert!(
            taken
                .to_string()
                .contains("rule 1 of `main` is already named `reads`")
        );
        assert!(matches!(refused(""), DraftError::EmptyName));
        assert!(matches!(
            refused("a\nb"),
            DraftError::UnsafeName { character: '\n' }
        ));
        // The same name under another caller is unambiguous.
        let draft = draft_named(
            named,
            "main",
            "b",
            json!({}),
            &VarBindings::new(),
            Some("writes"),
        )
        .expect("drafts");
        assert!(
            draft.rule.starts_with("  - name: writes\n"),
            "{}",
            draft.rule
        );
    }

    #[test]
    fn a_placeholder_in_a_value_is_refused() {
        let err = draft(
            ODD_LAYOUT,
            "main",
            "x",
            json!({ "id": "${vars.customerId}" }),
            &VarBindings::new(),
        )
        .expect_err("refused");
        assert!(matches!(err, DraftError::UnquotableValue { field } if field == "id"));
    }

    #[test]
    fn an_unaddressable_field_is_refused_and_nested_values_are_skipped() {
        let err = draft(
            ODD_LAYOUT,
            "main",
            "x",
            json!({ "content-type": "a" }),
            &VarBindings::new(),
        )
        .expect_err("refused");
        assert!(matches!(err, DraftError::UnaddressableField { field } if field == "content-type"));
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "x",
            json!({ "id": 1, "tags": ["a"], "meta": {} }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_eq!(rule_lines(&draft)[1], "             filter: id == 1");
    }

    #[test]
    fn an_integer_a_filter_cannot_tell_from_its_neighbours_is_refused() {
        // Filter numbers are f64: past 2^53 - 1, neighbouring integers compare equal.
        for id in [
            json!(9_007_199_254_740_992_u64),
            json!(-9_007_199_254_740_993_i64),
        ] {
            let err = draft(
                ODD_LAYOUT,
                "main",
                "x",
                json!({ "id": id }),
                &VarBindings::new(),
            )
            .expect_err("refused");
            assert!(
                matches!(&err, DraftError::InexactNumber { field } if field == "id"),
                "{err:?}"
            );
        }
        for (id, written) in [
            (json!(9_007_199_254_740_991_u64), "9007199254740991"),
            (json!(-9_007_199_254_740_991_i64), "-9007199254740991"),
            (json!(0.1), "0.1"),
        ] {
            let draft = draft(
                ODD_LAYOUT,
                "main",
                "x",
                json!({ "id": id }),
                &VarBindings::new(),
            )
            .expect("drafts");
            assert_eq!(
                rule_lines(&draft)[1],
                format!("             filter: id == {written}")
            );
        }
    }

    #[test]
    fn a_float_too_large_to_tell_from_neighbouring_integers_is_refused() {
        for id in [json!(1e16), json!(9_007_199_254_740_992.0), json!(-1e300)] {
            let err = draft(
                ODD_LAYOUT,
                "main",
                "x",
                json!({ "id": id }),
                &VarBindings::new(),
            )
            .expect_err("refused");
            assert!(
                matches!(&err, DraftError::InexactNumber { field } if field == "id"),
                "{err:?}"
            );
        }
        let draft = draft(
            ODD_LAYOUT,
            "main",
            "x",
            json!({ "id": 9_007_199_254_740_991.0 }),
            &VarBindings::new(),
        )
        .expect("drafts");
        assert_eq!(
            rule_lines(&draft)[1],
            "             filter: id == 9007199254740991.0"
        );
    }

    #[test]
    fn a_line_break_yaml_reads_but_this_scan_does_not_is_refused() {
        for brk in ["\r", "\u{85}", "\u{2028}", "\u{2029}"] {
            let text = format!(
                "name: a\npermissions:\n  main:\n    # note{brk}    - capability: hidden\n      \
                 action: allow\n    - capability: b\n      action: deny\n"
            );
            let err =
                draft(&text, "main", "c", json!({}), &VarBindings::new()).expect_err("refused");
            assert_eq!(
                err.to_string(),
                "line 4: a line ends with a break other than a line feed; add the rule by hand \
                 or rewrite the block in plain block style",
                "{brk:?}"
            );
        }
    }

    #[test]
    fn anchors_and_flow_rules_are_refused() {
        let anchored = "name: a\npermissions:\n  main: &rules\n    - capability: a\n      \
                        action: deny\n  other: *rules\n";
        let err =
            draft(anchored, "main", "b", json!({}), &VarBindings::new()).expect_err("refused");
        assert!(
            matches!(err, DraftError::UnsupportedLayout { line: 3, .. }),
            "{err:?}"
        );
        let flow = "name: a\npermissions:\n  main:\n    - { capability: a, action: deny }\n";
        let err = draft(flow, "main", "b", json!({}), &VarBindings::new()).expect_err("refused");
        assert!(
            matches!(err, DraftError::UnsupportedLayout { line: 4, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn a_refused_layout_in_another_callers_block_names_that_block() {
        let text = "name: a\npermissions:\n  main:\n    - capability: a\n      action: deny\n  \
                    other:\n    - { capability: a, action: deny }\n";
        let err = draft(text, "main", "b", json!({}), &VarBindings::new()).expect_err("refused");
        assert_eq!(
            err.to_string(),
            "line 7, in the `other` block: a flow collection is used; add the rule by hand or \
             rewrite the block in plain block style"
        );
        let header = "name: a\npermissions:\n  other: &rules\n    - capability: a\n      \
                      action: deny\n";
        let err = draft(header, "main", "b", json!({}), &VarBindings::new()).expect_err("refused");
        assert!(
            err.to_string()
                .starts_with("line 3, in the `other` block: a caller's rules"),
            "{err}"
        );
        let unreadable = "name: a\npermissions:\n  \"o\\u00e9\": []\n";
        let err =
            draft(unreadable, "main", "b", json!({}), &VarBindings::new()).expect_err("refused");
        assert!(
            err.to_string()
                .starts_with("line 3: a caller key is not a plain or quoted key"),
            "{err}"
        );
    }

    #[test]
    fn a_stale_or_allowed_decision_is_refused() {
        let context = json!({});
        // The default decided; a rule decides now.
        let err = draft_under(
            ODD_LAYOUT,
            "name: s\n",
            "acme.com/refunds.create",
            context.clone(),
        )
        .expect_err("refused");
        assert!(matches!(err, DraftError::DecisionChanged { .. }), "{err:?}");
        let err = draft(
            ODD_LAYOUT,
            "billing",
            "secrets.get",
            context,
            &VarBindings::new(),
        )
        .expect_err("refused");
        assert!(matches!(err, DraftError::AlreadyAllowed), "{err:?}");
    }

    #[test]
    fn the_same_rule_or_the_default_still_deciding_is_not_a_change() {
        const DENY: &str =
            "name: d\npermissions:\n  main:\n    - capability: b\n      action: deny\n";
        // The default decided before the caller had a block; no rule in the new block matches.
        let with_block =
            "name: d\npermissions:\n  main:\n    - capability: a\n      action: allow\n";
        let drafted = draft_under(with_block, "name: d\n", "b", json!({ "n": 2 })).expect("drafts");
        assert_eq!(drafted.overrides, None);
        // The deny rule that decided has moved down below a drafted allow.
        let moved = "name: d\npermissions:\n  main:\n    - capability: b\n      filter: n == 1\n      \
                     action: allow\n    - capability: b\n      action: deny\n";
        let drafted = draft_under(moved, DENY, "b", json!({ "n": 2 })).expect("drafts");
        assert_eq!(
            drafted.overrides,
            Some(RuleRef {
                caller: "main".into(),
                index: 1,
                name: None,
            })
        );
        // The rule in the deciding rule's place now says something else.
        let rewritten = "name: d\npermissions:\n  main:\n    - capability: b\n      filter: n != 3\n      \
                         action: deny\n";
        let err = draft_under(rewritten, DENY, "b", json!({ "n": 2 })).expect_err("refused");
        assert!(matches!(err, DraftError::DecisionChanged { .. }), "{err:?}");
        // A rule decided; the default decides now.
        let err = draft_under("name: d\n", DENY, "b", json!({ "n": 2 })).expect_err("refused");
        assert!(matches!(err, DraftError::DecisionChanged { .. }), "{err:?}");
    }

    fn check_splice(splice: &Splice) -> Result<String, DraftError> {
        let current = crate::parse(DENIES_STAGING).expect("parses");
        let lines = split_lines(DENIES_STAGING).expect("lines");
        let context = json!({});
        let call = DraftCall {
            caller: "main",
            capability: "b",
            context: &context,
            vars: &VarBindings::new(),
            name: None,
            decided_under: &current,
        };
        apply_checked(&current, &lines, splice, &call, 3, &[])
    }

    fn rule(dash: usize) -> Vec<String> {
        RuleText {
            name: None,
            capability: "b".into(),
            filter: None,
        }
        .lines(DashStyle {
            indent: dash,
            gap: 1,
        })
    }

    #[test]
    fn a_splice_that_does_not_parse_is_refused() {
        // A rule placed between top-level keys is not part of any block.
        let err = check_splice(&Splice {
            at: 1,
            inserted: rule(0),
            rule_start: 0,
            header: None,
        })
        .expect_err("refused");
        assert!(matches!(err, DraftError::DraftInvalid(_)), "{err:?}");
        // Over-indented under the last rule, the lines no longer form a rule.
        let err = check_splice(&Splice {
            at: 14,
            inserted: rule(8),
            rule_start: 0,
            header: None,
        })
        .expect_err("refused");
        assert!(matches!(err, DraftError::DraftInvalid(_)), "{err:?}");
    }

    #[test]
    fn a_splice_that_changes_existing_text_is_refused() {
        let placed = Splice {
            at: 14,
            inserted: rule(4),
            rule_start: 0,
            header: None,
        };
        check_splice(&placed).expect("a correct splice passes");
        let err = check_splice(&Splice {
            header: Some((0, "name: renamed".into())),
            ..placed
        })
        .expect_err("refused");
        assert!(matches!(err, DraftError::ExistingChanged), "{err:?}");
        // Placed above an existing rule, the drafted rule is not at its index.
        let err = check_splice(&Splice {
            at: 4,
            inserted: rule(4),
            rule_start: 0,
            header: None,
        })
        .expect_err("refused");
        assert!(matches!(err, DraftError::ExistingChanged), "{err:?}");
    }

    // -----------------------------------------------------------------------
    // Corpus: every blueprint in the repository's examples, package policy
    // tests, and server fixtures.

    fn corpus() -> Vec<(PathBuf, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        collect_yaml(&root.join("examples"), &mut files);
        collect_yaml(
            &root.join("crates/submilli-server/tests/fixtures"),
            &mut files,
        );
        let packages = std::fs::read_dir(root.join("packages")).expect("packages dir");
        for package in packages {
            let policy = package.expect("entry").path().join("tests/policy");
            collect_yaml(&policy, &mut files);
        }
        files.sort();
        files
            .into_iter()
            .filter_map(|path| {
                let text = std::fs::read_to_string(&path).expect("readable");
                crate::parse(&text).is_ok().then_some((path, text))
            })
            .collect()
    }

    fn collect_yaml(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                collect_yaml(&path, out);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
            {
                out.push(path);
            }
        }
    }

    /// `original` with the caller's `key: []` header written `key:`, the one
    /// existing line a draft may rewrite.
    fn with_block_header(original: &str, caller: &str) -> String {
        let key = yaml_scalar(caller);
        original
            .split_inclusive('\n')
            .map(|line| {
                let body = line.trim_start_matches(' ');
                if body.starts_with(&format!("{key}: []")) {
                    without_empty_flow(line)
                } else {
                    line.to_string()
                }
            })
            .collect()
    }

    #[test]
    fn corpus_drafts_keep_every_existing_line() {
        let corpus = corpus();
        assert!(corpus.len() >= 20, "found only {} blueprints", corpus.len());
        let bindings = VarBindings::new();
        let mut drafted = 0;
        for (path, text) in &corpus {
            let blueprint = crate::parse(text).expect("parses");
            let mut callers: Vec<&str> = blueprint.permissions.keys().map(String::as_str).collect();
            callers.push("@corpus/missing");
            let mut calls = vec![(
                "draft.test/synthetic",
                json!({ "id": "abc", "n": 3, "ok": true }),
            )];
            for rules in blueprint.permissions.values() {
                for rule in rules {
                    calls.push((rule.capability.as_str(), json!({ "id": "x\" or true" })));
                }
            }
            for caller in &callers {
                for (capability, context) in &calls {
                    let resolution =
                        blueprint.explain_permission(caller, capability, context, &bindings);
                    if resolution.action == Action::Allow {
                        continue;
                    }
                    let draft = draft(text, caller, capability, context.clone(), &bindings)
                        .unwrap_or_else(|err| {
                            panic!("{}: {caller} {capability}: {err}", path.display())
                        });
                    let created = match (
                        blueprint.permissions.contains_key(*caller),
                        blueprint.permissions.is_empty() && !text.contains("\npermissions:"),
                    ) {
                        (true, _) => 0,
                        (false, false) => 1,
                        (false, true) => 2,
                    };
                    assert_only_inserted(&with_block_header(text, caller), &draft, created);
                    let reparsed = crate::parse(&draft.text);
                    assert!(reparsed.is_ok(), "{}: {reparsed:?}", path.display());
                    drafted += 1;
                }
            }
        }
        assert!(drafted > corpus.len(), "drafted only {drafted}");
    }
}
