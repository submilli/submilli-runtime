//! Value operand inference and carrier checks for generic type arguments.

use crate::compiler_error::CompilerFailure;
use crate::{ExprId, Type};

use super::Inferer;

/// The inferred operand and whether an earlier contextual pass reported an error.
pub(super) struct ValueOperand {
    pub typed_expr: ExprId,
    pub ty: Type,
    pub already_errored: bool,
}

impl Inferer<'_> {
    /// Infer a value slot once, retaining errors from an earlier contextual pass.
    pub(super) fn infer_value_operand(
        &mut self,
        expr: ExprId,
        hint: Option<&Type>,
        cached: Option<(ExprId, Type, bool)>,
    ) -> Result<ValueOperand, CompilerFailure> {
        let errors_before = self.error_count();
        let (typed_expr, ty, cached_error) = cached.map_or_else(
            || {
                let (typed_expr, ty) = self.infer_expr(expr, hint)?;
                Ok((typed_expr, ty, false))
            },
            Ok::<_, CompilerFailure>,
        )?;
        let already_errored = cached_error || self.error_count() > errors_before;
        Ok(ValueOperand {
            typed_expr,
            ty,
            already_errored,
        })
    }
}

/// `never` remains forbidden where generic erasure requires a value carrier.
pub(super) fn valueless_within_type_argument(ty: &Type) -> Option<&Type> {
    match ty.peel() {
        t @ Type::Never => Some(t),
        Type::Union(members) => members.iter().find_map(valueless_within_type_argument),
        Type::Array(element) => valueless_within_type_argument(element),
        Type::Tuple(elements) => elements.iter().find_map(valueless_within_type_argument),
        Type::Function { params, .. } => params.iter().find_map(valueless_within_type_argument),
        Type::InterfaceRef { args, .. } | Type::ClassRef { args, .. } => {
            args.iter().find_map(valueless_within_type_argument)
        }
        _ => None,
    }
}
