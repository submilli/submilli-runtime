//! Function declarations inside a function body, which become local closures.
//!
//! JavaScript hoists a function declaration: its binding holds the function
//! from the start of the enclosing block, so it can be called before the
//! declaration and can call itself or a later sibling. Here, every nested
//! function's binding is declared at the block's start holding a placeholder,
//! and assigned its closure afterwards, so recursion goes through the binding.
//!
//! A closure captures what it uses when it is created. One that uses no
//! `let`/`const` of its own block is created at the block's start, as
//! JavaScript would. One that does is created as soon as the last of those is
//! declared: using it earlier would read a variable before its declaration,
//! which JavaScript rejects at runtime and this rejects at compile time.

use crate::compiler_error::CompilerFailure;

use crate::{
    ArrowBody, ClosureBody, Ident, ParamDecl, Span, StmtId, StmtKind, Type, TypeAnnotation,
    TypedExpr, TypedExprKind, TypedParam, TypedStmt, TypedStmtKind,
};

use super::Inferer;

pub(in crate::typechecker) struct NestedFunction {
    name: Ident,
    stmt: StmtId,
    ty: Type,
    /// The block declaring it; its siblings share it.
    block: StmtId,
    /// The last declared `let`/`const` of its block that it uses, if any: its
    /// closure is created right after that declaration, not at the block's start.
    created_after: Option<Ident>,
    /// Its closure has been assigned.
    defined: bool,
    /// The sibling functions its body uses, which must be defined before it
    /// can be called.
    uses: Vec<usize>,
}

/// A nested function declaration's parts, cloned out of the AST.
struct Declaration {
    name: Ident,
    generics: Vec<Ident>,
    params: Vec<ParamDecl>,
    return_type: Option<TypeAnnotation>,
    type_predicate: Option<crate::TypePredicateAnnotation>,
    body: StmtId,
}

impl Inferer<'_> {
    /// Bind every function `block` declares directly and return the statements
    /// that open the block: each binding with its placeholder, then the closures
    /// of the functions that capture none of its locals.
    pub(super) fn declare_nested_functions(
        &mut self,
        block: StmtId,
        stmts: &[StmtId],
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut opening = Vec::new();
        let mut hoisted = Vec::new();
        for &stmt in stmts {
            let Some(declaration) = self.nested_declaration(stmt)? else {
                continue;
            };
            let Some(ty) = self.nested_function_type(&declaration)? else {
                self.scopes.insert(
                    declaration.name.name.clone(),
                    Type::Error,
                    true,
                    declaration.name.span,
                );
                continue;
            };
            let created_after = self
                .nested_function_creation_points
                .get(&declaration.name.span)
                .cloned();
            let index = self.nested_functions.len();
            if created_after.is_none() {
                hoisted.push(index);
            }
            self.nested_functions.push(NestedFunction {
                name: declaration.name.clone(),
                stmt,
                ty: ty.clone(),
                block,
                created_after,
                defined: false,
                uses: Vec::new(),
            });
            self.scopes.insert_nested_function(
                declaration.name.name.clone(),
                ty.clone(),
                declaration.name.span,
                index,
            );
            opening.push(self.placeholder_binding(&declaration, &ty)?);
        }
        for index in hoisted {
            opening.push(self.define_nested_function(index)?);
        }
        Ok(opening)
    }

    /// The closures to create after `stmt`: those of the nested functions of
    /// its block whose last captured local it declares.
    pub(super) fn define_nested_functions_after(
        &mut self,
        stmt: StmtId,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let declared = match &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind {
            StmtKind::Let { name, .. }
            | StmtKind::Const { name, .. }
            | StmtKind::ConstRest { name, .. } => name.span,
            _ => return Ok(Vec::new()),
        };
        let ready: Vec<usize> = self
            .nested_functions
            .iter()
            .enumerate()
            .filter(|(_, function)| {
                !function.defined
                    && function
                        .created_after
                        .as_ref()
                        .is_some_and(|local| local.span == declared)
            })
            .map(|(index, _)| index)
            .collect();
        ready
            .into_iter()
            .map(|index| self.define_nested_function(index))
            .collect::<Result<_, _>>()
    }

    /// Check a use of nested function `index`. Inside a sibling's body, it is
    /// recorded as that sibling's dependency; anywhere else, it and every
    /// sibling it uses must already be defined.
    pub(super) fn check_nested_function_use(
        &mut self,
        index: usize,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let block = self
            .nested_functions
            .get(index)
            .ok_or_else(|| super::inference_failure("invalid nested function index"))?
            .block;
        let mut enclosing = None;
        for &body in &self.nested_function_bodies {
            let function = self
                .nested_functions
                .get(body)
                .ok_or_else(|| super::inference_failure("missing enclosing nested function"))?;
            if function.block == block {
                enclosing = Some(body);
                break;
            }
        }
        if let Some(user) = enclosing {
            let uses = &mut self
                .nested_functions
                .get_mut(user)
                .ok_or_else(|| super::inference_failure("invalid nested function index"))?
                .uses;
            if !uses.contains(&index) {
                uses.push(index);
            }
            return Ok(());
        }
        let Some(missing) = self.first_undefined(index)? else {
            return Ok(());
        };
        let name = &self
            .nested_functions
            .get(index)
            .ok_or_else(|| super::inference_failure("invalid nested function index"))?
            .name
            .name;
        let missing = self
            .nested_functions
            .get(missing)
            .ok_or_else(|| super::inference_failure("invalid nested function index"))?;
        let local = missing.created_after.as_ref().ok_or_else(|| {
            super::inference_failure("missing nested function creation point").with_span(span)
        })?;
        // A local declared below the function is already reported where the
        // function's body reads it, as for any closure.
        if local.span.start > missing.name.span.start {
            return Ok(());
        }
        let message = if missing.name.name == *name {
            format!(
                "`{name}` is used before `{}`, which it uses, is declared",
                local.name
            )
        } else {
            format!(
                "`{name}` is used before `{}`, which `{}` uses, is declared",
                local.name, missing.name.name
            )
        };
        let help = format!(
            "move this use below the declaration of `{}`: a nested function can \
             only be called once the variables it uses exist",
            local.name
        );
        let note = (local.span, format!("`{}` is declared here", local.name));
        self.error_with_help_and_notes(span, message, vec![help], vec![note]);
        Ok(())
    }

    /// The first function not yet defined among `index` and the siblings it
    /// uses, directly or through one another.
    fn first_undefined(&self, index: usize) -> Result<Option<usize>, CompilerFailure> {
        let mut pending = vec![index];
        let mut seen = vec![index];
        while let Some(next) = pending.pop() {
            let function = self
                .nested_functions
                .get(next)
                .ok_or_else(|| super::inference_failure("invalid nested function index"))?;
            if !function.defined {
                return Ok(Some(next));
            }
            for &used in &function.uses {
                if !seen.contains(&used) {
                    seen.push(used);
                    pending.push(used);
                }
            }
        }
        Ok(None)
    }

    fn nested_declaration(&self, stmt: StmtId) -> Result<Option<Declaration>, CompilerFailure> {
        let StmtKind::Function {
            name,
            generics,
            params,
            return_type,
            type_predicate,
            body,
            ..
        } = &self.ast.try_stmt(stmt).map_err(super::arena_failure)?.kind
        else {
            return Ok(None);
        };
        Ok(Some(Declaration {
            name: name.clone(),
            generics: generics.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            type_predicate: type_predicate.clone(),
            body: *body,
        }))
    }

    /// The declared type, or `None` after reporting what a closure can't have.
    fn nested_function_type(
        &mut self,
        declaration: &Declaration,
    ) -> Result<Option<Type>, CompilerFailure> {
        if let Some((span, message)) = nested_function_rejection(declaration) {
            self.error_with_help(
                span,
                message.to_string(),
                vec!["declare the function at the top level of the module".to_string()],
            );
            return Ok(None);
        }
        Ok(Some(self.declared_function_type(
            &declaration.params,
            declaration.return_type.as_ref(),
            declaration.type_predicate.as_ref(),
            None,
        )?))
    }

    /// `let name = <placeholder closure>`: the binding each nested function is
    /// assigned into. Every use before the real closure is assigned is
    /// rejected, so the placeholder never runs.
    fn placeholder_binding(
        &mut self,
        declaration: &Declaration,
        ty: &Type,
    ) -> Result<StmtId, crate::compiler_error::CompilerFailure> {
        let Type::Function { params, ret, .. } = ty else {
            return Err(
                super::inference_failure("nested function signature is not a function")
                    .with_span(declaration.name.span),
            );
        };
        let span = declaration.name.span;
        let body = self
            .typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Block(Vec::new()),
                span,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let params = declaration
            .params
            .iter()
            .zip(params)
            .map(|(param, ty)| TypedParam {
                name: param.name.clone(),
                ty: ty.clone(),
                boxed: false,
                rest: param.rest,
                default: None,
            })
            .collect();
        let placeholder = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Closure {
                    runtime_generics: Vec::new(),
                    params,
                    return_type: (**ret).clone(),
                    body: ClosureBody::Block(body),
                    captured: Vec::new(),
                },
                span,
                ty: ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        self.typed_ast.placeholder_closures.insert(placeholder);
        self.typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::Let {
                    name: declaration.name.clone(),
                    ty: ty.clone(),
                    value: placeholder,
                    boxed: false,
                    doc: None,
                },
                span,
            })
            .map_err(crate::typechecker::arena_failure)
    }

    /// Infer nested function `index`'s body as a closure and assign it to its
    /// binding.
    fn define_nested_function(&mut self, index: usize) -> Result<StmtId, CompilerFailure> {
        let declaration = self
            .nested_declaration(
                self.nested_functions
                    .get(index)
                    .ok_or_else(|| super::inference_failure("invalid nested function index"))?
                    .stmt,
            )?
            .ok_or_else(|| {
                super::inference_failure("registered nested function is not a declaration")
            })?;
        let ty = self
            .nested_functions
            .get(index)
            .ok_or_else(|| super::inference_failure("invalid nested function index"))?
            .ty
            .clone();
        let span = self
            .ast
            .try_stmt(declaration.body)
            .map_err(super::arena_failure)?
            .span;
        self.nested_function_bodies.push(index);
        self.enter_function_declaration_narrow_boundary();
        let (kind, closure_ty, _reported) = self.infer_arrow(
            declaration.params.clone(),
            declaration.return_type.clone(),
            declaration.type_predicate.clone(),
            ArrowBody::Block(declaration.body),
            Some(&ty),
            span,
        )?;
        self.exit_closure_narrow_boundary()?;
        self.nested_function_bodies.pop();
        self.nested_functions
            .get_mut(index)
            .ok_or_else(|| super::inference_failure("invalid nested function index"))?
            .defined = true;
        let value = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind,
                span,
                ty: closure_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        self.typed_ast
            .nested_function_names
            .insert(value, declaration.name.clone());
        self.typed_ast
            .try_push_stmt(TypedStmt {
                kind: TypedStmtKind::AssignLocal {
                    ident: declaration.name,
                    target_ty: ty,
                    value,
                    boxed: false,
                    narrowed_shadow_ty: None,
                },
                span,
            })
            .map_err(crate::typechecker::arena_failure)
    }
}

/// What a nested declaration has that a closure can't — type parameters or a
/// default value — and where.
fn nested_function_rejection(declaration: &Declaration) -> Option<(Span, &'static str)> {
    if let Some(generic) = declaration.generics.first() {
        return Some((generic.span, "a nested function cannot be generic"));
    }
    let defaulted = declaration.params.iter().find(|p| p.default.is_some())?;
    Some((
        defaulted.name.span,
        "a nested function's parameters cannot have default values",
    ))
}
