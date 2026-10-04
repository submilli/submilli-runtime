//! Split a YAML stream into verbatim blueprint documents and read their names.
//! The runtime server validates the complete schema.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug)]
pub struct ProbedDoc {
    pub file: PathBuf,
    /// 1-based document position within the file, for error messages.
    pub index: usize,
    pub name: String,
    /// The verbatim document text — posted unmodified.
    pub text: String,
}

/// Split a YAML stream on flush-left `---` marker lines into verbatim chunks.
/// Textual on purpose: parsing and re-serializing would destroy the "post the
/// document verbatim" guarantee. Flush-left markers cannot occur inside block
/// scalars (block content is always indented), so the split is safe. `...`
/// ends a document; whitespace/comment-only chunks are dropped.
pub fn split_documents(raw: &str) -> Vec<String> {
    let mut documents = Vec::new();
    let mut current = String::new();
    let mut flush = |chunk: &mut String| {
        if is_blank(chunk) {
            chunk.clear();
        } else {
            documents.push(std::mem::take(chunk));
        }
    };
    for line in raw.lines() {
        if is_document_marker(line, "---") || is_document_marker(line, "...") {
            flush(&mut current);
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    flush(&mut current);
    documents
}

fn is_document_marker(line: &str, marker: &str) -> bool {
    line == marker || (line.starts_with(marker) && line[marker.len()..].trim().is_empty())
}

fn is_blank(chunk: &str) -> bool {
    chunk
        .lines()
        .all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
}

/// Read `kind` (default `blueprint`) and `name` from one document. The only
/// client-side YAML parse in `apply`; complete schema validation stays
/// server-side.
pub fn probe(file: &Path, index: usize, text: String) -> Result<ProbedDoc> {
    let at = || format!("{} (document {index})", file.display());
    let value: serde_yml::Value = match serde_yml::from_str(&text) {
        Ok(v) => v,
        Err(e) => bail!("cannot parse {}: {e}", at()),
    };
    match value.get("kind") {
        None => {}
        Some(serde_yml::Value::String(s)) if s == "blueprint" => {}
        Some(other) => bail!(
            "unknown kind {} in {}: expected 'blueprint'",
            yaml_scalar(other),
            at()
        ),
    }
    let name = match value.get("name") {
        Some(serde_yml::Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => bail!(
            "missing 'name' in {}: every applied document needs one",
            at()
        ),
    };
    Ok(ProbedDoc {
        file: file.to_path_buf(),
        index,
        name,
        text,
    })
}

fn yaml_scalar(value: &serde_yml::Value) -> String {
    match value {
        serde_yml::Value::String(s) => format!("'{s}'"),
        other => serde_yml::to_string(other)
            .map_or_else(|_| "<non-scalar>".into(), |s| s.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_str(text: &str) -> Result<ProbedDoc> {
        probe(Path::new("test.yaml"), 1, text.to_string())
    }

    #[test]
    fn single_document_needs_no_marker() {
        assert_eq!(split_documents("name: a\n"), vec!["name: a\n"]);
    }

    #[test]
    fn splits_on_flush_left_markers() {
        let docs = split_documents("---\nname: a\n---\nname: b\n");
        assert_eq!(docs, vec!["name: a\n", "name: b\n"]);
    }

    #[test]
    fn dotdotdot_terminates_a_document() {
        let docs = split_documents("name: a\n...\n---\nname: b\n");
        assert_eq!(docs, vec!["name: a\n", "name: b\n"]);
    }

    #[test]
    fn indented_marker_inside_block_scalar_is_content() {
        let text = "name: a\nprompt: |\n  ---\n  not a marker\n";
        assert_eq!(split_documents(text), vec![text]);
    }

    #[test]
    fn comment_only_chunks_are_dropped() {
        let docs = split_documents("# leading comment\n---\nname: a\n---\n\n# trailer\n");
        assert_eq!(docs, vec!["name: a\n"]);
    }

    #[test]
    fn accepts_blueprints_with_or_without_kind() {
        for text in ["name: a\n", "kind: blueprint\nname: a\n"] {
            let doc = probe_str(text).unwrap();
            assert_eq!(doc.name, "a");
            assert_eq!(doc.text, text);
        }
    }

    #[test]
    fn unknown_kind_is_rejected_with_location() {
        let err = probe_str("kind: deployment\nname: a\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown kind 'deployment'"), "{msg}");
        assert!(msg.contains("test.yaml (document 1)"), "{msg}");
    }

    #[test]
    fn missing_name_is_rejected() {
        let err = probe_str("kind: blueprint\n").unwrap_err();
        assert!(err.to_string().contains("missing 'name'"));
    }
}
