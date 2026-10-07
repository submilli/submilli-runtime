//! Per-program GC box wrapper types for captured-binding storage.

use std::collections::HashSet;

use wasm_encoder::{
    CompositeInnerType, CompositeType, FieldType, StorageType, StructType, SubType, TypeSection,
    ValType,
};

use crate::codegen::symbol_table::SymbolTable;
use crate::{
    ClosureBody, ExprId, StmtId, Type, TypedAst, TypedExprKind, TypedParam, TypedStmtKind,
};

pub fn collect(
    ta: &TypedAst,
    symbols: &SymbolTable,
) -> Result<Vec<ValType>, crate::compiler_error::CompilerFailure> {
    let mut state = Collector {
        ta,
        symbols,
        seen: HashSet::new(),
        order: Vec::new(),
    };
    let function_bodies: Vec<(Vec<TypedParam>, StmtId)> = ta
        .functions
        .iter()
        .map(|f| (f.params.clone(), f.body))
        .collect();
    for (params, body) in function_bodies {
        for p in &params {
            if p.boxed {
                state.note(&p.ty)?;
            }
        }
        state.walk_stmt(body)?;
    }
    let stmt_ids: Vec<StmtId> = ta.top_level_statements.clone();
    for sid in stmt_ids {
        state.walk_stmt(sid)?;
    }
    // Class member bodies and field initializers: a closure declared in either
    // can capture a mutable binding, and its box cell needs a type just like a
    // top-level function's.
    for member in class_member_params(ta) {
        for p in member {
            if p.boxed {
                state.note(&p.ty)?;
            }
        }
    }
    for body in ta.class_body_roots() {
        state.walk_stmt(body)?;
    }
    for init in ta.class_field_initializers() {
        state.walk_expr(init)?;
    }
    Ok(state.order)
}

/// Parameter lists of every class member that has one, so a captured-and-boxed
/// member parameter registers its box type the way a function's does.
fn class_member_params(ta: &TypedAst) -> Vec<Vec<TypedParam>> {
    let mut out = Vec::new();
    for ty_decl in &ta.types {
        let crate::TypedTypeDecl::Class(c) = ty_decl else {
            continue;
        };
        if let Some(ctor) = &c.constructor {
            out.push(ctor.params.clone());
        }
        out.extend(c.methods.iter().map(|m| m.params.clone()));
        for a in &c.accessors {
            if let crate::TypedClassAccessor::Setter { param, .. } = a {
                out.push(vec![param.clone()]);
            }
        }
    }
    out
}

struct Collector<'a> {
    ta: &'a TypedAst,
    symbols: &'a SymbolTable,
    seen: HashSet<ValType>,
    order: Vec<ValType>,
}

impl Collector<'_> {
    /// Keyed on `slot_value_type`; see [`SymbolTable::box_type_idx`].
    fn note(&mut self, ty: &Type) -> Result<(), crate::compiler_error::CompilerFailure> {
        let valtype = self.symbols.slot_value_type(ty)?;
        if self.seen.insert(valtype) {
            self.order.push(valtype);
        };
        Ok(())
    }

    fn walk_stmt(&mut self, id: StmtId) -> Result<(), crate::compiler_error::CompilerFailure> {
        let _: () = match &self
            .ta
            .try_stmt(id)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
            TypedStmtKind::Block(stmts) => {
                for &s in stmts {
                    self.walk_stmt(s)?;
                }
            }
            TypedStmtKind::Let {
                ty, value, boxed, ..
            } => {
                if *boxed {
                    self.note(ty)?;
                }
                self.walk_expr(*value)?;
            }
            TypedStmtKind::Const { value, .. } => self.walk_expr(*value)?,
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.walk_expr(*condition)?;
                self.walk_stmt(*then_block)?;
                if let Some(eb) = else_block {
                    self.walk_stmt(*eb)?;
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.walk_expr(*condition)?;
                self.walk_stmt(*body)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.walk_stmt(*i)?;
                }
                if let Some(c) = condition {
                    self.walk_expr(*c)?;
                }
                if let Some(u) = update {
                    self.walk_stmt(*u)?;
                }
                self.walk_stmt(*body)?;
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.walk_expr(*iter)?;
                self.walk_stmt(*body)?;
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.walk_stmt(*body)?;
                self.walk_expr(*condition)?;
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.walk_expr(*discriminant)?;
                for comparison in cases.iter().flat_map(crate::TypedSwitchCase::label_comparisons) {
                    self.walk_expr(comparison)?;
                }
                for case in cases {
                    self.walk_stmt(case.body)?;
                }
                if let Some(d) = default {
                    self.walk_stmt(*d)?;
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(v) = value {
                    self.walk_expr(*v)?;
                }
            }
            TypedStmtKind::Expr(e) => self.walk_expr(*e)?,
            TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => self.walk_expr(*value)?,
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                self.walk_expr(*receiver)?;
                self.walk_expr(*value)?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.walk_expr(*receiver)?;
                self.walk_expr(*index)?;
                self.walk_expr(*value)?;
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                self.walk_expr(*source)?;
                self.walk_stmt(*body)?;
            }
            TypedStmtKind::Throw { value } => self.walk_expr(*value)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.walk_stmt(*body)?;
                for c in catches {
                    self.walk_stmt(c.body)?;
                }
                if let Some(f) = finally {
                    self.walk_stmt(*f)?;
                }
            }
        };
        Ok(())
    }

    fn walk_expr(&mut self, id: ExprId) -> Result<(), crate::compiler_error::CompilerFailure> {
        let _: () = match &self
            .ta
            .try_expr(id)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
            TypedExprKind::Closure {
                params,
                captured,
                body,
                ..
            } => {
                for p in params {
                    if p.boxed {
                        self.note(&p.ty)?;
                    }
                }
                for c in captured {
                    if c.boxed {
                        self.note(&c.ty)?;
                    }
                }
                match *body {
                    ClosureBody::Expr(e) => self.walk_expr(e)?,
                    ClosureBody::Block(b) => self.walk_stmt(b)?,
                }
            }
            TypedExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(*lhs)?;
                self.walk_expr(*rhs)?;
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.walk_expr(*effect)?;
                self.walk_expr(*result)?;
            }
            TypedExprKind::Sequence { stmts, result } => {
                for &stmt in stmts {
                    self.walk_stmt(stmt)?;
                }
                self.walk_expr(*result)?;
            }
            TypedExprKind::Unary { operand, .. } => self.walk_expr(*operand)?,
            TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
                self.walk_expr(*value)?;
            }
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. } => {
                for &a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.walk_expr(*callee)?;
                for &a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::GenericCall { args, .. } => {
                for a in args {
                    self.walk_expr(a.expr)?;
                }
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.walk_expr(*receiver)?;
                for &a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.walk_expr(*receiver)?;
                for a in args {
                    self.walk_expr(a.expr)?;
                }
            }
            TypedExprKind::IntrinsicCall { args, .. } => {
                for &a in args {
                    self.walk_expr(a)?;
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        self.walk_expr(expression)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for e in elements {
                    self.walk_expr(e.expr_id())?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for &e in elements {
                    self.walk_expr(e)?;
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.walk_expr(*receiver)?;
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                self.walk_expr(*receiver)?;
                self.walk_expr(*index)?;
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.walk_expr(*source)?;
                self.walk_expr(*inner)?;
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.walk_expr(*cond)?;
                self.walk_expr(*then_)?;
                self.walk_expr(*else_)?;
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.walk_expr(*lhs)?;
                self.walk_expr(*rhs)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.walk_expr(*base)?;
                for part in parts {
                    match part {
                        crate::TypedChainPart::Index { idx, .. } => self.walk_expr(*idx)?,
                        crate::TypedChainPart::Call { args, .. }
                        | crate::TypedChainPart::MethodCall { args, .. } => {
                            for a in args {
                                self.walk_expr(*a)?;
                            }
                        }
                        crate::TypedChainPart::Field { .. }
                        | crate::TypedChainPart::InterfaceProperty { .. }
                        | crate::TypedChainPart::NonNull { .. } => {}
                    }
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                crate::PostfixTarget::Local {
                    boxed, target_ty, ..
                } => {
                    if *boxed {
                        self.note(target_ty)?;
                    }
                }
                crate::PostfixTarget::Global { .. } => {}
                crate::PostfixTarget::Field { receiver, .. } => self.walk_expr(*receiver)?,
                crate::PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    self.walk_expr(*receiver)?;
                    self.walk_expr(*index)?;
                }
            },
            TypedExprKind::NonNullAssert { value } | TypedExprKind::Cast { value, .. } => {
                self.walk_expr(*value)?;
            }
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
        };
        Ok(())
    }
}

/// `is_final: false` so boxes can gain subtypes; no `supertype_idx` because they don't participate in `$Object` dispatch.
pub fn emit(
    value_types_to_emit: &[ValType],
    types: &mut TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
) {
    for val in value_types_to_emit {
        let field = FieldType {
            element_type: StorageType::Val(*val),
            mutable: true,
        };
        types.ty().subtype(&SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![field].into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
        symbols.record_box_type(*val, *next_type_idx);
        *next_type_idx += 1;
    }
}
