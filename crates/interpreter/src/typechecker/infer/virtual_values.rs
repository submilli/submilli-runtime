//! A function of a virtual package (`@tools`, `@mcp/<server>`, an embedder's
//! host tools) read as a value. Such a function has no Wasm function of its
//! own: a call lowers to the package's one host dispatch
//! ([`crate::TypedExprKind::VirtualCall`]). Reading it as a value reads a
//! synthesized top-level function that makes that call, so `typeof`, binding
//! and passing it as a callback work as for any other function, and every read
//! is the same closure.

use crate::compiler_error::CompilerFailure;
use crate::{Ident, MangledName, Span, Type, TypedExpr, TypedExprKind, TypedStmtKind};

use super::Inferer;
use super::expr::StaticCallTarget;

impl Inferer<'_> {
    /// Whether `package`'s functions are dispatched through its host call
    /// rather than defined as functions of their own.
    pub(super) fn is_virtual_package(&self, package: &str) -> bool {
        self.packages_by_name
            .get(package)
            .is_some_and(|declaration| declaration.virtual_dispatch.is_some())
    }

    /// The function that stands for virtual `package`'s `symbol` when it is
    /// read as a value. It is synthesized on the first read and reused after.
    pub(super) fn virtual_function_value(
        &mut self,
        package: &str,
        symbol: &str,
        mangled: &MangledName,
        params: &[crate::Param],
        ret: &Type,
        span: Span,
    ) -> Result<MangledName, CompilerFailure> {
        // The `:` keeps the name apart from every identifier a program writes.
        let adapter =
            crate::mangle::package_symbol(self.package_name, &format!("tool:value:{mangled}"));
        if self
            .typed_ast
            .functions
            .iter()
            .any(|function| function.mangled_name == adapter)
        {
            return Ok(adapter);
        }
        let typed_params: Vec<crate::TypedParam> = params
            .iter()
            .map(|param| crate::TypedParam {
                name: Ident {
                    name: param.name.clone(),
                    span,
                },
                optional: param.is_omittable(),
                ty: param.ty.clone(),
                boxed: false,
                rest: param.rest,
                default: None,
            })
            .collect();
        let mut args = Vec::with_capacity(typed_params.len());
        for param in &typed_params {
            args.push(
                self.typed_ast
                    .try_push_expr(TypedExpr {
                        kind: TypedExprKind::LocalRef {
                            ident: param.name.clone(),
                            boxed: false,
                        },
                        span,
                        ty: param.ty.clone(),
                    })
                    .map_err(crate::typechecker::arena_failure)?,
            );
        }
        let target = StaticCallTarget {
            package,
            symbol,
            mangled: mangled.clone(),
        };
        let (call, call_ty) = self.static_call_expr(target, args, None, ret, span)?;
        // A call the checker rejected keeps no function, so the next read
        // builds it again and reports the error again: a read inside an
        // inference trial drops the trial's diagnostics. The name then names
        // no function, which is safe because the reported error stops the
        // compile before codegen.
        if call_ty == Type::Error {
            return Ok(adapter);
        }
        let call = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: call,
                span,
                ty: call_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        let body = self.push_typed_stmt(TypedStmtKind::Return(Some(call)), span)?;
        let body = self.push_typed_stmt(TypedStmtKind::Block(vec![body]), span)?;
        self.typed_ast.functions.push(crate::TypedFunction {
            // Named as the program spells the tool, never as a function the
            // program could declare: entry selection looks `main` up by name.
            name: Ident {
                name: format!("{package}.{symbol}"),
                span,
            },
            mangled_name: adapter.clone(),
            generics: Default::default(),
            params: typed_params,
            return_type: ret.clone(),
            type_predicate: None,
            body,
            doc: None,
            span,
        });
        Ok(adapter)
    }
}
