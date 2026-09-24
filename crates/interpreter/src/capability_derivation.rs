use crate::{
    BinOp, Diagnostic, DocCapability, DocCapabilityBindingKind, DocCapabilityLiteral, ExprId,
    GlobalKind, MangledName, Param, Severity, Span, TypedAst, TypedExprKind, TypedStmtKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedCapability {
    pub capability: String,
    pub filter: Option<String>,
    pub warnings: Vec<Diagnostic>,
}

pub fn derive_call_site_capability(
    tag: &DocCapability,
    callee_params: &[Param],
    caller_ast: &TypedAst,
    actual_args: &[ExprId],
) -> DerivedCapability {
    let mut filters = Vec::new();
    let mut warnings = Vec::new();
    let mut unresolved_http_host_span = None;
    for binding in &tag.bindings {
        match &binding.kind {
            DocCapabilityBindingKind::Literal { value, .. } => {
                filters.push(format!("{} == {}", binding.field, literal_filter(value)));
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
                match literal_from_expr_path(caller_ast, *actual, path) {
                    Some(value) => filters.push(format!("{} == {}", binding.field, value)),
                    None if is_http_url_binding(tag, param, path) => {
                        if matches!(path.as_slice(), [component] if component == "host") {
                            unresolved_http_host_span = Some(caller_ast.expr(*actual).span);
                        }
                    }
                    None => warnings.push(warning(
                        caller_ast.expr(*actual).span,
                        format!(
                            "non-literal argument for `{param}`; no static capability filter for `{}`",
                            binding.field
                        ),
                    )),
                }
            }
        }
    }
    if let Some(span) = unresolved_http_host_span {
        warnings.push(unresolved_http_url_warning(span, &tag.capability));
    }
    DerivedCapability {
        capability: tag.capability.clone(),
        filter: (!filters.is_empty()).then(|| filters.join(" and ")),
        warnings,
    }
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

fn literal_from_expr_path(ast: &TypedAst, expr_id: ExprId, path: &[String]) -> Option<String> {
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
        if let Some(url) = resolve_string_literal(ast, expr_id) {
            return url_component(&url, segment);
        }
        if segment == "host" {
            return url_host_from_constant_prefix(ast, expr_id);
        }
    }
    let mut current = expr_id;
    for segment in path {
        let TypedExprKind::ObjectLiteral { fields, .. } = &ast.expr(current).kind else {
            return None;
        };
        let field = fields.iter().find(|field| field.name.name == *segment)?;
        current = field.source.literal_expr_id()?;
    }
    literal_from_expr(ast, current)
}

fn url_host_from_constant_prefix(ast: &TypedAst, expr_id: ExprId) -> Option<String> {
    let prefix = constant_string_prefix(ast, expr_id);
    let authority_start = prefix.find("://")? + 3;
    let path_start = prefix[authority_start..].find('/')? + authority_start;
    url_component(&prefix[..=path_start], "host")
}

fn constant_string_prefix(ast: &TypedAst, expr_id: ExprId) -> String {
    match &ast.expr(expr_id).kind {
        TypedExprKind::String(value) => value.clone(),
        TypedExprKind::GlobalRef { mangled, .. } => const_initializer(ast, mangled)
            .map_or_else(String::new, |value| constant_string_prefix(ast, value)),
        TypedExprKind::Binary {
            op: BinOp::Add,
            lhs,
            rhs,
        } => {
            let Some(mut value) = resolve_string_literal(ast, *lhs) else {
                return constant_string_prefix(ast, *lhs);
            };
            value.push_str(&constant_string_prefix(ast, *rhs));
            value
        }
        _ => String::new(),
    }
}

fn literal_from_expr(ast: &TypedAst, expr_id: ExprId) -> Option<String> {
    match &ast.expr(expr_id).kind {
        TypedExprKind::String(value) => Some(format!("\"{}\"", escape(value))),
        TypedExprKind::Binary { op: BinOp::Add, .. } => {
            resolve_string_literal(ast, expr_id).map(|value| format!("\"{}\"", escape(&value)))
        }
        TypedExprKind::Number(value) => Some(number_literal(*value)),
        TypedExprKind::Boolean(value) => Some(value.to_string()),
        TypedExprKind::Null => Some("null".to_string()),
        TypedExprKind::GlobalRef { mangled, .. } => {
            literal_from_expr(ast, const_initializer(ast, mangled)?)
        }
        _ => None,
    }
}

/// Resolve a value to its raw string, folding immutable top-level constants and
/// concatenations whose operands both resolve to strings.
fn resolve_string_literal(ast: &TypedAst, expr_id: ExprId) -> Option<String> {
    match &ast.expr(expr_id).kind {
        TypedExprKind::String(value) => Some(value.clone()),
        TypedExprKind::Binary {
            op: BinOp::Add,
            lhs,
            rhs,
        } => {
            let mut value = resolve_string_literal(ast, *lhs)?;
            value.push_str(&resolve_string_literal(ast, *rhs)?);
            Some(value)
        }
        TypedExprKind::GlobalRef { mangled, .. } => {
            resolve_string_literal(ast, const_initializer(ast, mangled)?)
        }
        _ => None,
    }
}

/// The initializer expression of a top-level `const` (not `let` — a `let` can be
/// reassigned, so folding it would be unsound). `None` for anything else.
fn const_initializer(ast: &TypedAst, mangled: &MangledName) -> Option<ExprId> {
    let is_const = ast
        .globals
        .iter()
        .any(|g| &g.mangled_name == mangled && matches!(g.kind, GlobalKind::Const));
    if !is_const {
        return None;
    }
    ast.top_level_statements
        .iter()
        .find_map(|stmt| match &ast.stmt(*stmt).kind {
            TypedStmtKind::AssignGlobal {
                mangled: target,
                value,
                ..
            } if target == mangled => Some(*value),
            _ => None,
        })
}

/// Parse `url` and return the requested component as a quoted filter value.
fn url_component(url: &str, component: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let value = match component {
        "host" => parsed.host_str()?.to_string(),
        "path" => parsed.path().to_string(),
        _ => return None,
    };
    Some(format!("\"{}\"", escape(&value)))
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn literal_filter(value: &DocCapabilityLiteral) -> String {
    match value {
        DocCapabilityLiteral::String(value) => format!("\"{}\"", escape(value)),
        DocCapabilityLiteral::Number(value) => value.clone(),
        DocCapabilityLiteral::Boolean(value) => value.to_string(),
        DocCapabilityLiteral::Null => "null".to_string(),
    }
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
        let crate::TypedStmtKind::Block(stmts) = &ta.stmt(caller.body).kind else {
            panic!("main body block");
        };
        let call_expr = stmts
            .iter()
            .find_map(|stmt| match &ta.stmt(*stmt).kind {
                crate::TypedStmtKind::Expr(call_expr) => Some(*call_expr),
                _ => None,
            })
            .expect("call expr stmt");
        let TypedExprKind::Call { args, .. } = &ta.expr(call_expr).kind else {
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
        assert_eq!(derived.filter.as_deref(), Some("name == \"TOKEN\""));
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn non_literal_arg_warns_without_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability secrets.get { name } */\n\
             function callee(name: string): void { }\n\
             function main(): void { let token = \"TOKEN\"; callee(token); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
        assert_eq!(derived.filter, None);
        assert_eq!(derived.warnings.len(), 1);
    }

    #[test]
    fn type_only_field_does_not_contribute_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability x/op { amount: number } */\n\
             function callee(amount: number): void { }\n\
             function main(): void { callee(1); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
        assert_eq!(
            derived.filter.as_deref(),
            Some("host == \"r.jina.ai\" and path == \"/read\"")
        );
        assert!(derived.warnings.is_empty(), "{:?}", derived.warnings);
    }

    #[test]
    fn const_url_folds_to_host_filter() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             const ENDPOINT: string = \"https://r.jina.ai/\";\n\
             function main(): void { callee(ENDPOINT); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
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
    fn mutable_global_let_is_not_folded() {
        let (ta, tag, params, args) = first_doc_capability(
            "/** @capability http.post { host: $url.host } */\n\
             function callee(url: string): void { }\n\
             let endpoint: string = \"https://r.jina.ai/\";\n\
             function main(): void { callee(endpoint); }\n",
        );
        let derived = derive_call_site_capability(&tag, &params, &ta, &args);
        assert_eq!(derived.filter, None);
        assert_eq!(derived.warnings.len(), 1);
    }
}
