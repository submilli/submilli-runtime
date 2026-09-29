use gimli::write::{
    Address, AttributeValue, EndianVec, FileId, LineProgram, LineString, Sections, Unit,
};
use gimli::{Encoding, Format, LineEncoding, LittleEndian, constants};

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::source::SourceError;
use crate::{LineIndex, Sources, Span};

pub struct DebugSources<'a> {
    pub file: crate::FileId,
    pub filename: &'a str,
    pub index: &'a LineIndex,
    pub sources: Option<&'a Sources>,
}

impl DebugSources<'_> {
    pub fn location(&self, span: Span) -> Result<(&str, u32, u32), CompilerFailure> {
        self.checked_location(span)
            .map_err(|error| error.into_compiler_failure(CompilerStage::Codegen))
    }

    fn checked_location(&self, span: Span) -> Result<(&str, u32, u32), SourceError> {
        Span::new(span.file, span.start, span.end)?;
        if let Some(path) = span.file.reserved_path() {
            return Ok((path, 0, 0));
        }
        if let Some(sources) = self.sources {
            let source = sources
                .get(span.file)
                .ok_or(SourceError::UnknownFile { file: span.file })?;
            source.span_text(span)?;
            let (line, col) = source.line_index().line_col(span.start)?;
            return Ok((source.path.as_str(), line, col));
        }
        span.text(self.index.source(), self.file)?;
        let (line, col) = self.index.line_col(span.start)?;
        Ok((self.filename, line, col))
    }
}

fn dwarf_failure(error: impl std::fmt::Display) -> CompilerFailure {
    CompilerFailure::Internal {
        stage: CompilerStage::Codegen,
        span: None,
        message: format!("invalid debug metadata: {error}"),
    }
}

pub struct FuncDebugInfo {
    pub name: String,
    pub low_pc: u64,
    pub body_len: u64,
    pub decl_span: Span,
    // Addresses are absolute (Code-section-content-relative); builder rebases to low_pc when emitting.
    pub lines: Vec<(u64, Span)>,
}

pub fn build_dwarf(
    funcs: &[FuncDebugInfo],
    code_size: u64,
    filename: &str,
    sources: &DebugSources<'_>,
) -> Result<Vec<(&'static str, Vec<u8>)>, CompilerFailure> {
    validate_path(filename)?;
    validate_addresses(funcs, code_size)?;
    let encoding = Encoding {
        // wasm32; must update with every Address if moving to wasm64.
        address_size: 4,
        format: Format::Dwarf32,
        version: 4,
    };

    let mut line_program = LineProgram::new(
        encoding,
        LineEncoding::default(),
        // Working directory left empty — the language has no file-system
        // story yet, and the embedder threads in only the filename.
        LineString::String(Vec::new()),
        None,
        LineString::String(filename.as_bytes().to_vec()),
        None,
    );
    let mut files = std::collections::BTreeMap::<String, FileId>::new();
    for function in funcs {
        for span in
            std::iter::once(function.decl_span).chain(function.lines.iter().map(|(_, span)| *span))
        {
            let (path, _, _) = sources.location(span)?;
            validate_path(path)?;
            files.entry(path.to_string()).or_insert_with(|| {
                line_program.add_file(
                    LineString::String(path.as_bytes().to_vec()),
                    line_program.default_directory(),
                    None,
                )
            });
        }
    }

    for f in funcs {
        line_program.row().address_offset = 0;
        line_program.set_address(Address::Constant(f.low_pc));
        for (abs_addr, span) in &f.lines {
            let (path, line, col) = sources.location(*span)?;
            line_program.row().address_offset = abs_addr
                .checked_sub(f.low_pc)
                .filter(|offset| *offset <= f.body_len)
                .ok_or_else(|| dwarf_failure("line address is outside its function"))?;
            line_program.row().file = *files
                .get(path)
                .ok_or_else(|| dwarf_failure("source file was not registered"))?;
            line_program.row().line = u64::from(line);
            line_program.row().column = u64::from(col);
            line_program.row().is_statement = true;
            line_program.generate_row();
        }
        line_program.end_sequence(f.body_len);
    }

    let mut dwarf = gimli::write::Dwarf::new();
    let unit_id = dwarf.units.add(Unit::new(encoding, line_program));
    let unit = dwarf.units.get_mut(unit_id);

    let cu_root = unit.root();
    let cu_name = dwarf.strings.add(filename);
    let cu_dir = dwarf.strings.add("");
    {
        let cu = unit.get_mut(cu_root);
        cu.set(constants::DW_AT_name, AttributeValue::StringRef(cu_name));
        cu.set(constants::DW_AT_comp_dir, AttributeValue::StringRef(cu_dir));
        // No DWARF code for TypeScript; C99 is the closest recognised substitute.
        cu.set(
            constants::DW_AT_language,
            AttributeValue::Language(constants::DW_LANG_C99),
        );
        cu.set(
            constants::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(0)),
        );
        cu.set(constants::DW_AT_high_pc, AttributeValue::Udata(code_size));
    }

    for f in funcs {
        let id = unit.add(cu_root, constants::DW_TAG_subprogram);
        let name_id = dwarf.strings.add(f.name.as_str());
        let entry = unit.get_mut(id);
        entry.set(constants::DW_AT_name, AttributeValue::StringRef(name_id));
        entry.set(
            constants::DW_AT_low_pc,
            AttributeValue::Address(Address::Constant(f.low_pc)),
        );
        // DWARF v4 allows `DW_AT_high_pc` to be a constant offset from
        // `DW_AT_low_pc` rather than an absolute address — keeps the
        // attribute width small (no relocation).
        entry.set(constants::DW_AT_high_pc, AttributeValue::Udata(f.body_len));
        let (path, line, _) = sources.location(f.decl_span)?;
        let file = *files
            .get(path)
            .ok_or_else(|| dwarf_failure("declaration file was not registered"))?;
        entry.set(
            constants::DW_AT_decl_file,
            AttributeValue::FileIndex(Some(file)),
        );
        entry.set(
            constants::DW_AT_decl_line,
            AttributeValue::Udata(u64::from(line)),
        );
    }

    let mut sections = Sections::new(EndianVec::new(LittleEndian));
    dwarf.write(&mut sections).map_err(dwarf_failure)?;

    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    sections
        .for_each(|id, data| -> std::result::Result<(), gimli::write::Error> {
            let bytes = data.slice();
            if !bytes.is_empty() {
                out.push((id.name(), bytes.to_vec()));
            }
            Ok(())
        })
        .map_err(dwarf_failure)?;
    Ok(out)
}

fn validate_path(path: &str) -> Result<(), CompilerFailure> {
    if path.is_empty() || path.contains('\0') {
        return Err(dwarf_failure(
            "source path is empty or contains a null byte",
        ));
    }
    Ok(())
}

fn validate_addresses(funcs: &[FuncDebugInfo], code_size: u64) -> Result<(), CompilerFailure> {
    if code_size > u64::from(u32::MAX) {
        return Err(dwarf_failure(
            "code section exceeds the wasm32 address range",
        ));
    }
    for function in funcs {
        if function.name.contains('\0') {
            return Err(dwarf_failure("function name contains a null byte"));
        }
        let end = function
            .low_pc
            .checked_add(function.body_len)
            .filter(|end| *end <= code_size)
            .ok_or_else(|| dwarf_failure("function is outside the code section"))?;
        let mut previous = function.low_pc;
        for (address, _) in &function.lines {
            if *address < previous || *address > end {
                return Err(dwarf_failure(
                    "line addresses must be ordered within their function",
                ));
            }
            previous = *address;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DebugSources, FuncDebugInfo};
    fn build_dwarf(
        funcs: &[FuncDebugInfo],
        size: u64,
        filename: &str,
        index: &crate::LineIndex,
    ) -> Vec<(&'static str, Vec<u8>)> {
        super::build_dwarf(
            funcs,
            size,
            filename,
            &DebugSources {
                file: crate::FileId(0),
                filename,
                index,
                sources: None,
            },
        )
        .unwrap()
    }
    use crate::LineIndex;

    #[test]
    fn rejects_invalid_paths_and_address_sequences_before_writing() {
        let index = LineIndex::new("x").unwrap();
        for filename in ["", "bad\0.ts"] {
            let sources = DebugSources {
                file: crate::FileId(0),
                filename,
                index: &index,
                sources: None,
            };
            assert!(super::build_dwarf(&[], 0, filename, &sources).is_err());
        }
        let sources = DebugSources {
            file: crate::FileId(0),
            filename: "test.ts",
            index: &index,
            sources: None,
        };
        for addresses in [vec![3], vec![9], vec![6, 5]] {
            let function = FuncDebugInfo {
                name: "main".into(),
                low_pc: 4,
                body_len: 4,
                decl_span: crate::Span::at(crate::FileId(0)),
                lines: addresses
                    .into_iter()
                    .map(|address| (address, crate::Span::at(crate::FileId(0))))
                    .collect(),
            };
            assert!(super::build_dwarf(&[function], 8, "test.ts", &sources).is_err());
        }
        assert!(super::build_dwarf(&[], u64::MAX, "test.ts", &sources).is_err());
        let function = FuncDebugInfo {
            name: "bad\0name".into(),
            low_pc: 0,
            body_len: 1,
            decl_span: crate::Span::at(crate::FileId(0)),
            lines: vec![],
        };
        assert!(super::build_dwarf(&[function], 1, "test.ts", &sources).is_err());
    }

    #[test]
    fn package_debug_rows_and_declarations_use_their_own_source() {
        let mut registry = crate::Sources::new();
        let root = registry.add("index.ts", "root").unwrap();
        let dependency = registry.add("util.ts", "// é\n\nfunction helper").unwrap();
        let source = registry.get(root).unwrap();
        let sources = DebugSources {
            file: root,
            filename: "index.ts",
            index: source.line_index(),
            sources: Some(&registry),
        };
        let span = crate::Span::new(dependency, 7, 15).unwrap();
        let sections = super::build_dwarf(
            &[FuncDebugInfo {
                name: "helper".into(),
                low_pc: 4,
                body_len: 2,
                decl_span: span,
                lines: vec![(4, span)],
            }],
            6,
            "index.ts",
            &sources,
        )
        .unwrap();
        let dwarf = gimli::Dwarf::load(|id| -> Result<_, gimli::Error> {
            let bytes = sections
                .iter()
                .find(|(name, _)| *name == id.name())
                .map_or(&[][..], |(_, bytes)| bytes.as_slice());
            Ok(gimli::EndianSlice::new(bytes, gimli::LittleEndian))
        })
        .unwrap();
        let unit = dwarf.unit(dwarf.units().next().unwrap().unwrap()).unwrap();
        let program = unit.line_program.clone().unwrap();
        let mut rows = program.rows();
        let (header, row) = rows.next_row().unwrap().unwrap();
        assert_eq!(row.line().unwrap().get(), 3);
        assert_eq!(
            dwarf
                .attr_string(&unit, row.file(header).unwrap().path_name())
                .unwrap()
                .slice(),
            b"util.ts"
        );
        let mut entries = unit.entries();
        entries.next_dfs().unwrap().unwrap();
        let entry = entries.next_dfs().unwrap().unwrap();
        assert_eq!(
            entry
                .attr_value(gimli::DW_AT_decl_line)
                .unwrap()
                .udata_value(),
            Some(3)
        );
        let gimli::AttributeValue::FileIndex(file) =
            entry.attr_value(gimli::DW_AT_decl_file).unwrap()
        else {
            panic!("file index");
        };
        let declaration = unit
            .line_program
            .as_ref()
            .unwrap()
            .header()
            .file(file)
            .unwrap();
        assert_eq!(
            dwarf
                .attr_string(&unit, declaration.path_name())
                .unwrap()
                .slice(),
            b"util.ts"
        );
    }

    #[test]
    fn empty_function_list_still_emits_compile_unit() {
        let li = LineIndex::new("").unwrap();
        let sections = build_dwarf(&[], /*code_size=*/ 0, "script.subm", &li);
        let names: Vec<&str> = sections.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&".debug_info"), "got {names:?}");
        assert!(names.contains(&".debug_abbrev"), "got {names:?}");
        assert!(names.contains(&".debug_str"), "got {names:?}");
    }

    #[test]
    fn single_function_strings_present() {
        let li = LineIndex::new("function main(): void { }").unwrap();
        let sections = build_dwarf(
            &[FuncDebugInfo {
                name: "main".to_string(),
                low_pc: 4,
                body_len: 2,
                decl_span: crate::Span::at(crate::FileId(0)),
                lines: Vec::new(),
            }],
            10,
            "script.subm",
            &li,
        );
        let strs = sections
            .iter()
            .find(|(n, _)| *n == ".debug_str")
            .expect(".debug_str");
        assert!(
            strs.1.windows(4).any(|w| w == b"main"),
            "expected `main` in .debug_str, got {:?}",
            strs.1,
        );
    }

    #[test]
    fn line_program_emitted_when_lines_present() {
        let src = "function main(): void { }";
        let li = LineIndex::new(src).unwrap();
        let sections = build_dwarf(
            &[FuncDebugInfo {
                name: "main".to_string(),
                low_pc: 4,
                body_len: 2,
                decl_span: crate::Span::at(crate::FileId(0)),
                lines: vec![(4, crate::Span::new(crate::FileId(0), 0, 8).unwrap())],
            }],
            6,
            "script.subm",
            &li,
        );
        assert!(
            sections.iter().any(|(n, _)| *n == ".debug_line"),
            ".debug_line should be emitted when lines are present",
        );
    }
}
