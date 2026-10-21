//! Our errors on a TypeScript conformance case, each marked by whether it could
//! agree with an error `tsc` reports. Shared by the runner and by the
//! `typescript_case_errors` example the pruner calls, so the two judge an error
//! the same way.
use interpreter::{
    Diagnostic, FileId, ModulePath, PackageSourceModule, Severity, TypedAst, parse_script,
    typecheck_package_to_typed_ast, typecheck_to_typed_ast,
};

use super::case_files;

/// The package a multi-file case's files are modules of.
const CASE_PACKAGE: &str = "@conformance/case";

/// Lexer and parser errors are about syntax we don't parse, except for these checks,
/// which `tsc` makes too and so can agree with it. ("invalid assignment target" is
/// not one: it also covers destructuring and cast targets, which `tsc` accepts.)
const SHARED_SYNTAX_CHECKS: &[&str] = &[
    "duplicate ",
    "must be the last",
    "must appear before",
    "must come before",
    "at most one visibility modifier",
    "cannot declare parameters",
    "cannot declare a return type",
    "must declare exactly one parameter",
    "a parameter property cannot",
    "only allowed in a constructor",
    "cannot have a default value",
    "requires parentheses",
    "cannot be the left operand of",
    "legacy octal literal",
    "cannot have a leading zero",
    "numeric separator",
    "is a complete number",
    "requires at least one of",
    "only applies to array and tuple types",
    "can't be the body of a statement without braces",
    "built-in type and can't be used as",
    "`<>` is not allowed",
    "can't name a binding in strict mode",
    "line break is not allowed before `=>`",
    "`\"use strict\"` directive",
];

/// Errors from any phase that name a feature we lack.
const MISSING_FEATURE_PHRASES: &[&str] = &[
    "not supported",
    "not yet supported",
    "requires a string literal on the left",
    "has an empty body",
    "must have at least one element",
    "mixed numeric and string enum members",
    "is not a valid narrowing guard",
    "only valid in the narrowing-guard form",
    "default value must be a literal",
    "lands in Layer",
    "cannot bind generic function",
    // A multi-file case is checked as a package, which `tsc` doesn't restrict so.
    "circular import",
    "packages cannot declare main",
];

/// Errors that name a feature we lack in some programs, and report the error `tsc`
/// reports in others: `unknown type` is a missing library type or a misspelling, and
/// `void` as a value is a gap where `tsc` takes it and a type error where it doesn't.
const MAYBE_MISSING_FEATURE_PHRASES: &[&str] = &[
    "unknown type",
    "requires a type annotation",
    "has no values",
    "got `void`",
    "for `void`",
    "to `void`:",
    "cannot bind a `void` value",
    "an equality operand must be a value",
    "duplicate declaration of interface",
    "on interface `",
    "is an enum type, not a value",
    "is a class, not a value",
    "declares no `new` method",
    // A package we lack (an ambient module), or one `tsc` can't find either.
    "package `",
];

/// TypeScript library types we don't have. `unknown type` naming one is a feature we
/// lack, where naming anything else may be a misspelling `tsc` reports too.
const LIBRARY_TYPES: &[&str] = &[
    "ArrayLike",
    "Awaited",
    "Exclude",
    "Extract",
    "Function",
    "InstanceType",
    "Iterable",
    "IterableIterator",
    "NoInfer",
    "NonNullable",
    "Object",
    "Omit",
    "Parameters",
    "Partial",
    "Pick",
    "PromiseLike",
    "Readonly",
    "Record",
    "RegExpExecArray",
    "Required",
    "ReturnType",
    "Symbol",
    "TemplateStringsArray",
    "Window",
    "symbol",
];

/// Whether one of our errors could agree with an error `tsc` reports on its line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Support {
    /// A type error, which agrees with `tsc` rejecting the line.
    Supported,
    /// About syntax or a feature we lack, so never agrees: the two reject the line
    /// for different reasons.
    Lacking,
    /// About a feature we may lack. Agrees with `tsc` rejecting the line, like a type
    /// error; where `tsc` accepts the line, it's a feature we lack.
    Unclear,
}

pub(super) struct CaseError {
    /// 1-based.
    pub line: usize,
    /// 1-based, in UTF-16 code units, as `tsc` counts them. The pruner needs it;
    /// the runner compares by line.
    #[allow(dead_code)]
    pub column: usize,
    /// The index of the case's file it is in, for the pruner: an error at the end
    /// of one file is on the case's last line, which another file owns.
    #[allow(dead_code)]
    pub file: usize,
    pub message: String,
    pub support: Support,
}

/// Our typed AST of a case, and our errors on it. A multi-file case is checked as
/// one package whose modules are its files, the last being the root.
pub(super) fn case_errors(source: &str) -> (TypedAst, Vec<CaseError>) {
    let (typed, diags, syntax_errors) = if let Some(files) = case_files::split(source) {
        typecheck_files(&files)
    } else {
        let parsed = parse_script(source, FileId(0));
        let (typed, diags) = typecheck_to_typed_ast(source, FileId(0));
        (typed, diags, parsed.diagnostics().to_vec())
    };
    let errors = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| {
            let (line, column) = line_and_column(source, d.span.start as usize);
            CaseError {
                line,
                column,
                file: d.span.file.0 as usize,
                message: d.message.clone(),
                support: support(d, &syntax_errors),
            }
        })
        .collect();
    (typed, errors)
}

/// The typed AST of a multi-file case, our diagnostics on it, and those of them
/// the parser reports. Each file's text keeps its offsets in the case, so a
/// diagnostic's span reads against the case whatever its file.
fn typecheck_files(files: &[case_files::CaseFile]) -> (TypedAst, Vec<Diagnostic>, Vec<Diagnostic>) {
    let syntax_errors = files
        .iter()
        // A file's id is its index, as `typecheck_package_to_typed_ast` gives it.
        .zip(0u32..)
        .flat_map(|(file, id)| parse_script(&file.text, FileId(id)).diagnostics().to_vec())
        .collect();
    let modules: Vec<PackageSourceModule<'_>> = files
        .iter()
        .map(|file| PackageSourceModule {
            path: ModulePath::from(file.module.as_str()),
            source: &file.text,
        })
        .collect();
    let root = modules
        .last()
        .map(|m| m.path.clone())
        .expect("a split case has files");
    let (typed, diags) = typecheck_package_to_typed_ast(CASE_PACKAGE, root, &modules);
    (typed, diags, syntax_errors)
}

fn support(diag: &Diagnostic, syntax_errors: &[Diagnostic]) -> Support {
    let mentions = |phrases: &[&str]| phrases.iter().any(|p| diag.message.contains(p));
    let from_parser = syntax_errors
        .iter()
        .any(|s| s.span == diag.span && s.message == diag.message);
    if (from_parser && !mentions(SHARED_SYNTAX_CHECKS))
        || mentions(MISSING_FEATURE_PHRASES)
        || names_library_type(&diag.message)
    {
        Support::Lacking
    } else if mentions(MAYBE_MISSING_FEATURE_PHRASES) || is_missing_module_export(&diag.message) {
        Support::Unclear
    } else {
        Support::Supported
    }
}

/// A module export a case's file lacks: one we don't make (a re-export of an
/// imported binding), or one `tsc` reports missing too.
fn is_missing_module_export(message: &str) -> bool {
    message.starts_with("module `") && message.contains("` does not export `")
}

fn names_library_type(message: &str) -> bool {
    let Some(rest) = message.strip_prefix("unknown type `") else {
        return false;
    };
    let name = rest.split(['`', '<', '.']).next().unwrap_or_default();
    LIBRARY_TYPES.contains(&name)
}

/// The 1-based line and UTF-16 column of a byte offset. Lines are counted by `\n`,
/// as `line_of` in the runner counts them.
fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = before.matches('\n').count() + 1;
    (line, before[line_start..].encode_utf16().count() + 1)
}
