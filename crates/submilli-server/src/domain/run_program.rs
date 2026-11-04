//! The program and execution conditions an accepted run must retain for replay.
use std::time::Duration;
use submilli_blueprint::{Blueprint, VarBindings};

#[derive(Clone, Debug, PartialEq)]
pub struct RunProgram {
    pub source: String,
    /// Application-tool declarations fixed at admission.
    pub tools: Option<super::tools::SessionTools>,
    /// Whether a call the blueprint asks about waits for its approver: true for
    /// a run admitted through the HTTP session API, which can answer it.
    pub approvals: bool,
    pub blueprint: Blueprint,
    pub variables: VarBindings,
    pub limits: RunLimits,
    /// Compatibility is checked before compilation or any native action.
    pub compatibility: String,
    /// The declarations the compiler saw; recovery never rediscovers them.
    pub mcp_catalog: std::sync::Arc<submilli_shared::mcp::McpCatalog>,
    /// Configured root identities and access modes, checked before replay.
    pub filesystem_config_fingerprint: Option<[u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunLimits {
    pub fuel: u64,
    pub max_wasm_stack: usize,
    pub memory_reservation: u64,
    pub memory_guard_size: u64,
    pub memory_reservation_for_growth: u64,
    pub max_store_bytes: u64,
    pub timeout: Option<Duration>,
    pub async_yield_fuel: Option<u64>,
    pub llm: Option<ModelLimits>,
    pub embedding: Option<EmbeddingLimits>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelLimits {
    pub per_execution_tokens: u64,
    pub max_held_tokens: u64,
    pub max_prompt_count: u64,
    pub max_prompt_bytes: u64,
    pub default_output_cap: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingLimits {
    pub per_execution_tokens: u64,
    pub max_held_tokens: u64,
    pub max_requests: u64,
    pub max_texts_per_call: u64,
    pub max_bytes_per_call: u64,
}
