//! Constant-string pool — pre-codegen sweep that collects every string literal
//! and makes each distinct value a passive data segment in the emitted module.

use std::collections::BTreeMap;

use crate::ExprId;
use crate::codegen::{internal_failure, wasm_u32};
use crate::compiler_error::CompilerFailure;
use crate::literal_units::literal_units;

#[derive(Default, Clone, Debug)]
pub struct StringPool {
    /// First-appearance order; index equals data-segment index.
    pub strings: Vec<String>,
    pub locations: BTreeMap<ExprId, usize>,
    /// For sites without an ExprId (e.g. switch case-label strings).
    pub text_to_idx: BTreeMap<String, usize>,
}

impl StringPool {
    pub(crate) fn intern_text(&mut self, text: &str) -> usize {
        if let Some(&existing) = self.text_to_idx.get(text) {
            return existing;
        }
        let idx = self.strings.len();
        self.strings.push(text.to_string());
        self.text_to_idx.insert(text.to_string(), idx);
        idx
    }

    pub(crate) fn record_expr(&mut self, expr_id: ExprId, text: &str) {
        let idx = self.intern_text(text);
        self.locations.insert(expr_id, idx);
    }

    pub fn lookup_text(&self, text: &str) -> Option<usize> {
        self.text_to_idx.get(text).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// UTF-16 code unit count for the literal at `idx` (= JS `.length`).
    pub fn code_units(&self, idx: usize) -> Result<u32, CompilerFailure> {
        wasm_u32(literal_units(self.string(idx)?).len())
    }

    pub fn utf16_le_bytes(&self, idx: usize) -> Result<Vec<u8>, CompilerFailure> {
        Ok(literal_units(self.string(idx)?)
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect())
    }

    fn string(&self, idx: usize) -> Result<&str, CompilerFailure> {
        self.strings
            .get(idx)
            .map(String::as_str)
            .ok_or_else(|| internal_failure("a string literal pool index is out of range"))
    }
}

#[cfg(test)]
mod tests {
    use super::StringPool;
    use crate::codegen::analysis::CodegenAnalysis;
    use crate::{Asi, Token, TokenKind, capture, check, desugar, infer, parse};

    fn pool(source: &str) -> StringPool {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let tok = asi.next_token();
            let is_eof = matches!(tok.kind, TokenKind::Eof);
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        let (ast, _) = parse(source, tokens, crate::FileId(0));
        let (prelude_defs, host_defs, _) =
            crate::runtime::prelude::cached_runtime_package_declarations();
        let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
        packages.extend(prelude_defs.iter());
        packages.extend(host_defs.iter());
        let (mut ta, mut diags) = infer(source, "main", &ast, &packages);
        diags.extend(check(&ta).unwrap());
        ta = capture(ta).unwrap();
        ta = desugar(ta, crate::FileId(0)).unwrap();
        assert!(diags.is_empty(), "unexpected typecheck diags: {diags:?}");
        CodegenAnalysis::collect(&ta, &[]).unwrap().string_pool
    }

    #[test]
    fn empty_program_pool_is_empty() {
        let p = pool("function main(): void { }");
        assert!(p.is_empty());
        assert!(p.strings.is_empty());
        assert!(p.locations.is_empty());
    }

    #[test]
    fn single_string_in_let_initializer() {
        let p = pool(r#"let x: string = "hello"; function main(): void { }"#);
        assert_eq!(p.strings, vec!["hello".to_string()]);
        assert_eq!(p.locations.len(), 1);
        assert_eq!(p.code_units(0).unwrap(), 5);
        assert_eq!(p.utf16_le_bytes(0).unwrap(), b"h\0e\0l\0l\0o\0".to_vec());
        assert!(p.code_units(1).is_err());
        assert!(p.utf16_le_bytes(1).is_err());
    }

    #[test]
    fn dedup_same_string_two_locations() {
        let p =
            pool(r#"let x: string = "hello"; let y: string = "hello"; function main(): void { }"#);
        assert_eq!(p.strings, vec!["hello".to_string()]);
        assert_eq!(p.locations.len(), 2);
        for idx in p.locations.values() {
            assert_eq!(*idx, 0);
        }
    }

    #[test]
    fn multiple_distinct_strings_keep_first_appearance_order() {
        let p =
            pool(r#"let x: string = "hello"; let y: string = "world"; function main(): void { }"#);
        assert_eq!(p.strings, vec!["hello".to_string(), "world".to_string()]);
        assert_eq!(p.locations.len(), 2);
    }

    #[test]
    fn string_in_function_body_is_collected() {
        let p = pool(r#"function main(): void { let s: string = "hi"; }"#);
        assert_eq!(p.strings, vec!["hi".to_string()]);
        assert_eq!(p.locations.len(), 1);
    }

    #[test]
    fn string_in_concat_is_collected() {
        let p = pool(r#"let z: string = "a" + "b"; function main(): void { }"#);
        assert_eq!(p.strings, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(p.locations.len(), 2);
    }

    #[test]
    fn string_inside_if_condition_branches_collected() {
        let p = pool(
            r#"
            function main(): void {
                const a: string = "a";
                if (a === "b") {
                    let x: string = "then";
                } else {
                    let y: string = "else";
                }
            }
            "#,
        );
        let mut s = p.strings.clone();
        s.sort();
        assert_eq!(s, vec!["a", "b", "else", "then"]);
    }
}
