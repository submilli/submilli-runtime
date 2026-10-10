//! The policy filter mini-language: a small boolean expression evaluated
//! host-side against the JSON context object passed to `check()`.
//!
//! The grammar is `or` over `and` over `not` over comparisons and parenthesized
//! groups; a bare comparison (`amount < 500`) is still a valid expression.
//! Leaves compare a dotted field path to a literal — numeric `< <= == != >= >`,
//! string/bool/null `==`/`!=`, `glob` (shell wildcards), `matches` (regex), or
//! `contains` (array membership).
//!
//! Expressions are parsed into an AST at registration time (via [`parse`], which
//! the [`FilterExpr`] serde impl calls), so a malformed filter fails when the
//! blueprint is added rather than at the first `check()`. Evaluation never errors:
//! a missing field or a wrong-kind value is a non-match.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Caller-supplied variable bindings (`${vars.NAME}` → value), resolved per
/// session and threaded into filter evaluation. Variables are strings; a
/// comparison coerces by the *other* operand's kind (see [`Comparison::eval`]).
pub type VarBindings = BTreeMap<String, String>;

/// Why one comparison did not hold, as reported by
/// [`FilterExpr::explain_with`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "variable")]
pub enum FailureReason {
    /// The context has no value at the comparison's field path.
    FieldMissing,
    /// The operand references a `${vars.NAME}` the session did not bind.
    VariableNotBound(String),
    /// The field and operand were both available and the comparison is false
    /// (or the value's JSON kind does not suit the operand).
    NotSatisfied,
}

/// One leaf comparison that kept a filter from matching.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComparisonFailure {
    /// The comparison as written, from its `Display` impl.
    pub comparison: String,
    /// The value the call's context holds at the field path, if any.
    pub actual: Option<serde_json::Value>,
    /// The operand with variables resolved: the bound variable's value, the
    /// interpolated string, or the literal. `None` when a variable is unbound.
    pub expected: Option<String>,
    pub reason: FailureReason,
    /// The comparison is under a `not` and held, which is what failed the
    /// filter.
    pub negated: bool,
}

/// The result of evaluating a filter for reporting: the same `matched` answer
/// as [`FilterExpr::matches_with`], plus the leaves behind a non-match.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FilterEvaluation {
    pub matched: bool,
    /// Empty when `matched`. Otherwise every leaf whose outcome ran against
    /// what the filter needed, in source order.
    pub failures: Vec<ComparisonFailure>,
}

/// A parsed filter expression. The grammar is `or` over `and` over `not` over
/// comparisons and parenthesized groups; a bare comparison is a valid expression.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterExpr(Expr);

// Only the bounded parser constructs trees. Private nodes keep the bound valid
// for evaluation, formatting, derived clone/equality, and recursive destruction.
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Compare(Comparison),
    Not(Box<FilterExpr>),
    And(Box<FilterExpr>, Box<FilterExpr>),
    Or(Box<FilterExpr>, Box<FilterExpr>),
}

/// A leaf predicate: a dotted field path, a comparison operator, and an operand.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    path: Vec<String>,
    op: CompareOp,
    operand: Operand,
}

/// A positive string match a filter places on a field, surfaced for
/// human-readable policy summaries (e.g. listing the hosts an `http.*` rule
/// permits). Negations and numeric comparisons are not represented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldMatch {
    /// `field == "value"`.
    Equals(String),
    /// `field glob "pattern"` — shell wildcards.
    Glob(String),
    /// `field matches "regex"`.
    Regex(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompareOp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
    Matches,
    Glob,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    Regex(RegexOperand),
    /// A bare `${vars.NAME}` reference, resolved against the session bindings at
    /// eval time and coerced by the field value's JSON kind.
    Var(String),
    /// A quoted string with one or more embedded `${vars.NAME}` placeholders,
    /// rendered against the session bindings at eval time. Used by `glob`, `==`,
    /// `!=`, and `contains`; the rendering escapes per operator (glob
    /// metacharacters for `glob`, verbatim otherwise) so a variable value can't
    /// widen the pattern.
    Interp(Vec<StrSegment>),
}

/// One piece of an interpolated string operand: literal text or a variable
/// reference. The literal is stored already-unescaped (post-lexer).
#[derive(Debug, Clone, PartialEq)]
enum StrSegment {
    Lit(String),
    Var(String),
}

/// A compiled `matches` regex paired with its source text. Compiled once at
/// parse time; compared and serialized on the source so `FilterExpr` can stay
/// `PartialEq` even though `regex::Regex` is not.
#[derive(Debug, Clone)]
struct RegexOperand {
    source: String,
    compiled: regex::Regex,
}

impl PartialEq for RegexOperand {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl CompareOp {
    fn as_str(self) -> &'static str {
        match self {
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
            CompareOp::Ge => ">=",
            CompareOp::Gt => ">",
            CompareOp::Matches => "matches",
            CompareOp::Glob => "glob",
            CompareOp::Contains => "contains",
        }
    }
}

impl FilterExpr {
    /// True when `ctx` satisfies the expression, with no variable bindings — a
    /// `${vars.NAME}` operand is therefore an (absent-variable) non-match.
    /// Convenience wrapper over [`Self::matches_with`].
    pub fn matches(&self, ctx: &serde_json::Value) -> bool {
        self.matches_with(ctx, &VarBindings::new())
    }

    /// True when `ctx` satisfies the expression, resolving `${vars.NAME}`
    /// operands against `vars`. Boolean operators short-circuit; a leaf whose
    /// field is missing — or whose value is the wrong JSON kind for the operand,
    /// or whose variable is absent/uncoercible — is a non-match, never a hard
    /// error at evaluation time.
    pub fn matches_with(&self, ctx: &serde_json::Value, vars: &VarBindings) -> bool {
        match &self.0 {
            Expr::Compare(c) => c.eval(ctx, vars),
            Expr::Not(inner) => !inner.matches_with(ctx, vars),
            Expr::And(left, right) => left.matches_with(ctx, vars) && right.matches_with(ctx, vars),
            Expr::Or(left, right) => left.matches_with(ctx, vars) || right.matches_with(ctx, vars),
        }
    }

    /// Evaluate for reporting. Walks every branch (no short-circuiting) so each
    /// failing leaf is found, and returns the same `matched` result as
    /// [`Self::matches_with`], which decides enforcement.
    pub fn explain_with(&self, ctx: &serde_json::Value, vars: &VarBindings) -> FilterEvaluation {
        let mut failures = Vec::new();
        let matched = self.evaluate_all(ctx, vars, true, &mut failures);
        if matched {
            failures.clear();
        }
        FilterEvaluation { matched, failures }
    }

    /// `wanted` is the outcome this subtree needs for the whole filter to
    /// match; it flips under `not`. A leaf whose outcome differs is recorded.
    fn evaluate_all(
        &self,
        ctx: &serde_json::Value,
        vars: &VarBindings,
        wanted: bool,
        failures: &mut Vec<ComparisonFailure>,
    ) -> bool {
        match &self.0 {
            Expr::Compare(c) => {
                let holds = c.eval(ctx, vars);
                if holds != wanted {
                    failures.push(c.failure(ctx, vars, !wanted));
                }
                holds
            }
            Expr::Not(inner) => !inner.evaluate_all(ctx, vars, !wanted, failures),
            Expr::And(left, right) => {
                let left_holds = left.evaluate_all(ctx, vars, wanted, failures);
                let right_holds = right.evaluate_all(ctx, vars, wanted, failures);
                left_holds && right_holds
            }
            Expr::Or(left, right) => {
                let left_holds = left.evaluate_all(ctx, vars, wanted, failures);
                let right_holds = right.evaluate_all(ctx, vars, wanted, failures);
                left_holds || right_holds
            }
        }
    }

    /// Best-effort: every string value this expression positively compares
    /// `field` to (via `==`, `glob`, or `matches`), in source order. Boolean
    /// structure is ignored — an `or` widens and a `not` inverts, neither of
    /// which this reflects — so the result is a summary of what the filter
    /// *mentions*, not an exact reachability set. An empty result means the
    /// filter never positively constrains `field`. For tooling, not enforcement.
    pub fn field_matches(&self, field: &str) -> Vec<FieldMatch> {
        let mut out = Vec::new();
        self.collect_field_matches(field, &mut out);
        out
    }

    fn collect_field_matches(&self, field: &str, out: &mut Vec<FieldMatch>) {
        match &self.0 {
            Expr::Compare(c) => c.collect_field_match(field, out),
            Expr::Not(inner) => inner.collect_field_matches(field, out),
            Expr::And(left, right) | Expr::Or(left, right) => {
                left.collect_field_matches(field, out);
                right.collect_field_matches(field, out);
            }
        }
    }

    /// Every `${vars.NAME}` this expression references, in source order
    /// (duplicates included). Used at parse time to reject a filter that names
    /// an undeclared variable.
    pub fn var_refs(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_var_refs(&mut out);
        out
    }

    /// The context field each comparison tests, in source order (duplicates
    /// included): the first segment of its path, so `order.total` yields
    /// `order`. Boolean structure is ignored, as in [`Self::field_matches`].
    pub fn top_level_fields(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_top_level_fields(&mut out);
        out
    }

    fn collect_top_level_fields<'a>(&'a self, out: &mut Vec<&'a str>) {
        match &self.0 {
            Expr::Compare(c) => out.extend(c.path.first().map(String::as_str)),
            Expr::Not(inner) => inner.collect_top_level_fields(out),
            Expr::And(left, right) | Expr::Or(left, right) => {
                left.collect_top_level_fields(out);
                right.collect_top_level_fields(out);
            }
        }
    }

    fn collect_var_refs<'a>(&'a self, out: &mut Vec<&'a str>) {
        match &self.0 {
            Expr::Compare(c) => match &c.operand {
                Operand::Var(name) => out.push(name),
                Operand::Interp(segs) => {
                    for seg in segs {
                        if let StrSegment::Var(name) = seg {
                            out.push(name);
                        }
                    }
                }
                _ => {}
            },
            Expr::Not(inner) => inner.collect_var_refs(out),
            Expr::And(left, right) | Expr::Or(left, right) => {
                left.collect_var_refs(out);
                right.collect_var_refs(out);
            }
        }
    }

    /// The top-level `and` operands, flattened: `a and (b and c)` yields `a`, `b`,
    /// `c`. A filter that is not an `and` is its own single conjunct.
    #[doc(hidden)]
    pub fn conjuncts(&self) -> Vec<&FilterExpr> {
        let mut out = Vec::new();
        self.collect_flat(&mut out, |expr| match expr {
            Expr::And(left, right) => Some((left, right)),
            _ => None,
        });
        out
    }

    /// The top-level `or` operands, flattened like [`Self::conjuncts`].
    #[doc(hidden)]
    pub fn disjuncts(&self) -> Vec<&FilterExpr> {
        let mut out = Vec::new();
        self.collect_flat(&mut out, |expr| match expr {
            Expr::Or(left, right) => Some((left, right)),
            _ => None,
        });
        out
    }

    fn collect_flat<'a>(
        &'a self,
        out: &mut Vec<&'a FilterExpr>,
        split: fn(&'a Expr) -> Option<(&'a FilterExpr, &'a FilterExpr)>,
    ) {
        match split(&self.0) {
            Some((left, right)) => {
                left.collect_flat(out, split);
                right.collect_flat(out, split);
            }
            None => out.push(self),
        }
    }

    /// `(field path, variable)` when this expression is exactly
    /// `field == ${vars.NAME}`: the shape that pins a rule to a session variable.
    #[doc(hidden)]
    pub fn as_pin(&self) -> Option<(String, &str)> {
        match &self.0 {
            Expr::Compare(Comparison {
                path,
                op: CompareOp::Eq,
                operand: Operand::Var(name),
            }) => Some((path.join("."), name.as_str())),
            _ => None,
        }
    }

    /// Binding tightness, used only to parenthesize `Display` minimally.
    fn precedence(&self) -> u8 {
        match &self.0 {
            Expr::Or(..) => 1,
            Expr::And(..) => 2,
            Expr::Not(_) => 3,
            Expr::Compare(_) => 4,
        }
    }
}

impl Comparison {
    fn collect_field_match(&self, field: &str, out: &mut Vec<FieldMatch>) {
        if self.path.len() != 1 || self.path[0] != field {
            return;
        }
        match (self.op, &self.operand) {
            (CompareOp::Eq, Operand::Str(s)) => out.push(FieldMatch::Equals(s.clone())),
            (CompareOp::Glob, Operand::Str(s)) => out.push(FieldMatch::Glob(s.clone())),
            (CompareOp::Matches, Operand::Regex(re)) => {
                out.push(FieldMatch::Regex(re.source.clone()));
            }
            _ => {}
        }
    }

    fn eval(&self, ctx: &serde_json::Value, vars: &VarBindings) -> bool {
        let Some(value) = resolve_path(ctx, &self.path) else {
            return false;
        };
        match self.op {
            CompareOp::Lt => self.eval_numeric(value, vars, |x, n| x < n),
            CompareOp::Le => self.eval_numeric(value, vars, |x, n| x <= n),
            CompareOp::Ge => self.eval_numeric(value, vars, |x, n| x >= n),
            CompareOp::Gt => self.eval_numeric(value, vars, |x, n| x > n),
            CompareOp::Eq => scalar_eq(value, &self.operand, vars).unwrap_or(false),
            CompareOp::Ne => match scalar_eq(value, &self.operand, vars) {
                Some(equal) => !equal,
                None => false,
            },
            CompareOp::Matches => {
                let (Operand::Regex(re), Some(x)) = (&self.operand, value.as_str()) else {
                    return false;
                };
                re.compiled.is_match(x)
            }
            CompareOp::Glob => {
                let Some(x) = value.as_str() else {
                    return false;
                };
                match &self.operand {
                    Operand::Str(pattern) => glob_match(pattern, x),
                    // Escape glob metacharacters in each variable value so a value
                    // like `*` matches literally and can't widen the pattern.
                    Operand::Interp(segs) => match render_segments(segs, vars, glob_escape) {
                        Some(pattern) => glob_match(&pattern, x),
                        None => false,
                    },
                    _ => false,
                }
            }
            CompareOp::Contains => {
                let Some(items) = value.as_array() else {
                    return false;
                };
                items
                    .iter()
                    .any(|el| scalar_eq(el, &self.operand, vars).unwrap_or(false))
            }
        }
    }

    fn failure(
        &self,
        ctx: &serde_json::Value,
        vars: &VarBindings,
        negated: bool,
    ) -> ComparisonFailure {
        let actual = resolve_path(ctx, &self.path).cloned();
        let reason = match (self.unbound_variable(vars), &actual) {
            (Some(name), _) => FailureReason::VariableNotBound(name.to_string()),
            (None, None) => FailureReason::FieldMissing,
            (None, Some(_)) => FailureReason::NotSatisfied,
        };
        ComparisonFailure {
            comparison: self.to_string(),
            actual,
            expected: self.resolved_operand(vars),
            reason,
            negated,
        }
    }

    /// The first `${vars.NAME}` the operand references that `vars` lacks.
    fn unbound_variable<'a>(&'a self, vars: &VarBindings) -> Option<&'a str> {
        match &self.operand {
            Operand::Var(name) if !vars.contains_key(name) => Some(name),
            Operand::Interp(segs) => segs.iter().find_map(|seg| match seg {
                StrSegment::Var(name) if !vars.contains_key(name) => Some(name.as_str()),
                _ => None,
            }),
            _ => None,
        }
    }

    fn resolved_operand(&self, vars: &VarBindings) -> Option<String> {
        match &self.operand {
            Operand::Number(n) => Some(format_number(*n)),
            Operand::Str(s) => Some(s.clone()),
            Operand::Bool(b) => Some(b.to_string()),
            Operand::Null => Some("null".to_string()),
            Operand::Regex(re) => Some(re.source.clone()),
            Operand::Var(name) => vars.get(name).cloned(),
            Operand::Interp(segs) => render_segments(segs, vars, str::to_string),
        }
    }

    fn eval_numeric(
        &self,
        value: &serde_json::Value,
        vars: &VarBindings,
        compare: impl FnOnce(f64, f64) -> bool,
    ) -> bool {
        let (Some(n), Some(x)) = (operand_as_f64(&self.operand, vars), value.as_f64()) else {
            return false;
        };
        compare(x, n)
    }
}

/// The numeric value of an operand for an ordering comparison: a literal number
/// as-is, or a `${vars.NAME}` parsed to f64. Anything else (or an absent /
/// unparseable variable) yields `None` — a non-match.
fn operand_as_f64(operand: &Operand, vars: &VarBindings) -> Option<f64> {
    match operand {
        Operand::Number(n) => Some(*n),
        Operand::Var(name) => vars.get(name)?.parse::<f64>().ok(),
        _ => None,
    }
}

/// Walk a dotted path into the context. Each segment indexes an object by key or
/// an array by its parsed index; a missing key, an out-of-range index, or a
/// scalar encountered mid-path yields `None` — a non-match, never an error.
fn resolve_path<'a>(ctx: &'a serde_json::Value, path: &[String]) -> Option<&'a serde_json::Value> {
    let mut current = ctx;
    for segment in path {
        current = match current {
            serde_json::Value::Object(map) => map.get(segment)?,
            serde_json::Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// Compare a JSON value to a scalar operand. `None` means the value's JSON kind
/// doesn't match the operand's, so a wrong-kind leaf is a non-match for both `==`
/// and `!=`. A `null` operand matches against any value via `is_null`.
///
/// A `${vars.NAME}` operand coerces by the *value's* JSON kind: a bool field
/// parses the variable as a bool, a number field as f64, a string field compares
/// the text. An absent variable, or one that fails to coerce, is `None` — a
/// non-match, never a trap.
fn scalar_eq(value: &serde_json::Value, operand: &Operand, vars: &VarBindings) -> Option<bool> {
    match operand {
        Operand::Number(n) => value.as_f64().map(|x| x == *n),
        Operand::Str(s) => value.as_str().map(|x| x == s),
        Operand::Bool(b) => value.as_bool().map(|x| x == *b),
        Operand::Null => Some(value.is_null()),
        Operand::Regex(_) => None,
        Operand::Var(name) => {
            let var = vars.get(name)?;
            match value {
                serde_json::Value::Bool(b) => var.parse::<bool>().ok().map(|x| x == *b),
                serde_json::Value::Number(_) => value
                    .as_f64()
                    .and_then(|x| var.parse::<f64>().ok().map(|v| v == x)),
                serde_json::Value::String(s) => Some(var == s),
                _ => None,
            }
        }
        // An interpolated string compares verbatim against a string value (no
        // escaping — there's no pattern to widen in an equality test).
        Operand::Interp(segs) => {
            let rendered = render_segments(segs, vars, str::to_string)?;
            value.as_str().map(|x| x == rendered)
        }
    }
}

/// Render an interpolated operand against the bindings, applying `escape` to each
/// variable's value (identity for an exact compare, glob-escaping for `glob`).
/// `None` if any referenced variable is absent — a non-match, never a trap.
fn render_segments(
    segs: &[StrSegment],
    vars: &VarBindings,
    escape: fn(&str) -> String,
) -> Option<String> {
    let mut out = String::new();
    for seg in segs {
        match seg {
            StrSegment::Lit(s) => out.push_str(s),
            StrSegment::Var(name) => out.push_str(&escape(vars.get(name)?)),
        }
    }
    Some(out)
}

/// Backslash-escape the glob metacharacters (`\ * ?`) so an interpolated value is
/// matched as a literal by [`glob_match`].
fn glob_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | '*' | '?') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl fmt::Display for FilterExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Expr::Compare(c) => write!(f, "{c}"),
            Expr::Not(inner) => {
                write!(f, "not ")?;
                write_child(f, inner, 3, false)
            }
            Expr::And(left, right) => {
                write_child(f, left, 2, false)?;
                write!(f, " and ")?;
                write_child(f, right, 2, true)
            }
            Expr::Or(left, right) => {
                write_child(f, left, 1, false)?;
                write!(f, " or ")?;
                write_child(f, right, 1, true)
            }
        }
    }
}

/// Write a child with the minimum parentheses that round-trip: a child binding
/// looser than its parent needs them, as does an equal-precedence right operand
/// (the grammar is left-associative).
fn write_child(
    f: &mut fmt::Formatter<'_>,
    child: &FilterExpr,
    parent_prec: u8,
    is_right: bool,
) -> fmt::Result {
    let child_prec = child.precedence();
    if child_prec < parent_prec || (is_right && child_prec == parent_prec) {
        write!(f, "({child})")
    } else {
        write!(f, "{child}")
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} ", self.path.join("."), self.op.as_str())?;
        match &self.operand {
            Operand::Number(n) => write!(f, "{}", format_number(*n)),
            Operand::Str(s) => write!(f, "\"{}\"", escape_str(s)),
            Operand::Bool(b) => write!(f, "{b}"),
            Operand::Null => write!(f, "null"),
            Operand::Regex(re) => write!(f, "\"{}\"", escape_str(&re.source)),
            Operand::Var(name) => write!(f, "${{vars.{name}}}"),
            Operand::Interp(segs) => {
                write!(f, "\"")?;
                for seg in segs {
                    match seg {
                        StrSegment::Lit(s) => write!(f, "{}", escape_str(s))?,
                        StrSegment::Var(name) => write!(f, "${{vars.{name}}}")?,
                    }
                }
                write!(f, "\"")
            }
        }
    }
}

/// Render a filter number without a trailing `.0` for integer-valued operands,
/// so `amount < 500` round-trips as written rather than `amount < 500.0`.
fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Whole-string glob match supporting `*` (any run, including empty), `?`
/// (exactly one char), and `\` escaping (`\*`, `\?`, `\\` match those characters
/// literally). Case-sensitive; classic two-pointer with backtracking on the last
/// `*`. (`matches` is the unanchored regex counterpart.)
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Resume point of the last `*`: (pattern index just past it, text index it
    // was anchored at).
    let mut star: Option<(usize, usize)> = None;
    while ti < txt.len() {
        match glob_token(&pat, pi) {
            Some((GlobToken::Star, width)) => {
                star = Some((pi + width, ti));
                pi += width;
            }
            Some((tok, width)) if tok.matches(txt[ti]) => {
                pi += width;
                ti += 1;
            }
            // Mismatch (or pattern exhausted): backtrack to the last `*`, letting
            // it absorb one more text char.
            _ => match star {
                Some((resume_pi, resume_ti)) => {
                    pi = resume_pi;
                    ti = resume_ti + 1;
                    star = Some((resume_pi, resume_ti + 1));
                }
                None => return false,
            },
        }
    }
    while let Some((GlobToken::Star, width)) = glob_token(&pat, pi) {
        pi += width;
    }
    pi == pat.len()
}

enum GlobToken {
    Star,
    AnyOne,
    Lit(char),
}

impl GlobToken {
    fn matches(&self, c: char) -> bool {
        match self {
            GlobToken::Star => true,
            GlobToken::AnyOne => true,
            GlobToken::Lit(p) => *p == c,
        }
    }
}

/// The pattern token at `pi` and how many chars it spans. `\X` is the literal
/// `X` (width 2); a trailing lone `\` is a literal backslash. `None` past the end.
fn glob_token(pat: &[char], pi: usize) -> Option<(GlobToken, usize)> {
    match pat.get(pi)? {
        '\\' => match pat.get(pi + 1) {
            Some(&escaped) => Some((GlobToken::Lit(escaped), 2)),
            None => Some((GlobToken::Lit('\\'), 1)),
        },
        '*' => Some((GlobToken::Star, 1)),
        '?' => Some((GlobToken::AnyOne, 1)),
        &c => Some((GlobToken::Lit(c), 1)),
    }
}

/// `value` as a quoted string literal that parses back to exactly `value`, or
/// `None` when no literal can: the tokenizer reads any `${vars.` inside quotes
/// as a variable placeholder and has no escape for it.
#[doc(hidden)]
pub fn quote_literal(value: &str) -> Option<String> {
    (!value.contains(VAR_PLACEHOLDER)).then(|| format!("\"{}\"", escape_str(value)))
}

/// True when `name` is written as a bare comparison field: one path segment
/// the tokenizer reads as a field rather than a keyword or literal.
#[doc(hidden)]
pub fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && matches!(
            classify_word(name, (0, name.len())),
            Ok(Token::Path(segments)) if segments.len() == 1
        )
}

/// Re-escape a string operand for the canonical quoted form, the inverse of the
/// tokenizer's string handling.
fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

type Span = (usize, usize);

/// A filter parse failure carrying the offending byte span, rendered with a
/// caret so an LLM can fix the expression from the message alone.
#[derive(Debug)]
struct FilterParseError {
    message: String,
    span: Span,
}

impl FilterParseError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    fn render(&self, src: &str) -> String {
        let start = self.span.0.min(src.len());
        let end = self.span.1.clamp(start, src.len());
        let column = src[..start].chars().count();
        let width = src[start..end].chars().count().max(1);
        format!(
            "invalid filter `{src}`: {message}\n  {src}\n  {pad}{caret}",
            message = self.message,
            pad = " ".repeat(column),
            caret = "^".repeat(width),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    And,
    Or,
    Not,
    Op(CompareOp),
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Path(Vec<String>),
    /// A bare `${vars.NAME}` reference; the inner string is `NAME`.
    Var(String),
    /// A quoted string carrying one or more `${vars.NAME}` placeholders.
    Interp(Vec<StrSegment>),
}

/// True for a syntactically valid `${vars.NAME}` variable name.
#[doc(hidden)]
pub fn is_valid_var_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What opens a variable placeholder inside a quoted string.
const VAR_PLACEHOLDER: &str = "${vars.";

/// Parse a (post-escape) string literal into a token: a plain [`Token::Str`] when
/// it has no `${vars.NAME}` placeholder, else a [`Token::Interp`] of literal /
/// variable segments. A malformed `${vars.…}` (bad/empty name, unterminated) is
/// an error; any other `$`/`{` stays literal.
fn string_token(value: String, span: Span) -> Result<Token, FilterParseError> {
    if !value.contains(VAR_PLACEHOLDER) {
        return Ok(Token::Str(value));
    }
    let mut segments = Vec::new();
    let mut lit = String::new();
    let mut rest = value.as_str();
    while let Some(idx) = rest.find(VAR_PLACEHOLDER) {
        lit.push_str(&rest[..idx]);
        let after = &rest[idx + VAR_PLACEHOLDER.len()..];
        let Some(end) = after.find('}') else {
            return Err(FilterParseError::new(
                "unterminated `${vars.NAME}` in string literal",
                span,
            ));
        };
        let name = &after[..end];
        if !is_valid_var_name(name) {
            return Err(FilterParseError::new(
                "`${vars.NAME}` needs NAME in [A-Za-z0-9_-]",
                span,
            ));
        }
        if !lit.is_empty() {
            segments.push(StrSegment::Lit(std::mem::take(&mut lit)));
        }
        segments.push(StrSegment::Var(name.to_string()));
        rest = &after[end + 1..];
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        segments.push(StrSegment::Lit(lit));
    }
    Ok(Token::Interp(segments))
}

struct Spanned {
    token: Token,
    span: Span,
}

fn tokenize(src: &str) -> Result<Vec<Spanned>, FilterParseError> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let end_byte = src.len();
    let byte_at = |k: usize| chars.get(k).map_or(end_byte, |&(b, _)| b);
    let mut tokens = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        let (start, c) = chars[k];
        match c {
            c if c.is_whitespace() => k += 1,
            '(' => {
                tokens.push(spanned(Token::LParen, start, byte_at(k + 1)));
                k += 1;
            }
            ')' => {
                tokens.push(spanned(Token::RParen, start, byte_at(k + 1)));
                k += 1;
            }
            '<' | '>' => {
                let two = chars.get(k + 1).map(|&(_, c)| c) == Some('=');
                let op = match (c, two) {
                    ('<', true) => CompareOp::Le,
                    ('<', false) => CompareOp::Lt,
                    ('>', true) => CompareOp::Ge,
                    _ => CompareOp::Gt,
                };
                let len = if two { 2 } else { 1 };
                tokens.push(spanned(Token::Op(op), start, byte_at(k + len)));
                k += len;
            }
            '=' | '!' => {
                if chars.get(k + 1).map(|&(_, c)| c) != Some('=') {
                    let hint = if c == '=' {
                        "expected `==`; a single `=` is not an operator"
                    } else {
                        "expected `!=`; write `not` for boolean negation"
                    };
                    return Err(FilterParseError::new(hint, (start, byte_at(k + 1))));
                }
                let op = if c == '=' {
                    CompareOp::Eq
                } else {
                    CompareOp::Ne
                };
                tokens.push(spanned(Token::Op(op), start, byte_at(k + 2)));
                k += 2;
            }
            '"' => {
                let mut j = k + 1;
                let mut value = String::new();
                let closed = loop {
                    let Some(&(_, ch)) = chars.get(j) else {
                        break false;
                    };
                    j += 1;
                    match ch {
                        '"' => break true,
                        '\\' => {
                            let Some(&(_, esc)) = chars.get(j) else {
                                break false;
                            };
                            j += 1;
                            match esc {
                                '"' => value.push('"'),
                                '\\' => value.push('\\'),
                                'n' => value.push('\n'),
                                't' => value.push('\t'),
                                'r' => value.push('\r'),
                                other => {
                                    value.push('\\');
                                    value.push(other);
                                }
                            }
                        }
                        other => value.push(other),
                    }
                };
                if !closed {
                    return Err(FilterParseError::new(
                        "unterminated string literal",
                        (start, end_byte),
                    ));
                }
                let span = (start, byte_at(j));
                tokens.push(spanned(string_token(value, span)?, start, byte_at(j)));
                k = j;
            }
            '$' => {
                if chars.get(k + 1).map(|&(_, c)| c) != Some('{') {
                    return Err(FilterParseError::new(
                        "expected a `${vars.NAME}` variable reference after `$`",
                        (start, byte_at(k + 1)),
                    ));
                }
                let mut j = k + 2;
                while j < chars.len() && chars[j].1 != '}' {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(FilterParseError::new(
                        "unterminated `${...}` variable reference",
                        (start, end_byte),
                    ));
                }
                let span = (start, byte_at(j + 1));
                let inner = &src[byte_at(k + 2)..byte_at(j)];
                let Some(name) = inner.strip_prefix("vars.") else {
                    return Err(FilterParseError::new(
                        "only `${vars.NAME}` is supported here",
                        span,
                    ));
                };
                if !is_valid_var_name(name) {
                    return Err(FilterParseError::new(
                        "`${vars.NAME}` needs NAME in [A-Za-z0-9_-]",
                        span,
                    ));
                }
                tokens.push(spanned(Token::Var(name.to_string()), start, byte_at(j + 1)));
                k = j + 1;
            }
            '-' | '0'..='9' => {
                let mut j = k + 1;
                while j < chars.len()
                    && matches!(chars[j].1, '0'..='9' | '.' | 'e' | 'E' | '+' | '-')
                {
                    j += 1;
                }
                let span = (start, byte_at(j));
                let text = &src[start..byte_at(j)];
                let value = text.parse::<f64>().map_err(|_| {
                    FilterParseError::new(format!("`{text}` is not a valid number"), span)
                })?;
                tokens.push(Spanned {
                    token: Token::Num(value),
                    span,
                });
                k = j;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let mut j = k;
                while j < chars.len()
                    && (chars[j].1.is_ascii_alphanumeric()
                        || chars[j].1 == '_'
                        || chars[j].1 == '.')
                {
                    j += 1;
                }
                let span = (start, byte_at(j));
                let word = &src[start..byte_at(j)];
                tokens.push(Spanned {
                    token: classify_word(word, span)?,
                    span,
                });
                k = j;
            }
            _ => {
                return Err(FilterParseError::new(
                    format!("unexpected character `{c}`"),
                    (start, byte_at(k + 1)),
                ));
            }
        }
    }
    Ok(tokens)
}

fn spanned(token: Token, start: usize, end: usize) -> Spanned {
    Spanned {
        token,
        span: (start, end),
    }
}

/// Classify a bareword: a reserved keyword/literal, or otherwise a dotted field
/// path. Real context fields are never named after a keyword, so keywords win.
fn classify_word(word: &str, span: Span) -> Result<Token, FilterParseError> {
    let token = match word {
        "and" => Token::And,
        "or" => Token::Or,
        "not" => Token::Not,
        "matches" => Token::Op(CompareOp::Matches),
        "glob" => Token::Op(CompareOp::Glob),
        "contains" => Token::Op(CompareOp::Contains),
        "true" => Token::Bool(true),
        "false" => Token::Bool(false),
        "null" => Token::Null,
        _ => {
            let segments: Vec<String> = word.split('.').map(str::to_string).collect();
            if segments.iter().any(String::is_empty) {
                return Err(FilterParseError::new(
                    "field path has an empty segment (check for a stray or doubled `.`)",
                    span,
                ));
            }
            Token::Path(segments)
        }
    };
    Ok(token)
}

struct Parser<'a> {
    tokens: &'a [Spanned],
    pos: usize,
    eof: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Spanned], eof: usize) -> Self {
        Self {
            tokens,
            pos: 0,
            eof,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|s| &s.token)
    }

    fn span_here(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or((self.eof, self.eof), |s| s.span)
    }

    fn parse(&mut self) -> Result<FilterExpr, FilterParseError> {
        let expr = self.parse_or()?;
        if self.pos < self.tokens.len() {
            return Err(FilterParseError::new(
                "unexpected trailing input; expected `and`, `or`, or end of expression",
                self.span_here(),
            ));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<FilterExpr, FilterParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            let right = self.parse_and()?;
            left = FilterExpr(Expr::Or(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<FilterExpr, FilterParseError> {
        let mut left = self.parse_not()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.pos += 1;
            let right = self.parse_not()?;
            left = FilterExpr(Expr::And(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<FilterExpr, FilterParseError> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.pos += 1;
            return Ok(FilterExpr(Expr::Not(Box::new(self.parse_not()?))));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<FilterExpr, FilterParseError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.pos += 1;
                let expr = self.parse_or()?;
                if matches!(self.peek(), Some(Token::RParen)) {
                    self.pos += 1;
                    Ok(expr)
                } else {
                    Err(FilterParseError::new("expected `)`", self.span_here()))
                }
            }
            Some(Token::Path(_)) => self.parse_comparison(),
            _ => Err(FilterParseError::new(
                "expected a field name or `(`",
                self.span_here(),
            )),
        }
    }

    fn parse_comparison(&mut self) -> Result<FilterExpr, FilterParseError> {
        let Some(Token::Path(path)) = self.peek().cloned() else {
            return Err(FilterParseError::new(
                "expected a field name",
                self.span_here(),
            ));
        };
        self.pos += 1;
        let op_span = self.span_here();
        let op = match self.peek() {
            Some(Token::Op(op)) => {
                let op = *op;
                self.pos += 1;
                op
            }
            _ => {
                return Err(FilterParseError::new(
                    format!(
                        "expected a comparison operator (<, <=, ==, !=, >=, >, glob, matches, contains) after `{}`",
                        path.join(".")
                    ),
                    op_span,
                ));
            }
        };
        let operand = self.parse_operand(op)?;
        Ok(FilterExpr(Expr::Compare(Comparison { path, op, operand })))
    }

    fn parse_operand(&mut self, op: CompareOp) -> Result<Operand, FilterParseError> {
        let span = self.span_here();
        let Some(token) = self.peek().cloned() else {
            return Err(FilterParseError::new(
                format!("expected an operand after `{}`", op.as_str()),
                span,
            ));
        };
        self.pos += 1;
        match (op, token) {
            (CompareOp::Matches, Token::Str(s)) => {
                let compiled = regex::Regex::new(&s)
                    .map_err(|e| FilterParseError::new(format!("invalid regex: {e}"), span))?;
                Ok(Operand::Regex(RegexOperand {
                    source: s,
                    compiled,
                }))
            }
            (CompareOp::Matches, Token::Interp(_)) => Err(FilterParseError::new(
                "`matches` takes a literal regex; `${vars.NAME}` interpolation \
                 isn't supported inside a regex pattern",
                span,
            )),
            (CompareOp::Matches, _) => Err(FilterParseError::new(
                "`matches` needs a \"quoted\" regex operand",
                span,
            )),
            (CompareOp::Glob, Token::Str(s)) => Ok(Operand::Str(s)),
            (CompareOp::Glob, Token::Interp(segs)) => Ok(Operand::Interp(segs)),
            (CompareOp::Glob, _) => Err(FilterParseError::new(
                "`glob` needs a \"quoted\" wildcard pattern operand",
                span,
            )),
            (CompareOp::Lt | CompareOp::Le | CompareOp::Ge | CompareOp::Gt, Token::Num(n)) => {
                Ok(Operand::Number(n))
            }
            (CompareOp::Lt | CompareOp::Le | CompareOp::Ge | CompareOp::Gt, Token::Var(name)) => {
                Ok(Operand::Var(name))
            }
            (CompareOp::Lt | CompareOp::Le | CompareOp::Ge | CompareOp::Gt, _) => Err(
                FilterParseError::new(format!("`{}` needs a numeric operand", op.as_str()), span),
            ),
            (CompareOp::Contains, Token::Num(n)) => Ok(Operand::Number(n)),
            (CompareOp::Contains, Token::Str(s)) => Ok(Operand::Str(s)),
            (CompareOp::Contains, Token::Bool(b)) => Ok(Operand::Bool(b)),
            (CompareOp::Contains, Token::Var(name)) => Ok(Operand::Var(name)),
            (CompareOp::Contains, Token::Interp(segs)) => Ok(Operand::Interp(segs)),
            (CompareOp::Contains, _) => Err(FilterParseError::new(
                "`contains` needs a string, number, or boolean operand",
                span,
            )),
            (CompareOp::Eq | CompareOp::Ne, Token::Num(n)) => Ok(Operand::Number(n)),
            (CompareOp::Eq | CompareOp::Ne, Token::Str(s)) => Ok(Operand::Str(s)),
            (CompareOp::Eq | CompareOp::Ne, Token::Bool(b)) => Ok(Operand::Bool(b)),
            (CompareOp::Eq | CompareOp::Ne, Token::Null) => Ok(Operand::Null),
            (CompareOp::Eq | CompareOp::Ne, Token::Var(name)) => Ok(Operand::Var(name)),
            (CompareOp::Eq | CompareOp::Ne, Token::Interp(segs)) => Ok(Operand::Interp(segs)),
            (CompareOp::Eq | CompareOp::Ne, _) => Err(FilterParseError::new(
                "expected a string, number, boolean, or null operand",
                span,
            )),
        }
    }
}

// A syntactic operator/group budget bounds both parser frames and tree height,
// including flat binary chains. Check it before constructing any recursive tree.
const MAX_FILTER_STRUCTURE: usize = 128;
const MAX_FILTER_BYTES: usize = 64 * 1024;

fn parse_filter(raw: &str) -> Result<FilterExpr, FilterParseError> {
    let tokens = tokenize(raw)?;
    let mut structure = 0;
    for token in &tokens {
        if matches!(
            token.token,
            Token::Not | Token::And | Token::Or | Token::LParen
        ) {
            structure += 1;
            if structure > MAX_FILTER_STRUCTURE {
                return Err(FilterParseError::new(
                    format!(
                        "filter exceeds {MAX_FILTER_STRUCTURE} operators/groups; simplify the filter"
                    ),
                    token.span,
                ));
            }
        }
    }
    Parser::new(&tokens, raw.len()).parse()
}

/// Parse a filter expression, rendering any error to a caret-annotated string.
/// The crate-internal entry point used by the serde impl (and tests).
#[doc(hidden)]
pub fn parse(raw: &str) -> Result<FilterExpr, String> {
    // Reject before tokenization or diagnostic rendering copies the input.
    if raw.len() > MAX_FILTER_BYTES {
        return Err(format!(
            "filter exceeds {MAX_FILTER_BYTES} bytes; shorten the filter"
        ));
    }
    let expression = parse_filter(raw).map_err(|e| e.render(raw))?;
    // Display adds spacing and escapes. Bound the persisted spelling too so
    // every accepted filter can be serialized and loaded again.
    if expression.to_string().len() > MAX_FILTER_BYTES {
        return Err(format!(
            "formatted filter exceeds {MAX_FILTER_BYTES} bytes; shorten the filter"
        ));
    }
    Ok(expression)
}

impl std::str::FromStr for FilterExpr {
    /// The caret-annotated parse error, same as a YAML `filter:` would report.
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        parse(raw)
    }
}

impl<'de> Deserialize<'de> for FilterExpr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        parse(&raw).map_err(de::Error::custom)
    }
}

impl Serialize for FilterExpr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn filter(s: &str) -> FilterExpr {
        parse_filter(s).unwrap_or_else(|e| panic!("{}", e.render(s)))
    }

    #[test]
    fn from_str_round_trips_and_annotates_errors() {
        let raw = "host == \"api.example.com\" and method == \"GET\"";
        let expr: FilterExpr = raw.parse().expect("valid filter");
        assert_eq!(expr.to_string(), raw);

        let err = "host ==".parse::<FilterExpr>().unwrap_err();
        assert!(err.contains('^'), "expected caret annotation, got: {err}");
    }

    #[test]
    fn numeric_comparisons() {
        let ctx = json!({ "amount": 100 });
        assert!(filter("amount < 500").matches(&ctx));
        assert!(filter("amount <= 100").matches(&ctx));
        assert!(filter("amount == 100").matches(&ctx));
        assert!(filter("amount != 5").matches(&ctx));
        assert!(filter("amount >= 100").matches(&ctx));
        assert!(!filter("amount > 100").matches(&ctx));
        assert!(!filter("amount < 50").matches(&ctx));
    }

    #[test]
    fn string_equality() {
        let ctx = json!({ "host": "api.weather.com" });
        assert!(filter("host == \"api.weather.com\"").matches(&ctx));
        assert!(!filter("host == \"evil.com\"").matches(&ctx));
        assert!(filter("host != \"evil.com\"").matches(&ctx));
    }

    #[test]
    fn bool_and_null_operands() {
        let ctx = json!({ "overwrite": true, "decompress": null });
        assert!(filter("overwrite == true").matches(&ctx));
        assert!(!filter("overwrite == false").matches(&ctx));
        assert!(filter("overwrite != false").matches(&ctx));
        assert!(filter("decompress == null").matches(&ctx));
        assert!(!filter("overwrite == null").matches(&ctx));
    }

    fn vars(pairs: &[(&str, &str)]) -> VarBindings {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn var_string_equality() {
        let ctx = json!({ "userId": "u_42" });
        let f = filter("userId == ${vars.uid}");
        assert!(f.matches_with(&ctx, &vars(&[("uid", "u_42")])));
        assert!(!f.matches_with(&ctx, &vars(&[("uid", "u_99")])));
        // `!=` is the negation.
        assert!(filter("userId != ${vars.uid}").matches_with(&ctx, &vars(&[("uid", "u_99")])));
    }

    #[test]
    fn var_numeric_coercion() {
        let ctx = json!({ "amount": 100 });
        assert!(filter("amount < ${vars.cap}").matches_with(&ctx, &vars(&[("cap", "500")])));
        assert!(!filter("amount < ${vars.cap}").matches_with(&ctx, &vars(&[("cap", "50")])));
        assert!(filter("amount == ${vars.cap}").matches_with(&ctx, &vars(&[("cap", "100")])));
    }

    #[test]
    fn var_bool_coercion() {
        let ctx = json!({ "overwrite": true });
        assert!(filter("overwrite == ${vars.ow}").matches_with(&ctx, &vars(&[("ow", "true")])));
        assert!(!filter("overwrite == ${vars.ow}").matches_with(&ctx, &vars(&[("ow", "false")])));
    }

    #[test]
    fn missing_var_is_non_match() {
        let ctx = json!({ "userId": "u_42" });
        // Absent binding: non-match for both `==` and `!=`.
        assert!(!filter("userId == ${vars.uid}").matches_with(&ctx, &vars(&[])));
        assert!(!filter("userId != ${vars.uid}").matches_with(&ctx, &vars(&[])));
        assert!(!filter("amount < ${vars.cap}").matches_with(&json!({ "amount": 1 }), &vars(&[])));
    }

    #[test]
    fn uncoercible_var_is_non_match() {
        // numeric op, non-numeric var
        assert!(
            !filter("amount < ${vars.cap}")
                .matches_with(&json!({ "amount": 1 }), &vars(&[("cap", "abc")]))
        );
        // bool field, non-bool var
        assert!(
            !filter("flag == ${vars.f}")
                .matches_with(&json!({ "flag": true }), &vars(&[("f", "yes")]))
        );
    }

    #[test]
    fn var_operand_round_trips_through_display() {
        let f = filter("userId == ${vars.uid}");
        assert_eq!(f.to_string(), "userId == ${vars.uid}");
        assert_eq!(filter(&f.to_string()), f);
    }

    #[test]
    fn var_refs_are_collected() {
        let f = filter("userId == ${vars.uid} and amount < ${vars.cap}");
        assert_eq!(f.var_refs(), vec!["uid", "cap"]);
    }

    #[test]
    fn matches_and_glob_reject_bare_var_operand() {
        // A bare `${vars.x}` operand is only for value comparisons; a pattern must
        // interpolate the variable inside a quoted string instead.
        assert!(parse_filter("path matches ${vars.re}").is_err());
        assert!(parse_filter("path glob ${vars.g}").is_err());
    }

    #[test]
    fn glob_interpolation_scopes_to_user_subtree() {
        let f = filter("path glob \"users/${vars.uid}/*\"");
        let allow = vars(&[("uid", "u_42")]);
        assert!(f.matches_with(&json!({ "path": "users/u_42/notes.txt" }), &allow));
        assert!(f.matches_with(&json!({ "path": "users/u_42/sub/x" }), &allow));
        // Another user's subtree is denied.
        assert!(!f.matches_with(&json!({ "path": "users/u_99/notes.txt" }), &allow));
        // Missing variable → non-match.
        assert!(!f.matches_with(&json!({ "path": "users/u_42/x" }), &vars(&[])));
    }

    #[test]
    fn glob_interpolation_escapes_metacharacters_in_value() {
        // A value of `*` must match literally, not widen the pattern: with uid="*"
        // the rule permits only the literal `users/*/…`, not every user's subtree.
        let f = filter("path glob \"users/${vars.uid}/*\"");
        let star = vars(&[("uid", "*")]);
        assert!(f.matches_with(&json!({ "path": "users/*/x" }), &star));
        assert!(!f.matches_with(&json!({ "path": "users/u_42/x" }), &star));
    }

    #[test]
    fn glob_match_honors_backslash_escapes() {
        assert!(glob_match(r"a\*b", "a*b"));
        assert!(!glob_match(r"a\*b", "axb"));
        assert!(glob_match(r"a\?b", "a?b"));
        assert!(!glob_match(r"a\?b", "axb"));
        // Unescaped metacharacters still behave as wildcards.
        assert!(glob_match("a*b", "axb"));
        assert!(glob_match("a?b", "axb"));
    }

    #[test]
    fn string_interpolation_in_equality() {
        let f = filter("host == \"api.${vars.tenant}.com\"");
        assert!(f.matches_with(
            &json!({ "host": "api.acme.com" }),
            &vars(&[("tenant", "acme")])
        ));
        assert!(!f.matches_with(
            &json!({ "host": "api.evil.com" }),
            &vars(&[("tenant", "acme")])
        ));
    }

    #[test]
    fn interpolation_round_trips_and_collects_refs() {
        let f = filter("path glob \"users/${vars.uid}/*\"");
        assert_eq!(f.to_string(), "path glob \"users/${vars.uid}/*\"");
        assert_eq!(filter(&f.to_string()), f);
        assert_eq!(f.var_refs(), vec!["uid"]);
    }

    #[test]
    fn matches_rejects_interpolation() {
        // Regex interpolation is intentionally unsupported (injection surface).
        assert!(parse_filter("path matches \"^/users/${vars.uid}/\"").is_err());
    }

    #[test]
    fn malformed_interpolation_is_rejected() {
        assert!(parse_filter("path glob \"users/${vars.}/x\"").is_err());
        assert!(parse_filter("path glob \"users/${vars.bad!}/x\"").is_err());
        // A bare `$` or unrelated `${...}` stays literal — not an error.
        assert!(parse_filter("name == \"$5.00\"").is_ok());
    }

    #[test]
    fn malformed_var_reference_is_rejected() {
        assert!(parse_filter("x == ${secrets.k}").is_err());
        assert!(parse_filter("x == ${vars.}").is_err());
        assert!(parse_filter("x == ${vars.a").is_err());
        assert!(parse_filter("x == $vars").is_err());
    }

    #[test]
    fn regex_matching() {
        let ctx = json!({ "path": "data/report.csv" });
        assert!(filter("path matches \"\\.csv$\"").matches(&ctx));
        assert!(filter("path matches \"^data/\"").matches(&ctx));
        assert!(!filter("path matches \"\\.json$\"").matches(&ctx));
    }

    #[test]
    fn regex_is_unanchored() {
        let ctx = json!({ "host": "api.stripe.com" });
        assert!(filter("host matches \"stripe\"").matches(&ctx));
        assert!(!filter("host matches \"^stripe\"").matches(&ctx));
    }

    #[test]
    fn glob_matching() {
        let ctx = json!({ "path": "data/report.csv" });
        assert!(filter("path glob \"*.csv\"").matches(&ctx));
        assert!(filter("path glob \"data/*\"").matches(&ctx));
        assert!(filter("path glob \"data/repor?.csv\"").matches(&ctx));
        assert!(!filter("path glob \"*.json\"").matches(&ctx));
        // glob is whole-string: a bare substring does not match
        assert!(!filter("path glob \"report\"").matches(&ctx));
    }

    #[test]
    fn glob_edge_cases() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("a?c", "abc"));
    }

    #[test]
    fn nested_path_access() {
        let ctx = json!({ "params": { "amount": 100 } });
        assert!(filter("params.amount < 500").matches(&ctx));
        assert!(!filter("params.amount > 500").matches(&ctx));
        // missing nested key is a non-match
        assert!(!filter("params.missing == 1").matches(&ctx));
        // a scalar encountered mid-path is a non-match
        assert!(!filter("params.amount.x == 1").matches(&ctx));
    }

    #[test]
    fn array_index_path() {
        let ctx = json!({ "items": [{ "price": 5 }, { "price": 50 }] });
        assert!(filter("items.0.price < 10").matches(&ctx));
        assert!(filter("items.1.price == 50").matches(&ctx));
        // out-of-range index is a non-match
        assert!(!filter("items.2.price == 0").matches(&ctx));
    }

    #[test]
    fn contains_membership() {
        let ctx = json!({ "methods": ["GET", "POST"], "codes": [200, 404] });
        assert!(filter("methods contains \"GET\"").matches(&ctx));
        assert!(!filter("methods contains \"DELETE\"").matches(&ctx));
        assert!(filter("codes contains 404").matches(&ctx));
        // a non-array field is a non-match
        assert!(!filter("methods contains \"GET\"").matches(&json!({ "methods": "GET" })));
    }

    #[test]
    fn missing_field_is_no_match() {
        let ctx = json!({ "other": 1 });
        assert!(!filter("amount < 500").matches(&ctx));
        assert!(!filter("host == \"x\"").matches(&ctx));
    }

    #[test]
    fn wrong_json_kind_is_no_match() {
        assert!(!filter("amount < 500").matches(&json!({ "amount": "100" })));
        assert!(!filter("host == \"x\"").matches(&json!({ "host": 1 })));
        // `!=` against a wrong-kind value is also a non-match
        assert!(!filter("host != \"x\"").matches(&json!({ "host": 1 })));
    }

    #[test]
    fn boolean_operators_and_precedence() {
        let ctx = json!({ "amount": 100, "currency": "USD" });
        assert!(filter("amount < 500 and currency == \"USD\"").matches(&ctx));
        assert!(!filter("amount < 50 and currency == \"USD\"").matches(&ctx));
        assert!(filter("amount < 50 or currency == \"USD\"").matches(&ctx));
        assert!(filter("not amount > 500").matches(&ctx));
        // `and` binds tighter than `or`: (false and false) or true == true
        assert!(filter("amount > 500 and currency == \"EUR\" or amount == 100").matches(&ctx));
        // parentheses override precedence: false and (false or true) == false
        assert!(!filter("amount > 500 and (currency == \"EUR\" or amount == 100)").matches(&ctx));
    }

    #[test]
    fn precedence_tree_shape() {
        // `and` binds tighter than `or`
        assert_eq!(
            parse_filter("a == 1 and b == 2 or c == 3").unwrap(),
            parse_filter("(a == 1 and b == 2) or c == 3").unwrap()
        );
        assert_eq!(
            parse_filter("a == 1 or b == 2 and c == 3").unwrap(),
            parse_filter("a == 1 or (b == 2 and c == 3)").unwrap()
        );
        // `not` binds tighter than `and`
        assert_eq!(
            parse_filter("not a == 1 and b == 2").unwrap(),
            parse_filter("(not a == 1) and b == 2").unwrap()
        );
    }

    #[test]
    fn short_circuit_ignores_missing_field() {
        let ctx = json!({ "amount": 100 });
        // left of `or` is true, so the missing-field right operand never decides it
        assert!(filter("amount == 100 or missing == 1").matches(&ctx));
        // left of `and` is false, so the right operand is irrelevant
        assert!(!filter("amount == 5 and missing == 1").matches(&ctx));
    }

    #[test]
    fn malformed_filters_rejected() {
        assert!(parse_filter("amount <> 5").is_err());
        assert!(parse_filter("amount 5").is_err());
        assert!(parse_filter("< 5").is_err());
        assert!(parse_filter("amount == unquoted").is_err());
        assert!(parse_filter("amount < \"x\"").is_err());
        assert!(parse_filter("path matches 5").is_err());
        assert!(parse_filter("path glob 5").is_err());
        assert!(parse_filter("tags contains null").is_err());
        assert!(parse_filter("amount = 5").is_err());
        assert!(parse_filter("amount < 5 and").is_err());
        assert!(parse_filter("(amount < 5").is_err());
        assert!(parse_filter("amount < 5)").is_err());
        assert!(parse_filter("host matches \"(\"").is_err()); // unbalanced regex group
        assert!(parse_filter("host == \"unterminated").is_err());
    }

    #[test]
    fn top_level_fields_name_each_comparison_through_not_and_or() {
        assert_eq!(
            filter("a == 1 and not (b.c == \"x\" or a < 2)").top_level_fields(),
            ["a", "b", "a"]
        );
    }

    #[test]
    fn field_matches_collects_positive_string_comparisons() {
        assert_eq!(
            filter("host == \"api.example.com\"").field_matches("host"),
            vec![FieldMatch::Equals("api.example.com".into())]
        );
        assert_eq!(
            filter("host glob \"*.example.com\"").field_matches("host"),
            vec![FieldMatch::Glob("*.example.com".into())]
        );
        assert_eq!(
            filter("host matches \"stripe\"").field_matches("host"),
            vec![FieldMatch::Regex("stripe".into())]
        );
        // and/or both branches contribute, in source order
        assert_eq!(
            filter("host == \"a\" or host == \"b\"").field_matches("host"),
            vec![
                FieldMatch::Equals("a".into()),
                FieldMatch::Equals("b".into())
            ]
        );
        // a different field, a numeric op, and a missing field all contribute nothing
        assert!(
            filter("method == \"GET\" and body_size < 1000")
                .field_matches("host")
                .is_empty()
        );
        assert!(
            filter("host != \"evil.com\"")
                .field_matches("host")
                .is_empty()
        );
    }

    #[test]
    fn error_message_has_caret() {
        let src = "amount < 500 nd currency == \"USD\"";
        let rendered = parse_filter(src).unwrap_err().render(src);
        assert!(rendered.contains('^'), "no caret in:\n{rendered}");
    }

    #[test]
    fn canonical_round_trips_through_string() {
        for src in [
            "amount < 500",
            "amount >= 12",
            "host == \"api.weather.com\"",
            "overwrite == true",
            "decompress == null",
            "path matches \"data\"",
            "path glob \"*.csv\"",
            "amount < 500 and currency == \"USD\"",
            "params.amount < 500",
            "items.0.price < 10",
            "methods contains \"GET\"",
        ] {
            assert_eq!(parse_filter(src).unwrap().to_string(), src);
        }
    }

    #[test]
    fn ast_round_trips_through_display() {
        for src in [
            "a == 1 and b == 2 or c == 3",
            "a == 1 or b == 2 and c == 3",
            "not a == 1 and b == 2",
            "a == 1 and (b == 2 or c == 3)",
            "not (a == 1 and b == 2)",
            "not not a == 1",
            "path matches \"^\\d+$\"",
            "host == \"a\\\"b\"",
        ] {
            let parsed = parse_filter(src).unwrap();
            let reparsed = parse_filter(&parsed.to_string()).unwrap();
            assert_eq!(parsed, reparsed, "round-trip changed AST for {src:?}");
        }
    }

    #[test]
    fn explain_matches_same_as_matches_with() {
        let ctx = json!({
            "host": "api.example.com",
            "amount": 100,
            "tags": ["a", "b"],
            "nested": { "id": "x" },
            "flag": true,
        });
        let bound = vars(&[("h", "api.example.com"), ("n", "100")]);
        let filters = [
            "amount < 500",
            "amount > 500",
            "host == ${vars.h} and amount == ${vars.n}",
            "host == ${vars.h} and amount > 500",
            "amount > 500 or host == \"api.example.com\"",
            "amount > 500 or host == \"other\"",
            "not amount > 500",
            "not (host == \"api.example.com\" and amount < 500)",
            "(amount < 5 or flag == true) and not nested.id == \"y\"",
            "missing == 1",
            "missing != 1",
            "not missing == 1",
            "amount == \"100\"",
            "host < 5",
            "tags contains \"b\"",
            "nested.id == ${vars.unbound}",
            "nested.id == ${vars.unbound} or flag == true",
            "host glob \"*.example.com\" and not flag == false",
        ];
        for text in filters {
            let f = filter(text);
            for bindings in [&bound, &VarBindings::new()] {
                let explained = f.explain_with(&ctx, bindings);
                assert_eq!(explained.matched, f.matches_with(&ctx, bindings), "{text}");
                assert_eq!(explained.matched, explained.failures.is_empty(), "{text}");
            }
        }
    }

    #[test]
    fn explain_reports_every_failing_leaf() {
        let f = filter("amount < 5 and host == \"a\" and missing == 1");
        let ctx = json!({ "amount": 100, "host": "b" });
        let explained = f.explain_with(&ctx, &VarBindings::new());
        assert!(!explained.matched);
        let rendered: Vec<&str> = explained
            .failures
            .iter()
            .map(|failure| failure.comparison.as_str())
            .collect();
        assert_eq!(rendered, ["amount < 5", "host == \"a\"", "missing == 1"]);
        assert_eq!(explained.failures[0].actual, Some(json!(100)));
        assert_eq!(explained.failures[0].expected.as_deref(), Some("5"));
        assert_eq!(explained.failures[0].reason, FailureReason::NotSatisfied);
        assert_eq!(explained.failures[2].actual, None);
        assert_eq!(explained.failures[2].reason, FailureReason::FieldMissing);
    }

    #[test]
    fn explain_reports_unbound_variable() {
        let f = filter("customerId == ${vars.customerId}");
        let explained = f.explain_with(&json!({ "customerId": "cus_1" }), &VarBindings::new());
        assert!(!explained.matched);
        let failure = &explained.failures[0];
        assert_eq!(
            failure.reason,
            FailureReason::VariableNotBound("customerId".to_string())
        );
        assert_eq!(failure.expected, None);
        assert_eq!(failure.actual, Some(json!("cus_1")));

        let interpolated = filter("url glob \"https://${vars.host}/*\"");
        let explained =
            interpolated.explain_with(&json!({ "url": "https://a/x" }), &VarBindings::new());
        assert_eq!(
            explained.failures[0].reason,
            FailureReason::VariableNotBound("host".to_string())
        );
    }

    #[test]
    fn explain_reports_bound_variable_value_and_negation() {
        let f = filter("customerId == ${vars.customerId}");
        let explained = f.explain_with(
            &json!({ "customerId": "cus_initech" }),
            &vars(&[("customerId", "cus_northwind")]),
        );
        assert_eq!(
            explained.failures[0].expected.as_deref(),
            Some("cus_northwind")
        );
        assert!(!explained.failures[0].negated);

        let negated = filter("not host == \"evil.com\"");
        let explained = negated.explain_with(&json!({ "host": "evil.com" }), &VarBindings::new());
        assert!(!explained.matched);
        assert!(explained.failures[0].negated);
    }
}
