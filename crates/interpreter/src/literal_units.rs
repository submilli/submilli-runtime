//! Lone surrogates in compile-time string text.
//!
//! JavaScript strings are UTF-16 code units, and a literal such as `"\ud800"`
//! holds an unpaired surrogate. The compiler keeps literal text in Rust
//! `String`s, which can't, so the lexer escapes it: a lone surrogate is
//! [`MARKER`] followed by a code point in Supplementary Private Use Area-B
//! that names it, and a genuine [`MARKER`] is written twice. Codegen turns the
//! text back into code units with [`literal_units`].

/// A noncharacter, so real text rarely holds it.
const MARKER: char = '\u{10FFFE}';
/// The code point after [`MARKER`] that names U+D800; U+DFFF is 0x7FF above.
const SURROGATE_BASE: u32 = 0x10_0000;
const SURROGATES: std::ops::RangeInclusive<u32> = 0xD800..=0xDFFF;
const HIGH_SURROGATES: std::ops::RangeInclusive<u32> = 0xD800..=0xDBFF;
const LOW_SURROGATES: std::ops::RangeInclusive<u32> = 0xDC00..=0xDFFF;

/// Append the code unit `unit` to literal text if it is a surrogate, returning
/// whether it was. A low surrogate right after a lone high one joins it into
/// one character, so `"\u{D83D}\u{DE00}"` is the same text as `"😀"`.
pub(crate) fn push_lone_surrogate(out: &mut String, unit: u32) -> bool {
    let Some(named) = SURROGATES
        .contains(&unit)
        .then(|| char::from_u32(SURROGATE_BASE + (unit - 0xD800)))
        .flatten()
    else {
        return false;
    };
    if LOW_SURROGATES.contains(&unit)
        && let Some(high) = trailing_high_surrogate(out)
        && let Some(Ok(pair)) = char::decode_utf16([high, unit as u16]).next()
    {
        // Drop the marker and the name the high half was written as.
        out.pop();
        out.pop();
        push_literal_char(out, pair);
        return true;
    }
    out.push(MARKER);
    out.push(named);
    true
}

/// Append literal text to literal text, joining a lone high surrogate at the
/// end of `out` with a lone low surrogate at the start of `text`, as
/// concatenating the strings they stand for does.
pub(crate) fn push_literal_text(out: &mut String, text: &str) {
    let mut chars = text.chars();
    if chars.next() == Some(MARKER)
        && let Some(low) = chars.next().and_then(named_surrogate)
        && LOW_SURROGATES.contains(&u32::from(low))
    {
        push_lone_surrogate(out, u32::from(low));
        out.push_str(chars.as_str());
        return;
    }
    out.push_str(text);
}

/// The lone high surrogate that ends `out`, if one does.
fn trailing_high_surrogate(out: &str) -> Option<u16> {
    let mut tail = out.chars().rev();
    let high = tail.next().and_then(named_surrogate)?;
    if !HIGH_SURROGATES.contains(&u32::from(high)) {
        return None;
    }
    // The name counts only after an odd run of markers; an even run is
    // escaped markers followed by a genuine character.
    let markers = tail.take_while(|c| *c == MARKER).count();
    (markers % 2 == 1).then_some(high)
}

/// Append a character to literal text, escaping [`MARKER`].
pub(crate) fn push_literal_char(out: &mut String, c: char) {
    out.push(c);
    if c == MARKER {
        out.push(MARKER);
    }
}

/// The UTF-16 code units that literal text stands for.
pub(crate) fn literal_units(text: &str) -> Vec<u16> {
    let mut units = Vec::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != MARKER {
            units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
            continue;
        }
        let next = chars.next();
        if let Some(unit) = next.and_then(named_surrogate) {
            units.push(unit);
            continue;
        }
        // Doubled, or not followed by a named surrogate, the marker stands for
        // itself.
        units.extend_from_slice(MARKER.encode_utf16(&mut [0; 2]));
        if let Some(c) = next.filter(|c| *c != MARKER) {
            units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
        }
    }
    units
}

/// The characters literal text stands for, with each lone surrogate as `Err`
/// of its code unit, for printing the text back as source.
pub(crate) fn literal_chars(text: &str) -> impl Iterator<Item = Result<char, u16>> {
    char::decode_utf16(literal_units(text)).map(|c| c.map_err(|lone| lone.unpaired_surrogate()))
}

/// The surrogate a code point after [`MARKER`] names, if it names one.
fn named_surrogate(named: char) -> Option<u16> {
    let offset = u32::from(named).checked_sub(SURROGATE_BASE)?;
    let unit = 0xD800 + offset;
    SURROGATES.contains(&unit).then_some(unit as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(units: &[u32]) -> String {
        let mut text = String::new();
        for &unit in units {
            if !push_lone_surrogate(&mut text, unit) {
                push_literal_char(&mut text, char::from_u32(unit).unwrap());
            }
        }
        text
    }

    #[test]
    fn a_lone_surrogate_round_trips_to_one_unit() {
        let text = text_of(&[0x61, 0xD800, 0xDFFF, 0x1F600]);
        assert_eq!(
            literal_units(&text),
            vec![0x61, 0xD800, 0xDFFF, 0xD83D, 0xDE00]
        );
    }

    #[test]
    fn two_halves_concatenate_into_a_pair() {
        let text = text_of(&[0xD83D]) + &text_of(&[0xDE00]);
        assert_eq!(String::from_utf16(&literal_units(&text)).unwrap(), "😀");
    }

    #[test]
    fn halves_written_separately_join_into_one_character() {
        assert_eq!(text_of(&[0xD83D, 0xDE00]), "😀");
        assert_eq!(
            text_of(&[0xDE00, 0xD83D]),
            text_of(&[0xDE00]) + &text_of(&[0xD83D])
        );
    }

    #[test]
    fn appended_text_joins_halves_across_the_seam() {
        let mut text = text_of(&[0x61, 0xD83D]);
        push_literal_text(&mut text, &text_of(&[0xDE00, 0x62]));
        assert_eq!(text, "a😀b");
        let mut doubled = text_of(&[0x10FFFE]);
        push_literal_text(&mut doubled, &text_of(&[0x10FFFE]));
        assert_eq!(doubled, text_of(&[0x10FFFE, 0x10FFFE]));
    }

    #[test]
    fn literal_chars_report_lone_surrogates() {
        let text = text_of(&[0x61, 0xD800, 0x10FFFE]);
        let chars: Vec<_> = literal_chars(&text).collect();
        assert_eq!(chars, vec![Ok('a'), Err(0xD800), Ok('\u{10FFFE}')]);
    }

    #[test]
    fn an_escaped_marker_before_a_named_character_does_not_join() {
        let text = text_of(&[0x10FFFE, 0x10_0000 + 0x3D, 0xDE00]);
        let expected: Vec<u16> = "\u{10FFFE}\u{10003D}"
            .encode_utf16()
            .chain([0xDE00])
            .collect();
        assert_eq!(literal_units(&text), expected);
    }

    #[test]
    fn the_marker_itself_round_trips() {
        let text = text_of(&[0x10FFFE, 0xD800, 0x10FFFE, 0x10FFFF]);
        let expected: Vec<u16> = "\u{10FFFE}"
            .encode_utf16()
            .chain([0xD800])
            .chain("\u{10FFFE}\u{10FFFF}".encode_utf16())
            .collect();
        assert_eq!(literal_units(&text), expected);
    }

    #[test]
    fn a_stray_marker_stands_for_itself() {
        let text = "\u{10FFFE}a\u{10FFFE}";
        assert_eq!(literal_units(text), text.encode_utf16().collect::<Vec<_>>());
    }

    #[test]
    fn text_without_the_marker_is_plain_utf16() {
        let text = "a\u{10FFFF}\u{100000}é";
        assert_eq!(literal_units(text), text.encode_utf16().collect::<Vec<_>>());
    }
}
