//! Recorded-world connectors: the three outside-world traits of
//! [`HostServices`](crate::runner::HostServices), answered from a recorded run.
//!
//! Every policy check runs before a transport is called, so a run whose HTTP client, MCP
//! transport and model provider are these enforces the current blueprint on every call and
//! has no route to the network. A call is answered with the next unused recording of the
//! same request; a call with none stops the run: the connector notes the miss in the
//! [`Cassette`], fires the run's cancel signal, and returns its error. The cancel ends the
//! run before the program can act on that error, so the program cannot catch the stop.
//!
//! The three share one [`Cassette`], built from the [`RecordedRun`](super::RecordedRun).

mod cassette;
mod http;
mod llm;
mod mcp;

pub use cassette::{Cassette, Miss, MissReason, Nearest, ReplayReport, Served};
pub use http::RecordedHttpClient;
pub use llm::RecordedLlmProvider;
pub use mcp::RecordedMcpTransport;
