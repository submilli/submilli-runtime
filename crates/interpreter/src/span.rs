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

    /// Virtual display path for a reserved (prelude/stdlib) id, e.g.
    /// `submilli:fs`. Returns `None` for ordinary user/script files, which
    /// instead resolve to a [`SourceFile`](crate::SourceFile) in the registry.
    pub fn reserved_path(self) -> Option<&'static str> {
        Some(match self {
            FileId::PRELUDE => "<prelude>",
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
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { file, start, end }
    }

    /// A zero-length placeholder anchored to `file` — for definitions and
    /// generated nodes that have a logical home file but no source range
    /// (prelude/stdlib declarations, desugared temporaries). The home is always
    /// named explicitly: a reserved id for prelude/stdlib, the script's id for
    /// compiler-generated user-code nodes.
    pub const fn at(file: FileId) -> Self {
        Span::new(file, 0, 0)
    }

    pub fn merge(self, other: Self) -> Self {
        debug_assert!(
            self.file == other.file,
            "cannot merge spans from different files"
        );
        Self {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }
}

#[derive(Clone, Debug)]
pub struct LineIndex {
    line_starts: Vec<u32>,
    source_len: u32,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let source_len = bytes.len() as u32;
        let mut line_starts = Vec::with_capacity(bytes.len() / 40 + 1);
        line_starts.push(0);
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    line_starts.push((i + 1) as u32);
                    i += 1;
                }
                b'\r' => {
                    let next_start = if bytes.get(i + 1) == Some(&b'\n') {
                        i + 2
                    } else {
                        i + 1
                    };
                    line_starts.push(next_start as u32);
                    i = next_start;
                }
                _ => i += 1,
            }
        }
        Self {
            line_starts,
            source_len,
        }
    }

    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let offset = offset.min(self.source_len);
        let count = self.line_starts.partition_point(|&start| start <= offset);
        let line_start = self.line_starts[count - 1];
        let line = count as u32;
        let col = offset - line_start + 1;
        (line, col)
    }

    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// Inverse of [`line_col`](Self::line_col). Out-of-range inputs clamp to the file's end.
    pub fn byte_offset(&self, line: u32, col: u32) -> u32 {
        if line == 0 {
            return 0;
        }
        let idx = ((line - 1) as usize).min(self.line_starts.len() - 1);
        let line_start = self.line_starts[idx];
        let col_off = col.saturating_sub(1);
        line_start.saturating_add(col_off).min(self.source_len)
    }

    pub fn line_text<'a>(&self, source: &'a str, line: u32) -> &'a str {
        if line == 0 || (line as usize) > self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[(line - 1) as usize] as usize;
        let end = self
            .line_starts
            .get(line as usize)
            .copied()
            .unwrap_or(self.source_len) as usize;
        source[start..end].trim_end_matches(['\n', '\r'])
    }
}

#[cfg(test)]
mod tests {
    use super::{FileId, LineIndex, Span};

    const F: FileId = FileId(0);

    #[test]
    fn new_stores_fields() {
        let s = Span::new(F, 3, 7);
        assert_eq!(s.start, 3);
        assert_eq!(s.end, 7);
    }

    #[test]
    fn new_allows_empty_span() {
        let s = Span::new(F, 5, 5);
        assert_eq!(s.start, 5);
        assert_eq!(s.end, 5);
    }

    #[test]
    fn merge_overlapping() {
        assert_eq!(
            Span::new(F, 0, 5).merge(Span::new(F, 3, 8)),
            Span::new(F, 0, 8)
        );
    }

    #[test]
    fn merge_disjoint_covers_gap() {
        assert_eq!(
            Span::new(F, 0, 2).merge(Span::new(F, 5, 7)),
            Span::new(F, 0, 7)
        );
    }

    #[test]
    fn merge_identical() {
        let s = Span::new(F, 4, 9);
        assert_eq!(s.merge(s), s);
    }

    #[test]
    fn merge_nested_returns_outer() {
        let outer = Span::new(F, 0, 10);
        let inner = Span::new(F, 3, 5);
        assert_eq!(outer.merge(inner), outer);
        assert_eq!(inner.merge(outer), outer);
    }

    #[test]
    fn merge_is_commutative() {
        let a = Span::new(F, 2, 6);
        let b = Span::new(F, 4, 10);
        assert_eq!(a.merge(b), b.merge(a));
    }

    #[test]
    fn contains_start_is_inclusive() {
        assert!(Span::new(F, 3, 7).contains(3));
    }

    #[test]
    fn contains_end_is_exclusive() {
        assert!(!Span::new(F, 3, 7).contains(7));
    }

    #[test]
    fn contains_strictly_inside() {
        assert!(Span::new(F, 3, 7).contains(5));
    }

    #[test]
    fn contains_before_start() {
        assert!(!Span::new(F, 3, 7).contains(2));
    }

    #[test]
    fn contains_after_end() {
        assert!(!Span::new(F, 3, 7).contains(8));
    }

    #[test]
    fn empty_span_contains_nothing() {
        let s = Span::new(F, 4, 4);
        assert!(!s.contains(3));
        assert!(!s.contains(4));
        assert!(!s.contains(5));
    }

    #[test]
    fn line_index_empty_file() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn line_index_single_line_no_terminator() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(1), (1, 2));
        assert_eq!(idx.line_col(5), (1, 6));
        assert_eq!(idx.line_count(), 1);
    }

    #[test]
    fn line_index_multi_line_lf() {
        let src = "abc\ndef\nghi";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(2), (1, 3));
        assert_eq!(idx.line_col(3), (1, 4)); // \n counts as last col of line 1
        assert_eq!(idx.line_col(4), (2, 1));
        assert_eq!(idx.line_col(7), (2, 4));
        assert_eq!(idx.line_col(8), (3, 1));
        assert_eq!(idx.line_col(10), (3, 3));
        assert_eq!(idx.line_count(), 3);
    }

    #[test]
    fn line_index_crlf() {
        let src = "abc\r\ndef";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(4), (1, 5));
        assert_eq!(idx.line_col(5), (2, 1));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_bare_cr() {
        let src = "abc\rdef";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(4), (2, 1));
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_trailing_newline() {
        let src = "abc\n";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(3), (1, 4));
        assert_eq!(idx.line_col(4), (2, 1)); // EOF = start of empty line 2
        assert_eq!(idx.line_count(), 2);
    }

    #[test]
    fn line_index_unicode() {
        // α, β, γ are each 2 UTF-8 bytes.
        let src = "αβ\nγ";
        assert_eq!(src.len(), 7);
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_col(0), (1, 1));
        assert_eq!(idx.line_col(2), (1, 3));
        assert_eq!(idx.line_col(3), (1, 4)); // col is byte-based, not char-based
        assert_eq!(idx.line_col(4), (1, 5));
        assert_eq!(idx.line_col(5), (2, 1));
        assert_eq!(idx.line_col(6), (2, 2));
    }

    #[test]
    fn line_index_clamps_past_eof() {
        let idx = LineIndex::new("abc");
        assert_eq!(idx.line_col(100), (1, 4));
    }

    #[test]
    fn line_index_clamps_past_eof_with_trailing_newline() {
        let idx = LineIndex::new("abc\n");
        assert_eq!(idx.line_col(999), (2, 1));
    }

    #[test]
    fn line_text_strips_terminators_and_clamps() {
        let src = "first\nsecond\r\nthird";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_text(src, 1), "first");
        assert_eq!(idx.line_text(src, 2), "second");
        assert_eq!(idx.line_text(src, 3), "third");
        assert_eq!(idx.line_text(src, 0), "");
        assert_eq!(idx.line_text(src, 99), "");
        // Trailing-newline: a final empty line exists at index line_count.
        let src2 = "a\n";
        let idx2 = LineIndex::new(src2);
        assert_eq!(idx2.line_text(src2, 1), "a");
        assert_eq!(idx2.line_text(src2, 2), "");
    }
}
