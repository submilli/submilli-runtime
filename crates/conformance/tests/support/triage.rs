//! Why each divergence is expected. A case's `.triage` explains lines of its
//! `.divergences`, one entry per line:
//!
//! ```text
//! line 12 type: by-design spec §1.2 an array literal's element type comes from its first element
//! line 14, 17 extra: bug SUB-1026 construct signatures read as a method
//! line 30 missed: artifact pruning removed the assignment that narrowed `x`
//! ```
//!
//! The divergences not yet explained are listed in `unexplained.txt`. The runner
//! fails on a divergence that is in neither, and on an explanation or a listed
//! divergence that no longer diverges, so the list only shrinks.
//!
//! An entry covers every divergence of its kind on its line, so a second one there
//! passes this check; the `.divergences` diff still shows it.
//!
//! A line that diverges for more than one reason names each, joined by `; also `.
//! Each starts with its own category, though only the first is checked, and a
//! reason never contains `; also ` itself.
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

/// What diverges on a line, as `.divergences` groups it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) enum Kind {
    /// A type we infer differently from `tsc`.
    Type,
    /// An error `tsc` reports that none of ours agrees with.
    Missed,
    /// An error of ours that none of `tsc`'s agrees with.
    Extra,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Type, Kind::Missed, Kind::Extra];

    fn name(self) -> &'static str {
        match self {
            Kind::Type => "type",
            Kind::Missed => "missed",
            Kind::Extra => "extra",
        }
    }

    fn parse(name: &str) -> Option<Kind> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A line of a case and what diverges on it.
pub(super) type Divergence = (usize, Kind);

/// The divergences a case's `.triage` explains; none when it has no `.triage`.
pub(super) fn read_explained(path: &Path) -> Result<BTreeSet<Divergence>, String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(BTreeSet::new());
    };
    let mut explained = BTreeSet::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = parse_entry(line).map_err(|e| format!("line {} of the triage: {e}", i + 1))?;
        for divergence in entry {
            if !explained.insert(divergence) {
                return Err(format!(
                    "line {} {} is explained twice",
                    divergence.0, divergence.1
                ));
            }
        }
    }
    Ok(explained)
}

/// `line <n>[, <n>…] <kind>: <explanation>`.
fn parse_entry(entry: &str) -> Result<Vec<Divergence>, String> {
    let malformed = || format!("`{entry}` is not `line <n>[, <n>…] <kind>: <explanation>`");
    let (head, explanation) = entry.split_once(':').ok_or_else(malformed)?;
    let (lines, kind) = head
        .strip_prefix("line ")
        .and_then(|h| h.rsplit_once(' '))
        .ok_or_else(malformed)?;
    let kind = Kind::parse(kind).ok_or_else(|| {
        format!("`{kind}` is not a kind of divergence: `type`, `missed` or `extra`")
    })?;
    check_explanation(explanation.trim())?;
    lines
        .split(',')
        .map(|n| n.trim().parse().map(|n| (n, kind)).map_err(|_| malformed()))
        .collect()
}

/// An explanation is a bug, with its issue; a difference by design, with the spec
/// section that makes it; or an artifact of the port, pruning or harness, with a
/// note saying which.
fn check_explanation(explanation: &str) -> Result<(), String> {
    let (reason, rest) = explanation.split_once(' ').unwrap_or((explanation, ""));
    let valid = match reason {
        "bug" => rest.strip_prefix("SUB-").is_some_and(|r| {
            let issue = r.split(' ').next().unwrap_or_default();
            !issue.is_empty() && issue.chars().all(|c| c.is_ascii_digit())
        }),
        "by-design" => rest
            .strip_prefix("spec §")
            .is_some_and(|r| r.starts_with(|c: char| !c.is_whitespace())),
        "artifact" => !rest.trim().is_empty(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "`{explanation}` is not `bug SUB-<n> …`, `by-design spec §<section> …` \
             or `artifact <note>`"
        ))
    }
}

/// The divergences not yet explained, by case path relative to the suite root.
#[derive(Clone, Default, PartialEq)]
pub(super) struct Unexplained(BTreeSet<(String, usize, Kind)>);

impl Unexplained {
    /// One `<case> <line> <kind>` per line.
    pub(super) fn read(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).unwrap_or_default();
        let mut entries = BTreeSet::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let mut fields = line.split(' ');
            let (Some(case), Some(n), Some(kind), None) =
                (fields.next(), fields.next(), fields.next(), fields.next())
            else {
                return Err(format!("`{line}` is not `<case> <line> <kind>`"));
            };
            let n = n
                .parse()
                .map_err(|_| format!("`{line}`: bad line number"))?;
            let kind = Kind::parse(kind).ok_or_else(|| format!("`{line}`: bad kind"))?;
            entries.insert((case.to_string(), n, kind));
        }
        Ok(Self(entries))
    }

    pub(super) fn write(&self, path: &Path) {
        let text: String = self
            .0
            .iter()
            .map(|(case, n, kind)| format!("{case} {n} {kind}\n"))
            .collect();
        fs::write(path, text).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    pub(super) fn of_case(&self, case: &str) -> BTreeSet<Divergence> {
        self.0
            .iter()
            .filter(|(c, _, _)| c == case)
            .map(|&(_, n, kind)| (n, kind))
            .collect()
    }

    pub(super) fn cases(&self) -> BTreeSet<String> {
        self.0.iter().map(|(case, _, _)| case.clone()).collect()
    }

    /// Replaces what is listed for `case`.
    pub(super) fn set_case(&mut self, case: &str, divergences: &BTreeSet<Divergence>) {
        self.0.retain(|(c, _, _)| c != case);
        self.0.extend(
            divergences
                .iter()
                .map(|&(n, kind)| (case.to_string(), n, kind)),
        );
    }
}

/// What is wrong with a case's explanations: a divergence neither explained nor
/// listed as unexplained, and an explanation or listing of a line that no longer
/// diverges (or, for a listing, that is now explained).
pub(super) fn check_case(
    divergent: &BTreeSet<Divergence>,
    explained: &BTreeSet<Divergence>,
    listed: &BTreeSet<Divergence>,
) -> Vec<String> {
    let describe = |&(n, kind): &Divergence| format!("line {n} {kind}");
    let unexplained: BTreeSet<Divergence> = divergent.difference(explained).copied().collect();
    let new = unexplained.difference(listed).map(|d| {
        format!(
            "{} diverges with no explanation: add it to the case's `.triage`",
            describe(d)
        )
    });
    let stale_explanations = explained.difference(divergent).map(|d| {
        format!(
            "{} is explained in the `.triage` but no longer diverges: remove the entry",
            describe(d)
        )
    });
    let stale_listings = listed.difference(&unexplained).map(|d| {
        let now = if divergent.contains(d) {
            "is explained now"
        } else {
            "no longer diverges"
        };
        format!(
            "{} is listed in `unexplained.txt` but {now}: \
             rerun with UPDATE_TYPESCRIPT_EXPECTED=1 to remove it",
            describe(d)
        )
    });
    new.chain(stale_explanations)
        .chain(stale_listings)
        .collect()
}

#[test]
fn an_entry_explains_each_listed_line() {
    assert_eq!(
        parse_entry("line 3, 7 extra: bug SUB-12 reads a construct signature as a method"),
        Ok(vec![(3, Kind::Extra), (7, Kind::Extra)]),
    );
    assert_eq!(
        parse_entry("line 5 type: by-design spec §3.2 first-element inference"),
        Ok(vec![(5, Kind::Type)]),
    );
    assert_eq!(
        parse_entry("line 9 missed: artifact pruning removed the narrowing assignment"),
        Ok(vec![(9, Kind::Missed)]),
    );
}

#[test]
fn an_explanation_needs_its_evidence() {
    for entry in [
        "line 3 type: bug",
        "line 3 type: bug see Linear",
        "line 3 type: bug SUB-12abc",
        "line 3 type: by-design spec § 1.2",
        "line 3 type: by-design",
        "line 3 type: by-design because",
        "line 3 type: artifact",
        "line 3 type: known",
        "line 3: bug SUB-1",
        "line 3 types: bug SUB-1",
        "line x type: bug SUB-1",
    ] {
        assert!(parse_entry(entry).is_err(), "{entry}");
    }
}

#[test]
fn every_divergence_is_explained_or_listed_and_neither_goes_stale() {
    let set = |items: &[Divergence]| items.iter().copied().collect::<BTreeSet<_>>();
    let divergent = set(&[(1, Kind::Type), (2, Kind::Missed), (3, Kind::Extra)]);
    assert!(
        check_case(
            &divergent,
            &set(&[(1, Kind::Type)]),
            &set(&[(2, Kind::Missed), (3, Kind::Extra)]),
        )
        .is_empty()
    );
    // New, a stale explanation, a listing now explained, a listing now fixed.
    let failures = check_case(
        &set(&[(1, Kind::Type), (2, Kind::Missed), (4, Kind::Type)]),
        &set(&[(1, Kind::Type), (1, Kind::Extra)]),
        &set(&[(1, Kind::Type), (5, Kind::Type)]),
    );
    assert_eq!(failures.len(), 5, "{failures:#?}");
    for (failure, starts) in failures.iter().zip([
        "line 2 missed diverges",
        "line 4 type diverges",
        "line 1 extra is explained",
        "line 1 type is listed",
        "line 5 type is listed",
    ]) {
        assert!(failure.starts_with(starts), "{failure}");
    }
}
