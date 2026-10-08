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

/// Append the code unit `unit` to literal text if it is a surrogate, returning
/// whether it was.
pub(crate) fn push_lone_surrogate(out: &mut String, unit: u32) -> bool {
    let Some(named) = SURROGATES
        .contains(&unit)
        .then(|| char::from_u32(SURROGATE_BASE + (unit - 0xD800)))
        .flatten()
    else {
        return false;
    };
    out.push(MARKER);
    out.push(named);
    true
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
        let c = if c == MARKER {
            match chars.next() {
                Some(MARKER) | None => MARKER,
                Some(named) => {
                    units.push((u32::from(named) - SURROGATE_BASE + 0xD800) as u16);
                    continue;
                }
            }
        } else {
            c
        };
        units.extend_from_slice(c.encode_utf16(&mut [0; 2]));
    }
    units
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
    fn text_without_the_marker_is_plain_utf16() {
        let text = "a\u{10FFFF}\u{100000}é";
        assert_eq!(literal_units(text), text.encode_utf16().collect::<Vec<_>>());
    }
}
