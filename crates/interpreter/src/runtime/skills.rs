//! The embedder-supplied seam for `submilli:skills`: a program reads the skills
//! the harness offers, their instructions and the files bundled with them.
//!
//! Where skills come from is the harness's choice. Everything that crosses
//! [`SkillProvider`] is owned, serializable data, so a harness may implement it
//! in process or forward it over the wire.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

pub const SKILLS_MODULE_NAME: &str = "submilli:skills";

/// One skill, as a listing shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    /// What the skill is for. The engine reduces it to one bounded line before a
    /// program sees it in a listing.
    pub description: Option<String>,
}

/// A loaded skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    /// The skill's instructions.
    pub content: String,
}

/// Why a skill or one of its files could not be read. Every message is safe to
/// show the program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillError {
    /// No [`SkillProvider`] is wired into the runtime.
    NotConfigured,
    /// The harness has no skill of this name.
    NotFound { name: String },
    /// The skill has no file at this path.
    FileNotFound { name: String, path: String },
    /// The path is not a relative path inside the skill. The engine refuses
    /// these before asking the provider; a provider may refuse more.
    InvalidPath { path: String },
    /// Reading failed for another reason. `message` is a fixed classification
    /// chosen by the provider.
    Failed { message: String },
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(
                f,
                "no skill provider is configured: this harness does not offer skills"
            ),
            Self::NotFound { name } => write!(
                f,
                "no skill named `{name}`; call `list()` for the skills you may load"
            ),
            Self::FileNotFound { name, path } => {
                write!(f, "skill `{name}` has no file `{path}`")
            }
            Self::InvalidPath { path } => write!(
                f,
                "`{path}` is not a path inside a skill: use a relative path without `.` or `..` segments"
            ),
            Self::Failed { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SkillError {}

/// The harness side of `submilli:skills`.
pub trait SkillProvider: Send + Sync {
    /// Every skill the harness offers. The engine removes the ones the policy
    /// denies to the caller before the program sees the list.
    fn list<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SkillInfo>, SkillError>> + Send + 'a>>;

    /// The skill named `name`.
    fn load<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Skill, SkillError>> + Send + 'a>>;

    /// The text of the file at `path` inside skill `name`. `path` is relative,
    /// uses `/` separators and has no empty, `.` or `..` segment.
    fn read_file<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, SkillError>> + Send + 'a>>;
}
