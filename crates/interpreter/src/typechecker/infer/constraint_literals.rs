//! Constraint context controls literal widening, never excess-property checks.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::compiler_error::CompilerFailure;
use crate::type_size::TypeBudget;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{
    ArrayLiteralElement, ArrowBody, ExprId, ExprKind, ObjectLiteralMember, Span, StmtId, StmtKind,
    Type,
};

use super::{Inferer, type_limit_at};

pub(super) struct ConstraintLiteralScope {
    kinds: HashMap<Span, LiteralKinds>,
    returns: HashMap<ExprId, Type>,
    constraints: BTreeMap<String, Type>,
}

impl ConstraintLiteralScope {
    fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.returns.is_empty()
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct LiteralKinds {
    string: bool,
    number: bool,
    boolean: bool,
    bigint: bool,
}

impl LiteralKinds {
    pub(super) fn is_empty(self) -> bool {
        !(self.string || self.number || self.boolean || self.bigint)
    }
    /// The kinds of literal a value keeps where `ty` is its context (a
    /// parameter's bound when `parameter`). A bound naming `keyof T` keeps
    /// string and number literals, as in tsc: `pick(o, ["a", "b"])` with `K
    /// extends keyof T`, or `K extends Extract<keyof T, string>`, binds
    /// `"a" | "b"`.
    fn from_context(ty: &Type, parameter: bool) -> Self {
        Self {
            string: ty.any_member(&|ty| {
                matches!(ty, Type::StringLiteral(_) | Type::TemplateLiteral { .. })
                    || literal_bound(ty)
                    || parameter && ty == &Type::String
            }),
            number: ty.any_member(&|ty| {
                matches!(ty, Type::NumberLiteral(_))
                    || literal_bound(ty)
                    || parameter && ty == &Type::Number
            }),
            boolean: ty.any_member(&|ty| matches!(ty, Type::Boolean | Type::BooleanLiteral(_))),
            bigint: ty.any_member(&|ty| {
                matches!(ty, Type::BigIntLiteral(_)) || parameter && ty == &Type::BigInt
            }),
        }
    }

    pub(super) fn keeps(self, ty: &Type) -> bool {
        match ty.peel() {
            Type::StringLiteral(_) => self.string,
            Type::NumberLiteral(_) => self.number,
            Type::BooleanLiteral(_) => self.boolean,
            Type::BigIntLiteral(_) => self.bigint,
            _ => false,
        }
    }

    fn merge(&mut self, other: Self) {
        self.string |= other.string;
        self.number |= other.number;
        self.boolean |= other.boolean;
        self.bigint |= other.bigint;
    }
}

impl Inferer<'_> {
    pub(super) fn with_constraint_literals<R>(
        &mut self,
        argument: ExprId,
        parameter: &Type,
        constraints: &BTreeMap<String, Type>,
        sub: &mut TypeParamSubstitution,
        infer: impl FnOnce(&mut Self, &mut TypeParamSubstitution) -> Result<R, CompilerFailure>,
    ) -> Result<R, CompilerFailure> {
        if constraints.is_empty() {
            return infer(self, sub);
        }
        let contexts = self.constraint_literal_contexts(argument, parameter, constraints, sub)?;
        if contexts.is_empty() {
            return infer(self, sub);
        }
        self.constraint_literal_scopes.push(contexts);
        let result = infer(self, sub);
        self.constraint_literal_scopes.pop();
        result
    }

    pub(super) fn constraint_literal_kinds(&self, span: Span) -> LiteralKinds {
        let mut kinds = LiteralKinds::default();
        for scope in &self.constraint_literal_scopes {
            if let Some(context) = scope.kinds.get(&span) {
                kinds.merge(*context);
            }
        }
        kinds
    }

    /// Return expressions are visited in their function's actual local scope,
    /// after parameters and preceding local constants have been declared.
    pub(super) fn deferred_constraint_literals(
        &self,
        expression: ExprId,
    ) -> Result<Option<ConstraintLiteralScope>, CompilerFailure> {
        let Some((ty, constraints)) =
            self.constraint_literal_scopes
                .iter()
                .rev()
                .find_map(|scope| {
                    scope
                        .returns
                        .get(&expression)
                        .map(|ty| (ty, &scope.constraints))
                })
        else {
            return Ok(None);
        };
        let scope = self.constraint_literal_contexts(
            expression,
            ty,
            constraints,
            &TypeParamSubstitution::new(),
        )?;
        Ok((!scope.is_empty()).then_some(scope))
    }

    fn constraint_literal_contexts(
        &self,
        argument: ExprId,
        parameter: &Type,
        constraints: &BTreeMap<String, Type>,
        sub: &TypeParamSubstitution,
    ) -> Result<ConstraintLiteralScope, CompilerFailure> {
        let span = self
            .ast
            .try_expr(argument)
            .map_err(super::arena_failure)?
            .span;
        let mut budget = self.type_limits.budget();
        let mut pending = Vec::new();
        enqueue(&mut pending, &mut budget, argument, parameter, false)
            .map_err(type_limit_at(span))?;
        let mut contexts = HashMap::<Span, LiteralKinds>::new();
        let mut returns = HashMap::new();
        let mut expanded = BTreeSet::new();
        while let Some((argument, ty, parameter)) = pending.pop() {
            budget.charge_work(1).map_err(type_limit_at(span))?;
            let argument = super::expr::peel_parens(self.ast, argument)?;
            if let Type::TypeVar(name) = ty.peel() {
                if expanded.insert((argument, name.clone()))
                    && let Some(bound) = constraints.get(name)
                {
                    let bound = sub
                        .apply(bound, &self.type_limits)
                        .map_err(type_limit_at(span))?;
                    // A key bound over a parameter inferred already
                    // evaluates: `Exclude<keyof U, "id">` is `"b"`. Any other
                    // bound keeps its form (`T[keyof T]` widens, as in tsc).
                    let bound = if literal_bound(&bound) {
                        super::utility_types::normalize(&bound, self.resolver())
                            .map_err(type_limit_at(span))?
                    } else {
                        bound
                    };
                    enqueue(&mut pending, &mut budget, argument, &bound, true)
                        .map_err(type_limit_at(span))?;
                }
                continue;
            }
            if let Type::Union(members) = ty.peel() {
                let expression = self.ast.try_expr(argument).map_err(super::arena_failure)?;
                if let ExprKind::ObjectLiteral { members: fields } = &expression.kind
                    && let Some(selected) = self.select_union_variant(members, fields)?
                {
                    enqueue(&mut pending, &mut budget, argument, selected, parameter)
                        .map_err(type_limit_at(span))?;
                    continue;
                }
                if matches!(expression.kind, ExprKind::ArrayLiteral { .. })
                    && let Some(array) = ty.array_like_union_view()
                {
                    enqueue(&mut pending, &mut budget, argument, &array, parameter)
                        .map_err(type_limit_at(span))?;
                    continue;
                }
                if !matches!(expression.kind, ExprKind::ObjectLiteral { .. })
                    || members
                        .iter()
                        .any(|member| matches!(member.peel(), Type::TypeVar(_)))
                {
                    for member in members {
                        let base = match member.peel() {
                            Type::TemplateLiteral { .. } => Some(Type::String),
                            literal if literal.is_literal_type() => Some(literal.widen_literal()),
                            _ => None,
                        };
                        if base
                            .as_ref()
                            .is_some_and(|base| members.iter().any(|other| other.peel() == base))
                        {
                            continue;
                        }
                        enqueue(&mut pending, &mut budget, argument, member, parameter)
                            .map_err(type_limit_at(span))?;
                    }
                    continue;
                }
            }
            let expression = self.ast.try_expr(argument).map_err(super::arena_failure)?;
            let kinds = LiteralKinds::from_context(&ty, parameter);
            if !kinds.is_empty() {
                contexts.entry(expression.span).or_default().merge(kinds);
            }
            match &expression.kind {
                ExprKind::ObjectLiteral { members } => {
                    let fields = self.field_slots(&ty).unwrap_or_default();
                    for member in members {
                        let (name, value) = match member {
                            ObjectLiteralMember::Field(field) => {
                                (field.name.name.clone(), field.value)
                            }
                            ObjectLiteralMember::Computed { key, value } => {
                                let Some(name) = self.literal_property_name(*key)? else {
                                    continue;
                                };
                                (name, *value)
                            }
                            ObjectLiteralMember::Spread { .. } => continue,
                        };
                        let index_value = self
                            .resolver()
                            .index_for_name(&ty, &name)
                            .map(|index| *index.value);
                        if let Some(slot) = fields.get(&name).or(index_value.as_ref()) {
                            enqueue(&mut pending, &mut budget, value, slot, false)
                                .map_err(type_limit_at(span))?;
                        }
                    }
                }
                ExprKind::ArrayLiteral { elements } => {
                    if let Type::Array(element) = ty.peel() {
                        for item in elements {
                            match item {
                                ArrayLiteralElement::Value(value) => {
                                    enqueue(&mut pending, &mut budget, *value, element, false)
                                }
                                ArrayLiteralElement::Spread { value, .. } => {
                                    enqueue(&mut pending, &mut budget, *value, &ty, false)
                                }
                            }
                            .map_err(type_limit_at(span))?;
                        }
                    } else if let Type::Tuple(slots) = ty.peel() {
                        for (item, slot) in elements.iter().zip(slots.iter()) {
                            let ArrayLiteralElement::Value(value) = item else {
                                break;
                            };
                            enqueue(&mut pending, &mut budget, *value, slot, false)
                                .map_err(type_limit_at(span))?;
                        }
                    }
                }
                ExprKind::Ternary { then_, else_, .. } => {
                    enqueue(&mut pending, &mut budget, *then_, &ty, parameter)
                        .map_err(type_limit_at(span))?;
                    enqueue(&mut pending, &mut budget, *else_, &ty, parameter)
                        .map_err(type_limit_at(span))?;
                }
                ExprKind::Arrow { body, .. } => {
                    if let Type::Function { ret, .. } = ty.peel() {
                        let values = match body {
                            ArrowBody::Expr(value) => vec![*value],
                            ArrowBody::Block(body) => {
                                self.owned_return_values(*body, &mut budget)?
                            }
                        };
                        let ret = sub
                            .apply(ret, &self.type_limits)
                            .map_err(type_limit_at(span))?;
                        for value in values {
                            if let Some(previous) = returns.remove(&value) {
                                // The union adds a level and revisits its earlier
                                // alternatives while normalizing the new context.
                                let extent = crate::type_size::measure(
                                    &previous,
                                    crate::compiler_limits::MAX_TYPE_NODES,
                                    crate::compiler_limits::MAX_TYPE_DEPTH - 1,
                                )
                                .map_err(type_limit_at(span))?;
                                budget
                                    .charge_work(extent.nodes)
                                    .map_err(type_limit_at(span))?;
                                budget.charge_copy(&ret, 2).map_err(type_limit_at(span))?;
                                budget.charge(1).map_err(type_limit_at(span))?;
                                returns.insert(value, Type::union(vec![previous, ret.clone()]));
                            } else {
                                budget.charge_copy(&ret, 1).map_err(type_limit_at(span))?;
                                returns.insert(value, ret.clone());
                            }
                        }
                    }
                }
                ExprKind::FunctionExpression { function, .. } => {
                    enqueue(&mut pending, &mut budget, *function, &ty, parameter)
                        .map_err(type_limit_at(span))?;
                }
                _ => {}
            }
        }
        let mut return_constraints = BTreeMap::new();
        if !returns.is_empty() {
            for (name, bound) in constraints {
                let bound = sub
                    .apply(bound, &self.type_limits)
                    .map_err(type_limit_at(span))?;
                budget.charge_copy(&bound, 0).map_err(type_limit_at(span))?;
                return_constraints.insert(name.clone(), bound);
            }
        }
        Ok(ConstraintLiteralScope {
            kinds: contexts,
            returns,
            constraints: return_constraints,
        })
    }

    fn owned_return_values(
        &self,
        body: StmtId,
        budget: &mut TypeBudget<'_>,
    ) -> Result<Vec<ExprId>, CompilerFailure> {
        let mut pending = vec![body];
        let mut values = Vec::new();
        while let Some(statement) = pending.pop() {
            let statement = self.ast.try_stmt(statement).map_err(super::arena_failure)?;
            budget
                .charge_work(1)
                .map_err(type_limit_at(statement.span))?;
            match &statement.kind {
                StmtKind::Return(Some(value)) => values.push(*value),
                StmtKind::Block(statements) => pending.extend(statements),
                StmtKind::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    pending.push(*then_block);
                    pending.extend(else_block);
                }
                StmtKind::While { body, .. }
                | StmtKind::DoWhile { body, .. }
                | StmtKind::For { body, .. }
                | StmtKind::ForOf { body, .. }
                | StmtKind::ForOfPattern { body, .. } => pending.push(*body),
                StmtKind::Switch { cases, default, .. } => {
                    pending.extend(cases.iter().map(|case| case.body));
                    pending.extend(default.iter().map(|case| case.body));
                }
                StmtKind::Try {
                    body,
                    catches,
                    finally,
                } => {
                    pending.push(*body);
                    pending.extend(catches.iter().map(|clause| clause.body));
                    pending.extend(finally);
                }
                // Nested declarations and expressions own their returns.
                _ => {}
            }
        }
        Ok(values)
    }

    pub(super) fn literal_property_name(
        &self,
        expression: ExprId,
    ) -> Result<Option<String>, CompilerFailure> {
        Ok(match self.literal_tag_value(expression)? {
            Some(super::narrowing::LiteralValue::String(name)) => Some(name),
            Some(super::narrowing::LiteralValue::Number(value)) => {
                Some(crate::runtime::number::format_number_js(value.0))
            }
            _ => None,
        })
    }
}

fn enqueue(
    pending: &mut Vec<(ExprId, Type, bool)>,
    budget: &mut TypeBudget<'_>,
    expression: ExprId,
    ty: &Type,
    parameter: bool,
) -> Result<(), crate::type_size::TypeTooLarge> {
    budget.charge_copy(ty, 0)?;
    pending.push((expression, ty.clone(), parameter));
    Ok(())
}

/// Whether a bound keeps the literal a value is inferred from, as tsc keeps
/// one for a primitive constraint: `keyof T`, a string mapping
/// (`Capitalize<S>`), and `Extract`, `Exclude`, `NonNullable` or `NoInfer`
/// of either or of a type parameter (`Extract<keyof T, string>`, `Extract<U,
/// string>`). A bound in an index position (`T[keyof T]`) names values, so
/// tsc widens there. Read from the application as written, so a concrete one
/// (`Exclude<keyof U, "id">` once `U` is inferred) counts too.
pub(super) fn literal_bound(ty: &Type) -> bool {
    use super::utility_types::{Utility, utility};
    match ty.without_aliases() {
        Type::KeyOf(_) => true,
        Type::Union(members) => members.iter().any(literal_bound),
        Type::AliasRef { mangled, args, .. } => match utility(mangled) {
            Some(mapping) if mapping.is_string_mapping() => true,
            Some(Utility::Extract | Utility::Exclude | Utility::NonNullable | Utility::NoInfer) => {
                args.first().is_some_and(|operand| {
                    literal_bound(operand)
                        || matches!(
                            operand.without_aliases(),
                            Type::TypeVar(_) | Type::GenericParam { .. }
                        )
                })
            }
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn constraint_literal_context_is_bounded_and_next_compilation_succeeds() {
        std::thread::Builder::new()
            .stack_size(crate::compiler_limits::COMPILER_STACK_BYTES)
            .spawn(|| {
                let count = crate::compiler_limits::MAX_TYPE_NODES as usize / 6 + 32;
                let values = std::iter::repeat_n("{ kind: \"a\", x: 1 }", count).collect::<Vec<_>>().join(",");
                let source = format!("function count<T extends {{ kind: \"a\"; x: number }}>(values: T[]): number {{ return values.length; }}\nfunction main(): number {{ return count([{values}]); }}");
                let (_, diagnostics) = crate::typecheck_to_typed_ast(&source, crate::FileId(0));
                assert!(diagnostics.iter().any(|d| d.message.contains("type is larger than the compiler limit")), "{diagnostics:?}");
                let (_, healthy) = crate::typecheck_to_typed_ast("function main(): number { return 42; }", crate::FileId(0));
                assert!(healthy.iter().all(|d| d.severity != crate::Severity::Error), "{healthy:?}");
            }).unwrap().join().unwrap();
    }
}
