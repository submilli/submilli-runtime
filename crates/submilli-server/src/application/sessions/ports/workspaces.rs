use crate::application::sessions::error::SessionError;
use crate::domain::session::RootVfs;
use std::path::PathBuf;
use submilli_blueprint::{Blueprint, VarBindings};

pub(crate) trait SessionWorkspaces: Send + Sync {
    fn resolve_root(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<RootVfs, SessionError>;
    fn provision(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: &VarBindings,
        existed: bool,
    ) -> Result<(), SessionError>;
    fn list(&self) -> Result<Vec<(String, PathBuf)>, SessionError>;
}
