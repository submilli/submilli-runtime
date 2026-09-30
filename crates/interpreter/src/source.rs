//! The source registry: maps a [`FileId`] to its source text and display path.
//!
//! Every [`Span`](crate::Span) carries a `FileId` indexing into a [`Sources`].
//! Single-file scripts build a registry with one entry; multi-file packages add
//! one per module. Diagnostics resolve a span back to `(path, text, line index)`
//! through this registry so they can render `--> util.subm:5:12` for the right file.

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::span::{FileId, LineIndex, Span};

#[derive(Debug)]
pub enum SourceError {
    InvalidSpan { span: Span, reason: &'static str },
    InvalidOffset { offset: u32 },
    InvalidPosition { line: u32, col: u32 },
    UnknownFile { file: FileId },
    SourceLimit { len: usize },
    FileLimit { len: usize },
    Allocation(std::collections::TryReserveError),
}

impl SourceError {
    pub const MAX_SOURCE_BYTES: usize = (u32::MAX - 4) as usize;

    pub fn check_source_len(len: usize) -> Result<(), Self> {
        if len > Self::MAX_SOURCE_BYTES {
            return Err(Self::SourceLimit { len });
        }
        Ok(())
    }

    pub fn into_compiler_failure(self, stage: CompilerStage) -> CompilerFailure {
        let message = self.to_string();
        match self {
            Self::SourceLimit { .. } | Self::FileLimit { .. } | Self::Allocation(_) => {
                CompilerFailure::Limit {
                    stage,
                    span: None,
                    message,
                    help: vec!["split the source into smaller modules".into()],
                }
            }
            _ => CompilerFailure::Internal {
                stage,
                span: None,
                message,
            },
        }
    }
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSpan { span, reason } => {
                write!(f, "invalid source span {span:?}: {reason}")
            }
            Self::InvalidOffset { offset } => write!(f, "invalid source byte offset {offset}"),
            Self::InvalidPosition { line, col } => {
                write!(f, "invalid source position {line}:{col}")
            }
            Self::UnknownFile { file } => write!(f, "unknown source file {}", file.0),
            Self::SourceLimit { len } => {
                write!(f, "source length {len} exceeds the supported byte limit")
            }
            Self::FileLimit { len } => {
                write!(f, "source file count {len} reaches the reserved file IDs")
            }
            Self::Allocation(error) => write!(f, "cannot reserve source storage: {error}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// A module's package-relative display path (e.g. `util`, `internal/math`). For
/// single-file scripts this is just the script filename. A thin newtype today;
/// later multi-file steps extend it with resolution helpers.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ModulePath(String);

impl ModulePath {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve a relative-path import specifier (`./util`, `../math`) against
    /// `self`, the *importing* module's package-relative path. Resolution is
    /// directory-relative like ES/Node: the base is the importer's directory —
    /// `self` minus its last path segment.
    ///
    /// A segment stack seeded with that directory walks the specifier's
    /// `/`-split segments: `.` and empty segments are skipped, `..` pops (an
    /// empty stack means it climbed above the package root → [`EscapesRoot`]),
    /// anything else is pushed. A trailing `.ts` or `.subm` extension is stripped. The
    /// result has no `./`, no `..`, no source extension, and `/` separators.
    ///
    /// Callers must classify with [`is_relative_specifier`] first — a package
    /// specifier passed here would be misread as a path.
    ///
    /// [`EscapesRoot`]: RelativeImportError::EscapesRoot
    pub fn resolve_relative(&self, specifier: &str) -> Result<ModulePath, RelativeImportError> {
        let mut stack: Vec<&str> = self
            .0
            .rsplit_once('/')
            .map_or(Vec::new(), |(dir, _last)| dir.split('/').collect());
        for segment in specifier.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    stack.pop().ok_or(RelativeImportError::EscapesRoot)?;
                }
                other => stack.push(other),
            }
        }
        let joined = stack.join("/");
        let canonical = joined
            .strip_suffix(".ts")
            .or_else(|| joined.strip_suffix(".subm"))
            .unwrap_or(&joined);
        Ok(ModulePath(canonical.to_string()))
    }
}

/// Error from resolving a relative import specifier against an importer's path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativeImportError {
    /// A `..` segment climbed above the package's root module.
    EscapesRoot,
}

/// True for ES-style relative-path specifiers (`./…`, `../…`). Package
/// specifiers (`submilli:http`, `@org/pkg`, `@mcp/x`) never match — none start
/// with `./` or `../`. Bare `.`/`..` (no trailing `/`) are intentionally not
/// treated as relative imports.
pub fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

impl From<&str> for ModulePath {
    fn from(s: &str) -> Self {
        ModulePath(s.to_string())
    }
}

impl From<String> for ModulePath {
    fn from(s: String) -> Self {
        ModulePath(s)
    }
}

impl std::fmt::Display for ModulePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One source file: its display path, full text, and a pre-built line index for
/// span → line/col resolution.
pub struct SourceFile {
    pub path: ModulePath,
    id: FileId,
    line_index: LineIndex,
}

impl SourceFile {
    pub fn text(&self) -> &str {
        self.line_index.source()
    }

    pub fn span_text(&self, span: Span) -> Result<&str, SourceError> {
        span.text(self.text(), self.id)
    }

    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }
}

/// Registry of every source file in a compilation, indexed by [`FileId`].
#[derive(Default)]
pub struct Sources {
    files: Vec<SourceFile>,
}

impl Sources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a file and return its `FileId`.
    pub fn add(
        &mut self,
        path: impl Into<ModulePath>,
        text: impl AsRef<str>,
    ) -> Result<FileId, SourceError> {
        let id = next_file_id(self.files.len())?;
        let line_index = LineIndex::new(text.as_ref())?;
        self.files.try_reserve(1).map_err(SourceError::Allocation)?;
        self.files.push(SourceFile {
            path: path.into(),
            id,
            line_index,
        });
        Ok(id)
    }

    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn find_path(&self, path: &str) -> Option<(FileId, &SourceFile)> {
        self.files
            .iter()
            .find(|file| file.path.as_str() == path)
            .map(|file| (file.id, file))
    }

    /// Convenience for the single-file path: a registry with one entry plus its `FileId`.
    pub fn single(
        path: impl Into<ModulePath>,
        text: impl AsRef<str>,
    ) -> Result<(Self, FileId), SourceError> {
        let mut sources = Self::new();
        let id = sources.add(path, text)?;
        Ok((sources, id))
    }
}

fn next_file_id(len: usize) -> Result<FileId, SourceError> {
    u32::try_from(len)
        .ok()
        .filter(|id| *id < FileId::FIRST_RESERVED)
        .map(FileId)
        .ok_or(SourceError::FileLimit { len })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_source_ids_stop_before_reserved_range() {
        let limit = FileId::FIRST_RESERVED as usize;
        assert_eq!(
            next_file_id(limit - 1).unwrap().0,
            FileId::FIRST_RESERVED - 1
        );
        assert!(matches!(
            next_file_id(limit),
            Err(SourceError::FileLimit { .. })
        ));
        assert!(next_file_id(usize::MAX).is_err());
    }

    #[test]
    fn add_assigns_sequential_ids() {
        let mut sources = Sources::new();
        let a = sources.add("a.subm", "let x = 1;").unwrap();
        let b = sources.add("b.subm", "let y = 2;").unwrap();
        assert_eq!(a, FileId(0));
        assert_eq!(b, FileId(1));
    }

    #[test]
    fn get_resolves_path_and_text() {
        let (sources, id) = Sources::single("script.subm", "let x = 1;").unwrap();
        let file = sources.get(id).unwrap();
        assert_eq!(file.path.as_str(), "script.subm");
        assert_eq!(file.text(), "let x = 1;");
    }

    #[test]
    fn get_unknown_file_is_none() {
        let sources = Sources::new();
        assert!(sources.get(FileId(0)).is_none());
        // Reserved ids are never stored in the registry.
        assert!(sources.get(FileId::PRELUDE).is_none());
    }

    #[test]
    fn find_path_returns_file_id_and_source() {
        let mut sources = Sources::new();
        sources.add("lib", "export const a = 1;").unwrap();
        let util = sources.add("util", "export const b = 2;").unwrap();

        let (id, file) = sources.find_path("util").expect("find util");

        assert_eq!(id, util);
        assert_eq!(file.text(), "export const b = 2;");
        assert!(sources.find_path("missing").is_none());
    }

    fn resolve(importer: &str, specifier: &str) -> Result<String, RelativeImportError> {
        ModulePath::from(importer)
            .resolve_relative(specifier)
            .map(|m| m.as_str().to_string())
    }

    #[test]
    fn resolve_relative_canonicalizes_against_importer_dir() {
        assert_eq!(resolve("lib", "./util").unwrap(), "util");
        assert_eq!(
            resolve("internal/handlers", "./util").unwrap(),
            "internal/util"
        );
        assert_eq!(resolve("internal/handlers", "../math").unwrap(), "math");
        assert_eq!(
            resolve("internal/handlers", "../shared/x").unwrap(),
            "shared/x"
        );
        assert_eq!(resolve("a/b/c", "./d/../e").unwrap(), "a/b/e");
    }

    #[test]
    fn resolve_relative_strips_subm_suffix() {
        assert_eq!(resolve("util", "./mod.subm").unwrap(), "mod");
    }

    #[test]
    fn resolve_relative_strips_ts_suffix() {
        assert_eq!(resolve("util", "./mod.ts").unwrap(), "mod");
    }

    #[test]
    fn resolve_relative_rejects_escaping_root() {
        assert_eq!(
            resolve("lib", "../x"),
            Err(RelativeImportError::EscapesRoot)
        );
        assert_eq!(
            resolve("util", "../../x"),
            Err(RelativeImportError::EscapesRoot)
        );
    }

    #[test]
    fn is_relative_specifier_classifies_dot_prefixes_only() {
        assert!(is_relative_specifier("./util"));
        assert!(is_relative_specifier("../math"));
        assert!(!is_relative_specifier("submilli:http"));
        assert!(!is_relative_specifier("@org/pkg"));
        assert!(!is_relative_specifier("@mcp/linear"));
        assert!(!is_relative_specifier("util"));
        // Bare `.` / `..` are deliberately not relative imports.
        assert!(!is_relative_specifier("."));
        assert!(!is_relative_specifier(".."));
    }
}
