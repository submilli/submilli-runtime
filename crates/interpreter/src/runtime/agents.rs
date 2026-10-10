//! The embedder-supplied seam for `submilli:agents`: a program hands work to a
//! sub-agent and gets its result back.
//!
//! The engine owns the guest surface, the capability check and the call log;
//! the harness owns what an agent is and how it runs. Everything that crosses
//! [`AgentProvider`] is owned, serializable data, so a harness may implement it
//! in process or forward it over the wire.
//!
//! **The provider bounds the run.** The execution timeout is checked only while
//! Wasm runs, so it does not interrupt a pending host call: a sub-agent run
//! lasts as long as the provider lets it. Time limits, token spend, recursion
//! depth and cancellation of a child when its parent goes away are the
//! provider's to enforce. Dropping the future returned by
//! [`AgentProvider::run`] is how the engine cancels a run.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub const AGENTS_MODULE_NAME: &str = "submilli:agents";

/// One sub-agent run, as the program asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    /// The agent's name, as [`AgentProvider::agents`] lists it.
    pub agent: String,
    /// The task for the agent.
    pub input: String,
    /// A JSON Schema the result must satisfy, present when the program wrote a
    /// type argument. The engine checks the result against the type itself; the
    /// schema tells the agent what shape to answer in.
    pub schema: Option<String>,
    /// The package whose code made the call: `main` for the program itself.
    pub caller: String,
}

/// What a finished run hands back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentOutcome {
    /// The agent's final answer. When the request carried a schema, this is the
    /// JSON text of a value meant to satisfy it.
    pub text: String,
    /// What the run cost, as far as the provider knows. Recorded in the call
    /// log; the engine charges nothing for it.
    pub usage: Option<AgentUsage>,
}

/// Tokens a run spent. `None` means unknown, never zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// One agent a program may hand work to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    /// Operator-authored text on what the agent is for. The engine reduces it to
    /// one bounded line before a program sees it.
    pub description: Option<String>,
}

/// Why a run did not produce a result. Every message is safe to show the
/// program: none carries the input or the agent's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCallError {
    /// No [`AgentProvider`] is wired into the runtime.
    NotConfigured,
    /// The harness has no agent of this name.
    NotFound { agent: String },
    /// The run started and failed. `message` is a fixed classification chosen by
    /// the provider, not the agent's output.
    Failed { agent: String, message: String },
    /// The run was stopped before it finished.
    Cancelled { agent: String },
}

impl std::fmt::Display for AgentCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(
                f,
                "no agent provider is configured: this harness does not run sub-agents"
            ),
            Self::NotFound { agent } => write!(
                f,
                "no agent named `{agent}`; call `list()` for the agents you may use"
            ),
            Self::Failed { agent, message } => write!(f, "agent `{agent}` failed: {message}"),
            Self::Cancelled { agent } => write!(f, "agent `{agent}` was cancelled"),
        }
    }
}

impl std::error::Error for AgentCallError {}

/// The harness side of `submilli:agents`.
pub trait AgentProvider: Send + Sync {
    /// Run `request.agent` on `request.input` and return its final answer.
    fn run<'a>(
        &'a self,
        request: AgentRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AgentOutcome, AgentCallError>> + Send + 'a>>;

    /// Every agent the harness offers. The engine removes the ones the policy
    /// denies to the caller before the program sees the list.
    fn agents<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AgentInfo>, AgentCallError>> + Send + 'a>>;
}
