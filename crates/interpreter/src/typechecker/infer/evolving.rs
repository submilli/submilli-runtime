//! Unannotated bindings take their readable type from the writes reaching them.
//! Storage is erased; ordinary flow joins retain concrete types at each read.

use std::collections::BTreeMap;

use super::expr::MethodCallSite;
use super::{Inferer, narrowing};
use crate::compiler_error::CompilerFailure;
use crate::{ChainPart, ExprId, ExprKind, Ident, Type, TypedExprKind};

impl Inferer<'_> {
    pub(super) fn report_evolving_read(&mut self, ident: &Ident, span: crate::Span) {
        let declaration = self
            .visible_local(&ident.name)
            .map(|entry| entry.decl_span)
            .or_else(|| {
                self.top_symbols
                    .get(&ident.name)
                    .map(|entry| entry.declaration_span)
            });
        let message = format!("cannot infer the type of `{}` at this read", ident.name);
        let help = vec![format!(
            "annotate its type, as in `{}: T[] = []` or `let {}: T | undefined;`",
            ident.name, ident.name
        )];
        if let Some(declaration) = declaration
            && !self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.span == declaration && diagnostic.message == message)
        {
            self.error_with_help(declaration, message.clone(), help.clone());
        }
        self.error_with_help(span, message, help);
    }

    pub(super) fn evolving_storage_type(&self, name: &Ident, inferred: Type) -> Type {
        if !self.evolving_bindings.contains(&name.span) {
            return inferred;
        }
        // A declaration initialized with [] remains array-only; an uninitialized
        // let has no such assignment contract. Only their readable types evolve.
        if matches!(inferred, Type::Array(_)) {
            Type::Array(Box::new(Type::Unknown))
        } else {
            Type::Unknown
        }
    }

    pub(super) fn evolving_binding_path(&self, name: &Ident) -> Option<narrowing::ReferencePath> {
        let (span, binding) = if let Some(entry) = self.visible_local(&name.name) {
            (
                entry.decl_span,
                narrowing::BindingId::Local {
                    name: name.name.clone(),
                    decl_scope: entry.decl_scope,
                },
            )
        } else {
            let entry = self.top_symbols.get(&name.name)?;
            (
                entry.declaration_span,
                narrowing::BindingId::Global(entry.mangled_name.clone()),
            )
        };
        self.evolving_bindings
            .contains(&span)
            .then(|| narrowing::ReferencePath::root(binding))
    }

    pub(super) fn is_evolving_path(&self, path: &narrowing::ReferencePath) -> bool {
        if !path.chain.is_empty() {
            return false;
        }
        match &path.root {
            narrowing::BindingId::Local { name, decl_scope } => {
                self.visible_local(name).is_some_and(|entry| {
                    entry.decl_scope == *decl_scope
                        && self.evolving_bindings.contains(&entry.decl_span)
                })
            }
            narrowing::BindingId::Global(mangled) => self.top_symbols.values().any(|entry| {
                &entry.mangled_name == mangled
                    && self.evolving_bindings.contains(&entry.declaration_span)
            }),
            narrowing::BindingId::This => false,
        }
    }

    pub(super) fn narrow_evolving_binding(
        &mut self,
        name: &Ident,
        ty: Type,
    ) -> Result<(), CompilerFailure> {
        let Some(path) = self.evolving_binding_path(name) else {
            return Ok(());
        };
        let evolving_array =
            matches!(&ty, Type::Array(element) if matches!(element.as_ref(), Type::Never));
        self.install_assignment_narrowing(path.clone(), name.clone(), ty, name.span)?;
        self.record_evolving_type(
            &path,
            if evolving_array {
                narrowing::EvolvingType::Array
            } else {
                narrowing::EvolvingType::Value
            },
        );
        Ok(())
    }

    pub(super) fn evolving_assignment_type(
        &self,
        name: &Ident,
        source: Option<ExprId>,
    ) -> Result<Option<narrowing::EvolvingType>, CompilerFailure> {
        if self.evolving_binding_path(name).is_none() {
            return Ok(None);
        }
        let Some(source) = source else {
            return Ok(Some(narrowing::EvolvingType::Value));
        };
        let expression = self.ast.try_expr(source).map_err(super::arena_failure)?;
        let is_empty_literal = matches!(&expression.kind,
            ExprKind::ArrayLiteral { elements } if elements.is_empty());
        Ok(Some(if is_empty_literal {
            narrowing::EvolvingType::Array
        } else {
            narrowing::EvolvingType::Value
        }))
    }

    pub(super) fn record_evolving_type(
        &mut self,
        path: &narrowing::ReferencePath,
        evolving: narrowing::EvolvingType,
    ) {
        let Some(view) = self
            .narrow_scopes
            .last_mut()
            .and_then(|env| env.get_mut(path))
        else {
            return;
        };
        view.evolving = Some(evolving);
        let view = view.clone();
        for writes in &mut self.clause_write_scopes {
            writes.record_evolving(path.clone(), view.clone());
        }
    }

    pub(super) fn record_evolving_array_growth(
        &mut self,
        name: &Ident,
        ty: Type,
    ) -> Result<(), CompilerFailure> {
        let Some(path) = self.evolving_binding_path(name) else {
            return Ok(());
        };
        let evolving = self
            .lookup_narrowed_view(&path)
            .and_then(|view| view.evolving)
            .unwrap_or(narrowing::EvolvingType::Array);
        self.install_assignment_narrowing(path.clone(), name.clone(), ty, name.span)?;
        self.record_evolving_type(&path, evolving);
        Ok(())
    }

    pub(super) fn evolving_array_receiver(
        &self,
        receiver: ExprId,
    ) -> Result<Option<Ident>, CompilerFailure> {
        let ExprKind::Identifier(name) = &self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .kind
        else {
            return Ok(None);
        };
        Ok(self
            .evolving_binding_path(name)
            .and_then(|path| self.lookup_narrowed_view(&path))
            .filter(|view| {
                matches!(
                    view.evolving,
                    Some(narrowing::EvolvingType::Array | narrowing::EvolvingType::UnresolvedArray)
                )
            })
            .filter(|view| array_element(&view.narrowed_ty).is_some())
            .map(|_| name.clone()))
    }

    pub(super) fn evolving_array_element(&self, binding: &Ident) -> Option<Type> {
        self.evolving_binding_path(binding)
            .and_then(|path| self.lookup_narrowed_view(&path))
            .filter(|view| {
                matches!(
                    view.evolving,
                    Some(narrowing::EvolvingType::Array | narrowing::EvolvingType::UnresolvedArray)
                )
            })
            .and_then(|view| array_element(&view.narrowed_ty))
    }

    /// Growth and length inspect an evolving array before its element type is known.
    pub(super) fn infer_array_operation_receiver(
        &mut self,
        receiver: ExprId,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        if let Some(name) = self.evolving_array_receiver(receiver)? {
            let span = name.span;
            let (kind, ty) = self.resolve_ident(name, span)?;
            let typed = self.push_synthetic_expr(kind, ty.clone(), span)?;
            return Ok((typed, ty));
        }
        self.infer_expr(receiver, None)
    }

    /// The evolving receiver is an array and its prelude growth method is present,
    /// so optional operators on this prefix cannot skip the call.
    pub(super) fn infer_evolving_chain(
        &mut self,
        base: ExprId,
        parts: &[crate::ChainPart],
        asserts_tail: bool,
    ) -> Result<Option<(TypedExprKind, Type)>, CompilerFailure> {
        let [
            ChainPart::Field { name, .. },
            ChainPart::Call {
                args,
                type_args,
                span,
                ..
            },
            tail @ ..,
        ] = parts
        else {
            return Ok(None);
        };
        if !is_growth_method(name) || self.held_arguments.contains_key(&base) {
            return Ok(None);
        }
        let Some(binding) = self.evolving_array_receiver(base)? else {
            return Ok(None);
        };
        let (kind, ty) = self.infer_evolving_array_call(
            base,
            binding,
            MethodCallSite {
                name: name.clone(),
                type_args: type_args.clone(),
                args: args.clone(),
                expected: None,
                span: *span,
            },
        )?;
        if tail.is_empty() {
            return Ok(Some((kind, ty)));
        }
        let value = self.push_synthetic_expr(kind, ty, *span)?;
        self.with_held_arguments(BTreeMap::from([(base, value)]), |this| {
            this.infer_chain_steps(base, tail.to_vec(), asserts_tail)
        })
        .map(Some)
    }

    pub(super) fn infer_evolving_array_call(
        &mut self,
        receiver: ExprId,
        binding: Ident,
        call: MethodCallSite<'_>,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (receiver, before) = self.infer_array_operation_receiver(receiver)?;
        let mut statements = Vec::new();
        let receiver = self.hold_in_temp(receiver, "evolving_receiver", &mut statements)?;
        let mut held = BTreeMap::new();
        let mut added = Vec::new();
        for &arg in &call.args {
            let source = arg;
            let (typed, ty) = self.infer_expr(source, None)?;
            let ty = super::stmt::widen_literal_members(ty);
            added.push(ty);
            held.insert(
                source,
                self.hold_in_temp(typed, "evolving_argument", &mut statements)?,
            );
        }
        let element = Type::union(added);
        // Argument expressions may grow or reassign this binding too. The call
        // still mutates the held receiver; only an evolving binding gains its type.
        let current_element = self.evolving_array_element(&binding);
        let after = Type::Array(Box::new(Type::union(vec![
            array_element(&before).unwrap_or(Type::Never),
            current_element.clone().unwrap_or(Type::Never),
            element.clone(),
        ])));
        let receiver = self.push_synthetic_expr(
            TypedExprKind::Cast {
                value: receiver,
                target_ty: after.clone(),
                check: None,
            },
            after.clone(),
            call.span,
        )?;
        let method = self.find_method(&after, &call.name.name).ok_or_else(|| {
            super::inference_failure("array growth method is declared in the prelude")
        })?;
        let span = call.span;
        let (kind, ty) = self.with_held_arguments(held, |this| {
            this.infer_found_method_call(receiver, &after, method, call)
        })?;
        let result = self.push_synthetic_expr(kind, ty.clone(), span)?;
        if current_element.is_some() {
            let element = Type::union(vec![current_element.unwrap_or(Type::Never), element]);
            self.record_evolving_array_growth(&binding, Type::Array(Box::new(element)))?;
        }
        Ok((
            TypedExprKind::Sequence {
                stmts: statements,
                result,
            },
            ty,
        ))
    }
}

/// Whether calling `name` on an evolving array grows it.
pub(super) fn is_growth_method(name: &Ident) -> bool {
    matches!(name.name.as_str(), "push" | "unshift")
}

/// Whether `expr` calls a growth method in a form inference sends to
/// [`Inferer::infer_evolving_array_call`] when its receiver is an evolving
/// array: `xs.push(v)`, `xs["push"](v)`, `xs?.push(v)`, `xs.push?.(v)` or
/// `xs["push"]?.(v)`. [`Inferer::changes_no_flow`] relies on it covering every
/// such form. It does not look at the receiver: a call that grows nothing only
/// costs that caller a snapshot.
pub(super) fn calls_growth_method(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let calls_method = |callee: ExprId| -> Result<bool, CompilerFailure> {
        Ok(matches!(
            super::expr::string_key_callee_as_field(ast, callee)?,
            ExprKind::FieldAccess { name, .. } if is_growth_method(&name)
        ))
    };
    match &ast.try_expr(expr).map_err(super::arena_failure)?.kind {
        ExprKind::Call { callee, .. } => calls_method(*callee),
        ExprKind::OptionalChain { base, parts } => match parts.as_slice() {
            [ChainPart::Field { name, .. }, ChainPart::Call { .. }, ..] => {
                Ok(is_growth_method(name))
            }
            [ChainPart::Call { optional: true, .. }, ..] => calls_method(*base),
            _ => Ok(false),
        },
        _ => Ok(false),
    }
}

pub(super) fn array_element(ty: &Type) -> Option<Type> {
    match ty.peel() {
        Type::Array(element) => Some((**element).clone()),
        Type::Union(members) => members
            .iter()
            .map(array_element)
            .collect::<Option<Vec<_>>>()
            .map(Type::union),
        _ => None,
    }
}
