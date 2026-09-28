use crate::source::SourceError;
use serde::{Deserialize, Serialize};

/// Index into a [`Sources`](crate::Sources) registry, identifying the file a
/// [`Span`] points at.
///
/// User/script modules occupy the low ids (the registry stores their text).
/// The prelude and each stdlib package own a fixed *reserved* id near
/// `u32::MAX`; these have no source text and are not stored in the registry —
/// the diagnostic renderer maps them to a virtual path via [`reserved_path`].
///
/// [`reserved_path`]: FileId::reserved_path
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

impl FileId {
    pub const FIRST_RESERVED: u32 = u32::MAX - 21;
    pub const COMPILER: FileId = FileId(Self::FIRST_RESERVED);

    pub const PRELUDE: FileId = FileId(u32::MAX);
    pub const FS: FileId = FileId(u32::MAX - 1);
    pub const HTTP: FileId = FileId(u32::MAX - 2);
    pub const URL: FileId = FileId(u32::MAX - 3);
    pub const UUID: FileId = FileId(u32::MAX - 4);
    pub const CRYPTO: FileId = FileId(u32::MAX - 5);
    pub const SECURITY: FileId = FileId(u32::MAX - 6);
    pub const MATH: FileId = FileId(u32::MAX - 7);
    pub const JSON: FileId = FileId(u32::MAX - 8);
    pub const TEMPORAL: FileId = FileId(u32::MAX - 9);
    pub const MCP: FileId = FileId(u32::MAX - 10);
    pub const NUMBER: FileId = FileId(u32::MAX - 11);
    pub const STRING: FileId = FileId(u32::MAX - 12);
    pub const REGEX: FileId = FileId(u32::MAX - 13);
    pub const URI: FileId = FileId(u32::MAX - 14);
    pub const SECRETS: FileId = FileId(u32::MAX - 15);
    pub const TEST: FileId = FileId(u32::MAX - 16);
    pub const SESSION: FileId = FileId(u32::MAX - 17);
    pub const LLM: FileId = FileId(u32::MAX - 18);
    pub const CODE: FileId = FileId(u32::MAX - 20);
    pub const GIT: FileId = FileId(u32::MAX - 19);

    /// Virtual display path for a reserved (prelude/stdlib) id, e.g.
    /// `submilli:fs`. Returns `None` for ordinary user/script files, which
    /// instead resolve to a [`SourceFile`](crate::SourceFile) in the registry.
    pub fn reserved_path(self) -> Option<&'static str> {
        Some(match self {
            FileId::COMPILER => "<compiler>",
            FileId::PRELUDE => "<prelude>",
            FileId::CODE => "submilli:code",
            FileId::GIT => "submilli:git",
            FileId::FS => "submilli:fs",
            FileId::HTTP => "submilli:http",
            FileId::URL => "submilli:url",
            FileId::URI => "submilli:uri",
            FileId::UUID => "submilli:uuid",
            FileId::CRYPTO => "submilli:crypto",
            FileId::SECURITY => "submilli:security",
            FileId::MATH => "submilli:math",
            FileId::JSON => "submilli:json",
            FileId::TEMPORAL => "submilli:temporal",
            FileId::MCP => "submilli:mcp",
            FileId::NUMBER => "submilli:number",
            FileId::STRING => "submilli:string",
            FileId::REGEX => "submilli:regex",
            FileId::SECRETS => "submilli:secrets",
            FileId::SESSION => "submilli:session",
            FileId::LLM => "submilli:llm",
            FileId::TEST => "submilli:test",
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(file: FileId, start: u32, end: u32) -> Result<Self, SourceError> {
        let span = Self { file, start, end };
        if start > end {
            return Err(SourceError::InvalidSpan {
                span,
                reason: "start exceeds end",
            });
        }
        Ok(span)
    }

    pub fn text(self, source: &str, file: FileId) -> Result<&str, SourceError> {
        if self.file != file {
            return Err(SourceError::InvalidSpan {
                span: self,
                reason: "span belongs to another file",
            });
        }
        source
            .get(self.start as usize..self.end as usize)
            .ok_or(SourceError::InvalidSpan {
                span: self,
                reason: "range is outside source or splits a UTF-8 character",
            })
    }

    /// A zero-length placeholder anchored to `file` — for definitions and
    /// generated nodes that have a logical home file but no source range
    /// (prelude/stdlib declarations, desugared temporaries). The home is always
    /// named explicitly: a reserved id for prelude/stdlib, the script's id for
    /// compiler-generated user-code nodes.
    pub const fn at(file: FileId) -> Self {
        Self {
            file,
            start: 0,
            end: 0,
        }
    }

    pub fn merge(self, other: Self) -> Result<Self, SourceError> {
        Self::new(self.file, self.start, self.end)?;
        Self::new(other.file, other.start, other.end)?;
        if self.file != other.file {
            return Err(SourceError::InvalidSpan {
                span: other,
                reason: "cannot merge spans from different files",
            });
        }
        Self::new(
            self.file,
            self.start.min(other.start),
            self.end.max(other.end),
        )
    }

    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }
}

/// Owns the text as well as its index, so line access cannot use unrelated text.
#[derive(Clone, Debug)]
pub struct LineIndex {
    source: String,
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Result<Self, SourceError> {
        SourceError::check_source_len(source.len())?;
        let mut text = String::new();
        text.try_reserve(source.len())
            .map_err(SourceError::Allocation)?;
        text.push_str(source);
        Self::from_owned(text)
    }

    pub(crate) fn from_owned(source: String) -> Result<Self, SourceError> {
        SourceError::check_source_len(source.len())?;
        let mut line_starts = Vec::new();
        line_starts
            .try_reserve(1)
            .map_err(SourceError::Allocation)?;
        line_starts.push(0);
        let mut bytes = source.bytes().enumerate().peekable();
        while let Some((offset, byte)) = bytes.next() {
            let end = match byte {
                b'\n' => offset + 1,
                b'\r' => match bytes.peek() {
                    Some((_, b'\n')) => {
                        bytes.next();
                        offset + 2
                    }
                    _ => offset + 1,
                },
                _ => continue,
            };
            let end =
                u32::try_from(end).map_err(|_| SourceError::SourceLimit { len: source.len() })?;
            line_starts
                .try_reserve(1)
                .map_err(SourceError::Allocation)?;
            line_starts.push(end);
        }
        Ok(Self {
            source,
            line_starts,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn line_col(&self, offset: u32) -> Result<(u32, u32), SourceError> {
        if !self.source.is_char_boundary(offset as usize) {
            return Err(SourceError::InvalidOffset { offset });
        }
        let count = self.line_starts.partition_point(|&start| start <= offset);
        let start = count
            .checked_sub(1)
            .and_then(|i| self.line_starts.get(i))
            .ok_or(SourceError::InvalidOffset { offset })?;
        let line = u32::try_from(count).map_err(|_| SourceError::SourceLimit {
            len: self.source.len(),
        })?;
        let column = offset
            .checked_sub(*start)
            .and_then(|n| n.checked_add(1))
            .ok_or(SourceError::InvalidOffset { offset })?;
        Ok((line, column))
    }

    pub fn line_count(&self) -> u32 {
        // Source length is bounded below u32::MAX; each line consumes a byte.
        self.line_starts.len() as u32
    }

    pub fn byte_offset(&self, line: u32, col: u32) -> Result<u32, SourceError> {
        let error = || SourceError::InvalidPosition { line, col };
        let start = line
            .checked_sub(1)
            .and_then(|i| self.line_starts.get(i as usize))
            .ok_or_else(error)?;
        let offset = col
            .checked_sub(1)
            .and_then(|n| start.checked_add(n))
            .ok_or_else(error)?;
        let (actual_line, _) = self.line_col(offset).map_err(|_| error())?;
        if actual_line != line {
            return Err(error());
        }
        Ok(offset)
    }

    pub fn line_text(&self, line: u32) -> Result<&str, SourceError> {
        let start = line
            .checked_sub(1)
            .and_then(|i| self.line_starts.get(i as usize))
            .ok_or(SourceError::InvalidPosition { line, col: 1 })?;
        let end = self
            .line_starts
            .get(line as usize)
            .map_or(self.source.len(), |n| *n as usize);
        self.source
            .get(*start as usize..end)
            .map(|text| text.trim_end_matches(['\n', '\r']))
            .ok_or(SourceError::InvalidPosition { line, col: 1 })
    }
}

#[cfg(test)]
mod tests {
    use super::{FileId, LineIndex, Span};

    const F: FileId = FileId(0);

    #[test]
    fn new_stores_fields() {
        let s = Span::new(F, 3, 7).unwrap();
        assert_eq!(s.start, 3);
        assert_eq!(s.end, 7);
    }

    #[test]
    fn new_allows_empty_span() {
        let s = Span::new(F, 5, 5).unwrap();
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 5);
    }

    #[test]
    fn merge_overlapping() {
        assert_eq!(
            Span::new(F, 0, 5)
                .unwrap()
                .merge(Span::new(F, 3, 8).unwrap())
                .unwrap(),
            Span::new(F, 0, 8).unwrap()
        );
    }

    #[test]
    fn merge_disjoint_covers_gap() {
        assert_eq!(
            Span::new(F, 0, 2)
                .unwrap()
                .merge(Span::new(F, 5, 7).unwrap())
                .unwrap(),
            Span::new(F, 0, 7).unwrap()
        );
    }

    #[test]
    fn merge_identical() {
        let s = Span::new(F, 4, 9).unwrap();
        assert_eq!(s.merge(s).unwrap(), s);
    }

    #[test]
    fn merge_nested_returns_outer() {
        let outer = Span::new(F, 0, 10).unwrap();
        let inner = Span::new(F, 3, 5).unwrap();
        assert_eq!(outer.merge(inner).unwrap(), outer);
        assert_eq!(inner.merge(outer).unwrap(), outer);
    }

    #[test]
    fn merge_is_commutative() {
        let a = Span::new(F, 2, 6).unwrap();
        let b = Span::new(F, 4, 10).unwrap();
        assert_eq!(a.merge(b).unwrap(), b.merge(a).unwrap());
    }

    #[test]
    fn contains_start_is_inclusive() {
        assert!(Span::new(F, 3, 7).unwrap().contains(3));
    }

    #[test]
    fn contains_end_is_exclusive() {
        assert!(!Span::new(F, 3, 7).unwrap().contains(7));
    }

    #[test]
    fn contains_strictly_inside() {
        assert!(Span::new(F, 3, 7).unwrap().contains(5));
    }

    #[test]
    fn contains_before_start() {
        assert!(!Span::new(F, 3, 7).unwrap().contains(2));
    }

    #[test]
    fn contains_after_end() {
        assert!(!Span::new(F, 3, 7).unwrap().contains(8));
    }

    #[test]
    fn empty_span_contains_nothing() {
        let s = Span::new(F, 4, 4).unwrap();
        assert!(!s.contains(3));
        assert!(!s.contains(4));
        assert!(!s.contains(5));
    }

    #[test]
    fn line_index_empty_file() {
        let idx = LineIndex::new("").unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn line_index_single_line_no_terminator() {
        let idx = LineIndex::new("hello").unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(1).unwrap(), (1, 2));
        assert_eq!(idx.line_col(5).unwrap(), (1, 6));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn line_index_multi_line_lf() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(2).unwrap(), (1, 3));
        assert_eq!(idx.line_col(3).unwrap(), (1, 4)); // \n counts as last col of line 1
        assert_eq!(idx.line_col(4).unwrap(), (2, 1));
        assert_eq!(idx.line_col(7).unwrap(), (2, 4));
        assert_eq!(idx.line_col(8).unwrap(), (3, 1));
        assert_eq!(idx.line_col(10).unwrap(), (3, 3));
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn line_index_crlf() {
        let src = "abc\r\ndef";
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(4).unwrap(), (1, 5));
        assert_eq!(idx.line_col(5).unwrap(), (2, 1));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_bare_cr() {
        let src = "abc\rdef";
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(4).unwrap(), (2, 1));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_trailing_newline() {
        let src = "abc\n";
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(3).unwrap(), (1, 4));
        assert_eq!(idx.line_col(4).unwrap(), (2, 1)); // EOF = start of empty line 2
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_unicode() {
        // α, β, γ are each 2 UTF-8 bytes.
        let src = "αβ\nγ";
        assert_eq!(src.len(), 7);
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_col(0).unwrap(), (1, 1));
        assert_eq!(idx.line_col(2).unwrap(), (1, 3));
        assert!(idx.line_col(3).is_err()); // middle of a UTF-8 character
        assert_eq!(idx.line_col(4).unwrap(), (1, 5));
        assert_eq!(idx.line_col(5).unwrap(), (2, 1));
        assert!(idx.line_col(6).is_err());
    }

    #[test]
    fn line_index_rejects_past_eof() {
        let idx = LineIndex::new("abc").unwrap();
        assert!(idx.line_col(100).is_err());
    }

    #[test]
    fn line_index_rejects_past_eof_with_trailing_newline() {
        let idx = LineIndex::new("abc\n").unwrap();
        assert!(idx.line_col(999).is_err());
    }

    #[test]
    fn line_text_strips_terminators_and_clamps() {
        let src = "first\nsecond\r\nthird";
        let idx = LineIndex::new(src).unwrap();
        assert_eq!(idx.line_text(1).unwrap(), "first");
        assert_eq!(idx.line_text(2).unwrap(), "second");
        assert_eq!(idx.line_text(3).unwrap(), "third");
        assert!(idx.line_text(0).is_err());
        assert!(idx.line_text(99).is_err());
        // Trailing-newline: a final empty line exists at index line_count.
        let src2 = "a\n";
        let idx2 = LineIndex::new(src2).unwrap();
        assert_eq!(idx2.line_text(1).unwrap(), "a");
        assert_eq!(idx2.line_text(2).unwrap(), "");
    }
}
