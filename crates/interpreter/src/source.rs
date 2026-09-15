//! The source registry: maps a [`FileId`] to its source text and display path.
//!
//! Every [`Span`](crate::Span) carries a `FileId` indexing into a [`Sources`].
//! Single-file scripts build a registry with one entry; multi-file packages add
//! one per module. Diagnostics resolve a span back to `(path, text, line index)`
//! through this registry so they can render `--> util.subm:5:12` for the right file.

use crate::span::{FileId, LineIndex};

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
    pub text: String,
    line_index: LineIndex,
}

impl SourceFile {
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
    pub fn add(&mut self, path: impl Into<ModulePath>, text: impl Into<String>) -> FileId {
        let text = text.into();
        let line_index = LineIndex::new(&text);
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile {
            path: path.into(),
            text,
            line_index,
        });
        id
    }

    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn find_path(&self, path: &str) -> Option<(FileId, &SourceFile)> {
        self.files
            .iter()
            .enumerate()
            .find(|(_, file)| file.path.as_str() == path)
            .map(|(index, file)| (FileId(index as u32), file))
    }

    /// Convenience for the single-file path: a registry with one entry plus its `FileId`.
    pub fn single(path: impl Into<ModulePath>, text: impl Into<String>) -> (Self, FileId) {
        let mut sources = Self::new();
        let id = sources.add(path, text);
        (sources, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_assigns_sequential_ids() {
        let mut sources = Sources::new();
        let a = sources.add("a.subm", "let x = 1;");
        let b = sources.add("b.subm", "let y = 2;");
        assert_eq!(a, FileId(0));
        assert_eq!(b, FileId(1));
    }

    #[test]
    fn get_resolves_path_and_text() {
        let (sources, id) = Sources::single("script.subm", "let x = 1;");
        let file = sources.get(id).unwrap();
        assert_eq!(file.path.as_str(), "script.subm");
        assert_eq!(file.text, "let x = 1;");
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
        sources.add("lib", "export const a = 1;");
        let util = sources.add("util", "export const b = 2;");

        let (id, file) = sources.find_path("util").expect("find util");

        assert_eq!(id, util);
        assert_eq!(file.text, "export const b = 2;");
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
