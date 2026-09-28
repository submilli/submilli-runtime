use crate::compiler_error::CompilerFailure;

use crate::{
    MangledName, Span, Type, TypeSymbol, ValueKind, ValueSymbol,
    ast::{Ast, ExprId, ExprKind, Ident},
    typed_ast::TypedExprKind,
};

use super::Inferer;

/// Owned so callers can drop the `&NamespaceSymbol` borrow before doing `&mut self` work.
pub(super) enum ChainResolution {
    Namespace {
        mangled_prefix: MangledName,
    },
    Value {
        value: ValueSymbol,
        index: usize,
    },
    Type {
        type_sym: TypeSymbol,
        index: usize,
    },
    NotFound {
        index: usize,
        parent_exports: Vec<String>,
    },
}

pub(super) fn extract_chain(
    ast: &Ast,
    expr: ExprId,
) -> Result<Option<(Ident, Vec<Ident>)>, CompilerFailure> {
    let mut segments: Vec<Ident> = Vec::new();
    let mut current = expr;
    loop {
        match &ast.try_expr(current).map_err(super::arena_failure)?.kind {
            ExprKind::FieldAccess { receiver, name } => {
                segments.push(name.clone());
                current = *receiver;
            }
            // `Math["floor"]` names the same member as `Math.floor`.
            ExprKind::IndexAccess { receiver, index } => {
                segments.push(match super::expr::string_key_name(ast, *index)? {
                    Some(value) => value,
                    None => return Ok(None),
                });
                current = *receiver;
            }
            ExprKind::Identifier(ident) => {
                segments.reverse();
                return Ok(Some((ident.clone(), segments)));
            }
            _ => return Ok(None),
        }
    }
}

fn path_string(root: &Ident, segments: &[Ident]) -> String {
    let mut out = root.name.clone();
    for seg in segments {
        out.push('.');
        out.push_str(&seg.name);
    }
    out
}

impl<'a> Inferer<'a> {
    fn resolve_namespace_chain(&self, root: &Ident, segments: &[Ident]) -> ChainResolution {
        let ns = self
            .namespace_symbols
            .get(&root.name)
            .cloned()
            .expect("caller checked namespace_symbols.contains_key");
        let mut current = ns;
        for (idx, seg) in segments.iter().enumerate() {
            if let Some(next) = current.child(&seg.name) {
                current = next;
                continue;
            }
            if let Some(value) = current.value(&seg.name) {
                return ChainResolution::Value {
                    value: value.clone(),
                    index: idx,
                };
            }
            if let Some(type_sym) = current.type_symbol(&seg.name) {
                return ChainResolution::Type {
                    type_sym: type_sym.clone(),
                    index: idx,
                };
            }
            return ChainResolution::NotFound {
                index: idx,
                parent_exports: current.exports(),
            };
        }
        ChainResolution::Namespace {
            mangled_prefix: current.mangled_prefix(),
        }
    }

    pub(super) fn infer_namespace_symbol_call(
        &mut self,
        root: Ident,
        segments: Vec<Ident>,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        debug_assert!(!segments.is_empty(), "namespace call needs ≥1 segment");
        let resolved = self.resolve_namespace_chain(&root, &segments);
        Ok(match resolved {
            ChainResolution::Value { value, index } => {
                let remaining = segments.len() - index - 1;
                if remaining == 0 {
                    self.dispatch_namespace_value_call(
                        &root, &segments, value, type_args, args, expected, span,
                    )?
                } else if remaining == 1 {
                    let method = segments.last().unwrap().clone();
                    self.dispatch_constructor_static_call(
                        &root, &segments, index, value, method, type_args, args, expected, span,
                    )?
                } else {
                    let prefix = path_string(&root, &segments[..=index]);
                    self.error(span, format!("`{prefix}` has no nested member access"));
                    error_call(value.mangled_name.clone())
                }
            }
            ChainResolution::Namespace { mangled_prefix } => {
                let full = path_string(&root, &segments);
                self.error(span, format!("`{full}` is a namespace, not a callable"));
                error_call(mangled_prefix)
            }
            ChainResolution::Type { type_sym, index } => {
                let prefix = path_string(&root, &segments[..=index]);
                self.error_with_help(
                    span,
                    format!("`{prefix}` is a type, not a callable"),
                    vec![format!(
                        "to construct or call a static method, use the matching constructor binding (e.g. `{}.from(…)`)",
                        prefix,
                    )],
                );
                error_call(type_sym.mangled_name)
            }
            ChainResolution::NotFound {
                index,
                parent_exports,
            } => {
                let placeholder = self
                    .namespace_symbols
                    .get(&root.name)
                    .map(super::module_symbols::NamespaceSymbolSet::mangled_prefix)
                    .expect("caller checked namespace_symbols.contains_key");
                self.namespace_member_not_found_error(
                    &root,
                    &segments,
                    index,
                    parent_exports,
                    span,
                );
                error_call(placeholder)
            }
        })
    }

    pub(super) fn infer_namespace_symbol_field_access(
        &mut self,
        root: Ident,
        segments: Vec<Ident>,
        span: Span,
    ) -> (TypedExprKind, Type) {
        debug_assert!(
            !segments.is_empty(),
            "namespace field access needs ≥1 segment"
        );
        let resolved = self.resolve_namespace_chain(&root, &segments);
        match resolved {
            ChainResolution::Value { value, index } => {
                let remaining = segments.len() - index - 1;
                if remaining == 0 {
                    match &value.kind {
                        ValueKind::Const { ty, .. } | ValueKind::Let { ty, .. } => (
                            TypedExprKind::GlobalRef {
                                mangled: value.mangled_name.clone(),
                                name: segments.last().unwrap().clone(),
                            },
                            ty.clone(),
                        ),
                        ValueKind::Function { .. } => {
                            let full = path_string(&root, &segments);
                            self.error_with_help(
                                span,
                                format!(
                                    "namespace function `{full}` is only valid in call position",
                                ),
                                vec![format!("call it directly: `{}(…)`", full)],
                            );
                            error_local_ref(segments.last().unwrap().clone())
                        }
                    }
                } else if remaining == 1
                    && let Some(fn_ref) = self.namespace_method_as_value(&value, &segments)
                {
                    fn_ref
                } else {
                    let full = path_string(&root, &segments);
                    self.error_with_help(
                        span,
                        format!("namespaced method `{full}` is only valid in call position",),
                        vec![format!("call it directly: `{}(…)`", full)],
                    );
                    error_local_ref(segments.last().unwrap().clone())
                }
            }
            ChainResolution::Namespace { .. } => {
                let full = path_string(&root, &segments);
                self.error_with_help(
                    span,
                    format!("namespace `{full}` cannot be used as a value"),
                    vec![format!(
                        "access a member: `{}.<name>` (or call: `{}.<name>(…)`)",
                        full, full,
                    )],
                );
                error_local_ref(segments.last().unwrap().clone())
            }
            ChainResolution::Type { .. } => {
                let full = path_string(&root, &segments);
                self.error(span, format!("type `{full}` is not a value"));
                error_local_ref(segments.last().unwrap().clone())
            }
            ChainResolution::NotFound {
                index,
                parent_exports,
            } => {
                self.namespace_member_not_found_error(
                    &root,
                    &segments,
                    index,
                    parent_exports,
                    span,
                );
                error_local_ref(segments.last().unwrap().clone())
            }
        }
    }

    pub(super) fn reject_bare_namespace_symbol(
        &mut self,
        ident: Ident,
        span: Span,
    ) -> (TypedExprKind, Type) {
        self.error_with_help(
            span,
            format!("namespace `{}` cannot be used as a value", ident.name),
            vec![format!(
                "access a member via `{}.<name>` (or call: `{}.<name>(…)`)",
                ident.name, ident.name,
            )],
        );
        error_local_ref(ident)
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_namespace_value_call(
        &mut self,
        root: &Ident,
        segments: &[Ident],
        value: ValueSymbol,
        _type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        _expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let mangled = value.mangled_name.clone();
        let (params, ret) = match &value.kind {
            ValueKind::Function {
                generics,
                params,
                ret,
                ..
            } => {
                if !generics.is_empty() {
                    self.error(
                        span,
                        format!(
                            "generic namespace functions are not supported (`{}` is generic)",
                            path_string(root, segments),
                        ),
                    );
                    return Ok(error_call(mangled));
                }
                (params.clone(), ret.clone())
            }
            ValueKind::Let { .. } | ValueKind::Const { .. } => {
                self.error(
                    span,
                    format!("`{}` is not callable", path_string(root, segments),),
                );
                return Ok(error_call(mangled));
            }
        };
        let has_rest = params.last().is_some_and(|p| p.rest);
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        let max_args = if has_rest { usize::MAX } else { params.len() };
        let min_args = params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count();
        let arity_ok = args.len() >= min_args && args.len() <= max_args;
        if !arity_ok {
            let path = path_string(root, segments);
            let msg = if has_rest {
                format!(
                    "function `{path}` expects at least {min_args} argument(s), got {}",
                    args.len(),
                )
            } else if min_args == max_args {
                format!(
                    "function `{path}` expects {max_args} argument(s), got {}",
                    args.len(),
                )
            } else {
                format!(
                    "function `{path}` expects {min_args}-{max_args} argument(s), got {}",
                    args.len(),
                )
            };
            self.error(span, msg);
        }
        let rest_elem_ty: Option<Type> = if has_rest {
            params.last().and_then(|p| match &p.ty {
                Type::Array(inner) => Some((**inner).clone()),
                _ => None,
            })
        } else {
            None
        };
        let mut typed_args: Vec<ExprId> = Vec::with_capacity(args.len().max(params.len()));
        for (i, arg) in args.iter().enumerate() {
            let hint_owned: Option<Type> = if has_rest && i >= fixed_count {
                rest_elem_ty.clone()
            } else {
                params.get(i).map(|p| p.ty.clone())
            };
            let (typed_arg, _) = self.infer_expr(*arg, hint_owned.as_ref())?;
            typed_args.push(typed_arg);
        }
        if arity_ok {
            self.fill_omitted_defaults(&params, args.len(), span, &mut typed_args);
        }
        if arity_ok && let Some(elem_ty) = rest_elem_ty {
            self.pack_rest_tail(fixed_count, elem_ty, span, &mut typed_args);
        }
        let type_predicate = match &value.kind {
            ValueKind::Function { type_predicate, .. } => type_predicate.clone().map(Box::new),
            _ => None,
        };
        // Namespace *symbols* are the always-in-scope prelude namespaces (`Math`,
        // `Temporal`) — never an `@mcp/<server>` package (those bind as namespace
        // *bindings* → `infer_namespace_call`), so this is always a plain `Call`.
        Ok((
            TypedExprKind::Call {
                mangled,
                args: typed_args,
                type_predicate,
            },
            ret,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_constructor_static_call(
        &mut self,
        root: &Ident,
        segments: &[Ident],
        ctor_index: usize,
        ctor_value: ValueSymbol,
        method: Ident,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let recv_ty = match &ctor_value.kind {
            ValueKind::Const { ty, .. } | ValueKind::Let { ty, .. } => ty.clone(),
            ValueKind::Function { .. } => {
                let prefix = path_string(root, &segments[..=ctor_index]);
                self.error(
                    span,
                    format!("cannot call static method on function value `{prefix}`",),
                );
                return Ok(error_call(ctor_value.mangled_name));
            }
        };
        let Some((sig, interface_bindings, iface_mangled, _dispatch)) =
            self.find_method(&recv_ty, &method.name)
        else {
            let prefix = path_string(root, &segments[..=ctor_index]);
            self.error(
                method.span,
                format!("no static method `{}` on `{}`", method.name, prefix),
            );
            return Ok(error_call(ctor_value.mangled_name));
        };
        let receiver_expr = self.synthetic_ctor_receiver(&ctor_value);
        Ok(
            if interface_bindings.is_empty() && sig.generics.is_empty() {
                self.infer_method_call(
                    receiver_expr,
                    iface_mangled,
                    method,
                    sig,
                    type_args,
                    args,
                    span,
                )?
            } else {
                self.infer_generic_method_call(
                    receiver_expr,
                    iface_mangled,
                    method,
                    sig,
                    interface_bindings,
                    type_args,
                    args,
                    expected,
                    span,
                )?
            },
        )
    }

    /// A namespace constructor static method referenced as a value, e.g.
    /// `Temporal.PlainDate.compare`. Materializes a first-class function value via
    /// the same `FunctionRef` → adapter path as a top-level function, but only when
    /// the member resolves to a non-generic `Static`-dispatch method: its wrapper
    /// takes exactly the declared params (the receiver is dropped), matching the
    /// adapter's call convention. Returns `None` for anything else (generic
    /// methods, instance/`VTable` dispatch, free functions) so the caller falls
    /// back to its error.
    fn namespace_method_as_value(
        &self,
        value: &ValueSymbol,
        segments: &[Ident],
    ) -> Option<(TypedExprKind, Type)> {
        let recv_ty = match &value.kind {
            ValueKind::Const { ty, .. } | ValueKind::Let { ty, .. } => ty,
            ValueKind::Function { .. } => return None,
        };
        let method = segments.last().unwrap();
        let (sig, bindings, iface_mangled, dispatch) = self.find_method(recv_ty, &method.name)?;
        if dispatch != crate::Dispatch::Static || !bindings.is_empty() || !sig.generics.is_empty() {
            return None;
        }
        let fn_ty = Type::Function {
            params: sig.params.iter().map(|p| p.ty.clone()).collect(),
            ret: Box::new(sig.ret.clone()),
            predicate: sig.predicate.clone().map(Box::new),
            has_rest: sig.params.last().is_some_and(|p| p.rest),
        };
        Some((
            TypedExprKind::FunctionRef {
                mangled: crate::mangle::extend(&iface_mangled, &method.name),
                name: method.clone(),
            },
            fn_ty,
        ))
    }

    fn synthetic_ctor_receiver(&mut self, value: &ValueSymbol) -> ExprId {
        let ty = match &value.kind {
            ValueKind::Const { ty, .. } | ValueKind::Let { ty, .. } => ty.clone(),
            ValueKind::Function { .. } => Type::Error,
        };
        let kind = TypedExprKind::GlobalRef {
            mangled: value.mangled_name.clone(),
            name: Ident {
                name: value.name.clone(),
                span: value.declaration_span,
            },
        };
        self.typed_ast.push_expr(crate::typed_ast::TypedExpr {
            kind,
            ty,
            span: value.declaration_span,
        })
    }

    fn namespace_member_not_found_error(
        &mut self,
        root: &Ident,
        segments: &[Ident],
        index: usize,
        parent_exports: Vec<String>,
        span: Span,
    ) {
        let parent_path = if index == 0 {
            root.name.clone()
        } else {
            path_string(root, &segments[..index])
        };
        let missing = &segments[index];
        let help = if parent_exports.is_empty() {
            Vec::new()
        } else {
            vec![format!(
                "exports: {}",
                parent_exports
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )]
        };
        self.error_with_help(
            span,
            format!(
                "namespace `{}` does not export `{}`",
                parent_path, missing.name,
            ),
            help,
        );
    }
}

fn error_call(mangled: MangledName) -> (TypedExprKind, Type) {
    (
        TypedExprKind::Call {
            mangled,
            args: Vec::new(),
            type_predicate: None,
        },
        Type::Error,
    )
}

fn error_local_ref(ident: Ident) -> (TypedExprKind, Type) {
    (
        TypedExprKind::LocalRef {
            ident,
            boxed: false,
        },
        Type::Error,
    )
}
