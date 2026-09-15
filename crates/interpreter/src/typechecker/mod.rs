pub mod capture;
pub mod desugar;
pub mod infer;
pub mod json_strategy;
pub mod rules;
pub mod type_param_substitution;

pub use capture::capture;
pub use desugar::desugar;
pub use infer::{infer, infer_package};
pub use rules::check;
