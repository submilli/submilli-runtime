//! Index reads that may find no value (SUB-2148).
//!
//! An index read is typed `T`, as in tsc's `--strict`. That covers an array
//! or string element, a `Uint8Array` byte, a regex match group, and a read
//! through an index signature (`r[k]`, `rec.foo`). When the element or key is
//! missing, the read is decided at the first of these places it reaches:
//!
//! - a check: `x ?? d`, `x === y` and `x !== y` (with any `y`), `!x`,
//!   `x || d`, a test in `if`, `while`, `for` or `?:`, the base of `x?.f`,
//!   and `typeof x`. Each one sees `undefined`, as in Node.
//! - a place whose declared type names `undefined` (a `| undefined` union,
//!   an optional parameter, or an optional field), which takes `undefined`.
//! - anywhere else, where the read throws `MissingValueError`.
//!
//! A read reaches a check in one of two ways. Either it is the check's operand
//! itself, or a `let`/`const` local holds it "`T`, maybe missing": an
//! unannotated local it initializes, or a local whose type names `undefined`
//! that it initializes or is assigned to, which then narrows to `T` as tsc
//! narrows it. Each use of such a local is decided the same way, and a
//! `return` reads it, so a maybe-missing value never leaves its function.
//! A local that a closure captures never holds one: the closure gets a copy,
//! so the read decides at once. Nor does a parameter, a global or a field:
//! those keep `T | undefined`. A local typed with a type parameter, whose
//! instantiation may hold `undefined`, holds one beside a present flag in
//! codegen, which tells a missing value from a present `undefined`.
//!
//! The decision lives in the typed tree, and codegen follows the types:
//! - A read that reaches a check, or a place that holds `undefined`, is
//!   retyped `T | undefined` there. The check sees `undefined` exactly where
//!   Node would, and the value goes no further.
//! - A read left at `T` throws when its value is missing.
//! - A maybe-missing local is stored as `T | undefined`
//!   ([`crate::TypedAst::maybe_missing_locals`]), and a use left at `T` throws,
//!   naming the read.
//!
//! A read whose `T` already holds `undefined` (`Record<string, unknown>`)
//! gives `undefined`, and is never missing.
//!
//! `x?.f` on a base that may be missing checks it, but the chain keeps the
//! type tsc gives it (`f`'s, without `undefined`). So the chain is itself a
//! missing read, decided by the rules above: `x?.f === "a"` is `false`, and
//! `const t = x?.f` holds it maybe missing. The same holds for a step of a
//! chain that may be missing, the last one or one before a `?.`, where no
//! `?.` short-circuits to give `undefined`.
//!
//! A `JSON.stringify` call that may have no JSON text (`undefined`, a function,
//! `unknown`, a type parameter, or a constructor binding) is decided the same
//! way. The call is typed `string`, as in tsc. A result with no JSON text is
//! `undefined` at a check or a place that names `undefined`, and throws
//! `MissingValueError` anywhere else.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Ident, Intrinsic, Span, Type, TypedExprKind, UnOp};

use super::Inferer;

/// What the expression `infer_expr` is dispatching reads, set just before its
/// inference returns and taken once the node is pushed.
#[derive(Clone, Debug)]
pub(in crate::typechecker) enum PendingRead {
    /// An index read, a read through an index signature, or a
    /// `JSON.stringify` call, that may find no value.
    Missable,
    /// An optional chain that may find no value; see [`MissableChain`].
    Chain(MissableChain),
    /// A read of a local that may hold a missing read.
    MaybeMissingLocal,
    /// A generic `o[k]` whose missing key reads `undefined`, as every
    /// instantiation's property holds it.
    GivesUndefined,
}

/// Where an optional chain may find no value. A `?.` after a missing base or
/// step checks it, but the chain keeps the type tsc gives it, so the chain is
/// itself a missing read; a check that takes it widens these parts.
#[derive(Clone, Debug, Default)]
pub(in crate::typechecker) struct MissableChain {
    /// The base may be missing.
    pub(in crate::typechecker) base: bool,
    /// The steps, by index, whose result may be missing.
    pub(in crate::typechecker) steps: Vec<usize>,
}

impl Inferer<'_> {
    /// Marks the expression being inferred as one that may find no value. It
    /// counts only if its type has no room for `undefined`.
    pub(super) fn note_missable_read(&mut self) {
        self.pending_read = Some(PendingRead::Missable);
    }

    /// Records what the node `id` just pushed reads, from `pending`.
    pub(super) fn record_pending_read(
        &mut self,
        pending: Option<PendingRead>,
        id: ExprId,
    ) -> Result<(), CompilerFailure> {
        let Some(pending) = pending else {
            return Ok(());
        };
        let expr = self.typed_ast.try_expr(id).map_err(super::arena_failure)?;
        let (kind, ty) = (&expr.kind, &expr.ty);
        match pending {
            PendingRead::Missable
                if missable_expr(kind)
                    && !ty.holds_undefined()
                    && !matches!(ty.peel(), Type::Error | Type::Never) =>
            {
                self.missable_reads.insert(id);
            }
            PendingRead::Chain(chain) if matches!(kind, TypedExprKind::OptionalChain { .. }) => {
                self.missable_reads.insert(id);
                self.missable_chains.insert(id, chain);
            }
            PendingRead::MaybeMissingLocal if matches!(kind, TypedExprKind::LocalRef { .. }) => {
                let accepts_undefined = missing_local_use_names_undefined(ty);
                self.maybe_missing_uses.insert(id);
                if accepts_undefined {
                    self.widen_with_undefined(id)?;
                }
            }
            PendingRead::GivesUndefined if matches!(kind, TypedExprKind::IndexAccess { .. }) => {
                self.typed_ast.missing_reads_undefined.insert(id);
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether `id` is a read that may find no value: a missable read, or a
    /// use of a maybe-missing local.
    pub(super) fn may_be_missing(&self, id: ExprId) -> bool {
        self.missable_reads.contains(&id) || self.maybe_missing_uses.contains(&id)
    }

    /// `id` reaches a check. If it may find no value, it reads as
    /// `T | undefined` there, so the check sees `undefined` when it does.
    /// Returns whether it may.
    pub(super) fn absorb_missing(&mut self, id: ExprId) -> Result<bool, CompilerFailure> {
        if !self.may_be_missing(id) {
            return Ok(false);
        }
        self.widen_with_undefined(id)?;
        Ok(true)
    }

    /// `id` goes straight into a place of type `declared`. Where that type
    /// names `undefined`, a missing value takes it.
    pub(super) fn absorb_missing_into(
        &mut self,
        id: ExprId,
        declared: &Type,
    ) -> Result<bool, CompilerFailure> {
        if !declared.names_undefined() {
            return Ok(false);
        }
        self.absorb_missing(id)
    }

    /// `id` is tested for truthiness: the condition of an `if`, a loop or a
    /// `?:`, or the operand of `!`. Both operands of `&&` and `||` there are
    /// tested too. An operator whose operand may be missing can then
    /// produce `undefined`, so it is widened as well. That only reaches the
    /// test, which treats it as falsy.
    pub(super) fn absorb_missing_in_test(&mut self, id: ExprId) -> Result<bool, CompilerFailure> {
        match self
            .typed_ast
            .try_expr(id)
            .map_err(super::arena_failure)?
            .kind
        {
            TypedExprKind::Binary {
                op: crate::BinOp::And | crate::BinOp::Or,
                lhs,
                rhs,
            } => {
                let left = self.absorb_missing_in_test(lhs)?;
                let right = self.absorb_missing_in_test(rhs)?;
                if left || right {
                    self.widen_with_undefined(id)?;
                }
                Ok(left || right)
            }
            // `!x` is boolean whatever `x` is.
            TypedExprKind::Unary {
                op: UnOp::Not,
                operand,
            } => self.absorb_missing_in_test(operand),
            _ => self.absorb_missing(id),
        }
    }

    fn widen_with_undefined(&mut self, id: ExprId) -> Result<(), CompilerFailure> {
        let expr = self.typed_ast.try_expr(id).map_err(super::arena_failure)?;
        // Codegen tells a check's read of a maybe-missing local by this
        // entry, which its type alone can't do where it already holds
        // `undefined` (`T & ({} | undefined)` after `v === null`).
        if expr.ty.holds_undefined() {
            let written = expr.ty.clone();
            self.typed_ast
                .widened_missing_reads
                .entry(id)
                .or_insert(written);
            return Ok(());
        }
        // A chain finds no value where its base or one of its steps is missing,
        // which the `?.` after it then sees, or which is the chain's result.
        let chain_base = match &expr.kind {
            TypedExprKind::OptionalChain { base, .. } => Some(*base),
            _ => None,
        };
        let chain = self.missable_chains.get(&id).cloned().unwrap_or_default();
        if chain.base
            && let Some(base) = chain_base
        {
            self.widen_with_undefined(base)?;
        }
        let expr = self
            .typed_ast
            .try_expr_mut(id)
            .map_err(super::arena_failure)?;
        if let TypedExprKind::OptionalChain { parts, .. } = &mut expr.kind {
            widen_missing_steps(parts, &chain.steps);
        }
        let widened = Type::union(vec![expr.ty.clone(), Type::Undefined]);
        let written = std::mem::replace(&mut expr.ty, widened);
        self.typed_ast
            .widened_missing_reads
            .entry(id)
            .or_insert(written);
        Ok(())
    }

    /// The read a maybe-missing value at `id` came from: `id` itself for a
    /// missable read, or the read the local it names holds.
    fn missing_read_origin(&self, id: ExprId) -> Result<Option<Span>, CompilerFailure> {
        let expr = self.typed_ast.try_expr(id).map_err(super::arena_failure)?;
        if self.missable_reads.contains(&id) {
            return Ok(Some(expr.span));
        }
        if self.maybe_missing_uses.contains(&id)
            && let TypedExprKind::LocalRef { ident, .. } = &expr.kind
        {
            return Ok(self
                .scopes
                .get(&ident.name)
                .and_then(|entry| entry.missing_read));
        }
        Ok(None)
    }

    /// `let name = value` or `const name = value` in a function, without an
    /// annotation. When `value` may be missing and no closure captures
    /// `name`, the local holds it maybe missing: the value reads as
    /// `T | undefined`, and the binding is stored that way. Returns the
    /// storage type, `None` when the local holds a plain value.
    ///
    /// Call after the binding is in scope.
    pub(super) fn bind_maybe_missing_local(
        &mut self,
        name: &Ident,
        annotated: bool,
        value: ExprId,
        bound: &Type,
    ) -> Result<Option<Type>, CompilerFailure> {
        if annotated || !self.local_can_hold_missing_read(&name.name, name.span) {
            return Ok(None);
        }
        let Some(read) = self.missing_read_origin(value)? else {
            return Ok(None);
        };
        let message = self.missing_local_message(&name.name, read);
        self.hold_read(&name.name, name.span, read, message);
        self.widen_with_undefined(value)?;
        self.typed_ast.widened_bindings.insert(name.span);
        Ok(Some(Type::union(vec![bound.clone(), Type::Undefined])))
    }

    /// Whether the local `name` declared at `decl_span` can hold a
    /// maybe-missing read; see [`ScopeEntry::can_hold_missing_read`].
    pub(super) fn local_can_hold_missing_read(&self, name: &str, decl_span: Span) -> bool {
        let captured = self.captured_locals.contains(&decl_span);
        self.scopes
            .get(name)
            .filter(|entry| entry.decl_span == decl_span)
            .is_some_and(|entry| entry.can_hold_missing_read(captured))
    }

    /// Records that the local `name` declared at `decl_span` holds `read`,
    /// which a use at `T` names with `message` when the value is missing.
    fn hold_read(&mut self, name: &str, decl_span: Span, read: Span, message: String) {
        if let Some(entry) = self.scopes.get_mut(name)
            && entry.decl_span == decl_span
        {
            entry.missing_read = Some(read);
            let has_present_flag = entry.ty.has_type_parameter_member()
                || entry.write_type().has_type_parameter_member();
            self.typed_ast.maybe_missing_locals.insert(
                decl_span,
                crate::MaybeMissingLocal {
                    message,
                    has_present_flag,
                },
            );
        }
    }

    /// The local `name`, declared at `decl_span` with a type that names
    /// `undefined`, took the missing value `value` may be as its initializer.
    /// It reads as `T` from here, as tsc narrows it, so it holds the read
    /// maybe missing: a read at `T` checks it.
    pub(super) fn hold_initializer_read(
        &mut self,
        name: &str,
        decl_span: Span,
        value: ExprId,
    ) -> Result<(), CompilerFailure> {
        if let Some(read) = self.missing_read_origin(value)? {
            let message = self.missing_local_message(name, read);
            self.hold_read(name, decl_span, read, message);
        }
        Ok(())
    }

    /// [`Self::hold_initializer_read`] for a later assignment, or another
    /// missing read a maybe-missing local takes: its message can no longer
    /// name one read.
    pub(super) fn hold_assigned_read(
        &mut self,
        name: &str,
        decl_span: Span,
        value: ExprId,
    ) -> Result<(), CompilerFailure> {
        if let Some(read) = self.missing_read_origin(value)? {
            self.hold_read(name, decl_span, read, assigned_missing_message(name));
        }
        Ok(())
    }

    /// A value stored in a place declared `declared` (a module-level binding,
    /// a field, an array element or a parameter default) takes a missing
    /// value where that type names `undefined`, and reads as maybe
    /// `undefined` there: such a place can't hold a maybe-missing read.
    /// Returns the value's flow type.
    pub(super) fn absorb_into_declared(
        &mut self,
        hint: Option<&Type>,
        typed_value: ExprId,
        value_ty: Type,
    ) -> Result<Type, CompilerFailure> {
        Ok(match hint {
            Some(hint) if self.absorb_missing_into(typed_value, hint)? => {
                Type::union(vec![value_ty, Type::Undefined])
            }
            _ => value_ty,
        })
    }

    /// Whether `receiver.name` reads through `receiver`'s index signature,
    /// rather than a field it declares, so the key may be missing.
    pub(super) fn reads_through_index_signature(&self, receiver: &Type, name: &str) -> bool {
        let declared = match receiver.peel() {
            Type::Object { fields, .. } => fields.contains_key(name),
            Type::InterfaceRef { .. } => self
                .assignment_target_fields(receiver)
                .is_some_and(|fields| fields.contains_key(name)),
            _ => return false,
        };
        !declared
            && self
                .resolver()
                .index_signatures(receiver)
                .iter()
                .any(|index| {
                    crate::property_keys::matches_name(&index.key, name, &self.type_limits)
                })
    }

    /// The file this module's source text belongs to, as inference finds it.
    fn ast_file(&self) -> crate::FileId {
        self.ast
            .source_statements()
            .first()
            .map(|stmt| stmt.span.file)
            .or_else(|| {
                self.ast
                    .source_expressions()
                    .first()
                    .map(|expr| expr.span.file)
            })
            .unwrap_or(crate::FileId(0))
    }

    /// What a use of the maybe-missing local `name` throws when the read it
    /// holds, at `read`, found no value. The use can be far from the read, so
    /// the message names the read and its line.
    fn missing_local_message(&self, name: &str, read: Span) -> String {
        let text = (read.file == self.ast_file())
            .then(|| self.source.get(read.start as usize..read.end as usize))
            .flatten()
            .filter(|text| text.chars().count() <= 64 && !text.contains('\n'));
        let line = (read.file == self.ast_file())
            .then(|| self.source.get(..read.start as usize))
            .flatten()
            .map(|before| before.matches('\n').count() + 1);
        // A pattern's hidden binding has no name a reader wrote.
        if crate::lower_patterns::is_pattern_value(name) {
            return match (text, line) {
                (Some(text), Some(line)) => {
                    format!("`{text}` on line {line} found no value to destructure")
                }
                (_, Some(line)) => {
                    format!("the read on line {line} found no value to destructure")
                }
                _ => "the read found no value to destructure".to_string(),
            };
        }
        match (text, line) {
            // A destructured binding's read is written as the binding itself.
            (Some(text), Some(line)) if text == name => {
                format!(
                    "`{name}` is missing: its destructured element on line {line} found no value"
                )
            }
            (Some(text), Some(line)) => {
                format!("`{name}` is missing: `{text}` on line {line} found no value")
            }
            (Some(text), None) => format!("`{name}` is missing: `{text}` found no value"),
            (None, Some(line)) => {
                format!("`{name}` is missing: the read on line {line} found no value")
            }
            (None, None) => format!("`{name}` is missing: the read it holds found no value"),
        }
    }
}

/// Widens the chain parts at `steps`, steps that may find no value, with
/// `undefined`.
pub(super) fn widen_missing_steps(parts: &mut [crate::TypedChainPart], steps: &[usize]) {
    for &step in steps {
        if let Some(part) = parts.get_mut(step) {
            let widened = Type::union(vec![part.result_ty().clone(), Type::Undefined]);
            part.set_result_ty(widened);
        }
    }
}

/// Whether `kind` is an expression that may find no value: an index read, a
/// field read through an index signature, or `JSON.stringify`.
fn missable_expr(kind: &TypedExprKind) -> bool {
    matches!(
        kind,
        TypedExprKind::IndexAccess { .. }
            | TypedExprKind::FieldAccess { .. }
            | TypedExprKind::IntrinsicCall {
                kind: Intrinsic::JsonStringify,
                ..
            }
    )
}

/// What a use of the maybe-missing local `name` throws when the value an
/// assignment gave it was missing; it may hold any of several reads.
fn assigned_missing_message(name: &str) -> String {
    format!("`{name}` is missing: a read assigned to it found no value")
}

// A null guard refines bare T to a shape containing undefined without
// making missing reads acceptable. Inspect its original type instead.
fn missing_local_use_names_undefined(ty: &Type) -> bool {
    match ty {
        Type::Refined { original, .. } => missing_local_use_names_undefined(original),
        Type::Alias { ty, .. } | Type::Readonly(ty) => missing_local_use_names_undefined(ty),
        Type::Union(members) => members.iter().any(missing_local_use_names_undefined),
        Type::Undefined | Type::Void => true,
        _ => false,
    }
}
