//! Recorded-world connectors: a run's outside-world connectors, answered from a
//! recorded run.
//!
//! Copied from `submilli-server`'s `record/replay/`.
//!
//! Every policy check runs before a transport is called, so a run whose HTTP client, MCP
//! transport and model provider are these enforces the current blueprint on every call and
//! has no route to the network. A call is answered with the next unused recording of the
//! same request; a call with none stops the run: the connector notes the miss in the
//! [`Cassette`], fires the run's cancel signal, and returns its error. The cancel ends the
//! run before the program can act on that error, so the program cannot catch the stop.
//!
//! The current blueprint's declarations are enforced beyond its permissions: an MCP server
//! or model it no longer declares is refused as a live run refuses it, whatever was recorded.
//!
//! A connector may be given its live counterpart for a mode that lets a call with nothing
//! recorded go live (`with_live`): the call is noted in the cassette and sent as a normal
//! run would send it.
//!
//! An embedding call is never answered from a recording, which keeps no vectors: it stops the
//! run, or goes live where the mode allows.
//!
//! The three share one [`Cassette`], built from the [`RecordedRun`](super::RecordedRun).

mod cassette;
mod embedding;
mod http;
mod llm;
mod mcp;

pub(crate) use cassette::call_key;
pub(crate) use cassette::{Cassette, Miss, MissReason, Nearest, Served};
pub(crate) use embedding::RecordedEmbeddingProvider;
pub(crate) use http::{LiveReach, RecordedHttpClient};
pub(crate) use llm::RecordedLlmProvider;
pub(crate) use mcp::RecordedMcpTransport;
