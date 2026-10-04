use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Diagnostic, DocComment, Ident, Severity, Span, Type, TypedAst, TypedInterfaceMember,
    TypedParam, TypedTypeDecl,
};

pub(super) fn run(
    ta: &TypedAst,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for f in &ta.functions {
        if let Some(doc) = &f.doc {
            validate_callable(
                doc,
                f.name.span,
                &f.params,
                &f.return_type,
                "parameter",
                diags,
            );
        }
    }

    for ty_decl in &ta.types {
        let iface = match ty_decl {
            TypedTypeDecl::Interface(iface) => iface,
            TypedTypeDecl::Class(class) => {
                validate_class(class, diags);
                continue;
            }
            TypedTypeDecl::NumberEnum(_)
            | TypedTypeDecl::StringEnum(_)
            | TypedTypeDecl::Alias(_) => continue,
        };
        for member in &iface.members {
            match member {
                TypedInterfaceMember::Method {
                    name,
                    params,
                    return_type,
                    doc: Some(doc),
                    ..
                } => {
                    validate_callable(
                        doc,
                        name.span,
                        params,
                        return_type,
                        "method parameter",
                        diags,
                    );
                }
                TypedInterfaceMember::Property {
                    name,
                    doc: Some(doc),
                    ..
                } => {
                    validate_property(doc, name.span, diags);
                }
                _ => {}
            }
        }
    }
    validate_global_arrows(ta, diags)?;
    Ok(())
}

fn validate_class(class: &crate::TypedClassDecl, diags: &mut Vec<Diagnostic>) {
    for method in &class.methods {
        if let Some(doc) = &method.doc {
            validate_callable(
                doc,
                method.name.span,
                &method.params,
                &method.return_type,
                "method parameter",
                diags,
            );
        }
    }
    if let Some(constructor) = &class.constructor
        && let Some(doc) = &constructor.doc
    {
        validate_callable(
            doc,
            constructor.span,
            &constructor.params,
            &Type::Void,
            "constructor parameter",
            diags,
        );
    }
}

fn validate_global_arrows(
    ta: &TypedAst,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let globals: BTreeMap<_, _> = ta
        .globals
        .iter()
        .filter(|global| global.kind == crate::GlobalKind::Const)
        .filter_map(|global| {
            global
                .doc
                .as_ref()
                .map(|doc| (&global.mangled_name, (global, doc)))
        })
        .collect();
    for statement in &ta.top_level_statements {
        let crate::TypedStmtKind::AssignGlobal { mangled, value, .. } = &ta
            .try_stmt(*statement)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        else {
            continue;
        };
        let Some((global, doc)) = globals.get(mangled) else {
            continue;
        };
        let initializer = arrow_initializer(ta, *value)?;
        let crate::TypedExprKind::Closure {
            params,
            return_type,
            ..
        } = &initializer.kind
        else {
            continue;
        };
        validate_callable(
            doc,
            global.name.span,
            params,
            return_type,
            "parameter",
            diags,
        );
    }
    Ok(())
}

fn arrow_initializer(
    ta: &TypedAst,
    mut id: crate::ExprId,
) -> Result<&crate::TypedExpr, crate::compiler_error::CompilerFailure> {
    for _ in 0..ta.exprs_len() {
        let expression = ta.try_expr(id).map_err(crate::typechecker::arena_failure)?;
        match &expression.kind {
            crate::TypedExprKind::Cast { value, .. }
            | crate::TypedExprKind::NonNullAssert { value } => id = *value,
            _ => return Ok(expression),
        }
    }
    Err(crate::compiler_error::CompilerFailure::Internal {
        stage: crate::compiler_error::CompilerStage::Infer,
        span: None,
        message: "cycle in documented arrow initializer".to_string(),
    })
}

fn validate_callable(
    doc: &DocComment,
    name_span: Span,
    params: &[TypedParam],
    return_type: &Type,
    param_label: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let mut by_name: BTreeMap<&str, (usize, &Ident)> = BTreeMap::new();
    for (i, p) in params.iter().enumerate() {
        if !is_destructured(p) {
            by_name.insert(p.name.name.as_str(), (i, &p.name));
        }
    }

    let mut seen: BTreeMap<&str, &crate::DocParam> = BTreeMap::new();
    let mut doc_indices: Vec<(usize, &crate::DocParam)> = Vec::new();
    for (tag_index, dp) in doc
        .params
        .iter()
        .filter(|p| !p.name.contains('.'))
        .enumerate()
    {
        if let Some(prev) = seen.get(dp.name.as_str()) {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                span: dp.tag_span,
                message: format!("duplicate `@param {}`", dp.name),
                help: vec![],
                notes: vec![(prev.tag_span, "first documented here".to_string())],
            });
            continue;
        }
        seen.insert(dp.name.as_str(), dp);
        match by_name.get(dp.name.as_str()) {
            Some((idx, _)) => doc_indices.push((*idx, dp)),
            // A destructured parameter has no name to document, so, as in
            // TypeScript, the n-th `@param` documents the n-th parameter.
            None if params.get(tag_index).is_some_and(is_destructured) => {
                doc_indices.push((tag_index, dp));
            }
            None => diags.push(warning(
                dp.name_span,
                format!(
                    "`@param {}` does not match any {} of this function",
                    dp.name, param_label
                ),
            )),
        }
    }

    for w in doc_indices.windows(2) {
        let [(prev_idx, _), (cur_idx, cur_dp)] = w else {
            continue;
        };
        if cur_idx < prev_idx {
            diags.push(warning(
                cur_dp.tag_span,
                format!(
                    "`@param {}` is out of order — appears before earlier params in the signature",
                    cur_dp.name
                ),
            ));
        }
    }

    let documented_positions: BTreeSet<usize> = doc_indices.iter().map(|(idx, _)| *idx).collect();
    for (idx, p) in params.iter().enumerate() {
        let message = if is_destructured(p) {
            if documented_positions.contains(&idx) {
                continue;
            }
            format!(
                "destructured {} {} is undocumented (add a `@param` in its position)",
                param_label,
                idx + 1
            )
        } else {
            // Matched by name, so a repeated parameter name (itself an error)
            // is not also reported as undocumented.
            if seen.contains_key(p.name.name.as_str()) {
                continue;
            }
            format!(
                "{} `{}` is undocumented (missing `@param {}`)",
                param_label, p.name.name, p.name.name
            )
        };
        diags.push(warning(p.name.span, message));
    }

    let is_void = return_type.is_void();
    match (&doc.returns, is_void) {
        (Some(r), true) => diags.push(warning(
            r.tag_span,
            "`@returns` on a `void`-returning function".to_string(),
        )),
        (None, false) => diags.push(warning(
            name_span,
            "missing `@returns` on a non-`void`-returning function".to_string(),
        )),
        _ => {}
    }

    // surfaced so LLMs can self-correct tag typos (e.g. @parm → @param)
    for u in &doc.unknown_tags {
        diags.push(warning(
            u.tag_span,
            format!("unknown JSDoc tag `@{}`", u.name),
        ));
    }
}

fn is_destructured(param: &TypedParam) -> bool {
    crate::lower_patterns::is_pattern_param(&param.name.name)
}

fn validate_property(doc: &DocComment, name_span: Span, diags: &mut Vec<Diagnostic>) {
    for p in &doc.params {
        diags.push(warning(
            p.tag_span,
            format!(
                "`@param {}` on a property — properties have no parameters",
                p.name
            ),
        ));
    }
    if let Some(r) = &doc.returns {
        diags.push(warning(
            r.tag_span,
            "`@returns` on a property — use the property's type annotation instead".to_string(),
        ));
    }
    let _ = name_span;
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
    use crate::{DocComment, DocParam, DocReturns, Ident, TypedParam};

    fn span() -> Span {
        Span::at(crate::FileId(0))
    }

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: span(),
        }
    }

    fn param(name: &str) -> TypedParam {
        TypedParam {
            name: ident(name),
            ty: Type::Number,
            boxed: false,
            rest: false,
            default: None,
        }
    }

    fn make_doc(params: &[&str], has_returns: bool) -> DocComment {
        DocComment {
            span: span(),
            summary: String::new(),
            params: params
                .iter()
                .map(|n| DocParam {
                    tag_span: span(),
                    name: n.to_string(),
                    name_span: span(),
                    description: String::new(),
                })
                .collect(),
            returns: has_returns.then(|| DocReturns {
                tag_span: span(),
                description: String::new(),
            }),
            capabilities: vec![],
            throws: vec![],
            deprecated: None,
            examples: vec![],
            unknown_tags: vec![],
        }
    }

    #[test]
    fn wrapped_arrow_docs_are_checked() {
        let diagnostics = super::super::test_util::run(
            r#"
            /** Cast. */
            const cast = ((n: number): number => n) as (n: number) => number;
            /** Non-null. */
            const nonnull = (((n: number): number => n) as (n: number) => number)!;
            /** Annotation. */
            const annotated: (n: number) => number = (n: number): number => n;
        "#,
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.message.contains("`n` is undocumented"))
                .count(),
            3,
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| d.message.contains("missing `@returns`"))
                .count(),
            3,
            "{diagnostics:?}"
        );
    }

    #[test]
    fn class_and_arrow_docs_are_checked() {
        let diagnostics = super::super::test_util::run(
            r#"
            class Counter {
                /** Construct. */
                constructor(n: number) {}
                /** Run. */
                run(n: number): number { return n; }
                /** Static. */
                static run(n: number): number { return n; }
            }
            /** Arrow. */
            const arrow = (n: number): number => n;
        "#,
        );
        let undocumented = diagnostics
            .iter()
            .filter(|d| d.message.contains("`n` is undocumented"))
            .count();
        assert_eq!(undocumented, 4, "{diagnostics:?}");
        let returns = diagnostics
            .iter()
            .filter(|d| d.message.contains("missing `@returns`"))
            .count();
        assert_eq!(returns, 3, "{diagnostics:?}");
    }

    #[test]
    fn dotted_tags_do_not_shift_destructured_positions() {
        let mut diagnostics = Vec::new();
        validate_callable(
            &make_doc(&["opts", "opts.a", "pair"], true),
            span(),
            &[param("opts"), param("#pattern_p_0")],
            &Type::Number,
            "parameter",
            &mut diagnostics,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn matching_params_no_warnings() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a", "b"], true);
        let params = vec![param("a"), param("b")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatched_param_name_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&["x"], true);
        let params = vec![param("a")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        assert_eq!(diags.len(), 2);
        assert!(diags.iter().any(|d| d.message.contains("`@param x`")));
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`a` is undocumented"))
        );
    }

    #[test]
    fn param_in_position_documents_destructured_param() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a", "options"], true);
        let params = vec![param("a"), param("#pattern_p_0")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn undocumented_destructured_param_warns_by_position() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a"], true);
        let params = vec![param("a"), param("#pattern_p_0")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        let messages: Vec<_> = diags.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            ["destructured parameter 2 is undocumented (add a `@param` in its position)"]
        );
    }

    #[test]
    fn param_in_position_documents_unlowered_destructured_param() {
        let mut diags = Vec::new();
        let doc = make_doc(&["options"], true);
        validate_callable(
            &doc,
            span(),
            &[param("")],
            &Type::Number,
            "method parameter",
            &mut diags,
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn duplicate_tag_counts_toward_destructured_position() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a", "a", "options"], true);
        let params = vec![param("a"), param("#pattern_p_0")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        let messages: Vec<_> = diags.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "duplicate `@param a`",
                "`@param options` does not match any parameter of this function",
                "destructured parameter 2 is undocumented (add a `@param` in its position)",
            ]
        );
    }

    #[test]
    fn repeated_param_name_is_documented_once_for_both() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a"], false);
        let params = vec![param("a"), param("a")];
        validate_callable(&doc, span(), &params, &Type::Void, "parameter", &mut diags);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn duplicate_param_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&["a", "a"], true);
        let params = vec![param("a")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        assert!(diags.iter().any(|d| d.message.contains("duplicate")));
    }

    #[test]
    fn order_mismatch_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&["b", "a"], true);
        let params = vec![param("a"), param("b")];
        validate_callable(
            &doc,
            span(),
            &params,
            &Type::Number,
            "parameter",
            &mut diags,
        );
        assert!(diags.iter().any(|d| d.message.contains("out of order")));
    }

    #[test]
    fn returns_on_void_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&[], true);
        validate_callable(&doc, span(), &[], &Type::Void, "parameter", &mut diags);
        assert!(diags.iter().any(|d| d.message.contains("on a `void`")));
    }

    #[test]
    fn missing_returns_on_non_void_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&[], false);
        validate_callable(&doc, span(), &[], &Type::Number, "parameter", &mut diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("missing `@returns`"))
        );
    }

    #[test]
    fn unknown_tag_warns() {
        let mut diags = Vec::new();
        let mut doc = make_doc(&[], true);
        doc.unknown_tags.push(crate::DocUnknownTag {
            tag_span: span(),
            name: "parm".to_string(),
            text: String::new(),
        });
        validate_callable(&doc, span(), &[], &Type::Number, "parameter", &mut diags);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unknown JSDoc tag"))
        );
    }

    #[test]
    fn property_doc_with_param_warns() {
        let mut diags = Vec::new();
        let doc = make_doc(&["x"], false);
        validate_property(&doc, span(), &mut diags);
        assert!(diags.iter().any(|d| d.message.contains("on a property")));
    }
}
