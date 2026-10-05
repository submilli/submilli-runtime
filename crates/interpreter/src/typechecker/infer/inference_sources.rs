//! Object literals whose type is a source for inference rather than a value
//! checked against a declared type, so they get no unknown-field check.
//!
//! tsc reports an object literal's unknown fields only where the literal is
//! assigned straight to a declared type. Where it is instead read for its own
//! type, the fields beyond the target are kept, and the result is related to
//! the target structurally, by width. That happens in two places:
//!
//! - an argument at a position typed by a type parameter that the call infers
//!   from literals alone. `const a: A = id({ a: 1, z: 2 })` infers `T` from
//!   the literal, so there is no declared type to check it against, and
//!   `two({ a: 1 }, { a: 2, z: 3 })` likewise infers `T` from both literals.
//!   The type parameter becomes a declared type once anything else is a
//!   candidate for it: a written type argument, or an argument anywhere in
//!   the call that isn't an object or array literal (`two(a, { a: 2, z: 3 })`
//!   with `a: A`). A parameter's own structure around the type parameter is
//!   declared too (`wrap<T>(x: { v: T })` rejects `wrap({ v: 1, z: 2 })`), and
//!   a union parameter is read through that structure rather than its bare
//!   member (`T | { v: T }` rejects it as well). The type the call's result is
//!   expected to have is not a candidate;
//! - a value returned from a function literal with no return annotation: its
//!   return type is inferred from the body and then related to the context,
//!   so `cb(() => ({ a: 1, z: 2 }))` is accepted against `() => A`.
//!
//! The literal still takes the context as a hint, so its fields are typed as
//! they would be against the declared type, and one sharing no field with a
//! target whose fields are all optional is still rejected (see
//! `report_no_field_in_common`).
//!
//! A value an unannotated function literal returns is inferred here too, since
//! it is read for its own type the same way, and the literal types it carries
//! are settled with it (see `Inferer::infer_returned_value`).

use crate::compiler_error::CompilerFailure;
use crate::{ArrayLiteralElement, ExprId, ExprKind, ObjectLiteralMember, Type};

use super::Inferer;
use super::expr::{is_type_parameter_position, mentions_type_var};

impl<'a> Inferer<'a> {
    /// The type parameters among `inferred_generics` that only object and
    /// array literals (or `null`) are candidates for, among `args`, each
    /// paired with the type of the parameter it is passed for.
    pub(super) fn literal_inferred_type_params(
        &self,
        args: &[(ExprId, Type)],
        inferred_generics: &[String],
    ) -> Result<Vec<String>, CompilerFailure> {
        let mut candidates = Vec::new();
        for (arg, param) in args {
            self.collect_candidates(*arg, param, inferred_generics, &mut candidates)?;
        }
        let mut literal_only = Vec::new();
        for name in inferred_generics {
            let mut all_literals = true;
            for (_, candidate) in candidates.iter().filter(|(of, _)| of == name) {
                all_literals &= self.is_literal_candidate(*candidate)?;
            }
            if all_literals {
                literal_only.push(name.clone());
            }
        }
        Ok(literal_only)
    }

    /// Mark the object literals in `expr` that `param` types only through a
    /// type parameter in `literal_inferred`, for the span of `infer`.
    pub(super) fn with_inferred_positions<R>(
        &mut self,
        expr: ExprId,
        param: &Type,
        literal_inferred: &[String],
        infer: impl FnOnce(&mut Self) -> Result<R, CompilerFailure>,
    ) -> Result<R, CompilerFailure> {
        let mut marked = Vec::new();
        self.mark_inferred_positions(expr, param, literal_inferred, &mut marked)?;
        self.with_marked(marked, infer)
    }

    /// Infer the value a `return` gives, marking the object literals it builds
    /// directly when the enclosing function literal infers its return type.
    /// That type widens a literal a generic call kept from a fresh argument
    /// (see [`Self::widen_kept_call_literals`]), unless the function literal
    /// keeps its returned literals for a type parameter (see
    /// `returns_keep_literals`) or has a contextual return type other than a
    /// bare type parameter: `() => id(1)` is `() => number`.
    pub(super) fn infer_returned_value(
        &mut self,
        expr: ExprId,
        hint: Option<&Type>,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        // Only an unannotated function literal collects its returns, to infer
        // its return type from them.
        if self.inferred_returns.is_none() {
            return self.infer_expr(expr, hint);
        }
        let mut marked = Vec::new();
        self.mark_inference_source(expr, &mut marked)?;
        let (typed, ty) = self.with_marked(marked, |this| {
            this.keeps_literal_types = this.returns_keep_literals;
            this.infer_expr(expr, hint)
        })?;
        let has_contextual_return_type = hint.is_some_and(|hint| !is_type_parameter_position(hint));
        if self.returns_keep_literals || has_contextual_return_type {
            return Ok((typed, ty));
        }
        Ok((typed, self.widen_kept_call_literals(typed, &ty)?))
    }

    pub(super) fn is_inference_source(&self, literal: ExprId) -> bool {
        self.inference_source_literals.contains(&literal)
    }

    fn with_marked<R>(
        &mut self,
        marked: Vec<ExprId>,
        infer: impl FnOnce(&mut Self) -> Result<R, CompilerFailure>,
    ) -> Result<R, CompilerFailure> {
        let result = infer(self);
        for literal in marked {
            self.inference_source_literals.remove(&literal);
        }
        result
    }

    /// Record, for each type parameter in `inferred_generics` that `param`
    /// mentions, the expressions within `expr` that are candidates for it:
    /// those at its positions, or the nearest one whose structure isn't
    /// followed. [`Self::mark_inferred_positions`] follows the same positions.
    fn collect_candidates(
        &self,
        expr: ExprId,
        param: &Type,
        inferred_generics: &[String],
        candidates: &mut Vec<(String, ExprId)>,
    ) -> Result<(), CompilerFailure> {
        let mentioned: Vec<&String> = inferred_generics
            .iter()
            .filter(|name| mentions_type_var(param, &|var| var == name.as_str()))
            .collect();
        if mentioned.is_empty() {
            return Ok(());
        }
        let add_candidate = |candidates: &mut Vec<(String, ExprId)>, candidate: ExprId| {
            for name in &mentioned {
                candidates.push(((*name).clone(), candidate));
            }
        };
        match param.peel() {
            Type::TypeVar(_) => {
                add_candidate(candidates, expr);
                return Ok(());
            }
            // Which member the argument is isn't known before inference, so
            // it is a candidate through each.
            Type::Union(members) => {
                for member in members {
                    self.collect_candidates(expr, member, inferred_generics, candidates)?;
                }
                return Ok(());
            }
            _ => {}
        }
        let param = param.peel();
        match self.expr_kind(expr)? {
            ExprKind::Paren(inner) => {
                self.collect_candidates(inner, param, inferred_generics, candidates)
            }
            ExprKind::Ternary { then_, else_, .. } => {
                self.collect_candidates(then_, param, inferred_generics, candidates)?;
                self.collect_candidates(else_, param, inferred_generics, candidates)
            }
            ExprKind::ObjectLiteral { members } => {
                let Some(fields) = self.field_types(param) else {
                    add_candidate(candidates, expr);
                    return Ok(());
                };
                for member in members {
                    match member {
                        ObjectLiteralMember::Field(field) => {
                            if let Some(declared) = fields.get(&field.name.name) {
                                self.collect_candidates(
                                    field.value,
                                    declared,
                                    inferred_generics,
                                    candidates,
                                )?;
                            }
                        }
                        ObjectLiteralMember::Spread { value, .. }
                        | ObjectLiteralMember::Computed { value, .. } => {
                            add_candidate(candidates, value);
                        }
                    }
                }
                Ok(())
            }
            ExprKind::ArrayLiteral { elements } => {
                for (index, element) in elements.into_iter().enumerate() {
                    let value = match element {
                        ArrayLiteralElement::Value(value) => value,
                        ArrayLiteralElement::Spread { value, .. } => {
                            add_candidate(candidates, value);
                            continue;
                        }
                    };
                    match element_type(param, index) {
                        Some(element_ty) => self.collect_candidates(
                            value,
                            element_ty,
                            inferred_generics,
                            candidates,
                        )?,
                        None => add_candidate(candidates, value),
                    }
                }
                Ok(())
            }
            _ => {
                add_candidate(candidates, expr);
                Ok(())
            }
        }
    }

    /// Whether a candidate for a type parameter leaves it inferred from
    /// literals: an object or array literal, through parentheses and both
    /// branches of a `?:`, or `null`, which only makes the inferred type
    /// nullable.
    fn is_literal_candidate(&self, candidate: ExprId) -> Result<bool, CompilerFailure> {
        match self.expr_kind(candidate)? {
            ExprKind::Paren(inner) => self.is_literal_candidate(inner),
            ExprKind::Ternary { then_, else_, .. } => {
                Ok(self.is_literal_candidate(then_)? && self.is_literal_candidate(else_)?)
            }
            ExprKind::ObjectLiteral { .. } | ExprKind::ArrayLiteral { .. } | ExprKind::Null => {
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Mark the object literals in `expr` at positions `param` types through
    /// one of `literal_inferred`. It follows the positions
    /// [`Self::collect_candidates`] does; spreads and the expressions that
    /// walk counts whole are never marked, since only a literal at such a
    /// position can be.
    fn mark_inferred_positions(
        &mut self,
        expr: ExprId,
        param: &Type,
        literal_inferred: &[String],
        marked: &mut Vec<ExprId>,
    ) -> Result<(), CompilerFailure> {
        let param = param.peel();
        if is_typed_by_type_param(param, literal_inferred) {
            return self.mark_inference_source(expr, marked);
        }
        // tsc infers from the structured members, so only their positions
        // are followed.
        if let Type::Union(members) = param {
            for member in members
                .iter()
                .filter(|member| !is_bare(member, literal_inferred))
            {
                self.mark_inferred_positions(expr, member, literal_inferred, marked)?;
            }
            return Ok(());
        }
        match self.expr_kind(expr)? {
            ExprKind::Paren(inner) => {
                self.mark_inferred_positions(inner, param, literal_inferred, marked)
            }
            ExprKind::Ternary { then_, else_, .. } => {
                self.mark_inferred_positions(then_, param, literal_inferred, marked)?;
                self.mark_inferred_positions(else_, param, literal_inferred, marked)
            }
            ExprKind::ObjectLiteral { members } => {
                let Some(fields) = self.field_types(param) else {
                    return Ok(());
                };
                for member in members {
                    if let ObjectLiteralMember::Field(field) = member
                        && let Some(declared) = fields.get(&field.name.name)
                    {
                        self.mark_inferred_positions(
                            field.value,
                            declared,
                            literal_inferred,
                            marked,
                        )?;
                    }
                }
                Ok(())
            }
            ExprKind::ArrayLiteral { elements } => {
                for (index, element) in elements.into_iter().enumerate() {
                    if let ArrayLiteralElement::Value(value) = element
                        && let Some(element_ty) = element_type(param, index)
                    {
                        self.mark_inferred_positions(value, element_ty, literal_inferred, marked)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The field types an object type declares, through an interface's
    /// structure, with its index signature's value for any other name.
    fn field_types(&self, ty: &Type) -> Option<FieldTypes> {
        let named = match ty {
            Type::Object { fields, .. } => fields.clone(),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => self.structural_form(mangled, name, args)?,
            _ => return None,
        };
        Some(FieldTypes {
            named,
            index: self
                .resolver()
                .index_signature(ty)
                .map(|index| *index.value),
        })
    }

    /// Mark the object literals `expr` builds directly: itself, and those its
    /// fields, elements and branches hold. Any other expression is a context
    /// of its own, such as a call checking its arguments.
    fn mark_inference_source(
        &mut self,
        expr: ExprId,
        marked: &mut Vec<ExprId>,
    ) -> Result<(), CompilerFailure> {
        match self.expr_kind(expr)? {
            ExprKind::Paren(inner) => self.mark_inference_source(inner, marked),
            ExprKind::Ternary { then_, else_, .. } => {
                self.mark_inference_source(then_, marked)?;
                self.mark_inference_source(else_, marked)
            }
            ExprKind::ObjectLiteral { members } => {
                if self.inference_source_literals.insert(expr) {
                    marked.push(expr);
                }
                for member in members {
                    if let ObjectLiteralMember::Field(field) = member {
                        self.mark_inference_source(field.value, marked)?;
                    }
                }
                Ok(())
            }
            ExprKind::ArrayLiteral { elements } => {
                for element in elements {
                    if let ArrayLiteralElement::Value(value) = element {
                        self.mark_inference_source(value, marked)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn expr_kind(&self, expr: ExprId) -> Result<ExprKind, CompilerFailure> {
        Ok(self
            .ast
            .try_expr(expr)
            .map_err(super::arena_failure)?
            .kind
            .clone())
    }
}

/// The types an object type gives its fields by name.
struct FieldTypes {
    named: std::collections::BTreeMap<String, crate::ObjectField>,
    index: Option<Type>,
}

impl FieldTypes {
    fn get(&self, name: &str) -> Option<&Type> {
        self.named
            .get(name)
            .map(|field| &field.ty)
            .or(self.index.as_ref())
    }
}

/// Whether `param` types a value only through one of `type_params`: it is one,
/// or a union with one as a member and none inside another member, since tsc
/// infers from a member's structure before a bare member.
fn is_typed_by_type_param(param: &Type, type_params: &[String]) -> bool {
    let is_bare = |ty: &Type| is_bare(ty, type_params);
    let mentions_one =
        |ty: &Type| mentions_type_var(ty, &|var| type_params.iter().any(|p| p == var));
    match param {
        Type::Union(members) => {
            members.iter().any(is_bare)
                && members
                    .iter()
                    .all(|member| is_bare(member) || !mentions_one(member))
        }
        other => is_bare(other),
    }
}

fn is_bare(ty: &Type, type_params: &[String]) -> bool {
    matches!(ty.peel(), Type::TypeVar(name) if type_params.contains(name))
}

/// The type an array literal's element at `index` takes from `param`.
fn element_type(param: &Type, index: usize) -> Option<&Type> {
    match param {
        Type::Array(element) => Some(element),
        Type::Tuple(elements) => elements.get(index),
        _ => None,
    }
}
