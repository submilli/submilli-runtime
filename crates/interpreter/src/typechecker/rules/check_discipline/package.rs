//! What the whole package says about a value, whichever body uses it.

use std::collections::BTreeSet;

use super::origin::ReadKey;
use crate::compiler_error::CompilerFailure;
use crate::typechecker::infer::narrowing::LiteralValue;
use crate::types::LiteralF64;
use crate::{
    ExprId, GlobalKind, MangledName, Span, Type, TypedAst, TypedExpr, TypedExprKind, TypedParam,
};

/// What the whole package says about a value, whichever body uses it.
pub(super) struct PackageFacts<'a> {
    pub(super) ta: &'a TypedAst,
    /// The globals the root module exports.
    pub(super) exported_globals: BTreeSet<&'a MangledName>,
}

impl<'a> PackageFacts<'a> {
    pub(super) fn expr_at(&self, id: ExprId) -> Result<&'a TypedExpr, CompilerFailure> {
        self.ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)
    }

    /// The arguments as written: a call site also holds the defaults of the
    /// ones it omits, and packs the ones a rest parameter takes.
    pub(super) fn authored<'s>(&'s self, span: Span, args: &'s [ExprId]) -> &'s [ExprId] {
        self.ta
            .authored_call_arguments(span)
            .map_or(args, Vec::as_slice)
    }

    /// Where a global the caller can reach is declared, when this package
    /// declares it. `None` for a global the caller cannot change.
    pub(super) fn caller_global(&self, mangled: &MangledName) -> Option<Option<Span>> {
        let declared = self
            .ta
            .globals
            .iter()
            .find(|global| global.mangled_name == *mangled);
        match declared {
            Some(global) => {
                let reachable =
                    global.kind == GlobalKind::Let || self.exported_globals.contains(mangled);
                reachable.then_some(Some(global.span))
            }
            // The runtime's own bindings, such as `console`, hold no state.
            // Another package's global is one the caller can import as well.
            None => (!is_runtime_symbol(mangled)).then_some(None),
        }
    }

    /// The read an index expression makes.
    pub(super) fn index_key(&self, index: ExprId) -> Result<ReadKey, CompilerFailure> {
        let index = self.expr_at(index)?;
        Ok(match &index.kind {
            TypedExprKind::Number(value) => {
                // `-0` and `0` index the same element.
                let value = if *value == 0.0 { 0.0 } else { *value };
                ReadKey::Element(LiteralValue::Number(LiteralF64(value)))
            }
            TypedExprKind::String(name) => ReadKey::Property(name.clone()),
            _ => match index.ty.peel() {
                Type::String | Type::StringLiteral(_) | Type::StringEnum { .. } => {
                    ReadKey::AnyProperty
                }
                _ => ReadKey::AnyElement,
            },
        })
    }
}

/// How messages name the parameter at `position`: a destructured one has
/// only the name the compiler made up.
pub(super) fn parameter_shown(param: &TypedParam, position: usize) -> String {
    if is_synthetic(&param.name.name) {
        format!("parameter {}", position.saturating_add(1))
    } else {
        param.name.name.clone()
    }
}

/// Whether the compiler made the name up, which no source name can start as.
pub(super) fn is_synthetic(name: &str) -> bool {
    name.starts_with('#')
}

/// The name a symbol has in the source: the last segment of its mangled name.
pub(super) fn source_name(mangled: &MangledName) -> &str {
    let mangled = mangled.as_str();
    mangled.rsplit(crate::mangle::SEP).next().unwrap_or(mangled)
}

/// Whether `mangled` is one of the runtime's own bindings, such as `console`.
pub(super) fn is_runtime_symbol(mangled: &MangledName) -> bool {
    mangled.as_str().starts_with("submilli:")
}
