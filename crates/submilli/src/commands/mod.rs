pub mod blueprint;
pub mod build;
pub mod builtins;
pub mod check;
mod discovery;
pub mod docs;
pub mod github;
pub mod http;
pub mod install;
pub mod local;
pub mod mcp;
pub mod playground;
pub mod run;
pub mod search;
pub mod secret;
pub mod server;
pub mod skill;
pub mod upgrade;

/// The front end the CLI compiles scripts with.
pub(crate) fn front_end() -> &'static dyn interpreter::FrontEnd {
    &interpreter::EngineFrontEnd
}
