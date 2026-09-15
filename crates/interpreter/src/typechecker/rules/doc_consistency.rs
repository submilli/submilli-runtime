use std::collections::BTreeMap;

use crate::{
    Diagnostic, DocComment, Ident, Severity, Span, Type, TypedAst, TypedInterfaceMember,
    TypedParam, TypedTypeDecl,
};

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) {
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
            TypedTypeDecl::Class(_)
            | TypedTypeDecl::NumberEnum(_)
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
        by_name.insert(p.name.name.as_str(), (i, &p.name));
    }

    let mut seen: BTreeMap<&str, &crate::DocParam> = BTreeMap::new();
    let mut doc_indices: Vec<(usize, &crate::DocParam)> = Vec::new();
    for dp in &doc.params {
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
        let (prev_idx, _) = w[0];
        let (cur_idx, cur_dp) = w[1];
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

    for p in params {
        if !seen.contains_key(p.name.name.as_str()) {
            diags.push(warning(
                p.name.span,
                format!(
                    "{} `{}` is undocumented (missing `@param {}`)",
                    param_label, p.name.name, p.name.name
                ),
            ));
        }
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
