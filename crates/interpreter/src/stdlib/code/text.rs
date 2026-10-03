//! Text transformations never perform I/O. A rejected edit cannot partially write.
use serde_json::{Value, json};
use similar::{ChangeTag, DiffOp};
use std::{collections::HashMap, fmt::Write};
use wasmtime::{Result, bail};

pub(super) const MAX_DIAGNOSTICS: usize = 1000;

pub(super) struct Edit {
    pub text: String,
    pub diagnostics: Vec<Value>,
}
impl Edit {
    pub fn success(text: String) -> Self {
        Self {
            text,
            diagnostics: vec![],
        }
    }
    pub fn reject(original: &str, diagnostics: Vec<Value>) -> Self {
        Self {
            text: original.into(),
            diagnostics,
        }
    }
}
pub(super) fn diagnostic(hunk: usize, line: usize, message: impl Into<String>) -> Value {
    json!({"hunk": hunk, "line": line, "message": message.into()})
}
pub(super) fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}
pub(super) fn line_text(text: &str) -> &str {
    match text.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => text,
    }
}

pub(super) fn numbered(text: &[&str], start: usize, end: usize) -> Vec<Value> {
    text.iter()
        .enumerate()
        .take(end)
        .skip(start)
        .map(|(i, s)| json!({"line": i + 1, "text": line_text(s)}))
        .collect()
}

pub(super) fn replace(
    text: &str,
    old: &str,
    new: &str,
    all: bool,
    near: usize,
    maximum: usize,
) -> Result<Edit> {
    if old.is_empty() {
        bail!("code.edit: oldString must not be empty; use insertAt");
    }
    if all && near != 0 {
        bail!("code.edit: replaceAll and nearLine cannot be combined");
    }
    let matches = occurrences(text, old);
    if matches.is_empty() {
        return whitespace_replace(text, old, new, maximum);
    }
    let positions = match_lines(text, &matches);
    let selected = select_matches(&positions, all, near, old.len());
    let Some(selected) = selected else {
        check_diagnostic_count(positions.len())?;
        return Ok(Edit::reject(
            text,
            positions
                .iter()
                .map(|&(_, line)| {
                    diagnostic(
                        0,
                        line,
                        "Anchor occurs here; include more context or pass nearLine.",
                    )
                })
                .collect(),
        ));
    };
    let length = text
        .len()
        .saturating_sub(selected.len().saturating_mul(old.len()))
        .saturating_add(selected.len().saturating_mul(new.len()));
    if length > maximum {
        bail!("code.edit: replacement exceeds maxReadSize; use a smaller edit");
    }
    let mut result = String::with_capacity(length);
    let mut start = 0;
    for index in selected {
        result.push_str(&text[start..index]);
        result.push_str(new);
        start = index + old.len();
    }
    result.push_str(&text[start..]);
    Ok(Edit::success(result))
}
pub(super) fn insert(text: &str, line: usize, new: &str) -> Result<Edit> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let parts = lines(body);
    if line == 0 || line > parts.len() + 1 {
        bail!(
            "code.insertAt: line must be between 1 and {}",
            parts.len() + 1
        );
    }
    let offset =
        text.len() - body.len() + parts.iter().take(line - 1).map(|s| s.len()).sum::<usize>();
    Ok(Edit::success(format!(
        "{}{}{}",
        &text[..offset],
        new,
        &text[offset..]
    )))
}

/// The lines `diff` compares: LF-terminated, the last one unterminated when
/// the text does not end in LF, and none at all for an empty text.
fn lines_inclusive(text: &[u16]) -> impl Iterator<Item = &[u16]> {
    text.split_inclusive(|&u| u == 10)
}

/// How many lines `diff` would compare for `text`.
pub(super) fn line_count(text: &[u16]) -> usize {
    lines_inclusive(text).count()
}

/// Bound retained line/frontier metadata independently of edit distance.
pub(super) fn check_diff_size(old_lines: usize, new_lines: usize) -> Result<()> {
    if old_lines.saturating_add(new_lines) > 1_000_000 {
        bail!("code.diff: comparison exceeds line-count limit; compare smaller sections");
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn diff(a: &[u16], b: &[u16]) -> Result<Vec<u16>> {
    diff_charged(a, b, |_| Ok(()))
}

/// Slice on LF without decoding UTF-16, preserving lone surrogates and final newlines.
pub(super) fn diff_charged(
    a: &[u16],
    b: &[u16],
    mut charge: impl FnMut(u64) -> Result<()>,
) -> Result<Vec<u16>> {
    let na = line_count(a);
    let nb = line_count(b);
    check_diff_size(na, nb)?;
    let mut old = Vec::new();
    let mut new = Vec::new();
    old.try_reserve_exact(na)
        .map_err(crate::runtime::host::fatal_host_error)?;
    new.try_reserve_exact(nb)
        .map_err(crate::runtime::host::fatal_host_error)?;
    old.extend(lines_inclusive(a));
    new.extend(lines_inclusive(b));
    let groups = diff_groups(diff_line_ops(&old, &new, &mut charge)?)?;
    let mut output = Vec::new();
    if groups.is_empty() {
        return Ok(output);
    }
    // Lines occur at most once across disjoint groups. Each line adds a sign
    // and at most one missing-newline marker; a hunk header fits 128 units.
    let capacity = a
        .len()
        .saturating_add(b.len())
        .saturating_add(29 * (na + nb))
        .saturating_add(128 * groups.len())
        .saturating_add(12);
    output
        .try_reserve_exact(capacity)
        .map_err(crate::runtime::host::fatal_host_error)?;
    output.extend("--- a\n+++ b\n".encode_utf16());
    for group in groups {
        let first = group.first().ok_or_else(|| {
            crate::runtime::host::fatal_host_error("code.diff: internal empty group")
        })?;
        let a_start = first.old_range().start;
        let a_len: usize = group.iter().map(|op| op.old_range().len()).sum();
        let b_start = first.new_range().start;
        let b_len: usize = group.iter().map(|op| op.new_range().len()).sum();
        let mut header = String::new();
        header
            .try_reserve_exact(128)
            .map_err(crate::runtime::host::fatal_host_error)?;
        writeln!(
            header,
            "@@ -{},{} +{},{} @@",
            a_start + usize::from(a_len != 0),
            a_len,
            b_start + usize::from(b_len != 0),
            b_len
        )
        .map_err(crate::runtime::host::fatal_host_error)?;
        output.extend(header.encode_utf16());
        for op in group {
            for change in op.iter_changes(&old, &new) {
                output.push(match change.tag() {
                    ChangeTag::Equal => 32,
                    ChangeTag::Delete => 45,
                    ChangeTag::Insert => 43,
                });
                let value = change.value();
                output.extend_from_slice(value);
                if !value.ends_with(&[10]) {
                    output.extend("\n\\ No newline at end of file\n".encode_utf16());
                }
            }
        }
    }
    Ok(output)
}

/// Keep three lines of context, reserving every group before it grows.
fn diff_groups(mut ops: Vec<DiffOp>) -> Result<Vec<Vec<DiffOp>>> {
    if let Some(DiffOp::Equal {
        old_index,
        new_index,
        len,
    }) = ops.first_mut()
    {
        let offset = len.saturating_sub(3);
        *old_index += offset;
        *new_index += offset;
        *len -= offset;
    }
    if let Some(DiffOp::Equal { len, .. }) = ops.last_mut() {
        *len = (*len).min(3);
    }
    let mut groups = Vec::new();
    let mut pending = Vec::new();
    for op in ops {
        if let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
            && len > 6
        {
            push_diff_item(
                &mut pending,
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len: 3,
                },
            )?;
            push_diff_item(&mut groups, std::mem::take(&mut pending))?;
            let offset = len - 3;
            push_diff_item(
                &mut pending,
                DiffOp::Equal {
                    old_index: old_index + offset,
                    new_index: new_index + offset,
                    len: 3,
                },
            )?;
            continue;
        }
        push_diff_item(&mut pending, op)?;
    }
    if !matches!(pending.as_slice(), [] | [DiffOp::Equal { .. }]) {
        push_diff_item(&mut groups, pending)?;
    }
    Ok(groups)
}

fn push_diff_item<T>(items: &mut Vec<T>, item: T) -> Result<()> {
    items
        .try_reserve(1)
        .map_err(crate::runtime::host::fatal_host_error)?;
    items.push(item);
    Ok(())
}

pub(super) fn occurrences(text: &str, anchor: &str) -> Vec<usize> {
    if anchor.is_empty() {
        return vec![];
    }
    let step = anchor.chars().next().map_or(1, char::len_utf8);
    let mut matches = Vec::new();
    let mut offset = 0;
    while let Some(found) = text[offset..].find(anchor) {
        let start = offset + found;
        matches.push(start);
        offset = start + step;
    }
    matches
}
fn match_lines(text: &str, offsets: &[usize]) -> Vec<(usize, usize)> {
    let mut previous = 0;
    let mut line = 1;
    offsets
        .iter()
        .map(|&offset| {
            line += text[previous..offset]
                .bytes()
                .filter(|&b| b == b'\n')
                .count();
            previous = offset;
            (offset, line)
        })
        .collect()
}
fn select_matches(
    matches: &[(usize, usize)],
    all: bool,
    near: usize,
    width: usize,
) -> Option<Vec<usize>> {
    if all {
        let mut end = 0;
        return Some(
            matches
                .iter()
                .filter_map(|&(offset, _)| {
                    if offset < end {
                        None
                    } else {
                        end = offset + width;
                        Some(offset)
                    }
                })
                .collect(),
        );
    }
    if matches.len() == 1 {
        return Some(vec![matches[0].0]);
    }
    if near == 0 {
        return None;
    }
    let mut distances: Vec<_> = matches
        .iter()
        .map(|&(offset, line)| (line.abs_diff(near), offset))
        .collect();
    distances.sort_unstable();
    if distances[0].0 == distances[1].0 {
        None
    } else {
        Some(vec![distances[0].1])
    }
}

fn whitespace_replace(text: &str, old: &str, new: &str, maximum: usize) -> Result<Edit> {
    let actual = lines(text);
    let anchor = lines(old);
    let candidates: Vec<_> = actual
        .windows(anchor.len())
        .enumerate()
        .filter(|(_, window)| {
            window
                .iter()
                .zip(&anchor)
                .all(|(a, b)| a.trim() == b.trim())
        })
        .map(|(i, _)| i)
        .collect();
    if candidates.len() == 1 {
        let start = candidates[0];
        if let Some(replacement) = reindent(
            &actual[start..start + anchor.len()],
            &anchor,
            new,
            maximum.saturating_sub(
                text.len()
                    - actual[start..start + anchor.len()]
                        .iter()
                        .map(|line| line.len())
                        .sum::<usize>(),
            ),
        ) {
            let mut result = actual[..start].concat();
            result.push_str(&replacement);
            result.push_str(&actual[start + anchor.len()..].concat());
            return Ok(Edit::success(result));
        }
    }
    check_diagnostic_count(candidates.len())?;
    let diagnostics = if candidates.is_empty() {
        // Token overlap is guidance only, never an edit target.
        let tokens: Vec<_> = old.split_whitespace().take(64).collect();
        let best = actual
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| tokens.iter().filter(|token| s.contains(**token)).count());
        vec![match best {
            Some((i, s)) => diagnostic(
                0,
                i + 1,
                format!(
                    "No exact anchor. Nearby candidate: {}. Copy its exact text and include context.",
                    line_text(s)
                ),
            ),
            None => diagnostic(0, 1, "No exact anchor; the file is empty. Use insertAt."),
        }]
    } else {
        candidates
            .iter()
            .map(|i| {
                diagnostic(
                    0,
                    i + 1,
                    "Whitespace near-match; provide the exact indentation and line endings.",
                )
            })
            .collect()
    };
    Ok(Edit::reject(text, diagnostics))
}
fn check_diagnostic_count(count: usize) -> Result<()> {
    if count > MAX_DIAGNOSTICS {
        bail!(
            "code.edit: anchor has {count} occurrences, exceeding the {MAX_DIAGNOSTICS} diagnostic limit; include more unique context"
        );
    }
    Ok(())
}
fn indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}
fn reindent(actual: &[&str], anchor: &[&str], new: &str, maximum: usize) -> Option<String> {
    let first = anchor.iter().position(|s| !s.trim().is_empty())?;
    let from = indent(anchor[first]);
    let to = indent(actual[first]);
    for (a, b) in actual.iter().zip(anchor) {
        if a.ends_with('\n') != b.ends_with('\n') {
            return None;
        }
        if !b.trim().is_empty() && indent(a) != format!("{}{}", to, indent(b).strip_prefix(from)?) {
            return None;
        }
    }
    let mut result = String::new();
    let crlf = actual
        .iter()
        .all(|s| !s.ends_with('\n') || s.ends_with("\r\n"));
    for line in lines(new) {
        let body = line_text(line);
        let content = if body.trim().is_empty() {
            body
        } else {
            body.strip_prefix(from)?
        };
        let prefix = if body.trim().is_empty() { "" } else { to };
        let ending = if line.ends_with('\n') {
            if crlf { "\r\n" } else { "\n" }
        } else {
            ""
        };
        if result
            .len()
            .saturating_add(prefix.len())
            .saturating_add(content.len())
            .saturating_add(ending.len())
            > maximum
        {
            return None;
        }
        result.push_str(prefix);
        result.push_str(content);
        result.push_str(ending);
    }
    Some(result)
}

fn diff_line_ops(
    old: &[&[u16]],
    new: &[&[u16]],
    charge: &mut impl FnMut(u64) -> Result<()>,
) -> Result<Vec<similar::DiffOp>> {
    // Intern full lines so Myers compares constant-size IDs, including long shared prefixes.
    let mut lines = HashMap::new();
    lines
        .try_reserve(old.len().saturating_add(new.len()))
        .map_err(crate::runtime::host::fatal_host_error)?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(old.len().saturating_add(new.len()))
        .map_err(crate::runtime::host::fatal_host_error)?;
    for line in old.iter().chain(new) {
        let next = lines.len();
        ids.push(*lines.entry(*line).or_insert(next));
    }
    let (old_ids, new_ids) = ids.split_at(old.len());
    super::myers::diff(old_ids, new_ids, charge)
}
