//! Pre-infer pass eliminating destructuring patterns from [`Ast`].
//! Runs between [`crate::parse`] and [`crate::infer`]; downstream sees only plain
//! `Let`/`Const`/`ConstRest`/`ForOf` and `ParamDecl` with `pattern: None`.
//! Synthesised array-pattern `IndexAccess` nodes are tagged in [`Ast::pattern_origins`]
//! so the typechecker can rephrase index errors as destructure-specific diagnostics.

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::tree_height;

use crate::{
    ArrowBody, Ast, Binding, BindingKind, Expr, ExprId, ExprKind, Ident, ParamDecl, PatternOrigin,
    Span, Stmt, StmtId, StmtKind,
};

pub fn lower(mut ast: Ast) -> Result<Ast, CompilerFailure> {
    // Callers may supply an AST that did not come from `parse_checked`. Pattern
    // lowering reports its failures in the inference stage it feeds.
    tree_height::check_syntax(&ast).map_err(|failure| failure.with_stage(CompilerStage::Infer))?;
    let mut ctx = LowerCtx { next_tmp: 0 };
    ctx.lower_arrows(&mut ast)?;
    ctx.lower_function_params(&mut ast)?;
    ctx.lower_for_of_patterns(&mut ast)?;
    ctx.lower_for_pattern_inits(&mut ast)?;
    ctx.lower_pattern_stmts(&mut ast)?;
    Ok(ast)
}

/// Starts every `fresh` name, so none can collide with a source identifier.
const FRESH_MARKER: &str = "#pattern_";

/// The `fresh` prefix of a lowered destructured parameter.
const PATTERN_PARAM: &str = "p";

/// Whether `name` stands in for a destructured parameter, which has no source
/// name: either the fresh `#pattern_p_N` from lowering, or the parser's empty
/// placeholder on an interface method signature, which has no body to lower.
pub(crate) fn is_pattern_param(name: &str) -> bool {
    name.is_empty()
        || name
            .strip_prefix(FRESH_MARKER)
            .and_then(|rest| rest.strip_prefix(PATTERN_PARAM))
            .is_some_and(|rest| rest.starts_with('_'))
}

struct LowerCtx {
    next_tmp: u32,
}

impl LowerCtx {
    fn fresh(&mut self, prefix: &str, span: Span) -> Result<Ident, CompilerFailure> {
        let n = self.next_tmp;
        self.next_tmp = self
            .next_tmp
            .checked_add(1)
            .ok_or_else(|| CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                span: None,
                message: "pattern temporary capacity exceeded".into(),
                help: vec![],
            })?;
        Ok(Ident {
            // Distinct from source identifiers and the later desugar pass.
            name: format!("{FRESH_MARKER}{prefix}_{n}"),
            span,
        })
    }

    fn lower_arrows(&mut self, ast: &mut Ast) -> Result<(), CompilerFailure> {
        for id in ast
            .expr_ids()
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        {
            let has_pattern = match &ast
                .try_expr(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind
            {
                ExprKind::Arrow { params, .. } => params.iter().any(|p| p.pattern.is_some()),
                _ => false,
            };
            if has_pattern {
                self.lower_arrow(ast, id)?;
            }
        }

        Ok(())
    }

    fn lower_arrow(&mut self, ast: &mut Ast, id: ExprId) -> Result<(), CompilerFailure> {
        let arrow_span = ast
            .try_expr(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .span;
        // Clone the kind to release the read borrow before we start
        // mutating other arena slots.
        let kind = ast
            .try_expr(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind
            .clone();
        let ExprKind::Arrow {
            mut params,
            return_type,
            type_predicate,
            body,
        } = kind
        else {
            return Err(lowering_failure(
                "unexpected node kind during pattern lowering",
            ));
        };

        let mut decompose: Vec<StmtId> = Vec::new();
        self.materialise_pattern_params(ast, &mut params, &mut decompose)?;

        let new_body = match body {
            ArrowBody::Block(block_id) => {
                self.prepend_to_block(ast, block_id, decompose)?;
                ArrowBody::Block(block_id)
            }
            ArrowBody::Expr(expr_id) => {
                let return_span = ast
                    .try_expr(expr_id)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .span;
                let return_stmt = ast
                    .try_push_stmt(Stmt {
                        kind: StmtKind::Return(Some(expr_id)),
                        span: return_span,
                    })
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                decompose.push(return_stmt);
                let block = ast
                    .try_push_stmt(Stmt {
                        kind: StmtKind::Block(decompose),
                        span: arrow_span,
                    })
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                ArrowBody::Block(block)
            }
        };

        ast.try_expr_mut(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind = ExprKind::Arrow {
            params,
            return_type,
            type_predicate,
            body: new_body,
        };

        Ok(())
    }

    fn lower_function_params(&mut self, ast: &mut Ast) -> Result<(), CompilerFailure> {
        for id in ast
            .stmt_ids()
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        {
            let has_pattern = match &ast
                .try_stmt(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind
            {
                StmtKind::Function { params, .. } => params.iter().any(|p| p.pattern.is_some()),
                _ => false,
            };
            if has_pattern {
                self.lower_function(ast, id)?;
            }
        }

        Ok(())
    }

    fn lower_function(&mut self, ast: &mut Ast, id: StmtId) -> Result<(), CompilerFailure> {
        let stmt_span = ast
            .try_stmt(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .span;
        let kind = std::mem::replace(
            &mut ast
                .try_stmt_mut(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind,
            StmtKind::Block(Vec::new()), // sentinel, restored before return
        );
        let StmtKind::Function {
            name,
            generics,
            mut params,
            return_type,
            type_predicate,
            body,
            doc,
        } = kind
        else {
            return Err(lowering_failure(
                "unexpected node kind during pattern lowering",
            ));
        };

        let mut decompose: Vec<StmtId> = Vec::new();
        self.materialise_pattern_params(ast, &mut params, &mut decompose)?;
        self.prepend_to_block(ast, body, decompose)?;

        ast.try_stmt_mut(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind = StmtKind::Function {
            name,
            generics,
            params,
            return_type,
            type_predicate,
            body,
            doc,
        };
        // Span is unchanged, but reassert for the linter.
        let _ = stmt_span;

        Ok(())
    }

    fn lower_for_of_patterns(&mut self, ast: &mut Ast) -> Result<(), CompilerFailure> {
        for id in ast
            .stmt_ids()
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        {
            if !matches!(
                ast.try_stmt(id)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .kind,
                StmtKind::ForOfPattern { .. }
            ) {
                continue;
            }
            self.expand_for_of_pattern(ast, id)?;
        }

        Ok(())
    }

    fn expand_for_of_pattern(&mut self, ast: &mut Ast, id: StmtId) -> Result<(), CompilerFailure> {
        let stmt_span = ast
            .try_stmt(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .span;
        let kind = std::mem::replace(
            &mut ast
                .try_stmt_mut(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind,
            StmtKind::Block(Vec::new()),
        );
        let StmtKind::ForOfPattern {
            binding_kind,
            binding,
            ty,
            iter,
            body,
        } = kind
        else {
            return Err(lowering_failure(
                "unexpected node kind during pattern lowering",
            ));
        };

        let pattern_span = binding.span();
        let dst = self.fresh("dst", pattern_span)?;
        let is_const = matches!(binding_kind, BindingKind::Const);
        let mut decompose = self.emit_decompose(
            ast,
            binding,
            dst.clone(),
            is_const,
            /*doc=*/ None,
            /*tuple_len=*/ None,
        )?;
        let mut source_bindings = Vec::new();
        for &id in &decompose {
            match &ast
                .try_stmt(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind
            {
                StmtKind::Let { name, .. }
                | StmtKind::Const { name, .. }
                | StmtKind::ConstRest { name, .. } => source_bindings.push(name.clone()),
                _ => {}
            }
        }
        ast.for_of_pattern_bindings.insert(id, source_bindings);
        // The loop head and the user's body are distinct lexical scopes.
        decompose.push(body);
        let body = ast
            .try_push_stmt(Stmt {
                kind: StmtKind::Block(decompose),
                span: ast
                    .try_stmt(body)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .span,
            })
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;

        ast.try_stmt_mut(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind = StmtKind::ForOf {
            binding_kind,
            name: dst,
            ty,
            iter,
            body,
        };
        let _ = stmt_span;

        Ok(())
    }

    fn lower_for_pattern_inits(&mut self, ast: &mut Ast) -> Result<(), CompilerFailure> {
        for id in ast
            .stmt_ids()
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        {
            let StmtKind::For {
                init: Some(init), ..
            } = ast
                .try_stmt(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind
            else {
                continue;
            };
            let is_let = match ast
                .try_stmt(init)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind
            {
                StmtKind::LetPattern { .. } => true,
                StmtKind::ConstPattern { .. } => false,
                _ => continue,
            };
            self.hoist_for_pattern_init(ast, id, init, is_let)?;
        }

        Ok(())
    }

    /// `for (let [i, j] = a; c; u) body` becomes
    /// `{ const dst = a; let i = dst[0]; let j = dst[1]; for (; c; u) body }`.
    /// The bindings keep the loop's scope but lose the per-iteration copies a
    /// `for` head's `let` gets, which only a closure capturing them observes;
    /// inference rejects that case using [`Ast::for_pattern_init_bindings`].
    fn hoist_for_pattern_init(
        &mut self,
        ast: &mut Ast,
        id: StmtId,
        init: StmtId,
        is_let: bool,
    ) -> Result<(), CompilerFailure> {
        let mut hoisted = self.expand_pattern_stmt(ast, init)?;
        let loop_stmt = ast
            .try_stmt_mut(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
        let span = loop_stmt.span;
        let kind = std::mem::replace(&mut loop_stmt.kind, StmtKind::Block(Vec::new()));
        let StmtKind::For {
            condition,
            update,
            body,
            ..
        } = kind
        else {
            return Err(lowering_failure(
                "unexpected node kind during pattern lowering",
            ));
        };
        let lowered_loop = ast
            .try_push_stmt(Stmt {
                kind: StmtKind::For {
                    init: None,
                    condition,
                    update,
                    body,
                },
                span,
            })
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
        if is_let {
            let bindings = let_names(ast, &hoisted)?;
            if !bindings.is_empty() {
                ast.for_pattern_init_bindings.insert(lowered_loop, bindings);
            }
        }
        hoisted.push(lowered_loop);
        ast.try_stmt_mut(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind = StmtKind::Block(hoisted);

        Ok(())
    }

    fn lower_pattern_stmts(&mut self, ast: &mut Ast) -> Result<(), CompilerFailure> {
        let top = std::mem::take(&mut ast.top_level);
        ast.top_level = self.expand_list(ast, top)?;

        for id in ast
            .stmt_ids()
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        {
            let needs = matches!(
                ast.try_stmt(id)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .kind,
                StmtKind::Block(_)
            );
            if !needs {
                continue;
            }
            let StmtKind::Block(taken) = std::mem::replace(
                &mut ast
                    .try_stmt_mut(id)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .kind,
                StmtKind::Block(Vec::new()),
            ) else {
                return Err(lowering_failure(
                    "unexpected node kind during pattern lowering",
                ));
            };
            let expanded = self.expand_list(ast, taken)?;
            ast.try_stmt_mut(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind = StmtKind::Block(expanded);
        }

        Ok(())
    }

    fn expand_list(
        &mut self,
        ast: &mut Ast,
        ids: Vec<StmtId>,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let is_pattern = matches!(
                ast.try_stmt(id)
                    .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                    .kind,
                StmtKind::LetPattern { .. } | StmtKind::ConstPattern { .. }
            );
            if !is_pattern {
                out.push(id);
                continue;
            }
            let expanded = self.expand_pattern_stmt(ast, id)?;
            out.extend(expanded);
        }
        Ok(out)
    }

    fn expand_pattern_stmt(
        &mut self,
        ast: &mut Ast,
        id: StmtId,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let span = ast
            .try_stmt(id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .span;
        let kind = std::mem::replace(
            &mut ast
                .try_stmt_mut(id)
                .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
                .kind,
            StmtKind::Block(Vec::new()), // sentinel; the slot is unreachable post-lowering
        );
        let (is_const, binding, ty, value, doc) = match kind {
            StmtKind::LetPattern {
                binding,
                ty,
                value,
                doc,
            } => (false, binding, ty, value, doc),
            StmtKind::ConstPattern {
                binding,
                ty,
                value,
                doc,
            } => (true, binding, ty, value, doc),
            _ => {
                return Err(lowering_failure(
                    "unexpected node kind during pattern lowering",
                ));
            }
        };

        let tuple_len = match ty {
            None => tuple_source_len(ast, &binding, value)?,
            Some(_) => None,
        };
        if tuple_len.is_some() {
            ast.tuple_pattern_sources.insert(value);
        }
        let pattern_span = binding.span();
        let dst = self.fresh("dst", pattern_span)?;

        // temp is always `const` even for `let` patterns — it's never re-assigned
        let dst_stmt = ast
            .try_push_stmt(Stmt {
                kind: StmtKind::Const {
                    name: dst.clone(),
                    ty,
                    value,
                    doc: None,
                },
                span,
            })
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;

        let mut out = vec![dst_stmt];
        out.extend(self.emit_decompose(ast, binding, dst, is_const, doc, tuple_len)?);
        Ok(out)
    }

    fn materialise_pattern_params(
        &mut self,
        ast: &mut Ast,
        params: &mut [ParamDecl],
        decompose: &mut Vec<StmtId>,
    ) -> Result<(), CompilerFailure> {
        for param in params.iter_mut() {
            let Some(pattern) = param.pattern.take() else {
                continue;
            };
            let fresh = self.fresh(PATTERN_PARAM, pattern.span())?;
            param.name = fresh.clone();
            decompose.extend(self.emit_decompose(
                ast, pattern, fresh, /*is_const=*/ true, /*doc=*/ None,
                /*tuple_len=*/ None,
            )?);
        }

        Ok(())
    }

    /// `doc` attaches to the first emitted stmt only. `tuple_len` is the length
    /// of the tuple-typed array literal `source` holds, if it holds one.
    fn emit_decompose(
        &mut self,
        ast: &mut Ast,
        binding: Binding,
        source: Ident,
        is_const: bool,
        mut doc: Option<crate::DocComment>,
        tuple_len: Option<usize>,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut out = Vec::new();
        match binding {
            Binding::Object {
                fields,
                rest,
                span: _,
            } => {
                let bound_names: Vec<Ident> = fields.iter().map(|f| f.source.clone()).collect();
                for field in fields {
                    let recv = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::Identifier(source.clone()),
                            span: field.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    let access = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::FieldAccess {
                                receiver: recv,
                                name: field.source.clone(),
                            },
                            span: field.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    let decl = ast
                        .try_push_stmt(Stmt {
                            kind: make_decl(is_const, field.local, access, doc.take()),
                            span: field.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    out.push(decl);
                }
                if let Some(rest_ident) = rest {
                    let src_expr = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::Identifier(source.clone()),
                            span: rest_ident.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    let decl = ast
                        .try_push_stmt(Stmt {
                            kind: StmtKind::ConstRest {
                                name: rest_ident.clone(),
                                source: src_expr,
                                exclude: bound_names,
                                ty: None,
                                doc: doc.take(),
                            },
                            span: rest_ident.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    out.push(decl);
                }
            }
            Binding::Array {
                elems,
                rest,
                span: pattern_span,
            } => {
                let elems_len = elems.len();
                for (i, slot) in elems.into_iter().enumerate() {
                    let Some(local) = slot else { continue };
                    let recv = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::Identifier(source.clone()),
                            span: local.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    let idx = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::Number(i as f64),
                            span: local.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    let access = ast
                        .try_push_expr(Expr {
                            kind: ExprKind::IndexAccess {
                                receiver: recv,
                                index: idx,
                            },
                            span: local.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    // tag so typechecker rephrases index errors as destructure-specific
                    ast.pattern_origins.insert(
                        access,
                        PatternOrigin {
                            pattern_span,
                            slot_arity: elems_len,
                        },
                    );
                    let decl = ast
                        .try_push_stmt(Stmt {
                            kind: make_decl(is_const, local.clone(), access, doc.take()),
                            span: local.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    out.push(decl);
                }
                if let Some(rest_ident) = rest {
                    let rest_value = match tuple_len {
                        Some(len) if len > elems_len => {
                            remaining_elements(ast, &source, elems_len..len, rest_ident.span)?
                        }
                        _ => array_rest(ast, &source, elems_len, rest_ident.span)?,
                    };
                    let decl = ast
                        .try_push_stmt(Stmt {
                            kind: make_decl(is_const, rest_ident.clone(), rest_value, doc.take()),
                            span: rest_ident.span,
                        })
                        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?;
                    out.push(decl);
                }
            }
        }
        Ok(out)
    }

    fn prepend_to_block(
        &mut self,
        ast: &mut Ast,
        block_id: StmtId,
        prefix: Vec<StmtId>,
    ) -> Result<(), CompilerFailure> {
        if prefix.is_empty() {
            return Ok(());
        }
        let original = match &ast
            .try_stmt(block_id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind
        {
            StmtKind::Block(stmts) => stmts.clone(),
            _ => {
                return Err(lowering_failure(
                    "unexpected node kind during pattern lowering",
                ));
            }
        };
        let combined: Vec<StmtId> = prefix.into_iter().chain(original).collect();
        ast.try_stmt_mut(block_id)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind = StmtKind::Block(combined);

        Ok(())
    }
}

/// The length of `value` when it is an array literal without spreads that
/// `binding`, an array pattern, takes apart slot by slot, so TypeScript types
/// it as a tuple. A pattern of only a rest element takes the literal whole, as
/// an array.
fn tuple_source_len(
    ast: &Ast,
    binding: &Binding,
    value: ExprId,
) -> Result<Option<usize>, CompilerFailure> {
    let Binding::Array { elems, .. } = binding else {
        return Ok(None);
    };
    if elems.is_empty() {
        return Ok(None);
    }
    let ExprKind::ArrayLiteral { elements } = &ast
        .try_expr(value)
        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
        .kind
    else {
        return Ok(None);
    };
    let has_spread = elements
        .iter()
        .any(|element| matches!(element, crate::ArrayLiteralElement::Spread { .. }));
    Ok((!has_spread).then_some(elements.len()))
}

/// `source.slice(from, source.length)`: the elements an array pattern's rest
/// element collects.
fn array_rest(
    ast: &mut Ast,
    source: &Ident,
    from: usize,
    span: Span,
) -> Result<ExprId, CompilerFailure> {
    let from_lit = push_expr(ast, ExprKind::Number(from as f64), span)?;
    let slice_receiver = push_expr(ast, ExprKind::Identifier(source.clone()), span)?;
    let slice_callee = push_expr(
        ast,
        ExprKind::FieldAccess {
            receiver: slice_receiver,
            name: Ident {
                name: "slice".to_string(),
                span,
            },
        },
        span,
    )?;
    let length_receiver = push_expr(ast, ExprKind::Identifier(source.clone()), span)?;
    let length_access = push_expr(
        ast,
        ExprKind::FieldAccess {
            receiver: length_receiver,
            name: Ident {
                name: "length".to_string(),
                span,
            },
        },
        span,
    )?;
    let slice_call = push_expr(
        ast,
        ExprKind::Call {
            callee: slice_callee,
            type_args: None,
            args: vec![from_lit, length_access],
        },
        span,
    )?;
    Ok(slice_call)
}

/// `[source[from], …, source[to - 1]].slice(0, to - from)`: the rest of a
/// tuple-typed literal. The remaining elements form a tuple of their own, and
/// slicing it yields an array of their types: one that can grow, where
/// TypeScript types the rest as a tuple.
fn remaining_elements(
    ast: &mut Ast,
    source: &Ident,
    indices: std::ops::Range<usize>,
    span: Span,
) -> Result<ExprId, CompilerFailure> {
    let count = indices.len();
    let mut elements = Vec::with_capacity(count);
    for i in indices {
        let receiver = push_expr(ast, ExprKind::Identifier(source.clone()), span)?;
        let index = push_expr(ast, ExprKind::Number(i as f64), span)?;
        let access = push_expr(ast, ExprKind::IndexAccess { receiver, index }, span)?;
        elements.push(crate::ArrayLiteralElement::Value(access));
    }
    let remaining = push_expr(ast, ExprKind::ArrayLiteral { elements }, span)?;
    ast.tuple_pattern_sources.insert(remaining);
    let slice = push_expr(
        ast,
        ExprKind::FieldAccess {
            receiver: remaining,
            name: Ident {
                name: "slice".to_string(),
                span,
            },
        },
        span,
    )?;
    let from = push_expr(ast, ExprKind::Number(0.0), span)?;
    let to = push_expr(ast, ExprKind::Number(count as f64), span)?;
    push_expr(
        ast,
        ExprKind::Call {
            callee: slice,
            type_args: None,
            args: vec![from, to],
        },
        span,
    )
}

fn push_expr(ast: &mut Ast, kind: ExprKind, span: Span) -> Result<ExprId, CompilerFailure> {
    ast.try_push_expr(Expr { kind, span })
        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))
}

fn make_decl(
    is_const: bool,
    name: Ident,
    value: ExprId,
    doc: Option<crate::DocComment>,
) -> StmtKind {
    if is_const {
        StmtKind::Const {
            name,
            ty: None,
            value,
            doc,
        }
    } else {
        StmtKind::Let {
            name,
            ty: None,
            value,
            doc,
        }
    }
}

/// The names the `let` statements among `stmts` declare.
fn let_names(ast: &Ast, stmts: &[StmtId]) -> Result<Vec<Ident>, CompilerFailure> {
    let mut names = Vec::new();
    for &stmt in stmts {
        if let StmtKind::Let { name, .. } = &ast
            .try_stmt(stmt)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))?
            .kind
        {
            names.push(name.clone());
        }
    }
    Ok(names)
}

fn lowering_failure(message: &str) -> CompilerFailure {
    CompilerFailure::Internal {
        stage: CompilerStage::Infer,
        span: None,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::lower;
    use crate::{Asi, Binding, Stmt, StmtKind, Token, TokenKind, parse};

    fn pipeline(source: &str) -> crate::Ast {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let t = asi.next_token();
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        let (mut ast, diags) = parse(source, tokens, crate::FileId(0));
        assert!(diags.is_empty(), "unexpected parse diags: {diags:?}");
        ast = lower(ast).unwrap();
        ast
    }

    fn no_patterns_left(ast: &crate::Ast) {
        for i in 0..ast.stmts_len() {
            let s = ast.try_stmt(crate::StmtId(i as u32)).unwrap();
            assert!(
                !matches!(
                    s.kind,
                    StmtKind::LetPattern { .. }
                        | StmtKind::ConstPattern { .. }
                        | StmtKind::ForOfPattern { .. }
                ),
                "found unlowered pattern at stmt #{i}: {:?}",
                s.kind,
            );
            if let StmtKind::Function { params, .. } = &s.kind {
                for (pi, p) in params.iter().enumerate() {
                    assert!(
                        p.pattern.is_none(),
                        "function param #{pi} still has a pattern after lowering",
                    );
                }
            }
        }
        for i in 0..ast.exprs_len() {
            let e = ast.try_expr(crate::ExprId(i as u32)).unwrap();
            if let crate::ExprKind::Arrow { params, .. } = &e.kind {
                for (pi, p) in params.iter().enumerate() {
                    assert!(
                        p.pattern.is_none(),
                        "arrow param #{pi} still has a pattern after lowering",
                    );
                }
            }
        }
    }

    #[test]
    fn lowers_simple_object_destructure() {
        let ast = pipeline("const { a, b } = obj;");
        no_patterns_left(&ast);
        // top_level: __dst, a-decl, b-decl
        assert_eq!(ast.top_level.len(), 3);
        let names: Vec<String> = ast
            .top_level
            .iter()
            .map(|id| match &ast.try_stmt(*id).unwrap().kind {
                StmtKind::Const { name, .. } => name.name.clone(),
                other => panic!("expected Const, got {other:?}"),
            })
            .collect();
        assert!(names[0].starts_with("#pattern_dst_"));
        assert_eq!(names[1], "a");
        assert_eq!(names[2], "b");
    }

    #[test]
    fn lowers_renamed_object_destructure() {
        let ast = pipeline("const { a: x, b: y } = obj;");
        no_patterns_left(&ast);
        let names: Vec<String> = ast
            .top_level
            .iter()
            .map(|id| match &ast.try_stmt(*id).unwrap().kind {
                StmtKind::Const { name, .. } => name.name.clone(),
                other => panic!("expected Const, got {other:?}"),
            })
            .collect();
        assert_eq!(names[1], "x");
        assert_eq!(names[2], "y");
    }

    #[test]
    fn lowers_array_destructure() {
        let ast = pipeline("const [x, y] = arr;");
        no_patterns_left(&ast);
        assert_eq!(ast.top_level.len(), 3);
        if let StmtKind::Const { value, .. } = &ast.try_stmt(ast.top_level[1]).unwrap().kind {
            assert!(matches!(
                ast.try_expr(*value).unwrap().kind,
                crate::ExprKind::IndexAccess { .. }
            ));
        }
    }

    #[test]
    fn lowers_array_with_hole_and_rest() {
        let ast = pipeline("const [, x, ...rest] = arr;");
        no_patterns_left(&ast);
        // __dst, x (= arr[1]), rest (= arr.slice(2))
        assert_eq!(ast.top_level.len(), 3);
        if let StmtKind::Const { name, value, .. } = &ast.try_stmt(ast.top_level[1]).unwrap().kind {
            assert_eq!(name.name, "x");
            if let crate::ExprKind::IndexAccess { index, .. } = &ast.try_expr(*value).unwrap().kind
            {
                assert_eq!(
                    ast.try_expr(*index).unwrap().kind,
                    crate::ExprKind::Number(1.0)
                );
            } else {
                panic!("expected IndexAccess");
            }
        }
        if let StmtKind::Const { name, value, .. } = &ast.try_stmt(ast.top_level[2]).unwrap().kind {
            assert_eq!(name.name, "rest");
            match &ast.try_expr(*value).unwrap().kind {
                crate::ExprKind::Call { callee, args, .. } => {
                    assert_eq!(args.len(), 2, "slice takes (start, end)");
                    assert_eq!(
                        ast.try_expr(args[0]).unwrap().kind,
                        crate::ExprKind::Number(2.0)
                    );
                    match &ast.try_expr(*callee).unwrap().kind {
                        crate::ExprKind::FieldAccess { name, .. } => {
                            assert_eq!(name.name, "slice");
                        }
                        other => panic!("expected slice FieldAccess, got {other:?}"),
                    }
                }
                other => panic!("expected Call, got {other:?}"),
            }
        }
    }

    #[test]
    fn lowers_object_rest_to_const_rest() {
        let ast = pipeline("const { a, ...rest } = obj;");
        no_patterns_left(&ast);
        assert_eq!(ast.top_level.len(), 3);
        match &ast.try_stmt(ast.top_level[2]).unwrap().kind {
            StmtKind::ConstRest { name, exclude, .. } => {
                assert_eq!(name.name, "rest");
                assert_eq!(exclude.len(), 1);
                assert_eq!(exclude[0].name, "a");
            }
            other => panic!("expected ConstRest, got {other:?}"),
        }
    }

    #[test]
    fn lowers_function_param_pattern() {
        let src = "function f({ a, b }: T): number { return a + b; }";
        let ast = pipeline(src);
        no_patterns_left(&ast);
        let Stmt {
            kind: StmtKind::Function { body, params, .. },
            ..
        } = ast.try_stmt(ast.top_level[0]).unwrap().clone()
        else {
            panic!("expected Function");
        };
        assert_eq!(params.len(), 1);
        assert!(params[0].pattern.is_none());
        assert!(params[0].name.name.starts_with("#pattern_p_"));
        let body_stmts = match &ast.try_stmt(body).unwrap().kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("body is not a Block"),
        };
        // a-decl, b-decl, return  (no #pattern_dst — params bind directly to #pattern_p_N)
        assert_eq!(body_stmts.len(), 3);
        if let StmtKind::Const { name, .. } = &ast.try_stmt(body_stmts[0]).unwrap().kind {
            assert_eq!(name.name, "a");
        } else {
            panic!("expected Const a");
        }
        if let StmtKind::Const { name, .. } = &ast.try_stmt(body_stmts[1]).unwrap().kind {
            assert_eq!(name.name, "b");
        } else {
            panic!("expected Const b");
        }
        assert!(matches!(
            ast.try_stmt(body_stmts[2]).unwrap().kind,
            StmtKind::Return(_)
        ));
    }

    #[test]
    fn lowers_arrow_expr_body_with_pattern() {
        let src = "const f = ({a, b}: T): number => a + b;";
        let ast = pipeline(src);
        no_patterns_left(&ast);
        let mut found = false;
        for i in 0..ast.exprs_len() {
            let e = ast.try_expr(crate::ExprId(i as u32)).unwrap();
            if let crate::ExprKind::Arrow { body, .. } = &e.kind {
                assert!(matches!(body, crate::ArrowBody::Block(_)));
                found = true;
            }
        }
        assert!(found, "no Arrow found in expr arena");
    }

    #[test]
    fn lowers_for_of_array_pattern() {
        // `for (const [a, b] of pairs) { … }` becomes
        // `for (const #pattern_dst_N of pairs) { const a = #pattern_dst_N[0];
        // const b = #pattern_dst_N[1]; { ... } }` after lowering.
        let src = "function main(): void { for (const [a, b] of pairs) { use(a, b); } }";
        let ast = pipeline(src);
        no_patterns_left(&ast);

        let Stmt {
            kind: StmtKind::Function { body, .. },
            ..
        } = ast.try_stmt(ast.top_level[0]).unwrap().clone()
        else {
            panic!("expected Function");
        };
        let body_stmts = match &ast.try_stmt(body).unwrap().kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("function body is not a Block"),
        };
        assert_eq!(body_stmts.len(), 1, "main body has just the for-of");

        let (loop_name, loop_body) = match &ast.try_stmt(body_stmts[0]).unwrap().kind {
            StmtKind::ForOf { name, body, .. } => (name.name.clone(), *body),
            other => panic!("expected ForOf, got {other:?}"),
        };
        assert!(
            loop_name.starts_with("#pattern_dst_"),
            "loop var was rewritten to a synth temp; got {loop_name}",
        );

        let loop_body_stmts = match &ast.try_stmt(loop_body).unwrap().kind {
            StmtKind::Block(s) => s.clone(),
            _ => panic!("for-of body is not a Block"),
        };
        assert_eq!(loop_body_stmts.len(), 3);
        if let StmtKind::Const { name, value, .. } = &ast.try_stmt(loop_body_stmts[0]).unwrap().kind
        {
            assert_eq!(name.name, "a");
            assert!(matches!(
                ast.try_expr(*value).unwrap().kind,
                crate::ExprKind::IndexAccess { .. }
            ));
            // Origin-tagged so the typechecker can rephrase index errors.
            assert!(ast.pattern_origins.contains_key(value));
        } else {
            panic!("expected Const a");
        }
        if let StmtKind::Const { name, .. } = &ast.try_stmt(loop_body_stmts[1]).unwrap().kind {
            assert_eq!(name.name, "b");
        } else {
            panic!("expected Const b");
        }
    }

    #[test]
    fn lowers_let_pattern_preserves_mutability() {
        // __dst is const; per-field bindings follow source kind (`let` stays `let`)
        let ast = pipeline("let { a } = obj;");
        no_patterns_left(&ast);
        // __dst is Const; `a` is Let.
        assert!(matches!(
            ast.try_stmt(ast.top_level[0]).unwrap().kind,
            StmtKind::Const { .. }
        ));
        assert!(matches!(
            ast.try_stmt(ast.top_level[1]).unwrap().kind,
            StmtKind::Let { .. }
        ));
    }

    #[test]
    fn no_op_on_plain_program() {
        let src = "const x = 1; function main(): void { }";
        let before = {
            let mut asi = Asi::new(src, crate::FileId(0));
            let mut tokens: Vec<Token> = Vec::new();
            loop {
                let t = asi.next_token();
                let is_eof = matches!(t.kind, TokenKind::Eof);
                tokens.push(t);
                if is_eof {
                    break;
                }
            }
            let _ = asi.into_diagnostics();
            let (ast, _) = parse(src, tokens, crate::FileId(0));
            ast
        };
        let after = pipeline(src);
        assert_eq!(format!("{before:?}"), format!("{after:?}"));
    }

    #[test]
    fn lowers_pattern_in_nested_block() {
        let src = "function main(): void { const { a } = obj; }";
        let ast = pipeline(src);
        no_patterns_left(&ast);
    }

    #[test]
    fn idempotent() {
        let src = "const { a, b } = obj;";
        let mut ast1 = pipeline(src);
        let snapshot = format!("{ast1:?}");
        ast1 = lower(ast1).unwrap();
        assert_eq!(snapshot, format!("{ast1:?}"));
    }

    #[test]
    fn binding_span_preserved() {
        let (ast, _) = parse(
            "const { a } = obj;",
            tokens_of("const { a } = obj;"),
            crate::FileId(0),
        );
        match &ast.try_stmt(ast.top_level[0]).unwrap().kind {
            crate::StmtKind::ConstPattern {
                binding: Binding::Object { span, .. },
                ..
            } => {
                // `{ a }` starts at offset 6 and ends at 11 (inclusive of `}`).
                assert_eq!(*span, crate::Span::new(crate::FileId(0), 6, 11).unwrap());
            }
            other => panic!("expected ConstPattern Object, got {other:?}"),
        }
    }

    fn tokens_of(source: &str) -> Vec<Token> {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens = Vec::new();
        loop {
            let t = asi.next_token();
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        tokens
    }
}
