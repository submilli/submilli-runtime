//! Overload selection reuses ordinary argument inference, including contextual
//! callback types and generic constraints, without committing rejected trials.
use std::collections::BTreeMap;

use super::Inferer;
use super::expr::{CallLift, StaticCallTarget};
use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Ident, MethodOverload, MethodSig, Span, Type, TypeAnnotation, TypedExprKind};

impl Inferer<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_overloaded_method_call(
        &mut self,
        receiver: ExprId,
        iface: crate::MangledName,
        member: crate::Member,
        signature: MethodSig,
        bindings: BTreeMap<String, Type>,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let mut candidates: Vec<_> = signature.signatures().collect();
        // TypeScript places signatures declaring a literal parameter before
        // general signatures. Aliases and literal unions do not specialize.
        candidates.sort_by_key(|candidate| {
            !candidate.params.iter().any(|param| {
                matches!(
                    param.ty,
                    Type::StringLiteral(_)
                        | Type::NumberLiteral(_)
                        | Type::BooleanLiteral(_)
                        | Type::BigIntLiteral(_)
                        | Type::Null
                )
            })
        });
        let mut winner = None;
        for (position, candidate) in candidates.iter().enumerate() {
            if !self.type_limits.spend_work(1) {
                return self.type_size_checkpoint(Some(span)).and_then(|()| {
                    Err(super::inference_failure(
                        "overload selection budget exhausted",
                    ))
                });
            }
            let applicable = self.with_expression_trial(|this| {
                let errors = this.error_count();
                this.infer_single_method_candidate(
                    receiver,
                    iface.clone(),
                    member.clone(),
                    candidate.clone(),
                    bindings.clone(),
                    type_args.clone(),
                    args.clone(),
                    expected,
                    span,
                )?;
                Ok(this.error_count() == errors)
            })?;
            if applicable {
                winner = Some(position);
                break;
            }
        }
        let Some(position) = winner else {
            let receiver_ty = self
                .typed_ast
                .try_expr(receiver)
                .map_err(super::arena_failure)?
                .ty
                .clone();
            let help = candidates
                .iter()
                .map(|sig| {
                    self.format_signature(super::format_signature::SignatureKind::Method {
                        receiver_ty: &receiver_ty,
                        member: member.key(),
                        sig,
                    })
                })
                .collect();
            self.error_with_help(
                span,
                format!(
                    "no overload of {} accepts these arguments",
                    member.key().describe()
                ),
                help,
            );
            self.walk_rejected_call_args(&args)?;
            return Ok((TypedExprKind::Undefined, Type::Error));
        };
        self.infer_single_method_candidate(
            receiver,
            iface,
            member,
            candidates.remove(position),
            bindings,
            type_args,
            args,
            expected,
            span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_single_method_candidate(
        &mut self,
        receiver: ExprId,
        iface: crate::MangledName,
        member: crate::Member,
        sig: MethodSig,
        bindings: BTreeMap<String, Type>,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let bindings: BTreeMap<_, _> = bindings
            .into_iter()
            .filter(|(name, _)| !sig.generics.names.contains(name))
            .collect();
        if bindings.is_empty() && sig.generics.is_empty() {
            self.infer_method_call(receiver, iface, member, sig, type_args, args, span)
        } else {
            self.infer_generic_method_call(
                receiver, iface, member, sig, bindings, type_args, args, expected, span,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_overloaded_function_call(
        &mut self,
        display: &str,
        callee_span: Span,
        target: StaticCallTarget<'_>,
        mut candidates: Vec<MethodOverload>,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        candidates.sort_by_key(|candidate| {
            !candidate.params.iter().any(|param| {
                matches!(
                    param.ty,
                    Type::StringLiteral(_)
                        | Type::NumberLiteral(_)
                        | Type::BooleanLiteral(_)
                        | Type::BigIntLiteral(_)
                        | Type::Null
                )
            })
        });
        let mut winner = None;
        for (position, candidate) in candidates.iter().enumerate() {
            if !self.type_limits.spend_work(1) {
                return self.type_size_checkpoint(Some(span)).and_then(|()| {
                    Err(super::inference_failure(
                        "overload selection budget exhausted",
                    ))
                });
            }
            let applicable = self.with_expression_trial(|this| {
                let errors = this.error_count();
                this.infer_single_function_candidate(
                    display,
                    callee_span,
                    StaticCallTarget {
                        package: target.package,
                        symbol: target.symbol,
                        mangled: target.mangled.clone(),
                    },
                    candidate.clone(),
                    type_args.clone(),
                    args.clone(),
                    expected,
                    span,
                )?;
                Ok(this.error_count() == errors)
            })?;
            if applicable {
                winner = Some(position);
                break;
            }
        }
        let Some(position) = winner else {
            let help = candidates
                .iter()
                .map(|sig| {
                    self.format_signature(super::format_signature::SignatureKind::Function {
                        name: display,
                        predicate: sig.predicate.as_ref(),
                        generics: &sig.generics,
                        params: &sig.params,
                        ret: &sig.ret,
                        doc: None,
                    })
                })
                .collect();
            self.error_with_help(
                span,
                format!("no overload of `{display}` accepts these arguments"),
                help,
            );
            self.walk_rejected_call_args(&args)?;
            return Ok((TypedExprKind::Undefined, Type::Error));
        };
        self.infer_single_function_candidate(
            display,
            callee_span,
            target,
            candidates.remove(position),
            type_args,
            args,
            expected,
            span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_single_function_candidate(
        &mut self,
        display: &str,
        callee_span: Span,
        target: StaticCallTarget<'_>,
        candidate: MethodOverload,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if !candidate.generics.is_empty() {
            return self.infer_generic_call(
                Ident {
                    name: display.to_string(),
                    span: callee_span,
                },
                candidate.generics,
                candidate.params,
                candidate.ret,
                target.mangled,
                candidate.predicate,
                type_args,
                args,
                expected,
                span,
                super::generic::GenericCallee::Function,
            );
        }
        if let Some(targs) = &type_args {
            self.error(
                span,
                format!(
                    "type arguments are only valid on generic functions; got {} argument(s)",
                    targs.len(),
                ),
            );
        }
        let typed_args = self.bind_param_call_args(
            &candidate.params,
            &candidate.ret,
            CallLift::Function { name: display },
            &args,
            span,
        )?;
        self.static_call_expr(
            target,
            typed_args,
            candidate.predicate.map(Box::new),
            &candidate.ret,
            span,
        )
    }
}
