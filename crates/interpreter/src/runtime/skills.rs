//! The embedder-supplied seam for `submilli:skills`: a program reads the skills
//! the harness offers, their instructions and the files bundled with them.
//!
//! Where skills come from is the harness's choice. Everything that crosses
//! [`SkillProvider`] is owned, serializable data, so a harness may implement it
//! in process or forward it over the wire.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::stdlib::shared::truncated;

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
    /// The name is not one skill name. The engine refuses these before asking
    /// the provider.
    InvalidName { name: String },
    /// The path is not a relative path inside the skill. The engine refuses
    /// these before asking the provider; a provider may refuse more.
    InvalidPath { path: String },
    /// The provider answered `load(name)` with a skill of another name. The
    /// engine refuses it: the policy decided on `name`.
    WrongSkill { name: String },
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
                "no skill named `{}`; call `list()` for the skills you may load",
                truncated(name)
            ),
            Self::FileNotFound { name, path } => write!(
                f,
                "skill `{}` has no file `{}`",
                truncated(name),
                truncated(path)
            ),
            Self::InvalidName { name } => write!(
                f,
                "`{}` is not a skill name: use a name as `list()` returns it — one segment of \
                 at most 4096 bytes, not empty, `.` or `..`, and without `/`, `\\` or NUL",
                truncated(name)
            ),
            Self::InvalidPath { path } => write!(
                f,
                "`{}` is not a path inside a skill: use a relative path of at most 4096 bytes \
                 with `/` separators, no empty, `.` or `..` segment, and no `\\`, `:` or NUL",
                truncated(path)
            ),
            Self::WrongSkill { name } => write!(
                f,
                "the harness answered `{}` with a different skill",
                truncated(name)
            ),
            Self::Failed { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SkillError {}

/// The harness side of `submilli:skills`.
pub trait SkillProvider: Send + Sync {
    /// Every skill the harness offers, each under a name [`Self::load`] accepts.
    /// The engine removes the ones the policy denies to the caller, and any whose
    /// name it would refuse, before the program sees the list.
    fn list<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SkillInfo>, SkillError>> + Send + 'a>>;

    /// The skill named `name`, whose returned `name` must be exactly `name`.
    ///
    /// `name` comes from the program and the policy decided on it as written, so
    /// match it byte for byte: no case folding, Unicode normalization or path
    /// canonicalization, or a policy that denies `secrets` would let `Secrets`
    /// through. The engine passes only one segment of at most 4096 bytes that is
    /// not empty, `.` or `..` and has no `/`, `\` or NUL; it may contain `:`, so
    /// never use it as a path.
    fn load<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Skill, SkillError>> + Send + 'a>>;

    /// The text of the file at `path` inside skill `name`. `name` is checked and
    /// must be matched as for [`Self::load`]. `path` is relative, uses `/`
    /// separators, has no empty, `.` or `..` segment and no `\`, `:` or NUL, and
    /// is at most 4096 bytes. The engine does not decode it: a provider that
    /// percent-decodes must check the decoded path again.
    fn read_file<'a>(
        &'a self,
        name: &'a str,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, SkillError>> + Send + 'a>>;
}
