//! Edit-distance "did you mean?" suggestions.
//!
//! Shared by typechecker diagnostics and by package/builtin discovery, which
//! needs the same candidate matching outside the typechecker.

pub fn closest_match<'a>(
    query: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let threshold = (query.len() / 3).max(2);
    let mut best: Option<(usize, &'a str)> = None;
    for c in candidates {
        let d = levenshtein(query, c);
        if d > threshold {
            continue;
        }
        match best {
            Some((bd, _)) if bd <= d => {}
            _ => best = Some((d, c)),
        }
    }
    best.map(|(_, c)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    // Make `a` the shorter side so the row vec is O(min(len)).
    let (a, b) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut prev: Vec<usize> = (0..=a.len()).collect();
    let mut curr: Vec<usize> = vec![0; a.len() + 1];
    for (j, &bc) in b.iter().enumerate() {
        curr[0] = j + 1;
        for (i, &ac) in a.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            curr[i + 1] = (curr[i] + 1).min(prev[i + 1] + 1).min(prev[i] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[a.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_char_typo_matches() {
        assert_eq!(
            closest_match("consol", ["console", "context", "main"]),
            Some("console"),
        );
    }

    #[test]
    fn transposition_matches() {
        assert_eq!(
            closest_match("numbre", ["number", "string", "boolean"]),
            Some("number"),
        );
    }

    #[test]
    fn unrelated_returns_none() {
        assert_eq!(closest_match("xyz", ["console", "string", "number"]), None,);
    }

    #[test]
    fn empty_candidates_returns_none() {
        let v: Vec<&str> = vec![];
        assert_eq!(closest_match("anything", v), None);
    }

    #[test]
    fn ties_break_by_first_iteration_order() {
        assert_eq!(closest_match("aa", ["ab", "ac"]), Some("ab"));
    }

    #[test]
    fn threshold_scales_with_length() {
        assert_eq!(
            closest_match("consoel", ["console", "xyzabcd"]),
            Some("console"),
        );
    }

    #[test]
    fn exact_match_returns_self() {
        assert_eq!(closest_match("foo", ["foo", "bar"]), Some("foo"));
    }

    #[test]
    fn longer_query_allows_more_edits() {
        assert_eq!(
            closest_match("prevDestnce", ["prevDistance", "unrelated"]),
            Some("prevDistance"),
        );
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("foo", "foo"), 0);
    }
}
