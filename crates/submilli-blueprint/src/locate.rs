//! Source positions for a blueprint's text. The blueprint types carry no
//! spans: `serde_yml` reports none, and the server stores a normalized form
//! (`permissions:` last, comments dropped) whose lines are not the author's.
//! So positions are read here, from the file bytes of a version, with a
//! position-reporting YAML event parser. A citation found this way follows
//! the text it was read from, whatever the stored form looks like.
//!
//! Where the text uses anchors or aliases under `permissions:`, or writes a
//! rule list in flow style (`[ ... ]`), a line would be a guess, so a rule is
//! cited by caller block and index instead. So is any rule in a text that
//! breaks a line at NEL, LS, PS, or a lone `\r`: the blueprint parser and this
//! one would disagree about where its rules are.

use saphyr_parser::{Event, Parser, ScalarStyle, Span};

use crate::PathSeg;

/// Nesting deeper than this is not located: the tree below is built without
/// recursion, but it is dropped recursively. Blueprints nest a handful of
/// levels; `serde_yml` refuses far shallower documents than this.
const MAX_DEPTH: usize = 256;

/// Where a construct sits in the source text. `line`, `column`, and
/// `end_line` are 1-based; `column` counts characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleLocation {
    pub line: usize,
    pub column: usize,
    /// The last line holding the construct's own text: trailing blank lines
    /// and comments after it are not part of it.
    pub end_line: usize,
}

/// How a decision cites the rule that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Citation {
    /// The rule's line range in the text it was located in.
    Line(RuleLocation),
    /// The text did not locate the rule: it does not parse, it breaks a
    /// line other than at `\n` or `\r\n`, anchors or aliases appear under
    /// `permissions:`, the rule list is flow style, or the caller block or
    /// index does not exist in the text. The caller
    /// block and zero-based index still name the rule.
    Index { caller: String, index: usize },
}

/// Cite rule `index` (zero-based, as `RuleRef.index` counts) of `caller`'s
/// list under the top-level `permissions:` in `text`. The line is that of
/// the rule's `-`; `end_line` is the rule's last line.
pub fn locate_rule(text: &str, caller: &str, index: usize) -> Citation {
    find_rule(text, caller, index).map_or_else(
        || Citation::Index {
            caller: caller.to_string(),
            index,
        },
        Citation::Line,
    )
}

/// Locate the key at the end of a path of mapping keys from the document
/// root (`["permissions", "billing"]`, or `["vfs"]`). The location is the
/// key's own; `end_line` is the last line of its value. `None` when the text
/// does not parse, a key on the path is missing or repeated, or an alias
/// stands where a mapping is expected.
pub fn locate_key(text: &str, path: &[&str]) -> Option<RuleLocation> {
    let (last, parents) = path.split_last()?;
    let source = Source::parse(text)?;
    let mut node = &source.root;
    for key in parents {
        node = unique_entry(node, key)?.1;
    }
    let (key, value) = unique_entry(node, last)?;
    Some(RuleLocation {
        line: key.line,
        column: key.col + 1,
        end_line: value.end_line.max(key.end_line),
    })
}

/// The 1-based `(line, column)` of the second `key:` in the mapping at
/// `map_path`, for a duplicate-key diagnostic. Path keys resolve to their
/// first occurrence, which is the one a deserializer has entered by the
/// time it meets a repeat.
pub(crate) fn repeated_key(text: &str, map_path: &[PathSeg], key: &str) -> Option<(usize, usize)> {
    let source = Source::parse(text)?;
    let mut node = &source.root;
    for segment in map_path {
        node = match segment {
            PathSeg::Key(name) => entries(node)?.find(|(k, _)| is_key(k, name))?.1,
            PathSeg::Index(index) => match node.kind {
                Kind::Seq => node.children.get(*index)?,
                _ => return None,
            },
        };
    }
    let (repeat, _) = entries(node)?.filter(|(k, _)| is_key(k, key)).nth(1)?;
    Some((repeat.line, repeat.col + 1))
}

fn find_rule(text: &str, caller: &str, index: usize) -> Option<RuleLocation> {
    let source = Source::parse(text)?;
    let (permissions_key, permissions) = unique_entry(&source.root, "permissions")?;
    // Conservative: an anchor or alias anywhere under `permissions:` can
    // splice a caller block or rules in from elsewhere (`<<: *base`), so the
    // text's order is no longer the order the rules are walked in.
    if permissions_key.aliased || permissions.aliased {
        return None;
    }
    let rules = unique_entry(permissions, caller)?.1;
    if rules.kind != Kind::Seq || rules.flow {
        return None;
    }
    let rule = rules.children.get(index)?;
    let (line, column) = source.dash_before(rule)?;
    Some(RuleLocation {
        line,
        column,
        end_line: rule.end_line.max(line),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Scalar(String),
    Seq,
    Map,
    Alias,
}

/// A parsed YAML node. A mapping's `children` alternate key, value.
#[derive(Debug)]
struct Node {
    kind: Kind,
    /// 1-based line and 0-based character column of the node's first token.
    line: usize,
    col: usize,
    /// 1-based last line of the node's own text.
    end_line: usize,
    /// Written as `[ ... ]` / `{ ... }`.
    flow: bool,
    /// This node or anything inside it carries an anchor or is an alias.
    aliased: bool,
    children: Vec<Node>,
}

struct Source<'t> {
    lines: Vec<&'t str>,
    root: Node,
}

impl<'t> Source<'t> {
    /// Parse the first and only document. `None` on a scan error, an empty
    /// or multi-document stream, nesting beyond [`MAX_DEPTH`], or a line
    /// break the two parsers read differently.
    fn parse(text: &'t str) -> Option<Self> {
        if has_ambiguous_line_break(text) {
            return None;
        }
        let lines: Vec<&str> = text.lines().collect();
        let mut parser = Parser::new_from_str(text);
        let mut open: Vec<Node> = Vec::new();
        let mut root = None;
        while let Some(event) = parser.next_event() {
            let (event, span) = event.ok()?;
            let node = match event {
                Event::StreamEnd => break,
                Event::Nothing
                | Event::StreamStart
                | Event::DocumentStart(_)
                | Event::DocumentEnd => continue,
                Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                    if open.len() >= MAX_DEPTH {
                        return None;
                    }
                    let kind = if matches!(event, Event::SequenceStart(..)) {
                        Kind::Seq
                    } else {
                        Kind::Map
                    };
                    let mut node = Node::at(kind, span, anchor != 0);
                    node.flow = matches!(char_at(&lines, &span), Some('[' | '{'));
                    open.push(node);
                    continue;
                }
                Event::SequenceEnd | Event::MappingEnd => {
                    let mut node = open.pop()?;
                    // A block collection's end mark sits on the next token,
                    // past any blank lines and comments; its children already
                    // carried its true last line up. A flow one ends at its
                    // closing bracket.
                    if node.flow {
                        node.end_line = node.end_line.max(span.end.line());
                    }
                    node
                }
                Event::Scalar(value, style, anchor, _) => {
                    let mut node = Node::at(Kind::Scalar(value.into_owned()), span, anchor != 0);
                    node.end_line = scalar_end_line(&lines, &span, style);
                    node
                }
                Event::Alias(_) => {
                    let mut node = Node::at(Kind::Alias, span, true);
                    node.end_line = span.end.line().max(node.line);
                    node
                }
            };
            match open.last_mut() {
                Some(parent) => {
                    parent.end_line = parent.end_line.max(node.end_line);
                    parent.aliased |= node.aliased;
                    parent.children.push(node);
                }
                None if root.is_none() => root = Some(node),
                None => return None,
            }
        }
        Some(Source { lines, root: root? })
    }

    /// The `-` that introduces sequence item `item`: on the item's own line
    /// before it, or alone (perhaps with a comment) on an earlier line.
    /// Returns its 1-based line and column.
    fn dash_before(&self, item: &Node) -> Option<(usize, usize)> {
        let mut line = item.line;
        let mut text: String = self
            .lines
            .get(line.checked_sub(1)?)?
            .chars()
            .take(item.col)
            .collect();
        loop {
            let content = strip_comment(&text).trim_end();
            if let Some(before) = content.strip_suffix('-') {
                return Some((line, before.chars().count() + 1));
            }
            if !content.trim_start().is_empty() || line <= 1 {
                return None;
            }
            line -= 1;
            text = (*self.lines.get(line.checked_sub(1)?)?).to_string();
        }
    }
}

/// Whether `text` holds a line break other than `\n` or `\r\n`. The blueprint
/// parser (`serde_yml`) also breaks lines at NEL, LS, and PS, which this
/// parser reads as content, so the two would see different rules; and a lone
/// `\r`, which this parser breaks at but [`str::lines`] does not, would
/// misalign the source lines read back by position.
fn has_ambiguous_line_break(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{85}' | '\u{2028}' | '\u{2029}' => return true,
            '\r' if chars.peek() != Some(&'\n') => return true,
            _ => {}
        }
    }
    false
}

impl Node {
    fn at(kind: Kind, span: Span, anchored: bool) -> Self {
        Node {
            kind,
            line: span.start.line(),
            col: span.start.col(),
            end_line: span.start.line(),
            flow: false,
            aliased: anchored,
            children: Vec::new(),
        }
    }
}

/// The `(key, value)` pairs of a mapping node.
fn entries(node: &Node) -> Option<impl Iterator<Item = (&Node, &Node)>> {
    if node.kind != Kind::Map {
        return None;
    }
    Some(
        node.children
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[key, value]| (key, value)),
    )
}

/// The one entry for `key`; `None` when it is absent or repeated.
fn unique_entry<'n>(node: &'n Node, key: &str) -> Option<(&'n Node, &'n Node)> {
    let mut matching = entries(node)?.filter(|(k, _)| is_key(k, key));
    let found = matching.next()?;
    matching.next().is_none().then_some(found)
}

fn is_key(node: &Node, key: &str) -> bool {
    matches!(&node.kind, Kind::Scalar(value) if value == key)
}

fn char_at(lines: &[&str], span: &Span) -> Option<char> {
    let line = lines.get(span.start.line().checked_sub(1)?)?;
    line.chars().nth(span.start.col())
}

/// A scalar's last line. A block scalar (`|`, `>`) and a multi-line plain
/// scalar end their span at the next token, past blank lines; step back to
/// the last line with text on it.
fn scalar_end_line(lines: &[&str], span: &Span, style: ScalarStyle) -> usize {
    let (start, end) = (span.start.line(), span.end.line());
    if end <= start || matches!(style, ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted) {
        return end.max(start);
    }
    let ends_on_text = lines.get(end.saturating_sub(1)).is_some_and(|line| {
        line.chars()
            .take(span.end.col())
            .any(|c| !c.is_whitespace())
    });
    if ends_on_text {
        return end;
    }
    (start..end)
        .rev()
        .find(|line| {
            lines
                .get(line.saturating_sub(1))
                .is_some_and(|text| !text.trim().is_empty())
        })
        .unwrap_or(start)
}

/// `text` without a trailing `# comment`. Only used on the text before a
/// sequence item, where a `#` that begins a token can only open a comment.
fn strip_comment(text: &str) -> &str {
    let mut previous = None;
    for (at, c) in text.char_indices() {
        if c == '#' && previous.is_none_or(char::is_whitespace) {
            return text.get(..at).unwrap_or(text);
        }
        previous = Some(c);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUEPRINT: &str = "\
name: billing
# Who may charge.
permissions:
  main:
    - capability: stripe.com/charge   # small charges only
      filter: amount < 100
      action: allow

    # Larger ones need a person.
    - name: large-charges
      capability: stripe.com/charge
      action: ask-human
  stripe.com/sdk:
  - capability: http.fetch
    action: allow
default: deny
";

    fn line(citation: Citation) -> RuleLocation {
        match citation {
            Citation::Line(location) => location,
            Citation::Index { caller, index } => {
                panic!("expected a line, got the fallback for {caller}[{index}]")
            }
        }
    }

    #[test]
    fn locates_rules_between_comments_and_blank_lines() {
        assert_eq!(
            line(locate_rule(BLUEPRINT, "main", 0)),
            RuleLocation {
                line: 5,
                column: 5,
                end_line: 7
            }
        );
        assert_eq!(
            line(locate_rule(BLUEPRINT, "main", 1)),
            RuleLocation {
                line: 10,
                column: 5,
                end_line: 12
            }
        );
        // A list written flush with its key.
        assert_eq!(
            line(locate_rule(BLUEPRINT, "stripe.com/sdk", 0)),
            RuleLocation {
                line: 14,
                column: 3,
                end_line: 15
            }
        );
    }

    #[test]
    fn a_comment_inserted_above_a_rule_moves_its_line() {
        let edited = BLUEPRINT.replace(
            "    - name: large-charges",
            "    # Reviewed quarterly.\n    - name: large-charges",
        );
        assert_eq!(line(locate_rule(&edited, "main", 1)).line, 11);
        assert_eq!(line(locate_rule(&edited, "main", 0)).line, 5);
    }

    #[test]
    fn permissions_may_come_before_or_after_other_keys() {
        let first = "permissions:\n  main:\n    - capability: a\n      action: allow\nname: x\n";
        let last = "name: x\ndefault: deny\nvfs: ephemeral\npermissions:\n  main:\n    - capability: a\n      action: allow\n";
        assert_eq!(line(locate_rule(first, "main", 0)).line, 3);
        assert_eq!(line(locate_rule(last, "main", 0)).line, 6);
    }

    #[test]
    fn a_dash_on_its_own_line_is_the_rules_line() {
        let text = "permissions:\n  main:\n    -  # spelled out\n      capability: a\n      action: allow\n";
        assert_eq!(
            line(locate_rule(text, "main", 0)),
            RuleLocation {
                line: 3,
                column: 5,
                end_line: 5
            }
        );
    }

    #[test]
    fn a_multi_line_filter_ends_the_rule_at_its_last_text() {
        let text = "permissions:\n  main:\n    - capability: a\n      filter: |\n        x == 1\n\n    - capability: b\n      action: deny\n";
        assert_eq!(line(locate_rule(text, "main", 0)).end_line, 5);
        assert_eq!(line(locate_rule(text, "main", 1)).line, 7);
    }

    #[test]
    fn anchors_and_aliases_fall_back_to_the_index() {
        let text = "permissions:\n  main: &shared\n    - capability: a\n      action: allow\n  worker: *shared\n";
        for caller in ["main", "worker"] {
            assert_eq!(
                locate_rule(text, caller, 0),
                Citation::Index {
                    caller: caller.to_string(),
                    index: 0
                }
            );
        }
        let merged = "permissions:\n  main:\n    - &rule\n      capability: a\n      action: allow\n    - *rule\n";
        assert!(matches!(
            locate_rule(merged, "main", 0),
            Citation::Index { .. }
        ));
    }

    #[test]
    fn a_flow_rule_list_falls_back_to_the_index() {
        let text = "permissions:\n  main: [ { capability: a, action: allow } ]\n";
        assert_eq!(
            locate_rule(text, "main", 0),
            Citation::Index {
                caller: "main".to_string(),
                index: 0
            }
        );
        let whole = "permissions: { main: [ { capability: a, action: allow } ] }\n";
        assert!(matches!(
            locate_rule(whole, "main", 0),
            Citation::Index { .. }
        ));
    }

    #[test]
    fn a_flow_rule_in_a_block_list_is_located() {
        let text = "permissions:\n  main:\n    - { capability: a, action: allow }\n";
        assert_eq!(
            line(locate_rule(text, "main", 0)),
            RuleLocation {
                line: 3,
                column: 5,
                end_line: 3
            }
        );
    }

    #[test]
    fn missing_rules_and_unparsable_text_fall_back_to_the_index() {
        for (text, caller, index) in [
            (BLUEPRINT, "main", 2),
            (BLUEPRINT, "nobody", 0),
            ("name: x\n", "main", 0),
            ("permissions:\n  main: [\n", "main", 0),
            ("", "main", 0),
        ] {
            assert_eq!(
                locate_rule(text, caller, index),
                Citation::Index {
                    caller: caller.to_string(),
                    index
                },
                "{text:?}"
            );
        }
    }

    /// The blueprint parser also breaks lines at NEL, LS, and PS, and a lone
    /// carriage return; this parser does not, so its rule count can differ.
    #[test]
    fn line_breaks_the_blueprint_parser_reads_differently_fall_back_to_the_index() {
        for brk in ["\u{85}", "\u{2028}", "\u{2029}", "\r"] {
            let text = format!(
                "name: a\npermissions:\n  main:\n    # note{brk}    - capability: hidden\n    \
                 - capability: b\n      action: allow\n"
            );
            assert_eq!(
                locate_rule(&text, "main", 0),
                Citation::Index {
                    caller: "main".to_string(),
                    index: 0
                },
                "{brk:?}"
            );
            assert_eq!(locate_key(&text, &["permissions", "main"]), None, "{brk:?}");
        }
        // A CRLF file is still located.
        let crlf =
            "name: a\r\npermissions:\r\n  main:\r\n    - capability: b\r\n      action: allow\r\n";
        assert_eq!(line(locate_rule(crlf, "main", 0)).line, 4);
    }

    #[test]
    fn locates_keys() {
        assert_eq!(
            locate_key(BLUEPRINT, &["permissions", "stripe.com/sdk"]),
            Some(RuleLocation {
                line: 13,
                column: 3,
                end_line: 15
            })
        );
        assert_eq!(
            locate_key(BLUEPRINT, &["default"]),
            Some(RuleLocation {
                line: 16,
                column: 1,
                end_line: 16
            })
        );
        assert_eq!(locate_key(BLUEPRINT, &["permissions", "nobody"]), None);
        assert_eq!(locate_key(BLUEPRINT, &[]), None);
    }

    #[test]
    fn finds_the_repeat_of_a_key() {
        let text = "name: x\nvfs:\n  mode: a\n  # again\n  mode: b\nname: y\n";
        assert_eq!(repeated_key(text, &[], "name"), Some((6, 1)));
        assert_eq!(
            repeated_key(text, &[PathSeg::Key("vfs".into())], "mode"),
            Some((5, 3))
        );
        assert_eq!(repeated_key(text, &[], "vfs"), None);
    }

    #[test]
    fn excessive_nesting_is_not_located() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(Source::parse(&deep).is_none());
    }

    /// Every rule in every example blueprint in the repository locates to
    /// a line that starts with its `-`.
    #[test]
    fn locates_every_rule_in_the_repository_blueprints() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut files = Vec::new();
        collect_yaml(&root, &mut files);
        let mut located = 0;
        for path in files {
            let text = std::fs::read_to_string(&path).expect("readable yaml");
            if !text.lines().any(|line| line.starts_with("permissions:")) {
                continue;
            }
            // Other YAML (workflows, charts) can have a `permissions:` key too.
            let Ok(blueprint) = crate::parse(&text) else {
                continue;
            };
            for (caller, rules) in &blueprint.permissions {
                for index in 0..rules.len() {
                    let location = line(locate_rule(&text, caller, index));
                    let source_line = text.lines().nth(location.line - 1).expect("line exists");
                    assert!(
                        source_line.trim_start().starts_with('-'),
                        "{}: {caller}[{index}] cited line {}: {source_line:?}",
                        path.display(),
                        location.line
                    );
                    assert!(location.end_line >= location.line);
                    located += 1;
                }
            }
        }
        assert!(located > 0, "no example blueprint rules were found");
    }

    fn collect_yaml(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !(name.starts_with('.') || name.starts_with("target") || name == "node_modules")
                {
                    collect_yaml(&path, out);
                }
            } else if name.ends_with(".yaml") || name.ends_with(".yml") {
                out.push(path);
            }
        }
    }
}
