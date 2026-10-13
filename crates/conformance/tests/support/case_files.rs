//! The files of a multi-file TypeScript conformance case. Such a case is one `.ts`
//! file whose `// @filename: <name>` lines each start a file, as upstream writes
//! them. `typescript-baselines/case-files.cjs` splits a case the same way, so `tsc`
//! and we check the same files.
//!
//! Each file's text is the whole case with every line of the other files blanked to
//! spaces, so an offset, line or column in any file is the same in the case. Lines
//! before the first `@filename` belong to the first file.

/// A file of a multi-file case.
pub(super) struct CaseFile {
    /// The module path the file is imported by: its name with no leading `/` or
    /// `./`, and no `.ts` extension.
    pub module: String,
    pub text: String,
}

/// The case's files, or `None` when it is one file: it has fewer than two
/// `@filename` lines.
pub(super) fn split(source: &str) -> Option<Vec<CaseFile>> {
    let mut starts = Vec::new(); // (line index, module)
    for (index, line) in source.split('\n').enumerate() {
        if let Some(name) = filename_option(line) {
            starts.push((index, module_path(name)));
        }
    }
    if starts.len() < 2 {
        return None;
    }
    let lines: Vec<&str> = source.split('\n').collect();
    let owner = |line: usize| starts.iter().rposition(|(start, _)| *start <= line).unwrap_or(0);
    let files = starts
        .iter()
        .enumerate()
        .map(|(file, (_, module))| {
            let text = lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    if owner(index) == file {
                        (*line).to_string()
                    } else {
                        " ".repeat(line.len())
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            CaseFile {
                module: module.clone(),
                text,
            }
        })
        .collect();
    Some(files)
}

/// The value of a `// @filename: <name>` line, matched as `tsc-case.cjs` matches
/// an option line.
fn filename_option(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("//")?.trim_start();
    let rest = rest.strip_prefix('@')?;
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if !rest[..name_end].eq_ignore_ascii_case("filename") {
        return None;
    }
    let value = rest[name_end..].trim_start().strip_prefix(':')?;
    Some(value.trim())
}

fn module_path(name: &str) -> String {
    let name = name.trim_start_matches("./").trim_start_matches('/');
    name.strip_suffix(".ts").unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_with_one_file_is_not_split() {
        assert!(split("// @filename: a.ts\nlet x = 1;\n").is_none());
        assert!(split("let x = 1;\n").is_none());
    }

    #[test]
    fn each_file_keeps_its_own_lines_in_place() {
        let source = "// @strict: true\n// @filename: a.ts\nexport let x = 1;\n// @Filename: /b.ts\nimport { x } from \"./a\";\n";
        let files = split(source).expect("two files");
        assert_eq!(files[0].module, "a");
        assert_eq!(files[1].module, "b");
        let blank = |line: &str| " ".repeat(line.len());
        assert_eq!(
            files[0].text,
            format!(
                "// @strict: true\n// @filename: a.ts\nexport let x = 1;\n{}\n{}\n",
                blank("// @Filename: /b.ts"),
                blank("import { x } from \"./a\";"),
            )
        );
        assert_eq!(files[1].text.len(), source.len());
        assert!(files[1].text.ends_with("// @Filename: /b.ts\nimport { x } from \"./a\";\n"));
        assert!(files[1].text.starts_with("                \n"));
    }
}
