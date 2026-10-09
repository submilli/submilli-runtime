//! Source-facing checks for the callable slot limit. Codegen keeps its own
//! checked conversion because callers can supply typed IR directly.

use std::collections::BTreeSet;

use crate::compiler_error::CompilerFailure;
use crate::compiler_limits::checked_closure_arity;
use crate::{ParamDecl, Span, Type, TypeKind};

use super::Inferer;

impl Inferer<'_> {
    pub(super) fn check_parameter_arity(
        &mut self,
        params: &[ParamDecl],
    ) -> Result<(), CompilerFailure> {
        if checked_closure_arity(params.len()).is_ok() {
            return Ok(());
        }
        if let (Some(first), Some(last)) = (params.first(), params.last()) {
            let span = first.name.span.merge(last.name.span).map_err(|error| {
                error.into_compiler_failure(crate::compiler_error::CompilerStage::Infer)
            })?;
            self.report_closure_arity(params.len(), span, None);
        }
        Ok(())
    }

    pub(super) fn report_closure_arity(&mut self, arity: usize, span: Span, context: Option<&str>) {
        let Err(error) = checked_closure_arity(arity) else {
            return;
        };
        let message = error.to_string();
        // Signature inference revisits annotations during body inference. An
        // enclosing expression may also carry the same offending callable.
        if let Some(diagnostic) = self.diagnostics.iter_mut().find(|diagnostic| {
            diagnostic.message == message
                && diagnostic.span.file == span.file
                && diagnostic.span.start <= span.end
                && span.start <= diagnostic.span.end
        }) {
            if let Some(context) = context {
                let help = format!("required by {context}");
                if !diagnostic.help.contains(&help) {
                    diagnostic.help.push(help);
                }
            }
            return;
        }
        let mut help = vec!["group arguments into an object or a rest parameter".into()];
        if let Some(context) = context {
            help.push(format!("required by {context}"));
        }
        self.error_with_help(span, message, help);
    }

    pub(super) fn check_expression_arity(
        &mut self,
        kind: &crate::TypedExprKind,
        ty: &Type,
        span: Span,
    ) {
        use crate::TypedExprKind;
        let context = match kind {
            TypedExprKind::FunctionRef { mangled, .. }
            | TypedExprKind::GlobalRef { mangled, .. } => Some(format!("`{mangled}`")),
            TypedExprKind::Call { mangled, .. } | TypedExprKind::GenericCall { mangled, .. } => {
                self.check_imported_call_arity(mangled, span);
                Some(format!("`{mangled}`"))
            }
            TypedExprKind::MethodCall { iface, name, .. }
            | TypedExprKind::GenericMethodCall { iface, name, .. } => {
                self.check_method_arity(iface, &name.name, span);
                Some(format!("`{iface}.{}`", name.name))
            }
            _ => None,
        };
        self.check_callable_type_in_context(ty, span, context);
    }

    fn check_imported_call_arity(&mut self, mangled: &crate::MangledName, span: Span) {
        // Raw host functions do not use the closure ABI. User-package functions
        // carry physical signatures; only those receive the declared-slot cap.
        let arity = self.packages_by_name.values().find_map(|package| {
            let signature = package.runtime_functions.get(mangled)?;
            let is_constructor = package
                .types
                .values()
                .chain(package.runtime_types.values())
                .any(|symbol| {
                    matches!(symbol.kind, TypeKind::Class { .. })
                        && crate::mangle::extend(&symbol.mangled_name, "constructor") == *mangled
                });
            (!is_constructor).then_some(signature.params.len())
        });
        if let Some(arity) = arity {
            self.report_closure_arity(arity, span, Some(&format!("`{mangled}`")));
        }
    }

    fn check_method_arity(&mut self, mangled: &crate::MangledName, name: &str, span: Span) {
        let arity = self
            .type_registry
            .lookup(mangled)
            .and_then(|symbol| match &symbol.kind {
                TypeKind::Interface {
                    methods,
                    dispatch: crate::Dispatch::VTable,
                    ..
                } => methods.get(name).map(|sig| sig.params.len()),
                TypeKind::Class { .. } => self
                    .class_method_in_chain(mangled, &[], name)
                    .map(|method| method.sig.params.len()),
                _ => None,
            });
        if let Some(arity) = arity {
            self.report_closure_arity(arity, span, Some(&format!("`{mangled}.{name}`")));
        }
    }

    pub(super) fn check_call_signature_types(
        &mut self,
        params: &[crate::Param],
        ret: &Type,
        lift: super::expr::CallLift<'_>,
        span: Span,
    ) {
        use super::expr::CallLift;
        let context = match lift {
            CallLift::Function { name } => {
                let mangled = self
                    .top_symbols
                    .get(name)
                    .map(|entry| &entry.mangled_name)
                    .or_else(|| {
                        let (namespace, member) = name.split_once('.')?;
                        self.namespace_bindings
                            .get(namespace)?
                            .members
                            .value(member)
                            .map(|value| &value.mangled_name)
                    });
                Some(mangled.map_or_else(|| format!("`{name}`"), |name| format!("`{name}`")))
            }
            CallLift::Method {
                receiver_ty, name, ..
            } => receiver_ty
                .interface_routing()
                .map(|(mangled, ..)| format!("`{mangled}.{name}`")),
            CallLift::Constructor { class_ty } => class_ty
                .interface_routing()
                .map(|(mangled, ..)| format!("`{mangled}` constructor")),
            CallLift::Anon { .. } => None,
        };
        for ty in params.iter().map(|p| &p.ty).chain(std::iter::once(ret)) {
            self.check_callable_type_in_context(ty, span, context.clone());
        }
    }

    pub(super) fn check_class_callable_types(&mut self, mangled: &crate::MangledName, span: Span) {
        let Some(symbol) = self.type_registry.lookup(mangled) else {
            return;
        };
        let mut pending = Vec::new();
        let mut failures = Vec::new();
        collect_class_arities(self, symbol, &mut pending, &mut failures);
        collect_callable_arities(self, pending, &mut failures);
        for (arity, context) in failures {
            self.report_closure_arity(arity, span, context.as_deref());
        }
    }

    pub(super) fn check_callable_type(&mut self, ty: &Type, span: Span) {
        self.check_callable_type_in_context(ty, span, None);
    }

    fn check_callable_type_in_context(&mut self, ty: &Type, span: Span, context: Option<String>) {
        let failures = callable_arities(self, ty, context);
        for (arity, context) in failures {
            self.report_closure_arity(arity, span, context.as_deref());
        }
    }
}

/// Iterative traversal avoids adding a recursive compiler frame. Nominal
/// back-edges are visited once: substitution can change slot types, never the
/// number of slots. Arguments are visited separately before following an edge.
fn callable_arities(
    inferer: &Inferer<'_>,
    root: &Type,
    context: Option<String>,
) -> Vec<(usize, Option<String>)> {
    let mut failures = Vec::new();
    collect_callable_arities(inferer, vec![(root, context)], &mut failures);
    failures
}

fn collect_callable_arities<'a>(
    inferer: &'a Inferer<'_>,
    mut pending: Vec<(&'a Type, Option<String>)>,
    failures: &mut Vec<(usize, Option<String>)>,
) {
    let mut visited = BTreeSet::new();
    while let Some((ty, context)) = pending.pop() {
        let context = match ty {
            Type::Alias { mangled, .. } => Some(format!("`{mangled}`")),
            _ => context,
        };
        match ty.peel() {
            Type::Function { params, ret, .. } => {
                if checked_closure_arity(params.len()).is_err() {
                    failures.push((params.len(), context.clone()));
                }
                pending.push((ret, context.clone()));
                pending.extend(params.iter().map(|ty| (ty, context.clone())));
            }
            Type::Array(element) => pending.push((element, context)),
            Type::Tuple(elements) => {
                pending.extend(elements.iter().map(|ty| (ty, context.clone())));
            }
            Type::Union(elements) => {
                pending.extend(elements.iter().map(|ty| (ty, context.clone())));
            }
            Type::Object { fields, index } => {
                pending.extend(fields.values().map(|field| (&field.ty, context.clone())));
                if let Some(index) = index {
                    pending.push((&index.value, context));
                }
            }
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            }
            | Type::ClassRef {
                mangled,
                name,
                args,
                ..
            }
            | Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                pending.extend(args.iter().map(|ty| (ty, context.clone())));
                if !visited.insert(mangled) {
                    continue;
                }
                let Some(symbol) = inferer.lookup_structural_type(mangled, name) else {
                    continue;
                };
                if let TypeKind::Alias { ty, .. } = &symbol.kind {
                    pending.push((ty, Some(format!("`{mangled}`"))));
                } else if matches!(symbol.kind, TypeKind::Interface { .. }) {
                    collect_interface_arities(symbol, &mut pending, failures);
                } else {
                    collect_class_arities(inferer, symbol, &mut pending, failures);
                }
            }
            _ => {}
        }
    }
}

fn collect_class_arities<'a>(
    inferer: &'a Inferer<'_>,
    mut symbol: &'a crate::TypeSymbol,
    pending: &mut Vec<(&'a Type, Option<String>)>,
    failures: &mut Vec<(usize, Option<String>)>,
) {
    let mut visited = BTreeSet::new();
    while visited.insert(&symbol.mangled_name) {
        let TypeKind::Class {
            methods,
            fields,
            constructor,
            extends,
            statics,
            static_fields,
            accessors,
            ..
        } = &symbol.kind
        else {
            break;
        };
        // Every instance method is carried in the class payload, including
        // inherited methods unused by a direct call.
        for (name, method) in methods {
            let context = Some(format!("`{}.{name}`", symbol.mangled_name));
            if checked_closure_arity(method.params.len()).is_err() {
                failures.push((method.params.len(), context.clone()));
            }
            pending.push((&method.ret, context.clone()));
            pending.extend(method.params.iter().map(|p| (&p.ty, context.clone())));
        }
        let context = Some(format!("`{}`", symbol.mangled_name));
        pending.extend(fields.values().map(|f| (&f.ty, context.clone())));
        pending.extend(constructor.iter().map(|p| (&p.ty, context.clone())));
        pending.extend(static_fields.values().map(|f| (&f.ty, context.clone())));
        for method in statics.values() {
            pending.push((&method.ret, context.clone()));
            pending.extend(method.params.iter().map(|p| (&p.ty, context.clone())));
        }
        for accessor in accessors {
            let ty = match accessor {
                crate::AccessorSig::Getter { ret_ty, .. } => ret_ty,
                crate::AccessorSig::Setter { param, .. } => &param.ty,
            };
            pending.push((ty, context.clone()));
        }
        let Some(parent) = extends else { break };
        pending.extend(parent.args.iter().map(|ty| (ty, context.clone())));
        let Some(parent_symbol) = inferer.type_registry.lookup(&parent.parent) else {
            break;
        };
        symbol = parent_symbol;
    }
}

fn collect_interface_arities<'a>(
    symbol: &'a crate::TypeSymbol,
    pending: &mut Vec<(&'a Type, Option<String>)>,
    failures: &mut Vec<(usize, Option<String>)>,
) {
    let TypeKind::Interface {
        methods,
        properties,
        index,
        dispatch,
        ..
    } = &symbol.kind
    else {
        return;
    };
    // A reference to the interface's value representation makes its full
    // surface reachable, just as DependencyUsage::is_interface_member_used does.
    for (name, method) in methods {
        let context = Some(format!("`{}.{name}`", symbol.mangled_name));
        if *dispatch == crate::Dispatch::VTable
            && checked_closure_arity(method.params.len()).is_err()
        {
            failures.push((method.params.len(), context.clone()));
        }
        pending.push((&method.ret, context.clone()));
        pending.extend(method.params.iter().map(|p| (&p.ty, context.clone())));
    }
    let context = Some(format!("`{}`", symbol.mangled_name));
    pending.extend(properties.values().map(|p| (&p.ty, context.clone())));
    if let Some(index) = index {
        pending.push((&index.value, context));
    }
}
