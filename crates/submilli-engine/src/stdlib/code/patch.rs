//! Unified patch parsing and location against the original file, never header offsets.
use super::text::{Edit, diagnostic, lines};
use wasmtime::{Result, bail};

struct Hunk {
    old: String,
    new: String,
    new_ended: bool,
}
pub(super) fn apply(text: &str, patch: &str) -> Result<Edit> {
    let hunks = parse(patch)?;
    let mut located = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, hunk) in hunks.iter().enumerate() {
        let candidates = locations(text, &hunk.old);
        if candidates.len() != 1 {
            diagnostics.push(diagnostic(
                index + 1,
                0,
                if candidates.is_empty() {
                    "Hunk context not found. Re-read the file and copy exact context lines."
                } else {
                    "Hunk context is ambiguous. Include more unchanged context lines."
                },
            ));
            continue;
        }
        let start = candidates[0];
        let end = start + hunk.old.len();
        if hunk.new_ended && end != text.len() {
            diagnostics.push(diagnostic(index + 1, 0, "New-side EOF marker precedes unchanged file content; include the remaining lines or retain the newline."));
            continue;
        }
        if located
            .iter()
            .any(|&(a, b, _)| start < b && end > a || start == a)
        {
            diagnostics.push(diagnostic(
                index + 1,
                0,
                "Hunk overlaps another hunk; combine the changes into one hunk.",
            ));
        } else {
            located.push((start, end, index));
        }
    }
    if !diagnostics.is_empty() {
        return Ok(Edit::reject(text, diagnostics));
    }
    located.sort_unstable();
    let mut result = String::new();
    let mut end = 0;
    for (start, next, index) in located {
        result.push_str(&text[end..start]);
        result.push_str(&hunks[index].new);
        end = next;
    }
    result.push_str(&text[end..]);
    Ok(Edit::success(result))
}
fn locations(text: &str, anchor: &str) -> Vec<usize> {
    if anchor.is_empty() {
        return if text.is_empty() { vec![0] } else { vec![] };
    }
    // Only line starts can anchor a hunk. Searching overlapping byte matches first
    // would do quadratic work on long repeated lines before discarding them.
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(offset, _)| offset + 1))
        .filter(|i| {
            (anchor.ends_with('\n') || i + anchor.len() == text.len())
                && text[*i..].starts_with(anchor)
        })
        .collect()
}
fn parse(patch: &str) -> Result<Vec<Hunk>> {
    if patch.is_empty() {
        return Ok(vec![]);
    }
    let source = lines(patch);
    if source.len() < 2 || !source[0].starts_with("--- ") || !source[1].starts_with("+++ ") {
        bail!("code.applyPatch: expected single-file unified diff (--- and +++ headers)");
    }
    let header =
        regex::Regex::new(r"^@@ -[0-9]+(?:,([0-9]+))? \+[0-9]+(?:,([0-9]+))? @@(?:[^\n]*)\n?$")?;
    let mut offset = 2;
    let mut hunks = Vec::new();
    while offset < source.len() {
        hunks.push(parse_hunk(&source, &mut offset, &header)?);
    }
    if hunks.is_empty() {
        bail!("code.applyPatch: patch contains no hunks");
    }
    Ok(hunks)
}
fn parse_hunk(source: &[&str], offset: &mut usize, header: &regex::Regex) -> Result<Hunk> {
    let Some(captures) = header.captures(source[*offset]) else {
        bail!(
            "code.applyPatch: invalid hunk header at patch line {}",
            *offset + 1
        );
    };
    let old_count = captures
        .get(1)
        .map_or(Ok(1), |m| m.as_str().parse::<usize>())?;
    let new_count = captures
        .get(2)
        .map_or(Ok(1), |m| m.as_str().parse::<usize>())?;
    *offset += 1;
    let mut old = String::new();
    let mut new = String::new();
    let mut removed = 0;
    let mut added = 0;
    let mut previous = None;
    let mut old_ended = false;
    let mut new_ended = false;
    while *offset < source.len() && !source[*offset].starts_with("@@ ") {
        let line = source[*offset];
        if line.trim_end() == "\\ No newline at end of file" {
            match previous.take() {
                Some(b' ') => {
                    remove_newline(&mut old)?;
                    remove_newline(&mut new)?;
                    old_ended = true;
                    new_ended = true;
                }
                Some(b'-') => {
                    remove_newline(&mut old)?;
                    old_ended = true;
                }
                Some(b'+') => {
                    remove_newline(&mut new)?;
                    new_ended = true;
                }
                _ => bail!("code.applyPatch: misplaced no-newline marker"),
            }
        } else {
            if !line.ends_with('\n') {
                bail!(
                    "code.applyPatch: unterminated patch line; use a no-newline marker for file content"
                );
            }
            let tag = line.as_bytes()[0];
            if (old_ended && matches!(tag, b' ' | b'-'))
                || (new_ended && matches!(tag, b' ' | b'+'))
            {
                bail!("code.applyPatch: content follows a no-newline EOF marker");
            }
            match tag {
                b' ' => {
                    old.push_str(&line[1..]);
                    new.push_str(&line[1..]);
                    removed += 1;
                    added += 1;
                }
                b'-' => {
                    old.push_str(&line[1..]);
                    removed += 1;
                }
                b'+' => {
                    new.push_str(&line[1..]);
                    added += 1;
                }
                _ => bail!("code.applyPatch: invalid hunk line {}", *offset + 1),
            }
            previous = Some(tag);
        }
        *offset += 1;
    }
    if removed != old_count || added != new_count {
        bail!("code.applyPatch: hunk line counts do not match its header");
    }
    if old.is_empty() && new.is_empty() {
        bail!("code.applyPatch: empty hunk");
    }
    Ok(Hunk {
        old,
        new,
        new_ended,
    })
}
fn remove_newline(text: &mut String) -> Result<()> {
    if !text.ends_with('\n') {
        bail!("code.applyPatch: duplicate no-newline marker");
    }
    text.pop();
    Ok(())
}
