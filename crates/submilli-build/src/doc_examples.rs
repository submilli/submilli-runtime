//! Compile-checked examples in package docs: extract fenced `ts` blocks from a
//! package's `docs/readme.md` and typecheck them against the package surface,
//! so shipped examples can't rot. Extraction-only here; `submilli build test`
//! and the workspace doc-examples test drive the checks.

use interpreter::{PackageDeclaration, Sources, compile_script, diagnostics};

/// One fenced example: the code plus the 1-based line of its opening fence.
pub struct DocExample {
    pub fence_line: usize,
    pub source: String,
}

/// Extract the fenced code blocks whose info string is exactly `ts` or
/// `typescript`. Other fences (```text, ```yaml, ```ts ignore, …) are skipped;
/// an unterminated fence yields nothing.
pub fn extract_doc_examples(markdown: &str) -> Vec<DocExample> {
    let mut examples = Vec::new();
    let mut open: Option<(usize, bool, Vec<&str>)> = None;
    for (index, line) in markdown.lines().enumerate() {
        match open {
            Some((fence_line, checked, ref mut body)) => {
                if line.trim_end() == "```" {
                    if checked {
                        examples.push(DocExample {
                            fence_line: fence_line + 1,
                            source: body.join("\n"),
                        });
                    }
                    open = None;
                } else {
                    body.push(line);
                }
            }
            None => {
                if let Some(info) = line.strip_prefix("```") {
                    let checked = matches!(info.trim(), "ts" | "typescript");
                    open = Some((index, checked, Vec::new()));
                }
            }
        }
    }
    examples
}

/// Typecheck one example against `declarations`. The source is padded with
/// blank lines so rendered diagnostics point at the real line in the readme.
/// Returns the rendered diagnostics on failure.
pub fn compile_check_doc_example(
    example: &DocExample,
    display_path: &str,
    declarations: &[&PackageDeclaration],
) -> Result<(), String> {
    let padded = format!("{}{}", "\n".repeat(example.fence_line), example.source);
    let (sources, file) = Sources::single(display_path.to_string(), padded.clone())
        .map_err(|error| error.to_string())?;
    match compile_script(&padded, display_path, file, declarations, &[]) {
        Ok(_) => Ok(()),
        Err(diags) => {
            let mut rendered = String::new();
            for d in &diags {
                rendered.push_str(&diagnostics::render(d, &sources));
            }
            Err(rendered)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ts_and_typescript_fences() {
        let md = "intro\n```ts\nlet a = 1;\n```\ntext\n```typescript\nlet b = 2;\n```\n";
        let examples = extract_doc_examples(md);
        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].source, "let a = 1;");
        assert_eq!(examples[0].fence_line, 2);
        assert_eq!(examples[1].source, "let b = 2;");
        assert_eq!(examples[1].fence_line, 6);
    }

    #[test]
    fn skips_other_fences_and_modifiers() {
        let md = "```text\nnot code\n```\n```yaml\na: b\n```\n```ts ignore\nskip\n```\n";
        assert!(extract_doc_examples(md).is_empty());
    }

    #[test]
    fn unterminated_fence_yields_nothing() {
        let md = "```ts\nlet a = 1;\n";
        assert!(extract_doc_examples(md).is_empty());
    }

    #[test]
    fn multiline_body_is_preserved() {
        let md = "```ts\nfunction main(): void {\n    assert(true, \"ok\");\n}\n```\n";
        let examples = extract_doc_examples(md);
        assert_eq!(examples.len(), 1);
        assert_eq!(
            examples[0].source,
            "function main(): void {\n    assert(true, \"ok\");\n}"
        );
    }

    #[test]
    fn compile_check_reports_the_readme_line() {
        let md = "# Docs\n\nprose\n\n```ts\nfunction main(): void {\n    let x: number = \"nope\";\n}\n```\n";
        let examples = extract_doc_examples(md);
        assert_eq!(examples.len(), 1);
        let err = compile_check_doc_example(&examples[0], "packages/demo/docs/readme.md", &[])
            .unwrap_err();
        assert!(err.contains("packages/demo/docs/readme.md:7"), "{err}");
    }

    #[test]
    fn compile_check_accepts_a_valid_example() {
        let md = "```ts\nfunction main(): number {\n    return 1 + 1;\n}\n```\n";
        let examples = extract_doc_examples(md);
        compile_check_doc_example(&examples[0], "docs/readme.md", &[]).unwrap();
    }

    #[test]
    fn fence_line_padding_matches_source_layout() {
        let md = "a\nb\n```ts\nlet broken =\n```\n";
        let examples = extract_doc_examples(md);
        assert_eq!(examples[0].fence_line, 3);
        let err = compile_check_doc_example(&examples[0], "docs/readme.md", &[]).unwrap_err();
        assert!(err.contains("docs/readme.md:4"), "{err}");
    }
}
