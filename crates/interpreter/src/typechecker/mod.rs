pub mod capture;
pub mod desugar;
pub mod infer;
pub mod json_schema;
pub mod json_strategy;
pub mod rules;
pub mod type_param_substitution;

pub use capture::capture;
pub(crate) use capture::{ResolvedLocals, resolve_locals};
pub use desugar::desugar;
pub use infer::{infer, infer_package};
pub use rules::{capability_binding_type, check};

pub(crate) fn arena_failure(
    error: crate::arena::ArenaError,
) -> crate::compiler_error::CompilerFailure {
    error.into_compiler_failure(crate::compiler_error::CompilerStage::Infer)
}

pub(crate) fn invariant_failure(
    message: impl Into<String>,
) -> crate::compiler_error::CompilerFailure {
    crate::compiler_error::CompilerFailure::Internal {
        stage: crate::compiler_error::CompilerStage::Infer,
        span: None,
        message: message.into(),
    }
}
