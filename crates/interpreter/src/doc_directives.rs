//! Author-controlled regions in agent-facing Markdown, such as a package's
//! `docs/readme.md`. A region is shown only when the capabilities it names are
//! available to the reader:
//!
//! ```markdown
//! <!-- submilli:if capability="linear.app/createIssue" -->
//! ## Creating issues
//! <!-- submilli:endif -->
//! ```
//!
//! `all="a b"` needs every name, `any="a b"` at least one, and a nested region
//! needs its parents' conditions too. A directive is a whole line, indented by
//! at most three spaces, outside fenced code, so an example in a fence that
//! shows the syntax stays literal. Parsing collects every malformed directive
//! with its line rather than stopping at the first, and rendering drops the
//! directive lines.

use std::collections::BTreeMap;
use std::fmt;

/// Regions nest at most this deep.
pub const MAX_DIRECTIVE_DEPTH: usize = 8;

/// Documents are parsed only up to this size, so a malformed artifact cannot
/// make parsing unbounded.
pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

const DIRECTIVE_PREFIX: &str = "submilli:";

/// When a region is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Every capability is available; `capability="x"` is `All` of one.
    All(Vec<String>),
    /// At least one capability is available.
    Any(Vec<String>),
}

impl Condition {
    /// The capabilities the condition names.
    pub fn capabilities(&self) -> &[String] {
        match self {
            Condition::All(names) | Condition::Any(names) => names,
        }
    }

    /// Whether the condition holds, given which capabilities are available.
    pub fn holds(&self, available: impl Fn(&str) -> bool) -> bool {
        match self {
            Condition::All(names) => names.iter().all(|name| available(name)),
            Condition::Any(names) => names.iter().any(|name| available(name)),
        }
    }
}

/// A malformed directive, by its one-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for DirectiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// A parsed document: text and the regions around parts of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocMarkdown {
    blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Text(String),
    Region {
        condition: Condition,
        line: usize,
        blocks: Vec<Block>,
    },
}

/// Parse `markdown`, or report every malformed directive in it.
pub fn parse_doc_directives(markdown: &str) -> Result<DocMarkdown, Vec<DirectiveError>> {
    if markdown.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![DirectiveError {
            line: 1,
            message: format!(
                "the document is {} bytes; documentation is limited to {MAX_DOCUMENT_BYTES}",
                markdown.len()
            ),
        }]);
    }
    let mut parser = Parser::default();
    for (index, line) in markdown.split_inclusive('\n').enumerate() {
        parser.line(index.saturating_add(1), line);
    }
    parser.finish()
}

impl DocMarkdown {
    /// Each region's condition with the line it opens on, in document order.
    pub fn regions(&self) -> Vec<(usize, &Condition)> {
        let mut out = Vec::new();
        collect_regions(&self.blocks, &mut out);
        out
    }

    /// The document with each region whose condition `keep` rejects removed,
    /// and every directive line dropped. A region inside a removed one is
    /// removed with it.
    pub fn render(&self, keep: &dyn Fn(&Condition) -> bool) -> String {
        let mut out = Output::default();
        render_blocks(&self.blocks, keep, &mut out);
        out.text
    }
}

fn collect_regions<'a>(blocks: &'a [Block], out: &mut Vec<(usize, &'a Condition)>) {
    for block in blocks {
        if let Block::Region {
            condition,
            line,
            blocks,
        } = block
        {
            out.push((*line, condition));
            // Bounded by `MAX_DIRECTIVE_DEPTH`, which parsing enforces.
            collect_regions(blocks, out);
        }
    }
}

fn render_blocks(blocks: &[Block], keep: &dyn Fn(&Condition) -> bool, out: &mut Output) {
    for block in blocks {
        match block {
            Block::Text(text) => out.push(text),
            Block::Region {
                condition, blocks, ..
            } => {
                out.seam();
                if keep(condition) {
                    // Bounded by `MAX_DIRECTIVE_DEPTH`, which parsing enforces.
                    render_blocks(blocks, keep, out);
                    out.seam();
                }
            }
        }
    }
}

/// Rendered text. A dropped directive line leaves a seam, where blank lines
/// on both sides would otherwise pile up.
#[derive(Default)]
struct Output {
    text: String,
    at_seam: bool,
}

impl Output {
    fn seam(&mut self) {
        self.at_seam = true;
    }

    fn push(&mut self, text: &str) {
        let mut text = text;
        if std::mem::take(&mut self.at_seam) && (self.text.is_empty() || self.ends_blank()) {
            while let Some(rest) = strip_blank_line(text) {
                text = rest;
            }
        }
        self.text.push_str(text);
    }

    /// The text ends with a blank line, whatever its line endings.
    fn ends_blank(&self) -> bool {
        let Some(body) = self.text.strip_suffix('\n') else {
            return false;
        };
        body.rsplit_once('\n')
            .is_some_and(|(_, last)| last.trim().is_empty())
    }
}

fn strip_blank_line(text: &str) -> Option<&str> {
    let (line, rest) = text.split_once('\n')?;
    line.trim().is_empty().then_some(rest)
}

#[derive(Default)]
struct Parser {
    /// Open regions, innermost last, each with the blocks gathered inside it.
    open: Vec<OpenRegion>,
    blocks: Vec<Block>,
    errors: Vec<DirectiveError>,
    fence: Option<Fence>,
}

struct Fence {
    marker: u8,
    len: usize,
    /// Where the fence opens.
    line: usize,
    /// The first directive-shaped line inside it.
    directive_line: Option<usize>,
}

/// A region whose `endif` is still to come.
struct OpenRegion {
    condition: Condition,
    line: usize,
    blocks: Vec<Block>,
}

impl Parser {
    fn line(&mut self, number: usize, line: &str) {
        let content = line.trim_end_matches(['\n', '\r']);
        if let Some(fence) = &mut self.fence {
            if closes_fence(content, fence) {
                self.fence = None;
            } else if fence.directive_line.is_none() && is_directive_like(content.trim()) {
                fence.directive_line = Some(number);
            }
            self.text(line);
            return;
        }
        // Indented code is not tracked: inside a list item the same
        // indentation continues the item, so an indented directive is refused
        // rather than guessed at. Examples of directives belong in a fence.
        if indentation(content) >= 4 && is_directive_like(content.trim_start()) {
            self.error(
                number,
                "a directive must not be indented by four or more spaces".to_string(),
            );
            self.text(line);
            return;
        }
        if let Some(fence) = opens_fence(content, number) {
            self.fence = Some(fence);
            self.text(line);
            return;
        }
        let trimmed = content.trim();
        if is_directive_like(trimmed) {
            self.directive(number, trimmed);
            return;
        }
        if contains_directive(content) {
            self.error(number, "a directive must stand on its own line".to_string());
        }
        self.text(line);
    }

    fn text(&mut self, line: &str) {
        let blocks = match self.open.last_mut() {
            Some(region) => &mut region.blocks,
            None => &mut self.blocks,
        };
        match blocks.last_mut() {
            Some(Block::Text(text)) => text.push_str(line),
            _ => blocks.push(Block::Text(line.to_string())),
        }
    }

    fn directive(&mut self, number: usize, trimmed: &str) {
        let inner = match trimmed
            .strip_prefix("<!--")
            .and_then(|rest| rest.strip_suffix("-->"))
        {
            Some(inner) => inner.trim(),
            None if trimmed.contains("-->") => {
                return self.error(number, "a directive must stand on its own line".to_string());
            }
            None => {
                return self.error(
                    number,
                    "a directive must close with `-->` on the same line".to_string(),
                );
            }
        };
        let Some(directive) = inner.strip_prefix(DIRECTIVE_PREFIX) else {
            return self.error(
                number,
                format!("a directive starts with `{DIRECTIVE_PREFIX}`, in lowercase"),
            );
        };
        let (verb, rest) = directive
            .split_once(char::is_whitespace)
            .unwrap_or((directive, ""));
        match verb {
            "if" => {
                // A malformed `if` still opens its region, so its `endif` is
                // not reported as unmatched too.
                let condition = parse_condition(rest.trim()).unwrap_or_else(|message| {
                    self.error(number, message);
                    Condition::All(Vec::new())
                });
                self.open_region(number, condition);
            }
            "endif" if rest.trim().is_empty() => self.close_region(number),
            "endif" => self.error(number, "`submilli:endif` takes no attributes".to_string()),
            _ => self.error(
                number,
                format!(
                    "unknown directive `submilli:{verb}`; use `submilli:if` or `submilli:endif`"
                ),
            ),
        }
    }

    fn open_region(&mut self, number: usize, condition: Condition) {
        if self.open.len() >= MAX_DIRECTIVE_DEPTH {
            return self.error(
                number,
                format!("regions nest at most {MAX_DIRECTIVE_DEPTH} deep"),
            );
        }
        self.open.push(OpenRegion {
            condition,
            line: number,
            blocks: Vec::new(),
        });
    }

    fn close_region(&mut self, number: usize) {
        let Some(OpenRegion {
            condition,
            line,
            blocks,
        }) = self.open.pop()
        else {
            return self.error(
                number,
                "`submilli:endif` has no matching `submilli:if`".to_string(),
            );
        };
        let region = Block::Region {
            condition,
            line,
            blocks,
        };
        match self.open.last_mut() {
            Some(parent) => parent.blocks.push(region),
            None => self.blocks.push(region),
        }
    }

    fn error(&mut self, line: usize, message: String) {
        self.errors.push(DirectiveError { line, message });
    }

    fn finish(mut self) -> Result<DocMarkdown, Vec<DirectiveError>> {
        for region in std::mem::take(&mut self.open) {
            self.error(
                region.line,
                "`submilli:if` has no matching `submilli:endif`".to_string(),
            );
        }
        // An unclosed fence runs to the end, so a directive after it would be
        // shown to every reader as text.
        if let Some(Fence {
            line,
            directive_line: Some(directive_line),
            ..
        }) = self.fence.take()
        {
            self.error(
                line,
                format!(
                    "this code fence is never closed, so the directive on line {directive_line} is shown as text"
                ),
            );
        }
        if self.errors.is_empty() {
            Ok(DocMarkdown {
                blocks: self.blocks,
            })
        } else {
            self.errors.sort_by_key(|error| error.line);
            Err(self.errors)
        }
    }
}

/// `capability="x"`, `all="a b"` or `any="a b"`.
fn parse_condition(attributes: &str) -> Result<Condition, String> {
    let Some((name, rest)) = attributes.split_once('=') else {
        return Err("`submilli:if` needs one attribute: `capability`, `all` or `any`".to_string());
    };
    let name = name.trim();
    let rest = rest.trim_start();
    let Some(value) = rest.strip_prefix('"') else {
        return Err(format!("the `{name}` value must be in double quotes"));
    };
    let Some((value, after)) = value.split_once('"') else {
        return Err(format!("the `{name}` value has no closing quote"));
    };
    if !after.trim().is_empty() {
        return Err("`submilli:if` takes exactly one attribute".to_string());
    }
    let names = capability_names(name, value)?;
    match name {
        "capability" if names.len() == 1 => Ok(Condition::All(names)),
        "capability" => {
            Err("`capability` names one capability; use `all` or `any` for several".to_string())
        }
        "all" => Ok(Condition::All(names)),
        "any" => Ok(Condition::Any(names)),
        _ => Err(format!(
            "unknown attribute `{name}`; use `capability`, `all` or `any`"
        )),
    }
}

fn capability_names(attribute: &str, value: &str) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for name in value.split_whitespace() {
        if !is_capability_name(name) {
            return Err(format!("`{name}` is not a capability name"));
        }
        if names.iter().any(|seen| seen == name) {
            return Err(format!("`{name}` appears twice in `{attribute}`"));
        }
        names.push(name.to_string());
    }
    if names.is_empty() {
        return Err(format!("`{attribute}` names no capability"));
    }
    Ok(names)
}

fn is_capability_name(name: &str) -> bool {
    name.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'/' | b'-' | b'_' | b':' | b'@')
    })
}

fn is_directive_like(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("<!--")
        .is_some_and(|rest| starts_like_directive(rest.trim_start()))
}

/// `text` starts with the directive prefix in any case, so a miscased
/// directive is reported rather than shown as a comment.
fn starts_like_directive(text: &str) -> bool {
    text.get(..DIRECTIVE_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(DIRECTIVE_PREFIX))
}

/// A directive-shaped comment anywhere in the line, outside inline code.
fn contains_directive(line: &str) -> bool {
    let prose = without_code_spans(line);
    prose.match_indices("<!--").any(|(at, _)| {
        prose
            .get(at.saturating_add(4)..)
            .is_some_and(|rest| starts_like_directive(rest.trim_start()))
    })
}

/// `line` with each inline code span removed: text from a backtick run to
/// the next run of the same length. One pass, so a line of many unmatched
/// runs stays linear.
fn without_code_spans(line: &str) -> String {
    // Each backtick run as (start, end) byte offsets.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte != b'`' {
            index = index.saturating_add(1);
            continue;
        }
        let start = index;
        while bytes.get(index) == Some(&b'`') {
            index = index.saturating_add(1);
        }
        runs.push((start, index));
    }
    // For each run, the next run of the same length.
    let mut next_same: Vec<Option<usize>> = vec![None; runs.len()];
    let mut last_by_length: BTreeMap<usize, usize> = BTreeMap::new();
    for (position, (start, end)) in runs.iter().enumerate().rev() {
        let length = end.saturating_sub(*start);
        if let Some(slot) = next_same.get_mut(position) {
            *slot = last_by_length.insert(length, position);
        }
    }
    let mut out = String::with_capacity(line.len());
    let mut copied = 0;
    let mut position = 0;
    while let Some(&(start, end)) = runs.get(position) {
        if let Some(closing) = next_same.get(position).copied().flatten() {
            out.push_str(line.get(copied..start).unwrap_or(""));
            copied = runs.get(closing).map_or(line.len(), |(_, end)| *end);
            position = closing.saturating_add(1);
        } else {
            out.push_str(line.get(copied..end).unwrap_or(""));
            copied = end;
            position = position.saturating_add(1);
        }
    }
    out.push_str(line.get(copied..).unwrap_or(""));
    out
}

/// Leading indentation in columns, a tab counting as four.
fn indentation(line: &str) -> usize {
    let mut columns: usize = 0;
    for byte in line.bytes() {
        match byte {
            b' ' => columns = columns.saturating_add(1),
            b'\t' => columns = columns.saturating_add(4),
            _ => break,
        }
    }
    columns
}

fn opens_fence(content: &str, line: usize) -> Option<Fence> {
    if indentation(content) >= 4 {
        return None;
    }
    let trimmed = content.trim_start();
    let marker = *trimmed.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let len = trimmed.bytes().take_while(|byte| *byte == marker).count();
    if len < 3 {
        return None;
    }
    // A backtick fence's info string cannot hold a backtick.
    let info = trimmed.get(len..).unwrap_or("");
    if marker == b'`' && info.contains('`') {
        return None;
    }
    Some(Fence {
        marker,
        len,
        line,
        directive_line: None,
    })
}

fn closes_fence(line: &str, fence: &Fence) -> bool {
    if indentation(line) >= 4 {
        return false;
    }
    let trimmed = line.trim();
    let len = trimmed
        .bytes()
        .take_while(|byte| *byte == fence.marker)
        .count();
    len >= fence.len && trimmed.len() == len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_with(markdown: &str, available: &[&str]) -> String {
        let doc = parse_doc_directives(markdown).expect("valid directives");
        doc.render(&|condition| condition.holds(|name| available.contains(&name)))
    }

    fn errors(markdown: &str) -> Vec<String> {
        parse_doc_directives(markdown)
            .expect_err("invalid directives")
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    const DOC: &str = "# Linear\n\nRead issues.\n\n<!-- submilli:if capability=\"linear.app/createIssue\" -->\n\n## Creating issues\n\nCall `createIssue`.\n\n<!-- submilli:endif -->\n\n## Teams\n";

    #[test]
    fn a_region_is_removed_when_its_capability_is_unavailable() {
        assert_eq!(
            render_with(DOC, &[]),
            "# Linear\n\nRead issues.\n\n## Teams\n"
        );
    }

    #[test]
    fn a_kept_region_loses_its_markers() {
        assert_eq!(
            render_with(DOC, &["linear.app/createIssue"]),
            "# Linear\n\nRead issues.\n\n## Creating issues\n\nCall `createIssue`.\n\n## Teams\n"
        );
        let doc = parse_doc_directives(DOC).unwrap();
        assert_eq!(
            doc.render(&|_| true),
            render_with(DOC, &["linear.app/createIssue"])
        );
    }

    #[test]
    fn all_any_and_nesting() {
        let doc = "<!-- submilli:if all=\"a b\" -->\nboth\n<!-- submilli:endif -->\n<!-- submilli:if any=\"a b\" -->\neither\n<!-- submilli:if capability=\"c\" -->\nand c\n<!-- submilli:endif -->\n<!-- submilli:endif -->\n";
        assert_eq!(render_with(doc, &["a", "b"]), "both\neither\n");
        assert_eq!(render_with(doc, &["b"]), "either\n");
        assert_eq!(render_with(doc, &["b", "c"]), "either\nand c\n");
        assert_eq!(render_with(doc, &["c"]), "");
        let parsed = parse_doc_directives(doc).unwrap();
        let named = parsed
            .regions()
            .into_iter()
            .flat_map(|(_, condition)| condition.capabilities())
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(named, ["a", "b", "a", "b", "c"]);
    }

    #[test]
    fn directives_in_code_stay_literal() {
        let doc = "```markdown\n<!-- submilli:if capability=\"x\" -->\n```\n\n~~~~\n<!-- submilli:endif -->\n```\n~~~~\n";
        assert_eq!(render_with(doc, &[]), doc);
    }

    #[test]
    fn an_indented_directive_is_refused_even_after_a_blank_line() {
        // In a list item this indentation continues the item, so it is not
        // taken for code that would leave the directive in place.
        assert_eq!(
            errors(
                "- item\n\n    <!-- submilli:if capability=\"x\" -->\n    text\n    <!-- submilli:endif -->\n"
            ),
            [
                "line 3: a directive must not be indented by four or more spaces",
                "line 5: a directive must not be indented by four or more spaces",
            ]
        );
    }

    #[test]
    fn slips_that_would_show_a_region_to_everyone_are_reported() {
        assert_eq!(
            errors(
                "```\ncode\n\n<!-- submilli:if capability=\"x\" -->\nhidden\n<!-- submilli:endif -->\n"
            ),
            [
                "line 1: this code fence is never closed, so the directive on line 4 is shown as text"
            ]
        );
        // An unclosed fence with no directive after it is ordinary Markdown.
        assert!(parse_doc_directives("```\ncode\n").is_ok());
        assert_eq!(
            errors("<!-- Submilli:if capability=\"x\" -->\n"),
            ["line 1: a directive starts with `submilli:`, in lowercase"]
        );
        assert_eq!(
            errors("<!-- submilli:endif --> trailing\n"),
            ["line 1: a directive must stand on its own line"]
        );
    }

    #[test]
    fn crlf_seams_collapse_like_lf() {
        let doc = "a\r\n\r\n<!-- submilli:if capability=\"x\" -->\r\n\r\nhidden\r\n\r\n<!-- submilli:endif -->\r\n\r\nb\r\n";
        assert_eq!(render_with(doc, &[]), "a\r\n\r\nb\r\n");
    }

    #[test]
    fn code_spans_close_on_a_run_of_the_same_length() {
        assert_eq!(without_code_spans("a ``x ` y`` b"), "a  b");
        assert_eq!(without_code_spans("a ``` b ` c"), "a ``` b ` c");
        assert_eq!(without_code_spans("`one` and `two`"), " and ");
        let many = "`".repeat(3000) + &" `".repeat(10);
        assert!(!without_code_spans(&many).is_empty());
    }

    #[test]
    fn inline_code_may_mention_a_directive() {
        let doc = "Wrap it in `<!-- submilli:if capability=\"x\" -->`.\n";
        assert_eq!(render_with(doc, &[]), doc);
    }

    #[test]
    fn malformed_directives_report_their_lines() {
        assert_eq!(
            errors("a\n<!-- submilli:if capability=\"x\" -->\nb\n"),
            ["line 2: `submilli:if` has no matching `submilli:endif`"]
        );
        assert_eq!(
            errors("<!-- submilli:endif -->\n"),
            ["line 1: `submilli:endif` has no matching `submilli:if`"]
        );
        assert_eq!(
            errors("<!-- submilli:if role=\"x\" -->\n<!-- submilli:endif -->\n"),
            ["line 1: unknown attribute `role`; use `capability`, `all` or `any`"]
        );
        assert_eq!(
            errors("<!-- submilli:if capability='x' -->\n<!-- submilli:endif -->\n"),
            ["line 1: the `capability` value must be in double quotes"]
        );
        assert_eq!(
            errors("<!-- submilli:if all=\"a a\" -->\n<!-- submilli:endif -->\n"),
            ["line 1: `a` appears twice in `all`"]
        );
        assert_eq!(
            errors("<!-- submilli:if capability=\"a b\" -->\n<!-- submilli:endif -->\n"),
            ["line 1: `capability` names one capability; use `all` or `any` for several"]
        );
        assert_eq!(
            errors("<!-- submilli:if any=\"\" -->\n<!-- submilli:endif -->\n"),
            ["line 1: `any` names no capability"]
        );
        assert_eq!(
            errors("<!-- submilli:else -->\n"),
            ["line 1: unknown directive `submilli:else`; use `submilli:if` or `submilli:endif`"]
        );
        assert_eq!(
            errors("text <!-- submilli:endif -->\n"),
            ["line 1: a directive must stand on its own line"]
        );
        assert_eq!(
            errors("<!-- submilli:if capability=\"x\"\n"),
            ["line 1: a directive must close with `-->` on the same line"]
        );
        assert_eq!(
            errors("para\n    <!-- submilli:endif -->\n"),
            ["line 2: a directive must not be indented by four or more spaces"]
        );
    }

    #[test]
    fn every_error_is_reported() {
        assert_eq!(
            errors("<!-- submilli:endif -->\n<!-- submilli:if x=\"y\" -->\n"),
            [
                "line 1: `submilli:endif` has no matching `submilli:if`",
                "line 2: unknown attribute `x`; use `capability`, `all` or `any`",
                "line 2: `submilli:if` has no matching `submilli:endif`",
            ]
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let mut doc = String::new();
        for _ in 0..=MAX_DIRECTIVE_DEPTH {
            doc.push_str("<!-- submilli:if capability=\"x\" -->\n");
        }
        for _ in 0..=MAX_DIRECTIVE_DEPTH {
            doc.push_str("<!-- submilli:endif -->\n");
        }
        let errors = errors(&doc);
        assert!(
            errors.contains(&format!(
                "line {}: regions nest at most {MAX_DIRECTIVE_DEPTH} deep",
                MAX_DIRECTIVE_DEPTH + 1
            )),
            "{errors:?}"
        );
    }

    #[test]
    fn a_document_without_directives_is_unchanged() {
        let doc = "# Title\n\n\n\nText with <!-- a comment -->.\n";
        assert_eq!(render_with(doc, &[]), doc);
    }
}
