use gimli::write::{
    Address, AttributeValue, EndianVec, FileId, LineProgram, LineString, Sections, Unit,
};
use gimli::{Encoding, Format, LineEncoding, LittleEndian, constants};

use crate::{LineIndex, Span};

pub struct FuncDebugInfo {
    pub name: String,
    pub low_pc: u64,
    pub body_len: u64,
    pub decl_line: u64,
    // Addresses are absolute (Code-section-content-relative); builder rebases to low_pc when emitting.
    pub lines: Vec<(u64, Span)>,
}

pub fn build_dwarf(
    funcs: &[FuncDebugInfo],
    code_size: u64,
    filename: &str,
    line_index: &LineIndex,
) -> Vec<(&'static str, Vec<u8>)> {
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
    let main_file: FileId = line_program.add_file(
        LineString::String(filename.as_bytes().to_vec()),
        line_program.default_directory(),
        None,
    );

    for f in funcs {
        line_program.row().address_offset = 0;
        line_program.set_address(Address::Constant(f.low_pc));
        for (abs_addr, span) in &f.lines {
            let (line, col) = line_index.line_col(span.start);
            line_program.row().address_offset = abs_addr - f.low_pc;
            line_program.row().file = main_file;
            line_program.row().line = line as u64;
            line_program.row().column = col as u64;
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
        // File index 1 — gimli emits added files starting at DWARF v4
        // index 1; we have exactly one source file.
        entry.set(constants::DW_AT_decl_file, AttributeValue::Udata(1));
        entry.set(
            constants::DW_AT_decl_line,
            AttributeValue::Udata(f.decl_line),
        );
    }

    let mut sections = Sections::new(EndianVec::new(LittleEndian));
    dwarf
        .write(&mut sections)
        .expect("DWARF buffers are infallible — only IO writers can fail");

    let mut out: Vec<(&'static str, Vec<u8>)> = Vec::new();
    sections
        .for_each(|id, data| -> std::result::Result<(), gimli::write::Error> {
            let bytes = data.slice();
            if !bytes.is_empty() {
                out.push((id.name(), bytes.to_vec()));
            }
            Ok(())
        })
        .expect("for_each closure returns Ok");
    out
}

#[cfg(test)]
mod tests {
    use super::{FuncDebugInfo, build_dwarf};
    use crate::LineIndex;

    #[test]
    fn empty_function_list_still_emits_compile_unit() {
        let li = LineIndex::new("");
        let sections = build_dwarf(&[], /*code_size=*/ 0, "script.subm", &li);
        let names: Vec<&str> = sections.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&".debug_info"), "got {names:?}");
        assert!(names.contains(&".debug_abbrev"), "got {names:?}");
        assert!(names.contains(&".debug_str"), "got {names:?}");
    }

    #[test]
    fn single_function_strings_present() {
        let li = LineIndex::new("function main(): void { }");
        let sections = build_dwarf(
            &[FuncDebugInfo {
                name: "main".to_string(),
                low_pc: 4,
                body_len: 2,
                decl_line: 1,
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
        let li = LineIndex::new(src);
        let sections = build_dwarf(
            &[FuncDebugInfo {
                name: "main".to_string(),
                low_pc: 4,
                body_len: 2,
                decl_line: 1,
                lines: vec![(4, crate::Span::new(crate::FileId(0), 0, 8))],
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
