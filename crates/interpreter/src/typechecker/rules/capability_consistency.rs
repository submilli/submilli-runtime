use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Diagnostic, DocCapabilityBinding, DocCapabilityBindingKind, ExprId, MangledName, Severity,
    Span, StmtId, Type, TypedAst, TypedChainPart, TypedExprKind, TypedFunction, TypedStmtKind,
};

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) {
    let security_check =
        crate::mangle::package_symbol(crate::stdlib::security::MODULE_NAME, "check");
    for f in &ta.functions {
        validate_doc_parser_diagnostics(f, diags);
        validate_param_bindings(f, diags);

        let mut checks = Vec::new();
        collect_checks_in_stmt(ta, f.body, &security_check, &mut checks);
        validate_check_tags(f, &checks, diags);
    }
}

fn validate_doc_parser_diagnostics(f: &TypedFunction, diags: &mut Vec<Diagnostic>) {
    let Some(doc) = &f.doc else { return };
    for cap in &doc.capabilities {
        for diag in &cap.diagnostics {
            diags.push(warning(diag.span, diag.message.clone()));
        }
    }
}

fn validate_param_bindings(f: &TypedFunction, diags: &mut Vec<Diagnostic>) {
    let Some(doc) = &f.doc else { return };
    let params = f
        .params
        .iter()
        .map(|p| (p.name.name.as_str(), p))
        .collect::<BTreeMap<_, _>>();
    for cap in &doc.capabilities {
        for binding in &cap.bindings {
            let DocCapabilityBindingKind::Parameter { param, path, span } = &binding.kind else {
                continue;
            };
            let Some(param_decl) = params.get(param.as_str()) else {
                diags.push(warning(
                    *span,
                    format!("unknown parameter binding `${param}` in `@capability`"),
                ));
                continue;
            };
            if let Some(missing) = missing_path_segment(&param_decl.ty, path) {
                diags.push(warning(
                    *span,
                    format!("unknown field `{missing}` in `@capability` binding `${param}`"),
                ));
            }
        }
    }
}

fn missing_path_segment(ty: &Type, path: &[String]) -> Option<String> {
    let mut current = ty;
    for segment in path {
        let Type::Object { fields } = current else {
            return Some(segment.clone());
        };
        let Some(field) = fields.get(segment) else {
            return Some(segment.clone());
        };
        current = &field.ty;
    }
    None
}

#[derive(Debug)]
struct SecurityCheck {
    span: Span,
    capability: Option<(String, Span)>,
    payload_keys: Option<BTreeMap<String, Span>>,
}

fn validate_check_tags(f: &TypedFunction, checks: &[SecurityCheck], diags: &mut Vec<Diagnostic>) {
    let Some(doc) = &f.doc else {
        for check in checks {
            if let Some((capability, span)) = &check.capability {
                diags.push(warning(
                    *span,
                    format!("missing `@capability {capability}` for `check()` call"),
                ));
            } else {
                diags.push(warning(
                    check.span,
                    "dynamic capability string in `check()`; use a string literal".to_string(),
                ));
            }
        }
        return;
    };

    let mut checks_by_capability: BTreeMap<&str, Vec<&SecurityCheck>> = BTreeMap::new();
    for check in checks {
        match &check.capability {
            Some((capability, _)) => {
                checks_by_capability
                    .entry(capability.as_str())
                    .or_default()
                    .push(check);
            }
            None => diags.push(warning(
                check.span,
                "dynamic capability string in `check()`; use a string literal".to_string(),
            )),
        }
    }

    for (capability, matching_checks) in &checks_by_capability {
        if !doc
            .capabilities
            .iter()
            .any(|tag| tag.capability == *capability)
        {
            let span = matching_checks[0]
                .capability
                .as_ref()
                .map_or(matching_checks[0].span, |(_, span)| *span);
            diags.push(warning(
                span,
                format!("missing `@capability {capability}` for `check()` call"),
            ));
        }
    }

    for tag in &doc.capabilities {
        let Some(matching_checks) = checks_by_capability.get(tag.capability.as_str()) else {
            diags.push(warning(
                tag.capability_span,
                format!(
                    "extra `@capability {}` has no matching `check()` call",
                    tag.capability
                ),
            ));
            continue;
        };
        for check in matching_checks {
            let Some(payload_keys) = &check.payload_keys else {
                continue;
            };
            validate_payload_keys(&tag.bindings, payload_keys, diags);
        }
    }
}

fn validate_payload_keys(
    bindings: &[DocCapabilityBinding],
    payload_keys: &BTreeMap<String, Span>,
    diags: &mut Vec<Diagnostic>,
) {
    let binding_keys = bindings
        .iter()
        .map(|binding| binding.field.as_str())
        .collect::<BTreeSet<_>>();
    for (key, span) in payload_keys {
        if !binding_keys.contains(key.as_str()) {
            diags.push(warning(
                *span,
                format!("payload key `{key}` missing from `@capability` binding"),
            ));
        }
    }
    for binding in bindings {
        if !payload_keys.contains_key(binding.field.as_str()) {
            diags.push(warning(
                binding.field_span,
                format!(
                    "`@capability` binding key `{}` is missing from `check()` payload",
                    binding.field
                ),
            ));
        }
    }
}

fn collect_checks_in_stmt(
    ta: &TypedAst,
    stmt_id: StmtId,
    security_check: &MangledName,
    out: &mut Vec<SecurityCheck>,
) {
    match &ta.stmt(stmt_id).kind {
        TypedStmtKind::Let { value, .. }
        | TypedStmtKind::Const { value, .. }
        | TypedStmtKind::Expr(value)
        | TypedStmtKind::Throw { value }
        | TypedStmtKind::AssignLocal { value, .. }
        | TypedStmtKind::AssignGlobal { value, .. } => {
            collect_checks_in_expr(ta, *value, security_check, out);
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_checks_in_expr(ta, *condition, security_check, out);
            collect_checks_in_stmt(ta, *then_block, security_check, out);
            if let Some(else_block) = else_block {
                collect_checks_in_stmt(ta, *else_block, security_check, out);
            }
        }
        TypedStmtKind::While { condition, body } | TypedStmtKind::DoWhile { body, condition } => {
            collect_checks_in_expr(ta, *condition, security_check, out);
            collect_checks_in_stmt(ta, *body, security_check, out);
        }
        TypedStmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                collect_checks_in_stmt(ta, *init, security_check, out);
            }
            if let Some(condition) = condition {
                collect_checks_in_expr(ta, *condition, security_check, out);
            }
            if let Some(update) = update {
                collect_checks_in_stmt(ta, *update, security_check, out);
            }
            collect_checks_in_stmt(ta, *body, security_check, out);
        }
        TypedStmtKind::ForOf { iter, body, .. } => {
            collect_checks_in_expr(ta, *iter, security_check, out);
            collect_checks_in_stmt(ta, *body, security_check, out);
        }
        TypedStmtKind::Switch {
            discriminant,
            cases,
            default,
            ..
        } => {
            collect_checks_in_expr(ta, *discriminant, security_check, out);
            for case in cases {
                collect_checks_in_stmt(ta, case.body, security_check, out);
            }
            if let Some(default) = default {
                collect_checks_in_stmt(ta, *default, security_check, out);
            }
        }
        TypedStmtKind::Return(value) => {
            if let Some(value) = value {
                collect_checks_in_expr(ta, *value, security_check, out);
            }
        }
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            collect_checks_in_stmt(ta, *body, security_check, out);
            for clause in catches {
                collect_checks_in_stmt(ta, clause.body, security_check, out);
            }
            if let Some(finally) = finally {
                collect_checks_in_stmt(ta, *finally, security_check, out);
            }
        }
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_checks_in_stmt(ta, *stmt, security_check, out);
            }
        }
        TypedStmtKind::AssignField {
            receiver, value, ..
        } => {
            collect_checks_in_expr(ta, *receiver, security_check, out);
            collect_checks_in_expr(ta, *value, security_check, out);
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            ..
        } => {
            collect_checks_in_expr(ta, *receiver, security_check, out);
            collect_checks_in_expr(ta, *index, security_check, out);
            collect_checks_in_expr(ta, *value, security_check, out);
        }
        TypedStmtKind::NarrowRegion { source, body, .. } => {
            collect_checks_in_expr(ta, *source, security_check, out);
            collect_checks_in_stmt(ta, *body, security_check, out);
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
    }
}

fn collect_checks_in_expr(
    ta: &TypedAst,
    expr_id: ExprId,
    security_check: &MangledName,
    out: &mut Vec<SecurityCheck>,
) {
    let expr = ta.expr(expr_id);
    match &expr.kind {
        TypedExprKind::Call { mangled, args, .. } if mangled == security_check => {
            out.push(security_check_from_args(ta, expr.span, args));
            for arg in args {
                collect_checks_in_expr(ta, *arg, security_check, out);
            }
        }
        TypedExprKind::Call { args, .. }
        | TypedExprKind::McpCall { args, .. }
        | TypedExprKind::SuperCtorCall { args, .. }
        | TypedExprKind::SuperMethodCall { args, .. } => {
            for arg in args {
                collect_checks_in_expr(ta, *arg, security_check, out);
            }
        }
        TypedExprKind::GenericCall { mangled, args, .. } if mangled == security_check => {
            let check_args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
            out.push(security_check_from_args(ta, expr.span, &check_args));
            for arg in args {
                collect_checks_in_expr(ta, arg.expr, security_check, out);
            }
        }
        TypedExprKind::GenericCall { args, .. } => {
            for arg in args {
                collect_checks_in_expr(ta, arg.expr, security_check, out);
            }
        }
        TypedExprKind::CallClosure { callee, args } => {
            collect_checks_in_expr(ta, *callee, security_check, out);
            for arg in args {
                collect_checks_in_expr(ta, *arg, security_check, out);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                collect_checks_in_expr(ta, *arg, security_check, out);
            }
        }
        TypedExprKind::MethodCall { receiver, args, .. } => {
            collect_checks_in_expr(ta, *receiver, security_check, out);
            for arg in args {
                collect_checks_in_expr(ta, *arg, security_check, out);
            }
        }
        TypedExprKind::GenericMethodCall { receiver, args, .. } => {
            collect_checks_in_expr(ta, *receiver, security_check, out);
            for arg in args {
                collect_checks_in_expr(ta, arg.expr, security_check, out);
            }
        }
        TypedExprKind::Binary { lhs, rhs, .. } => {
            collect_checks_in_expr(ta, *lhs, security_check, out);
            collect_checks_in_expr(ta, *rhs, security_check, out);
        }
        TypedExprKind::EffectThen { effect, result } => {
            collect_checks_in_expr(ta, *effect, security_check, out);
            collect_checks_in_expr(ta, *result, security_check, out);
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::FieldAccess {
            receiver: operand, ..
        }
        | TypedExprKind::InterfacePropertyAccess {
            receiver: operand, ..
        }
        | TypedExprKind::TypeofTag { value: operand, .. }
        | TypedExprKind::InstanceOf { value: operand, .. }
        | TypedExprKind::NonNullAssert { value: operand }
        | TypedExprKind::Cast { value: operand, .. } => {
            collect_checks_in_expr(ta, *operand, security_check, out);
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            collect_checks_in_expr(ta, *receiver, security_check, out);
            collect_checks_in_expr(ta, *index, security_check, out);
        }
        TypedExprKind::ObjectLiteral { members, .. } => {
            for member in members {
                collect_checks_in_expr(ta, member.expr_id(), security_check, out);
            }
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for elem in elements {
                collect_checks_in_expr(ta, elem.expr_id(), security_check, out);
            }
        }
        TypedExprKind::TupleLiteral { elements, .. } => {
            for elem in elements {
                collect_checks_in_expr(ta, *elem, security_check, out);
            }
        }
        TypedExprKind::Closure { body, .. } => match body {
            crate::ClosureBody::Expr(expr) => {
                collect_checks_in_expr(ta, *expr, security_check, out);
            }
            crate::ClosureBody::Block(stmt) => {
                collect_checks_in_stmt(ta, *stmt, security_check, out);
            }
        },
        TypedExprKind::Narrowed { source, inner, .. } => {
            collect_checks_in_expr(ta, *source, security_check, out);
            collect_checks_in_expr(ta, *inner, security_check, out);
        }
        TypedExprKind::Ternary { cond, then_, else_ } => {
            collect_checks_in_expr(ta, *cond, security_check, out);
            collect_checks_in_expr(ta, *then_, security_check, out);
            collect_checks_in_expr(ta, *else_, security_check, out);
        }
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            collect_checks_in_expr(ta, *lhs, security_check, out);
            collect_checks_in_expr(ta, *rhs, security_check, out);
        }
        TypedExprKind::OptionalChain { base, parts } => {
            collect_checks_in_expr(ta, *base, security_check, out);
            for part in parts {
                match part {
                    TypedChainPart::Index { idx, .. } => {
                        collect_checks_in_expr(ta, *idx, security_check, out);
                    }
                    TypedChainPart::Call { args, .. } | TypedChainPart::MethodCall { args, .. } => {
                        for arg in args {
                            collect_checks_in_expr(ta, *arg, security_check, out);
                        }
                    }
                    TypedChainPart::Field { .. }
                    | TypedChainPart::InterfaceProperty { .. }
                    | TypedChainPart::NonNull { .. } => {}
                }
            }
        }
        TypedExprKind::PostfixUnary { target, .. } => match target {
            crate::PostfixTarget::Field { receiver, .. } => {
                collect_checks_in_expr(ta, *receiver, security_check, out);
            }
            crate::PostfixTarget::Index {
                receiver, index, ..
            } => {
                collect_checks_in_expr(ta, *receiver, security_check, out);
                collect_checks_in_expr(ta, *index, security_check, out);
            }
            crate::PostfixTarget::Local { .. } | crate::PostfixTarget::Global { .. } => {}
        },
        TypedExprKind::Number(_)
        | TypedExprKind::BigInt(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Boolean(_)
        | TypedExprKind::Null
        | TypedExprKind::This
        | TypedExprKind::Regex { .. }
        | TypedExprKind::LocalRef { .. }
        | TypedExprKind::LocalNarrowRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::NumberEnumMember { .. }
        | TypedExprKind::StringEnumMember { .. } => {}
    }
}

fn security_check_from_args(ta: &TypedAst, span: Span, args: &[ExprId]) -> SecurityCheck {
    let capability = args.first().and_then(|id| match &ta.expr(*id).kind {
        TypedExprKind::String(value) => Some((value.clone(), ta.expr(*id).span)),
        _ => None,
    });
    let payload_keys = args.get(1).and_then(|id| match &ta.expr(*id).kind {
        TypedExprKind::ObjectLiteral { fields, .. } => Some(
            fields
                .iter()
                .map(|field| (field.name.name.clone(), field.name.span))
                .collect::<BTreeMap<_, _>>(),
        ),
        _ => None,
    });
    SecurityCheck {
        span,
        capability,
        payload_keys,
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
    use crate::{Asi, Diagnostic, TokenKind, infer, parse};

    fn diagnostics(source: &str) -> Vec<Diagnostic> {
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
        let mut diags = asi.into_diagnostics();
        let (ast, parse_diags) = parse(source, tokens, crate::FileId(0));
        diags.extend(parse_diags);
        assert!(diags.is_empty(), "{diags:?}");
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let stdlib = crate::stdlib::stdlib_package_declarations();
        let mut package_refs = Vec::with_capacity(1 + host_defs.len() + stdlib.len());
        package_refs.extend(prelude_defs.iter());
        package_refs.extend(host_defs.iter());
        package_refs.extend(stdlib.iter());
        let (ta, infer_diags) = infer(source, "main", &ast, &package_refs);
        diags.extend(infer_diags);
        diags.extend(crate::check(&ta));
        diags
    }

    fn messages(source: &str) -> Vec<String> {
        diagnostics(source)
            .into_iter()
            .map(|diag| diag.message)
            .collect()
    }

    #[test]
    fn missing_capability_tag_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             function f(): void { check(\"x/op\", { a: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("missing `@capability x/op`")),
            "{messages:?}"
        );
    }

    #[test]
    fn extra_capability_tag_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a } */\n\
             function f(a: number): void { }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("extra `@capability x/op`")),
            "{messages:?}"
        );
    }

    #[test]
    fn payload_key_missing_from_binding_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a } */\n\
             function f(a: number, b: number): void { check(\"x/op\", { a, b }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("payload key `b` missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn binding_key_missing_from_payload_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { a, c } */\n\
             function f(a: number): void { check(\"x/op\", { a }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("binding key `c` is missing")),
            "{messages:?}"
        );
    }

    #[test]
    fn unknown_parameter_binding_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /** @capability x/op { foo: $bar } */\n\
             function f(): void { check(\"x/op\", { foo: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unknown parameter binding `$bar`")),
            "{messages:?}"
        );
    }

    #[test]
    fn dynamic_capability_string_warns() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             function f(): void { let cap = \"x/op\"; check(cap, { a: 1 }); }\n\
             function main(): void { }\n",
        );
        assert!(
            messages
                .iter()
                .any(|m| m.contains("dynamic capability string")),
            "{messages:?}"
        );
    }

    #[test]
    fn documented_multiple_checks_do_not_warn() {
        let messages = messages(
            "import { check } from \"submilli:security\";\n\
             /**\n\
              * @param a A.\n\
              * @param b B.\n\
              * @capability x/a { a }\n\
              * @capability x/b { b }\n\
              */\n\
             function f(a: number, b: number): void {\n\
               check(\"x/a\", { a });\n\
               check(\"x/b\", { b });\n\
             }\n\
             function main(): void { }\n",
        );
        assert!(
            !messages
                .iter()
                .any(|m| m.contains("@capability") || m.contains("capability string")),
            "{messages:?}"
        );
    }
}
