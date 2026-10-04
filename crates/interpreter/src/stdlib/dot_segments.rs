//! Refusal of URLs whose path holds a `.` or `..` segment.
//!
//! The URL parser removes a dot segment, and a `..` takes the segment before it
//! along, so `/customers/%2E%2E/admin` is sent as `/admin`. A package that puts a
//! caller's id into a path would then reach an endpoint other than the one its
//! capability check described. The test reads the URL before it is parsed,
//! because once it is parsed the segment is already gone.

use crate::stdlib::url::is_scheme;

/// A URL refused because its path holds a dot segment.
pub(crate) struct DotSegmentRefusal {
    /// The segment as written, such as `..` or `%2E%2E`.
    pub segment: String,
    /// The path the segment was found in, before normalization; tabs and
    /// newlines, which the parser drops, are already gone.
    pub path: String,
}

impl DotSegmentRefusal {
    pub(crate) fn into_error(self, operation: &str) -> wasmtime::Error {
        let DotSegmentRefusal { segment, path } = self;
        crate::runtime::host::type_error(format!(
            "{operation}: the URL path {path:?} has the dot segment {segment:?}; a URL parser \
             removes a `.` segment, and a `..` segment with the segment before it, so build \
             the path without them"
        ))
    }
}

/// Refuse `url` if its path holds a `.` or `..` segment in any spelling the URL
/// parser reads as one.
///
/// The path is found the way the WHATWG URL parser finds it: tabs and newlines
/// are dropped, surrounding controls and spaces are trimmed, the slashes after
/// the scheme are skipped, and the authority ends at the first `/`, `\`, `?` or
/// `#`. A URL with no scheme cannot be parsed and is left for the transport to
/// refuse.
pub(crate) fn refuse_dot_segments(url: &str) -> Result<(), DotSegmentRefusal> {
    let cleaned = without_tabs_and_newlines(url.trim_matches(|c: char| c <= ' '));
    match raw_path(&cleaned) {
        Some(path) => refuse_segments(path),
        None => Ok(()),
    }
}

/// Refuse `path`, the path of a URL, if it holds a `.` or `..` segment. Tabs and
/// newlines are dropped and `\` separates segments as `/` does, as the parser does
/// for `http` and `https`. For a scheme where `\` is an ordinary character this
/// can only over-refuse, and only `http` and `https` URLs are sent.
pub(crate) fn refuse_dot_segments_in_path(path: &str) -> Result<(), DotSegmentRefusal> {
    refuse_segments(&without_tabs_and_newlines(path))
}

/// Refuse `path`, already without tabs and newlines, if a segment is a dot segment.
fn refuse_segments(path: &str) -> Result<(), DotSegmentRefusal> {
    match path
        .split(['/', '\\'])
        .find(|segment| is_dot_segment(segment))
    {
        Some(segment) => Err(DotSegmentRefusal {
            segment: segment.to_string(),
            path: path.to_string(),
        }),
        None => Ok(()),
    }
}

fn without_tabs_and_newlines(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect()
}

/// The path of `url`, up to its query or fragment; `None` without a scheme.
fn raw_path(url: &str) -> Option<&str> {
    let (scheme, after_scheme) = url.split_once(':')?;
    if !is_scheme(scheme) {
        return None;
    }
    let authority_and_path = after_scheme.trim_start_matches(['/', '\\']);
    let after_authority =
        authority_and_path.trim_start_matches(|c| !matches!(c, '/' | '\\' | '?' | '#'));
    after_authority.split(['?', '#']).next()
}

/// The spellings of `.` and `..`, with each dot literal or `%2E`, compared
/// without regard to case.
const DOT_SEGMENT_SPELLINGS: [&str; 6] = [".", "%2e", "..", ".%2e", "%2e.", "%2e%2e"];

fn is_dot_segment(segment: &str) -> bool {
    DOT_SEGMENT_SPELLINGS
        .iter()
        .any(|spelling| segment.eq_ignore_ascii_case(spelling))
}

#[cfg(test)]
mod tests {
    use super::{refuse_dot_segments, refuse_dot_segments_in_path};

    fn refused(url: &str) -> Option<(String, String)> {
        refuse_dot_segments(url)
            .err()
            .map(|refusal| (refusal.segment, refusal.path))
    }

    #[test]
    fn every_spelling_is_refused_in_the_middle_and_at_the_end() {
        for spelling in [
            "..", ".", "%2E%2E", "%2e%2e", ".%2E", "%2E.", "%2e", "%2E", "%2e%2E",
        ] {
            let middle = format!("https://example.com/customers/{spelling}/admin");
            assert_eq!(
                refused(&middle),
                Some((spelling.to_string(), format!("/customers/{spelling}/admin"))),
                "{middle}"
            );
            let last = format!("https://example.com/customers/{spelling}");
            assert_eq!(
                refused(&last),
                Some((spelling.to_string(), format!("/customers/{spelling}"))),
                "{last}"
            );
        }
    }

    /// Each of these reaches the parser as a dot segment, which the parser removes.
    #[test]
    fn spellings_the_parser_cleans_up_are_refused() {
        for (url, normalized) in [
            ("https://example.com/customers\\..\\admin", "/admin"),
            ("https://example.com/customers/.\t./admin", "/admin"),
            ("https://example.com/customers/%2\nE%2E/admin", "/admin"),
            ("https://example.com/customers/.. ", "/"),
            ("  https://example.com/customers/../admin", "/admin"),
            ("https:example.com/customers/../admin", "/admin"),
            ("https:\\\\example.com/customers/../admin", "/admin"),
            ("HTTPS://example.com/customers/../admin", "/admin"),
            ("https://example.com/customers/..?q=1", "/"),
            ("https://example.com/customers/..#top", "/"),
        ] {
            let parsed = url::Url::parse(url).expect("the probe parses");
            assert_eq!(parsed.path(), normalized, "the parser normalizes {url:?}");
            assert!(refused(url).is_some(), "{url:?} must be refused");
        }
    }

    #[test]
    fn names_with_dots_queries_and_fragments_are_accepted() {
        for url in [
            "https://example.com/repo.js",
            "https://example.com/.env",
            "https://example.com/...",
            "https://example.com/a..b",
            "https://example.com/..%20",
            "https://example.com/%252E%252E",
            "https://example.com/v1.2.3/x",
            "https://example.com/x?q=..",
            "https://example.com/x?q=/../..",
            "https://example.com/x#/../",
            "https://example.com",
            "https://example.com/",
            "https://example.com?..",
            "https://user:pa..ss@example.com/x",
            "https://example.com:443/x",
            "not a url",
            "",
        ] {
            assert!(refused(url).is_none(), "{url:?} must be accepted");
        }
    }

    #[test]
    fn a_path_alone_is_split_as_the_parser_splits_it() {
        for path in ["/a/../b", "/a\\..\\b", "/a/.\t./b", "..", "/a/%2e"] {
            assert!(refuse_dot_segments_in_path(path).is_err(), "{path:?}");
        }
        for path in ["/a/b", "/repo.js/...", "", "/a..b/..%20"] {
            assert!(refuse_dot_segments_in_path(path).is_ok(), "{path:?}");
        }
    }

    #[test]
    fn a_host_that_looks_like_a_dot_segment_is_not_a_path() {
        assert!(refused("https://../x").is_none());
    }
}
