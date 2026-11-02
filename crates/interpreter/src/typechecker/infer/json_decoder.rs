//! Compiler-supplied JSON decoders use ordinary closures, casts, and typed catches.
use super::Inferer;
use crate::{
    ClosureBody, ExprId, Ident, Intrinsic, Span, StmtId, Type, TypedCatchClause, TypedExpr,
    TypedExprKind, TypedObjectFieldOrigin, TypedObjectFieldSource, TypedObjectMember, TypedParam,
    TypedStmt, TypedStmtKind, compiler_error::CompilerFailure,
};

impl Inferer<'_> {
    /// `(text) => { try { return {ok: true, value: JSON.parse(text) as T}; }
    /// catch (e: SyntaxError) { ... } catch (e: TypeError) { ... }
    /// catch (e: RangeError) { ... } }`.
    /// The schema gate has already established that T is runtime-checkable.
    pub(super) fn json_decoder(
        &mut self,
        target: &Type,
        span: Span,
    ) -> Result<ExprId, CompilerFailure> {
        let text = Ident {
            name: "#decode_text".into(),
            span,
        };
        let input = self.decoder_expr(
            TypedExprKind::LocalRef {
                ident: text.clone(),
                boxed: false,
            },
            Type::String,
            span,
        )?;
        let (checked, ty) = self.schema_checked_cast(
            TypedExprKind::IntrinsicCall {
                kind: Intrinsic::JsonParse,
                args: vec![input],
            },
            target,
            span,
        )?;
        let value = self.decoder_expr(checked, ty, span)?;
        let ok = self.decoder_expr(
            TypedExprKind::Boolean(true),
            Type::BooleanLiteral(true),
            span,
        )?;
        let success = self.decoder_object(vec![("ok", ok), ("value", value)], span)?;
        let body = self.decoder_return(success, span)?;
        let syntax = self.decoder_failure("SyntaxError", span)?;
        let mismatch = self.decoder_failure("TypeError", span)?;
        // JSON.parse uses RangeError only for an out-of-range numeric literal.
        // The checked cast has no user callbacks and reports only TypeError;
        // resource, fuel and cancellation failures keep propagating.
        let range = self.decoder_failure("RangeError", span)?;
        let body = self.decoder_stmt(
            TypedStmtKind::Try {
                body,
                catches: vec![syntax, mismatch, range],
                finally: None,
            },
            span,
        )?;
        let return_type = crate::stdlib::llm::declaration::decoder_result_type(target.clone());
        let function_type = crate::stdlib::llm::declaration::decoder_type(target.clone());
        self.decoder_expr(
            TypedExprKind::Closure {
                runtime_generics: Vec::new(),
                params: vec![TypedParam {
                    name: text,
                    optional: false,
                    ty: Type::String,
                    boxed: false,
                    rest: false,
                    default: None,
                }],
                return_type,
                body: ClosureBody::Block(body),
                captured: Vec::new(),
            },
            function_type,
            span,
        )
    }

    fn decoder_failure(
        &mut self,
        class: &str,
        span: Span,
    ) -> Result<TypedCatchClause, CompilerFailure> {
        let binding = Ident {
            name: format!("#decode_{class}"),
            span,
        };
        let ty = Type::class_ref(
            crate::Package::prelude(),
            class,
            crate::mangle::prelude(class),
            Vec::new(),
        );
        let reason = if class == "SyntaxError" {
            // Do not expose completion text through JSON parser diagnostics.
            self.decoder_expr(
                TypedExprKind::String("invalid JSON: the model's response is not JSON".into()),
                Type::String,
                span,
            )?
        } else if class == "RangeError" {
            self.decoder_expr(
                TypedExprKind::String("invalid JSON: a number is out of range".into()),
                Type::String,
                span,
            )?
        } else {
            let error = self.decoder_expr(
                TypedExprKind::LocalRef {
                    ident: binding.clone(),
                    boxed: false,
                },
                ty.clone(),
                span,
            )?;
            self.decoder_expr(
                TypedExprKind::FieldAccess {
                    receiver: error,
                    name: Ident {
                        name: "message".into(),
                        span,
                    },
                },
                Type::String,
                span,
            )?
        };
        let ok = self.decoder_expr(
            TypedExprKind::Boolean(false),
            Type::BooleanLiteral(false),
            span,
        )?;
        let kind = self.decoder_expr(
            TypedExprKind::String(class.into()),
            Type::StringLiteral(class.into()),
            span,
        )?;
        let result =
            self.decoder_object(vec![("ok", ok), ("reason", reason), ("kind", kind)], span)?;
        let body = self.decoder_return(result, span)?;
        Ok(TypedCatchClause {
            binding,
            ty,
            body,
            boxed: false,
            span,
        })
    }

    fn decoder_object(
        &mut self,
        entries: Vec<(&str, ExprId)>,
        span: Span,
    ) -> Result<ExprId, CompilerFailure> {
        let mut fields = Vec::new();
        let mut types = std::collections::BTreeMap::new();
        let members = entries
            .iter()
            .map(|(_, value)| TypedObjectMember::Value(*value))
            .collect();
        for (name, value) in entries {
            let ty = self
                .typed_ast
                .try_expr(value)
                .map_err(crate::typechecker::arena_failure)?
                .ty
                .clone();
            types.insert(name.to_string(), crate::ObjectField::required(ty.clone()));
            fields.push(TypedObjectFieldOrigin {
                name: Ident {
                    name: name.into(),
                    span,
                },
                source: TypedObjectFieldSource::Literal(value),
                optional: false,
                ty,
            });
        }
        fields.sort_by(|left, right| left.name.name.cmp(&right.name.name));
        self.decoder_expr(
            TypedExprKind::ObjectLiteral { members, fields },
            Type::Object {
                fields: types,
                index: None,
            },
            span,
        )
    }

    fn decoder_return(&mut self, value: ExprId, span: Span) -> Result<StmtId, CompilerFailure> {
        let ret = self.decoder_stmt(TypedStmtKind::Return(Some(value)), span)?;
        self.decoder_stmt(TypedStmtKind::Block(vec![ret]), span)
    }

    fn decoder_expr(
        &mut self,
        kind: TypedExprKind,
        ty: Type,
        span: Span,
    ) -> Result<ExprId, CompilerFailure> {
        self.typed_ast
            .try_push_expr(TypedExpr { kind, ty, span })
            .map_err(crate::typechecker::arena_failure)
    }

    fn decoder_stmt(&mut self, kind: TypedStmtKind, span: Span) -> Result<StmtId, CompilerFailure> {
        self.typed_ast
            .try_push_stmt(TypedStmt { kind, span })
            .map_err(crate::typechecker::arena_failure)
    }
}
