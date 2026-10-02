//! JSDoc-style doc comments.

use crate::{FileId, Span};
use serde::{Deserialize, Serialize};

/// Delimiters `/**` and `*/` are included; span covers both.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawDoc {
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocComment {
    pub span: Span,
    pub summary: String,
    pub params: Vec<DocParam>,
    pub returns: Option<DocReturns>,
    pub capabilities: Vec<DocCapability>,
    pub throws: Vec<DocText>,
    pub deprecated: Option<DocText>,
    pub examples: Vec<DocText>,
    pub unknown_tags: Vec<DocUnknownTag>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocParam {
    /// Span of `@param` (the tag keyword only).
    pub tag_span: Span,
    pub name: String,
    pub name_span: Span,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocReturns {
    pub tag_span: Span,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocCapability {
    pub tag_span: Span,
    pub capability: String,
    pub capability_span: Span,
    pub bindings: Vec<DocCapabilityBinding>,
    pub description: String,
    pub diagnostics: Vec<DocCapabilityDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocCapabilityBinding {
    pub field: String,
    pub field_span: Span,
    pub kind: DocCapabilityBindingKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocCapabilityBindingKind {
    Parameter {
        param: String,
        path: Vec<String>,
        span: Span,
    },
    Type {
        name: String,
        span: Span,
    },
    Literal {
        value: DocCapabilityLiteral,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocCapabilityLiteral {
    String(String),
    Number(String),
    Boolean(bool),
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocCapabilityDiagnostic {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocText {
    pub tag_span: Span,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocUnknownTag {
    pub tag_span: Span,
    /// Tag name without the leading `@`.
    pub name: String,
    pub text: String,
}

pub fn parse_doc_comment(raw: &RawDoc) -> Result<DocComment, crate::source::SourceError> {
    use crate::source::SourceError;
    SourceError::check_source_len(raw.span.end as usize)?;
    if raw
        .span
        .end
        .checked_sub(raw.span.start)
        .map(|len| len as usize)
        != Some(raw.text.len())
        || !raw.text.starts_with("/**")
        || !raw.text.ends_with("*/")
    {
        return Err(SourceError::InvalidSpan {
            span: raw.span,
            reason: "documentation text does not match its span",
        });
    }
    let mut doc = DocComment {
        span: raw.span,
        summary: String::new(),
        params: Vec::new(),
        returns: None,
        capabilities: Vec::new(),
        throws: Vec::new(),
        deprecated: None,
        examples: Vec::new(),
        unknown_tags: Vec::new(),
    };
    let inner = strip_delimiters(&raw.text);
    // +3 skips the opening `/**`.
    let inner_start = raw.span.start + 3;
    let lines = split_lines_with_offsets(inner, inner_start);

    let mut cleaned: Vec<(u32, &str)> = Vec::with_capacity(lines.len());
    for (offset, line) in lines {
        let (skipped, rest) = strip_star_prefix(line);
        cleaned.push((offset + skipped as u32, rest));
    }

    let mut current_tag: Option<PendingTag> = None;
    for (offset, text) in &cleaned {
        if let Some(tag_offset) = first_at_offset(text) {
            // Flush whatever tag was being accumulated.
            if let Some(t) = current_tag.take() {
                push_tag(&mut doc, t)?;
            }
            let tag_byte = *offset + tag_offset as u32;
            let after_at = &text[tag_offset + 1..];
            let (tag_name, name_end) = read_tag_name(after_at);
            let tag_name_str = tag_name.to_string();
            let tag_span = Span::new(raw.span.file, tag_byte, tag_byte + 1 + name_end as u32)?;
            let body_start = tag_offset + 1 + name_end;
            let body = text[body_start..].trim_start();
            let body_offset = *offset + (text.len() - body.len()) as u32;
            current_tag = Some(PendingTag {
                tag_span,
                name: tag_name_str,
                body: body.to_string(),
                segments: vec![BodySegment {
                    start: 0,
                    len: body.len(),
                    source_start: body_offset,
                }],
            });
        } else if let Some(tag) = &mut current_tag {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                if !tag.body.is_empty() {
                    tag.body.push(' ');
                }
                tag.segments
                    .try_reserve(1)
                    .map_err(SourceError::Allocation)?;
                tag.segments.push(BodySegment {
                    start: tag.body.len(),
                    len: trimmed.len(),
                    source_start: *offset + (text.len() - text.trim_start().len()) as u32,
                });
                tag.body.push_str(trimmed);
            }
        } else {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                if !doc.summary.is_empty() {
                    doc.summary.push(' ');
                }
                doc.summary.push_str(trimmed);
            }
        }
    }
    if let Some(t) = current_tag {
        push_tag(&mut doc, t)?;
    }
    Ok(doc)
}

/// Build a [`DocComment`] from a literal that has no real source range —
/// `file` names its logical home (e.g. [`FileId::PRELUDE`] or a stdlib id).
pub fn doc(file: FileId, literal: &str) -> Option<DocComment> {
    if !literal.starts_with("/**") || !literal.ends_with("*/") {
        return None;
    }
    let raw = RawDoc {
        text: literal.to_string(),
        span: Span::new(file, 0, u32::try_from(literal.len()).ok()?).ok()?,
    };
    parse_doc_comment(&raw).ok()
}

struct PendingTag {
    tag_span: Span,
    name: String,
    body: String,
    segments: Vec<BodySegment>,
}

struct BodySegment {
    start: usize,
    len: usize,
    source_start: u32,
}

impl PendingTag {
    /// Normalization joins lines and strips their prefixes; map token boundaries
    /// back to the original text rather than treating the joined body as source.
    fn source_span(&self, start: usize, end: usize) -> Result<Span, crate::source::SourceError> {
        let offset = |offset: usize| {
            self.segments
                .iter()
                .rev()
                .find_map(|segment| {
                    let relative = offset
                        .checked_sub(segment.start)
                        .filter(|n| *n <= segment.len)?;
                    segment
                        .source_start
                        .checked_add(u32::try_from(relative).ok()?)
                })
                .ok_or(crate::source::SourceError::InvalidSpan {
                    span: self.tag_span,
                    reason: "documentation offset has no source segment",
                })
        };
        Span::new(self.tag_span.file, offset(start)?, offset(end)?)
    }
}

fn push_tag(doc: &mut DocComment, t: PendingTag) -> Result<(), crate::source::SourceError> {
    let trimmed = t.body.trim();
    let body = strip_jsdoc_type(trimmed);
    let body_start = t.body.len() - t.body.trim_start().len() + trimmed.len() - body.len();
    match t.name.as_str() {
        "param" => {
            let (param_name, name_len_with_ws) = read_param_name(body);
            let name_span = t.source_span(body_start, body_start + param_name.len())?;
            let description = body[name_len_with_ws..].trim().to_string();
            doc.params.push(DocParam {
                tag_span: t.tag_span,
                name: param_name.to_string(),
                name_span,
                description,
            });
        }
        "returns" | "return" => {
            doc.returns = Some(DocReturns {
                tag_span: t.tag_span,
                description: body.to_string(),
            });
        }
        "capability" => {
            let mut capability = parse_capability_tag(t.tag_span, body, 0);
            let remap = |span: &mut Span| -> Result<(), crate::source::SourceError> {
                *span = t.source_span(
                    body_start + span.start as usize,
                    body_start + span.end as usize,
                )?;
                Ok(())
            };
            remap(&mut capability.capability_span)?;
            for binding in &mut capability.bindings {
                remap(&mut binding.field_span)?;
                match &mut binding.kind {
                    DocCapabilityBindingKind::Parameter { span, .. }
                    | DocCapabilityBindingKind::Type { span, .. }
                    | DocCapabilityBindingKind::Literal { span, .. } => remap(span)?,
                }
            }
            for diagnostic in &mut capability.diagnostics {
                remap(&mut diagnostic.span)?;
            }
            doc.capabilities.push(capability);
        }
        "throws" | "throw" => doc.throws.push(DocText {
            tag_span: t.tag_span,
            text: body.to_string(),
        }),
        "deprecated" => {
            doc.deprecated = Some(DocText {
                tag_span: t.tag_span,
                text: body.to_string(),
            });
        }
        "example" => doc.examples.push(DocText {
            tag_span: t.tag_span,
            text: body.to_string(),
        }),
        _ => doc.unknown_tags.push(DocUnknownTag {
            tag_span: t.tag_span,
            name: t.name,
            text: body.to_string(),
        }),
    }
    Ok(())
}

fn strip_delimiters(text: &str) -> &str {
    let inner = text.strip_prefix("/**").unwrap_or(text);
    inner.strip_suffix("*/").unwrap_or(inner)
}

fn split_lines_with_offsets(text: &str, base: u32) -> Vec<(u32, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            out.push((base + start as u32, &text[start..i]));
            start = i + 1;
        } else if bytes[i] == b'\r' {
            out.push((base + start as u32, &text[start..i]));
            start = if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                i + 2
            } else {
                i + 1
            };
            i = start;
            continue;
        }
        i += 1;
    }
    if start < bytes.len() {
        out.push((base + start as u32, &text[start..]));
    }
    out
}

/// Returns (bytes_skipped, remaining_text).
fn strip_star_prefix(line: &str) -> (usize, &str) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'*' {
        i += 1;
        if i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
    }
    (i, &line[i..])
}

/// Only recognises `@` at the start of a trimmed line.
fn first_at_offset(line: &str) -> Option<usize> {
    let trimmed_start = line.len() - line.trim_start().len();
    if line[trimmed_start..].starts_with('@') {
        Some(trimmed_start)
    } else {
        None
    }
}

fn read_tag_name(text: &str) -> (&str, usize) {
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    (&text[..end], end)
}

/// Returns (name, bytes_including_trailing_ws); name chars: `[alnum_$]+`.
fn read_param_name(body: &str) -> (&str, usize) {
    let end = body
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '$' | '.'))
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    let (name, rest) = body.split_at(end);
    let tail = body.len() - rest.trim_start_matches([' ', '\t']).len();
    (name, tail)
}

/// Strips leading `{type}` since Submilli has TS-style annotations.
fn strip_jsdoc_type(s: &str) -> &str {
    let s = s.trim_start();
    if !s.starts_with('{') {
        return s;
    }
    let mut depth = 0u32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return s[i + 1..].trim_start();
                }
            }
            _ => {}
        }
    }
    // Unterminated `{` — keep as-is.
    s
}

fn parse_capability_tag(tag_span: Span, body: &str, body_offset: u32) -> DocCapability {
    let mut parser = CapabilityParser {
        file: tag_span.file,
        body,
        body_offset,
        pos: 0,
        diagnostics: Vec::new(),
    };
    parser.skip_ws();
    let cap_start = parser.pos;
    let capability = parser.read_capability_name().to_string();
    let capability_span = parser.span(cap_start, parser.pos);
    if capability.is_empty() {
        parser.error(cap_start, "missing capability name after `@capability`");
    }
    parser.skip_ws();

    let bindings = if parser.peek_char() == Some('{') {
        parser.parse_binding_map()
    } else {
        Vec::new()
    };
    parser.skip_ws();
    let description = parser
        .body
        .get(parser.pos..)
        .unwrap_or("")
        .trim_start_matches(['-', '\u{2014}'])
        .trim()
        .to_string();

    DocCapability {
        tag_span,
        capability,
        capability_span,
        bindings,
        description,
        diagnostics: parser.diagnostics,
    }
}

struct CapabilityParser<'a> {
    file: FileId,
    body: &'a str,
    body_offset: u32,
    pos: usize,
    diagnostics: Vec<DocCapabilityDiagnostic>,
}

impl CapabilityParser<'_> {
    fn parse_binding_map(&mut self) -> Vec<DocCapabilityBinding> {
        self.bump_char();
        let mut bindings = Vec::new();
        loop {
            self.skip_ws();
            match self.peek_char() {
                Some('}') => {
                    self.bump_char();
                    break;
                }
                None => {
                    self.error(self.pos, "unterminated `@capability` binding map");
                    break;
                }
                _ => {}
            }

            let Some(binding) = self.parse_binding() else {
                self.recover_to_next_binding();
                continue;
            };
            bindings.push(binding);

            self.skip_ws();
            match self.peek_char() {
                Some(',') => {
                    self.bump_char();
                }
                Some('}') => {}
                Some(_) => {
                    self.error(self.pos, "expected `,` or `}` in `@capability` binding map");
                    self.recover_to_next_binding();
                }
                None => {
                    self.error(self.pos, "unterminated `@capability` binding map");
                    break;
                }
            }
        }
        bindings
    }

    fn parse_binding(&mut self) -> Option<DocCapabilityBinding> {
        let field_start = self.pos;
        let field = self.read_ident().to_string();
        if field.is_empty() {
            self.error(
                field_start,
                "expected payload field name in `@capability` binding",
            );
            return None;
        }
        let field_span = self.span(field_start, self.pos);
        self.skip_ws();
        if self.peek_char() != Some(':') {
            return Some(DocCapabilityBinding {
                field: field.to_string(),
                field_span,
                kind: DocCapabilityBindingKind::Parameter {
                    param: field.clone(),
                    path: Vec::new(),
                    span: field_span,
                },
            });
        }
        self.bump_char();
        self.skip_ws();
        let value_start = self.pos;
        let Some(kind) = self.parse_binding_value(value_start) else {
            self.error(value_start, "expected binding value after `:`");
            return None;
        };
        Some(DocCapabilityBinding {
            field,
            field_span,
            kind,
        })
    }

    fn parse_binding_value(&mut self, start: usize) -> Option<DocCapabilityBindingKind> {
        match self.peek_char()? {
            '$' => {
                self.bump_char();
                let param_start = self.pos;
                let param = self.read_ident().to_string();
                if param.is_empty() {
                    self.error(param_start, "expected parameter name after `$`");
                    return None;
                }
                let mut path = Vec::new();
                while self.peek_char() == Some('.') {
                    self.bump_char();
                    let part_start = self.pos;
                    let part = self.read_ident().to_string();
                    if part.is_empty() {
                        self.error(part_start, "expected field name after `.`");
                        break;
                    }
                    path.push(part);
                }
                Some(DocCapabilityBindingKind::Parameter {
                    param,
                    path,
                    span: self.span(start, self.pos),
                })
            }
            '"' => {
                self.parse_string_literal(start)
                    .map(|value| DocCapabilityBindingKind::Literal {
                        value: DocCapabilityLiteral::String(value),
                        span: self.span(start, self.pos),
                    })
            }
            '0'..='9' | '-' => {
                let number = self.read_number();
                Some(DocCapabilityBindingKind::Literal {
                    value: DocCapabilityLiteral::Number(number.to_string()),
                    span: self.span(start, self.pos),
                })
            }
            _ => {
                let mut ident = self.read_ident().to_string();
                while self
                    .body
                    .get(self.pos..)
                    .is_some_and(|remaining| remaining.starts_with("[]"))
                {
                    self.bump_char();
                    self.bump_char();
                    ident.push_str("[]");
                }
                match ident.as_str() {
                    "" => None,
                    "true" => Some(DocCapabilityBindingKind::Literal {
                        value: DocCapabilityLiteral::Boolean(true),
                        span: self.span(start, self.pos),
                    }),
                    "false" => Some(DocCapabilityBindingKind::Literal {
                        value: DocCapabilityLiteral::Boolean(false),
                        span: self.span(start, self.pos),
                    }),
                    "null" => Some(DocCapabilityBindingKind::Literal {
                        value: DocCapabilityLiteral::Null,
                        span: self.span(start, self.pos),
                    }),
                    _ => Some(DocCapabilityBindingKind::Type {
                        name: ident,
                        span: self.span(start, self.pos),
                    }),
                }
            }
        }
    }

    fn parse_string_literal(&mut self, start: usize) -> Option<String> {
        self.bump_char();
        let mut out = String::new();
        while let Some(c) = self.peek_char() {
            self.bump_char();
            match c {
                '"' => return Some(out),
                '\\' => {
                    let Some(next) = self.peek_char() else {
                        self.error(
                            start,
                            "unterminated string literal in `@capability` binding",
                        );
                        return None;
                    };
                    self.bump_char();
                    out.push(next);
                }
                _ => out.push(c),
            }
        }
        self.error(
            start,
            "unterminated string literal in `@capability` binding",
        );
        None
    }

    // Consumes the `,` it stops at — the caller re-enters the binding loop,
    // and leaving the comma unconsumed would make a failed binding parse spin
    // forever (fail at `,`, recover to the same `,`, repeat).
    fn recover_to_next_binding(&mut self) {
        while let Some(c) = self.peek_char() {
            match c {
                ',' => {
                    self.bump_char();
                    return;
                }
                '}' => return,
                _ => self.bump_char(),
            }
        }
    }

    fn read_capability_name(&mut self) -> &str {
        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() || c == '{' {
                break;
            }
            self.bump_char();
        }
        &self.body[start..self.pos]
    }

    fn read_ident(&mut self) -> &str {
        let start = self.pos;
        while let Some(c) = self.peek_char() {
            if c.is_alphanumeric() || c == '_' {
                self.bump_char();
            } else {
                break;
            }
        }
        &self.body[start..self.pos]
    }

    fn read_number(&mut self) -> &str {
        let start = self.pos;
        if self.peek_char() == Some('-') {
            self.bump_char();
        }
        while let Some(c) = self.peek_char() {
            if c.is_ascii_digit() || matches!(c, '.') {
                self.bump_char();
            } else {
                break;
            }
        }
        &self.body[start..self.pos]
    }

    fn skip_ws(&mut self) {
        while self.peek_char().is_some_and(char::is_whitespace) {
            self.bump_char();
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.body.get(self.pos..)?.chars().next()
    }

    fn bump_char(&mut self) {
        if let Some(c) = self.peek_char() {
            self.pos += c.len_utf8();
        }
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span {
            file: self.file,
            start: self.body_offset + start as u32,
            end: self.body_offset + end as u32,
        }
    }

    fn error(&mut self, pos: usize, message: &str) {
        let end = self
            .body
            .get(pos..)
            .and_then(|tail| tail.chars().next())
            .map_or(pos, |character| pos + character.len_utf8());
        self.diagnostics.push(DocCapabilityDiagnostic {
            span: self.span(pos, end),
            message: message.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> DocComment {
        parse_doc_comment(&RawDoc {
            text: text.to_string(),
            span: Span::new(crate::FileId(0), 0, text.len() as u32).unwrap(),
        })
        .unwrap()
    }

    #[test]
    fn dotted_param_name_is_preserved() {
        let d = parse("/** @param opts.a Nested property. */");
        assert_eq!(d.params[0].name, "opts.a");
        assert_eq!(d.params[0].description, "Nested property.");
        assert_eq!(d.params[0].name_span.end - d.params[0].name_span.start, 6);
    }

    #[test]
    fn empty_doc_yields_empty_summary() {
        let d = parse("/** */");
        assert_eq!(d.summary, "");
        assert!(d.params.is_empty());
        assert!(d.returns.is_none());
    }

    #[test]
    fn single_line_summary() {
        let d = parse("/** Sum two numbers. */");
        assert_eq!(d.summary, "Sum two numbers.");
    }

    #[test]
    fn multi_line_summary_with_star_prefix() {
        let d = parse("/**\n * First line.\n * Second line.\n */");
        assert_eq!(d.summary, "First line. Second line.");
    }

    #[test]
    fn param_tag_with_description() {
        let d = parse("/**\n * Summary.\n * @param x The first operand.\n */");
        assert_eq!(d.summary, "Summary.");
        assert_eq!(d.params.len(), 1);
        assert_eq!(d.params[0].name, "x");
        assert_eq!(d.params[0].description, "The first operand.");
    }

    #[test]
    fn returns_tag() {
        let d = parse("/** @returns The result. */");
        assert!(d.returns.is_some());
        assert_eq!(d.returns.as_ref().unwrap().description, "The result.");
    }

    #[test]
    fn deprecated_and_throws_and_example() {
        let d = parse(
            "/**\n * @deprecated Use foo instead.\n * @throws when x is negative.\n * @example bar()\n */",
        );
        assert!(d.deprecated.is_some());
        assert_eq!(d.throws.len(), 1);
        assert_eq!(d.examples.len(), 1);
    }

    #[test]
    fn unknown_tag_lands_in_unknown() {
        let d = parse("/** @internal Some text. */");
        assert_eq!(d.unknown_tags.len(), 1);
        assert_eq!(d.unknown_tags[0].name, "internal");
        assert_eq!(d.unknown_tags[0].text, "Some text.");
    }

    #[test]
    fn capability_tag_with_bindings() {
        let d = parse(
            "/**\n * @capability stripe.com/charge { amount, currency, customer: $params.customerId, readonly: true, query: string, label: \"x\" } - Charge customer.\n */",
        );
        assert!(d.unknown_tags.is_empty());
        assert_eq!(d.capabilities.len(), 1);
        let cap = &d.capabilities[0];
        assert_eq!(cap.capability, "stripe.com/charge");
        assert_eq!(cap.description, "Charge customer.");
        assert_eq!(cap.bindings.len(), 6);
        assert!(cap.diagnostics.is_empty(), "{:?}", cap.diagnostics);
        assert!(matches!(
            cap.bindings[0].kind,
            DocCapabilityBindingKind::Parameter { ref param, ref path, .. }
                if param == "amount" && path.is_empty()
        ));
        assert!(matches!(
            cap.bindings[2].kind,
            DocCapabilityBindingKind::Parameter { ref param, ref path, .. }
                if param == "params" && path == &vec!["customerId".to_string()]
        ));
        assert!(matches!(
            cap.bindings[3].kind,
            DocCapabilityBindingKind::Literal {
                value: DocCapabilityLiteral::Boolean(true),
                ..
            }
        ));
        assert!(matches!(
            cap.bindings[4].kind,
            DocCapabilityBindingKind::Type { ref name, .. } if name == "string"
        ));
        assert!(matches!(
            cap.bindings[5].kind,
            DocCapabilityBindingKind::Literal {
                value: DocCapabilityLiteral::String(ref s),
                ..
            } if s == "x"
        ));
    }

    #[test]
    fn capability_type_binding_preserves_array_suffix() {
        let d = parse("/** @capability mail/send { recipients: string[] } */");
        let cap = &d.capabilities[0];
        assert!(cap.diagnostics.is_empty(), "{:?}", cap.diagnostics);
        assert!(matches!(
            cap.bindings[0].kind,
            DocCapabilityBindingKind::Type { ref name, .. } if name == "string[]"
        ));
    }

    #[test]
    fn malformed_capability_binding_records_diagnostic() {
        let d = parse("/** @capability x/op { a: $ } */");
        assert_eq!(d.capabilities.len(), 1);
        assert!(
            d.capabilities[0]
                .diagnostics
                .iter()
                .any(|diag| diag.message.contains("expected parameter name")),
            "{:?}",
            d.capabilities[0].diagnostics
        );
    }

    #[test]
    fn binding_error_before_a_comma_recovers_to_the_next_binding() {
        // `b.c` fails mid-binding; recovery must consume the `,` or the
        // binding loop re-fails at the same position forever.
        let d = parse("/** @capability x/op { a: b.c, d } */");
        let cap = &d.capabilities[0];
        assert!(!cap.diagnostics.is_empty());
        assert!(
            cap.bindings.iter().any(|b| b.field == "d"),
            "{:?}",
            cap.bindings
        );
    }

    #[test]
    fn jsdoc_type_annotation_stripped() {
        let d = parse("/** @param {string} name The user's name. */");
        assert_eq!(d.params[0].name, "name");
        assert_eq!(d.params[0].description, "The user's name.");
    }

    #[test]
    fn multiple_params() {
        let d = parse("/**\n * @param a First.\n * @param b Second.\n */");
        assert_eq!(d.params.len(), 2);
        assert_eq!(d.params[0].name, "a");
        assert_eq!(d.params[0].description, "First.");
        assert_eq!(d.params[1].name, "b");
        assert_eq!(d.params[1].description, "Second.");
    }

    #[test]
    fn multi_line_param_description() {
        let d =
            parse("/**\n * @param callback Called once per element.\n *   Should not throw.\n */");
        assert_eq!(d.params.len(), 1);
        assert_eq!(
            d.params[0].description,
            "Called once per element. Should not throw."
        );
    }

    #[test]
    fn doc_helper_for_synthetic_prelude_docs() {
        let d = doc(
            crate::FileId(0),
            "/** Returns this number as a base-10 string. */",
        )
        .unwrap();
        assert_eq!(d.summary, "Returns this number as a base-10 string.");
    }

    #[test]
    fn doc_helper_rejects_non_doc_literal() {
        assert!(doc(crate::FileId(0), "// not a doc").is_none());
        assert!(doc(crate::FileId(0), "/* regular block */").is_none());
    }

    #[test]
    fn param_name_span_anchors_to_source_offset() {
        let raw = RawDoc {
            text: "/** @param foo desc */".to_string(),
            span: Span::new(crate::FileId(0), 100, 100 + 22).unwrap(),
        };
        let d = parse_doc_comment(&raw).unwrap();
        // `@param` is at byte 4 of the raw text → source offset 104.
        assert_eq!(d.params[0].tag_span.start, 104);
        // `foo` starts at byte 11 of the raw text → 111.
        assert_eq!(d.params[0].name_span.start, 111);
        assert_eq!(d.params[0].name_span.end, 114);
    }
}
