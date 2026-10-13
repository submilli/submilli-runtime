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
    let lines: Vec<&str> = source.split('\n').collect();
    let starts: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| filename_option(line).map(|name| (index, module_path(name))))
        .collect();
    if starts.len() < 2 {
        return None;
    }
    let owners = line_owners(lines.len(), &starts);
    let files = starts
        .into_iter()
        .enumerate()
        .map(|(file, (_, module))| CaseFile {
            module,
            text: blank_other_files(&lines, &owners, file),
        })
        .collect();
    Some(files)
}

/// For each line, the index of the file it belongs to: the last whose `@filename`
/// line is at or before it, or the first.
fn line_owners(line_count: usize, starts: &[(usize, String)]) -> Vec<usize> {
    let mut owners = Vec::with_capacity(line_count);
    let mut file = 0;
    for index in 0..line_count {
        while starts
            .get(file + 1)
            .is_some_and(|(start, _)| *start <= index)
        {
            file += 1;
        }
        owners.push(file);
    }
    owners
}

fn blank_other_files(lines: &[&str], owners: &[usize], file: usize) -> String {
    lines
        .iter()
        .zip(owners)
        .map(|(line, &owner)| {
            if owner == file {
                (*line).to_string()
            } else {
                " ".repeat(line.len())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The value of a `// @filename: <name>` line, matched as `tsc-case.cjs` matches
/// an option line.
fn filename_option(line: &str) -> Option<&str> {
    let rest = trim_js_space(trim_js_space(line).strip_prefix("//")?);
    let rest = rest.strip_prefix('@')?;
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if !rest[..name_end].eq_ignore_ascii_case("filename") {
        return None;
    }
    let value = trim_js_space(&rest[name_end..]).strip_prefix(':')?;
    Some(value.trim())
}

/// `line` without the leading characters JavaScript's `\s` matches.
pub(super) fn trim_js_space(line: &str) -> &str {
    line.trim_start_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

fn module_path(name: &str) -> String {
    let name = name.trim_start_matches("./").trim_start_matches('/');
    name.strip_suffix(".ts").unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filename_line_after_a_byte_order_mark_starts_a_file() {
        let files = split("\u{feff}// @filename: a.ts\nexport let x = 1;\n// @filename: b.ts\n");
        assert_eq!(files.map(|f| f.len()), Some(2));
    }

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
        assert!(
            files[1]
                .text
                .ends_with("// @Filename: /b.ts\nimport { x } from \"./a\";\n")
        );
        assert!(
            files[1]
                .text
                .starts_with(&format!("{}\n", blank("// @strict: true")))
        );
    }
}
