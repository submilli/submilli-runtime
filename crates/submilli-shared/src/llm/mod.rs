//! The outbound model provider: the runtime side of an `llm.call` / `llm.batch`.
//!
//! Mirrors [`crate::mcp::transport`] — an [`interpreter::runtime::LlmProvider`]
//! implementation bound to one blueprint, resolving credentials from the same
//! secret store and reading its providers and models from the blueprint's `llm:`
//! block.

pub mod dispatch;
pub mod provider;
pub mod wire;

pub use dispatch::HttpModelDispatch;
pub use provider::{
    BlueprintLlmProvider, ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse,
    ProviderUsage, StopReason,
};
pub use wire::ProviderKind;
