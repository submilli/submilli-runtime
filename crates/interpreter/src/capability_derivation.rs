use std::collections::BTreeMap;

use crate::stdlib::capabilities::{self, FieldNormalization};
use crate::{
    BinOp, Diagnostic, DocCapability, DocCapabilityBindingKind, DocCapabilityLiteral, ExprId,
    GlobalKind, MangledName, Param, Severity, Span, TypedAst, TypedExprKind, TypedStmtKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedCapability {
    pub capability: String,
    pub filter: Option<String>,
    /// Statically known filter operands, keyed by capability field.
    pub known_bindings: BTreeMap<String, String>,
    pub warnings: Vec<Diagnostic>,
}

pub fn derive_call_site_capability(
    tag: &DocCapability,
    callee_params: &[Param],
    caller_ast: &TypedAst,
    actual_args: &[ExprId],
) -> Result<DerivedCapability, crate::compiler_error::CompileError> {
    let mut warnings = Vec::new();
    let mut unresolved_http_host_span = None;
    let filters = derive_filters(
        tag,
        callee_params,
        caller_ast,
        actual_args,
        &mut warnings,
        &mut unresolved_http_host_span,
    );
    if let Some(span) = unresolved_http_host_span {
        warnings.push(unresolved_http_url_warning(span, &tag.capability));
    }
    let filters = filters.map_err(|fatal| {
        crate::compiler_error::CompileError::from(fatal).with_prior_diagnostics(&warnings)
    })?;
    let known_bindings = filters.iter().cloned().collect();
    Ok(DerivedCapability {
        capability: tag.capability.clone(),
        filter: (!filters.is_empty()).then(|| {
            filters
                .iter()
                .map(|(field, operand)| format!("{field} == {operand}"))
                .collect::<Vec<_>>()
                .join(" and ")
        }),
        known_bindings,
        warnings,
    })
}

fn derive_filters(
    tag: &DocCapability,
    callee_params: &[Param],
    caller_ast: &TypedAst,
    actual_args: &[ExprId],
    warnings: &mut Vec<Diagnostic>,
    unresolved_http_host_span: &mut Option<Span>,
) -> Result<Vec<(String, String)>, crate::compiler_error::CompilerFailure> {
    let mut filters = Vec::new();
    for binding in &tag.bindings {
        match &binding.kind {
            DocCapabilityBindingKind::Literal { value, .. } => {
                // The tag's span is in the callee's source, not the caller's, so a
                // literal the runtime refuses keeps its spelling without a warning.
                filters.extend(binding_filter(
                    tag,
                    &binding.field,
                    StaticValue::from(value),
                    None,
                    warnings,
                ));
            }
            DocCapabilityBindingKind::Type { .. } => {}
            DocCapabilityBindingKind::Parameter { param, path, span } => {
                let Some(param_index) = callee_params.iter().position(|p| p.name == *param) else {
                    warnings.push(warning(
                        *span,
                        format!("cannot derive filter for unknown parameter `${param}`"),
                    ));
                    continue;
                };
                let Some(actual) = actual_args.get(param_index) else {
                    warnings.push(warning(
                        *span,
                        format!("cannot derive filter for missing argument `${param}`"),
                    ));
                    continue;
                };
                let actual_span = caller_ast
                    .try_expr(*actual)
                    .map_err(crate::typechecker::arena_failure)?
                    .span;
                match literal_from_expr_path(caller_ast, *actual, path)? {
                    Some(value) => filters.extend(binding_filter(
                        tag,
                        &binding.field,
                        value,
                        Some(actual_span),
                        warnings,
                    )),
                    None if is_http_url_binding(tag, param, path) => {
                        if matches!(path.as_slice(), [component] if component == "host") {
                            *unresolved_http_host_span = Some(actual_span);
                        }
                    }
                    // Dynamic arguments leave the requirement broad; they do not
                    // demonstrate an error in the caller.
                    None => {}
                }
            }
        }
    }
    Ok(filters)
}

/// A value known at the call site, before it becomes a filter operand.
enum StaticValue {
    String(String),
    /// The number as the filter spells it.
    Number(String),
    Boolean(bool),
    Null,
}

impl From<&DocCapabilityLiteral> for StaticValue {
    fn from(value: &DocCapabilityLiteral) -> Self {
        match value {
            DocCapabilityLiteral::String(value) => Self::String(value.clone()),
            DocCapabilityLiteral::Number(value) => Self::Number(value.clone()),
            DocCapabilityLiteral::Boolean(value) => Self::Boolean(*value),
            DocCapabilityLiteral::Null => Self::Null,
        }
    }
}

/// `field == value`, with a string in the form the runtime checks.
fn binding_filter(
    tag: &DocCapability,
    field: &str,
    value: StaticValue,
    warn_at: Option<Span>,
    warnings: &mut Vec<Diagnostic>,
) -> Option<(String, String)> {
    let operand = match value {
        StaticValue::String(value) => {
            if field_normalization(&tag.capability, field) == FieldNormalization::VfsPath
                && !value.starts_with('/')
            {
                if let Some(span) = warn_at {
                    warnings.push(warning(span, format!("relative `{field}` depends on the session cwd; no static path filter for `{}`", tag.capability)));
                }
                return None;
            }
            let normalized = normalized_string(tag, field, value, warn_at, warnings);
            let Some(literal) = filter_string_literal(&normalized) else {
                if let Some(span) = warn_at {
                    warnings.push(warning(
                        span,
                        format!(
                            "`{field}` value contains `{VAR_PLACEHOLDER}`, which a filter reads as a variable reference; no static filter for `{}`",
                            tag.capability
                        ),
                    ));
                }
                return None;
            };
            literal
        }
        StaticValue::Number(spelling) => spelling,
        StaticValue::Boolean(value) => value.to_string(),
        StaticValue::Null => "null".to_string(),
    };
    Some((field.to_string(), operand))
}

/// The string the runtime checks for `value`. A value the runtime refuses keeps
/// its spelling, so the filter fails closed; the call cannot succeed, which a
/// warning at `warn_at` reports when that location is in the caller's source.
fn normalized_string(
    tag: &DocCapability,
    field: &str,
    value: String,
    warn_at: Option<Span>,
    warnings: &mut Vec<Diagnostic>,
) -> String {
    field_normalization(&tag.capability, field)
        .apply(&value)
        .unwrap_or_else(|reason| {
            if let Some(span) = warn_at {
                warnings.push(warning(
                    span,
                    format!(
                        "`{}` refuses `{field}` value \"{}\" at runtime: {reason}",
                        tag.capability,
                        escape(&value)
                    ),
                ));
            }
            value
        })
}

fn field_normalization(capability: &str, field: &str) -> FieldNormalization {
    capabilities::find_any(capability)
        .and_then(|entry| entry.filter_fields.iter().find(|f| f.name == field))
        .map_or(FieldNormalization::Verbatim, |f| f.normalization)
}

fn is_http_url_binding(tag: &DocCapability, param: &str, path: &[String]) -> bool {
    tag.capability.starts_with("http.")
        && param == "url"
        && matches!(path, [component] if matches!(component.as_str(), "host" | "path"))
}

fn unresolved_http_url_warning(span: Span, capability: &str) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        span,
        message: format!(
            "cannot statically resolve the host in the URL passed to `{capability}`; \
             no host capability filter was derived"
        ),
        help: vec![
            "call the HTTP function directly with a URL literal or a concatenation of \
             string literals and top-level string constants"
                .to_string(),
            "when appending a dynamic path, include `/` after the host in the constant prefix, \
             e.g. `\"https://api.example.com/\" + path` where `path` has no leading slash; \
             this makes the host boundary statically known"
                .to_string(),
        ],
        notes: vec![],
    }
}

fn literal_from_expr_path(
    ast: &TypedAst,
    expr_id: ExprId,
    path: &[String],
) -> Result<Option<StaticValue>, crate::compiler_error::CompilerFailure> {
    if path.is_empty() {
        return literal_from_expr(ast, expr_id);
    }
    // `$url.host` / `$url.path`: a single `host`/`path` segment on a value that
    // resolves to a string URL is filled by parsing the URL, not by walking an
    // object field — this is how the http capabilities bind `host`/`path` from
    // their `url` argument. Guarded on the base resolving to a string literal,
    // so an object field literally named `host`/`path` still resolves below.
    if let [segment] = path
        && matches!(segment.as_str(), "host" | "path")
    {
        if let Some(url) = resolve_string_literal(ast, expr_id)? {
            return Ok(url_component(&url, segment).map(StaticValue::String));
        }
        if segment == "host" {
            return url_host_from_constant_prefix(ast, expr_id);
        }
    }
    let mut current = expr_id;
    for segment in path {
        let TypedExprKind::ObjectLiteral { fields, .. } = &ast
            .try_expr(current)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            return Ok(None);
        };
        let Some(field) = fields.iter().find(|field| field.name.name == *segment) else {
            return Ok(None);
        };
        current = match field.source.literal_expr_id() {
            Some(value) => value,
            None => return Ok(None),
        };
    }
    literal_from_expr(ast, current)
}

fn url_host_from_constant_prefix(
    ast: &TypedAst,
    expr_id: ExprId,
) -> Result<Option<StaticValue>, crate::compiler_error::CompilerFailure> {
    let prefix = constant_string_prefix(ast, expr_id)?;
    let authority_start = match prefix.find("://") {
        Some(value) => value,
        None => return Ok(None),
    } + 3;
    let path_start = match prefix[authority_start..].find('/') {
        Some(value) => value,
        None => return Ok(None),
    } + authority_start;
    Ok(url_component(&prefix[..=path_start], "host").map(StaticValue::String))
}

fn constant_string_prefix(
    ast: &TypedAst,
    expr_id: ExprId,
) -> Result<String, crate::compiler_error::CompilerFailure> {
    Ok(
        match &ast
            .try_expr(expr_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedExprKind::String(value) => value.clone(),
            TypedExprKind::GlobalRef { mangled, .. } => match const_initializer(ast, mangled)? {
                Some(value) => constant_string_prefix(ast, value)?,
                None => String::new(),
            },
            TypedExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                let Some(mut value) = resolve_string_literal(ast, *lhs)? else {
                    return constant_string_prefix(ast, *lhs);
                };
                value.push_str(&constant_string_prefix(ast, *rhs)?);
                value
            }
            _ => String::new(),
        },
    )
}

fn literal_from_expr(
    ast: &TypedAst,
    expr_id: ExprId,
) -> Result<Option<StaticValue>, crate::compiler_error::CompilerFailure> {
    Ok(
        match &ast
            .try_expr(expr_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedExprKind::String(value) => Some(StaticValue::String(value.clone())),
            TypedExprKind::Binary { op: BinOp::Add, .. } => {
                resolve_string_literal(ast, expr_id)?.map(StaticValue::String)
            }
            TypedExprKind::Number(value) => Some(StaticValue::Number(number_literal(*value))),
            TypedExprKind::Boolean(value) => Some(StaticValue::Boolean(*value)),
            TypedExprKind::Null => Some(StaticValue::Null),
            TypedExprKind::GlobalRef { mangled, .. } => literal_from_expr(
                ast,
                match const_initializer(ast, mangled)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            )?,
            _ => None,
        },
    )
}

/// Resolve a value to its raw string, folding immutable top-level constants and
/// concatenations whose operands both resolve to strings.
fn resolve_string_literal(
    ast: &TypedAst,
    expr_id: ExprId,
) -> Result<Option<String>, crate::compiler_error::CompilerFailure> {
    Ok(
        match &ast
            .try_expr(expr_id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedExprKind::String(value) => Some(value.clone()),
            TypedExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                let Some(mut value) = resolve_string_literal(ast, *lhs)? else {
                    return Ok(None);
                };
                value.push_str(&match resolve_string_literal(ast, *rhs)? {
                    Some(value) => value,
                    None => return Ok(None),
                });
                Some(value)
            }
            TypedExprKind::GlobalRef { mangled, .. } => resolve_string_literal(
                ast,
                match const_initializer(ast, mangled)? {
                    Some(value) => value,
                    None => return Ok(None),
                },
            )?,
            _ => None,
        },
    )
}

/// The initializer expression of a top-level `const` (not `let` — a `let` can be
/// reassigned, so folding it would be unsound). `None` for anything else.
fn const_initializer(
    ast: &TypedAst,
    mangled: &MangledName,
) -> Result<Option<ExprId>, crate::compiler_error::CompilerFailure> {
    let is_const = ast
        .globals
        .iter()
        .any(|g| &g.mangled_name == mangled && matches!(g.kind, GlobalKind::Const));
    if !is_const {
        return Ok(None);
    }
    ast.top_level_statements
        .iter()
        .map(|stmt| {
            Ok::<_, crate::compiler_error::CompilerFailure>(
                match &ast
                    .try_stmt(*stmt)
                    .map_err(crate::typechecker::arena_failure)?
                    .kind
                {
                    TypedStmtKind::AssignGlobal {
                        mangled: target,
                        value,
                        ..
                    } if target == mangled => Some(*value),
                    _ => None,
                },
            )
        })
        .find_map(Result::transpose)
        .transpose()
}

/// Parse `url` and return the requested component.
fn url_component(url: &str, component: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let value = match component {
        // The spelling the runtime capability contexts use.
        "host" => {
            parsed.host_str()?;
            crate::stdlib::url::host_without_trailing_dots(&parsed).to_string()
        }
        "path" => parsed.path().to_string(),
        _ => return None,
    };
    Some(value)
}

/// What a filter reads inside a quoted string as the start of a variable reference.
const VAR_PLACEHOLDER: &str = "${vars.";

/// `value` as a quoted filter string, in the canonical form the policy parser
/// reads back as exactly `value`. `None` when no quoted string can spell it: the
/// parser reads `${vars.` inside quotes as a variable reference.
///
/// Must agree with the policy crate's own quoting; a test there compares the two.
#[doc(hidden)]
pub fn filter_string_literal(value: &str) -> Option<String> {
    (!value.contains(VAR_PLACEHOLDER)).then(|| format!("\"{}\"", escape(value)))
}

/// The escapes the filter tokenizer reads inside a quoted string.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
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

fn number_literal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn warning(span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        span,
        message,
        help: vec![],
        notes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Asi, TokenKind, infer, parse};

    fn typed(source: &str) -> TypedAst {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let lex_diags = asi.into_diagnostics();
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
        assert!(parse_diags.is_empty(), "{parse_diags:?}");
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        packages.extend(prelude_defs.iter());
        packages.extend(host_defs.iter());
        let (ta, infer_diags) = infer(source, "main", &ast, &packages);
        assert!(infer_diags.is_empty(), "{infer_diags:?}");
        ta
    }

    fn first_doc_capability(source: &str) -> (TypedAst, DocCapability, Vec<Param>, Vec<ExprId>) {
        let ta = typed(source);
        let callee = ta
            .functions
            .iter()
            .find(|f| f.name.name == "callee")
            .expect("callee");
        let tag = callee
            .doc
            .as_ref()
            .and_then(|doc| doc.capabilities.first())
            .cloned()
            .expect("capability tag");
        let params = callee
            .params
            .iter()
            .map(|p| Param::new(p.name.name.clone(), p.ty.clone()))
            .collect::<Vec<_>>();
        let caller = ta
            .functions
            .iter()
            .find(|f| f.name.name == "main")
            .expect("main");
        let crate::TypedStmtKind::Block(stmts) = &ta.try_stmt(caller.body).unwrap().kind else {
            panic!("main body block");
        };
        let call_expr = stmts
            .iter()
            .find_map(|stmt| match &ta.try_stmt(*stmt).unwrap().kind {
                crate::TypedStmtKind::Expr(call_expr) => Some(*call_expr),
                _ => None,
            })
            .expect("call expr stmt");
        let TypedExprKind::Call { args, .. } = &ta.try_expr(call_expr).unwrap().kind else {
            panic!("direct call");
        };
        let args = args.clone();
        (ta, tag, params, args)
    }

    #[test]
    fn literal_arg_contributes_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability secrets.get { name } */\n\
             function callee(name: string): void { }\n\
             function main(): void { callee(\"TOKEN\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("name == \"TOKEN\""));
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    /// A filter reads `${vars.` inside quotes as a variable, so no literal can spell
    /// such a value: the binding is dropped, with a warning, rather than written as
    /// a variable reference the program never meant.
    #[test]
    fn literal_holding_a_variable_reference_contributes_no_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability secrets.get { name } */\n\
             function callee(name: string): void { }\n\
             function main(): void { callee(\"${vars.who}\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter, None);
        assert!(derived.known_bindings.is_empty());
        assert_eq!(derived.warnings.len(), 1, "{:?}", derived.warnings);
        assert!(derived.warnings[0].message.contains("${vars."));
    }

    #[test]
    fn string_literal_escapes_what_the_filter_tokenizer_reads() {
        assert_eq!(
            filter_string_literal("a\"b\\c\nd\te\rf").as_deref(),
            Some("\"a\\\"b\\\\c\\nd\\te\\rf\""),
        );
        assert_eq!(filter_string_literal("${vars.x}"), None);
        assert_eq!(
            filter_string_literal("${other}").as_deref(),
            Some("\"${other}\"")
        );
    }

    #[test]
    fn non_literal_arg_retains_requirement_without_warning() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability secrets.get { name } */\n\
             function callee(name: string): void { }\n\
             function main(): void { let token = \"TOKEN\"; callee(token); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.capability, "secrets.get");
        assert_eq!(derived.filter, None);
        assert!(derived.known_bindings.is_empty());
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn type_only_field_does_not_contribute_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability x/op { amount: number } */\n\
             function callee(amount: number): void { }\n\
             function main(): void { callee(1); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter, None);
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn fixed_literal_contributes_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability db.query { readonly: true, query: string } */\n\
             function callee(query: string): void { }\n\
             function main(): void { callee(\"select 1\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("readonly == true"));
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn url_host_and_path_extracted_from_literal() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host, path: $url.path } */\n\
             function callee(url: string): void { }\n\
             function main(): void { callee(\"https://r.jina.ai/read\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(
            derived.filter.as_deref(),
            Some("host == \"r.jina.ai\" and path == \"/read\"")
        );
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn fully_qualified_host_derives_the_filter_the_runtime_checks() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             function main(): void { callee(\"https://r.jina.ai./read\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("host == \"r.jina.ai\""));
    }

    #[test]
    fn const_url_folds_to_host_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             const ENDPOINT: string = \"https://r.jina.ai/\";\n\
             function main(): void { callee(ENDPOINT); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("host == \"r.jina.ai\""));
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn constant_string_concatenation_folds_to_url_filters() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host, path: $url.path } */\n\
             function callee(url: string): void { }\n\
             const ORIGIN: string = \"https://\" + \"api.example.com\";\n\
             const VERSION: string = \"/v1\";\n\
             function main(): void { callee(ORIGIN + VERSION + \"/items\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(
            derived.filter.as_deref(),
            Some("host == \"api.example.com\" and path == \"/v1/items\"")
        );
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn constant_url_prefix_folds_only_the_host_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.get { host: $url.host, path: $url.path } */\n\
             function callee(url: string): void { }\n\
             const API: string = \"https://api.example.com/v1/\";\n\
             function main(path: string): void { callee(API + path); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(
            derived.filter.as_deref(),
            Some("host == \"api.example.com\"")
        );
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn constant_url_prefix_without_path_does_not_claim_a_host() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.get { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             const ORIGIN: string = \"https://api.example.com\";\n\
             function main(suffix: string): void { callee(ORIGIN + suffix); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter, None);
        assert_eq!(derived.warnings.len(), 1);
    }

    #[test]
    fn http_helper_result_is_not_folded_and_gets_one_targeted_warning() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.get { host: $url.host, path: $url.path } */\n\
             function callee(url: string): void { }\n\
             const ORIGIN: string = \"https://api.example.com\";\n\
             function endpoint(): string { return ORIGIN + \"/v1/items\"; }\n\
             function main(): void { callee(endpoint()); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), None);
        assert_eq!(derived.warnings.len(), 1);
        assert!(
            derived.warnings[0]
                .message
                .contains("cannot statically resolve the host in the URL passed to `http.get`")
        );
        assert!(
            derived.warnings[0].help[0].contains("call the HTTP function directly"),
            "{:?}",
            derived.warnings[0].help
        );
    }

    #[test]
    fn repository_url_and_path_derive_the_values_the_runtime_checks() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability git.clone { path, remote: $url, remoteName: \"origin\" } */\n\
             function callee(url: string, path: string): void { }\n\
             function main(): void { callee(\"https://GitHub.com:443\", \"repo/./x/..\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(
            derived.filter.as_deref(),
            Some("remote == \"https://github.com/\" and remoteName == \"origin\"")
        );
        assert_eq!(derived.warnings.len(), 1, "{:?}", derived.warnings);
    }

    #[test]
    fn vfs_path_fields_derive_normalized_paths() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability fs.move { from, to } */\n\
             function callee(from: string, to: string): void { }\n\
             function main(): void { callee(\"/data/../in.csv\", \"out/\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("from == \"/in.csv\""));
        assert_eq!(derived.warnings.len(), 1, "{:?}", derived.warnings);
    }

    #[test]
    fn fixed_vfs_path_literal_is_normalized() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability fs.read { path: \"data\" } */\n\
             function callee(): void { }\n\
             function main(): void { callee(); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter, None);
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn fixed_literal_the_runtime_refuses_keeps_its_spelling_without_a_warning() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability fs.read { path: \"/..\" } */\n\
             function callee(): void { }\n\
             function main(): void { callee(); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("path == \"/..\""));
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn download_destination_and_const_folded_paths_are_normalized() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.download { host: $url.host, vfs_path: $path } */\n\
             function callee(url: string, path: string): void { }\n\
             const DIR: string = \"out/\";\n\
             function main(): void { callee(\"https://Example.com/f\", DIR + \"../dl.csv\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("host == \"example.com\""));
        assert_eq!(derived.warnings.len(), 1, "{:?}", derived.warnings);
    }

    #[test]
    fn same_field_name_outside_the_catalog_stays_verbatim() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability acme.com/open { path } */\n\
             function callee(path: string): void { }\n\
             function main(): void { callee(\"repo\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter.as_deref(), Some("path == \"repo\""));
    }

    #[test]
    fn value_the_runtime_refuses_keeps_its_spelling_and_warns() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability git.clone { path, remote: $url } */\n\
             function callee(url: string, path: string): void { }\n\
             function main(): void { callee(\"http://github.com/a.git\", \"/..\"); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(
            derived.filter.as_deref(),
            Some("path == \"/..\" and remote == \"http://github.com/a.git\"")
        );
        let messages = derived
            .warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                "`git.clone` refuses `path` value \"/..\" at runtime: path escapes the VFS root",
                "`git.clone` refuses `remote` value \"http://github.com/a.git\" at runtime: \
                 git: remote must be an HTTPS repository URL without credentials, query, or fragment",
            ]
        );
    }

    #[test]
    fn mutable_global_let_is_not_folded() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             let endpoint: string = \"https://r.jina.ai/\";\n\
             function main(): void { callee(endpoint); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args).unwrap();
        assert_eq!(derived.filter, None);
        assert_eq!(derived.warnings.len(), 1);
    }
}
