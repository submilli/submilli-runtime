//! Submilli interpreter library.
//!

pub mod arena;
pub(crate) mod artifact_f64;
pub mod asi;
pub mod ast;
pub mod backtrace;
pub mod capability_derivation;
pub mod codegen;
pub mod compile;
mod compile_capabilities;
pub mod compiler_error;
pub mod compiler_limits;
pub mod diagnostics;
pub mod did_you_mean;
pub mod doc_comment;
pub mod lexer;
pub mod lower_patterns;
pub mod mangle;
pub mod package_declaration;
pub mod packages;
pub mod parser;
pub mod runtime;
pub mod shape;
pub mod source;
mod source_validation;
pub mod span;
pub mod stdlib;
pub mod token;
pub mod type_info;
pub mod typechecker;
pub mod typed_ast;
pub mod types;

pub use asi::Asi;
pub use ast::{
    AccessorKind, ArrayLiteralElement, ArrowBody, Ast, BinOp, Binding, BindingKind, CatchClause,
    ChainPart, ClassMember, ClassModifiers, EnumInitializer, EnumMember, ExportedDecl, Expr,
    ExprId, ExprKind, Ident, ImportKind, ImportSpecifier, IndexSignatureAnnotation,
    InterfaceMember, ObjectLiteralField, ObjectLiteralMember, ObjectPatternField, ParamDecl,
    PatternOrigin, PostfixOp, Stmt, StmtId, StmtKind, SwitchCase, SwitchDefault, TypeAnnotation,
    TypeAnnotationField, TypeAnnotationKind, TypePredicateAnnotation, UnOp, Visibility,
};
pub use backtrace::{BacktraceMode, render as render_backtrace};
pub use capability_derivation::{DerivedCapability, derive_call_site_capability};
pub use codegen::{SymbolTable, codegen};
pub use compile::{
    CompiledPackage, CompiledScript, PackageSourceModule, ParsedScript, PhaseTimings,
    ScriptImports, compile_package, compile_package_with_transitive, compile_parsed_script_timed,
    compile_script, compile_script_owned_by, parse_script, typecheck, typecheck_to_typed_ast,
};
pub use diagnostics::{Diagnostic, Severity};
pub use doc_comment::{
    DocCapability, DocCapabilityBinding, DocCapabilityBindingKind, DocCapabilityDiagnostic,
    DocCapabilityLiteral, DocComment, DocParam, DocReturns, DocText, DocUnknownTag, RawDoc, doc,
    parse_doc_comment,
};
pub use lexer::Lexer;
pub use lower_patterns::lower as lower_patterns;
pub use mangle::MangledName;
pub use package_declaration::{
    AccessorSig, ClassExtends, DefaultValue, Dispatch, EnumVariantValue, FieldSig, MethodSig,
    NamespaceSymbol, PackageDeclaration, Param, PropertySig, RuntimeFunction, TypeKind, TypeSymbol,
    ValueKind, ValueSymbol,
};
pub use parser::parse;
pub use runtime::{RunResult, RuntimeConfig, dispatch_main_async};
pub use shape::Shape;
pub use source::{ModulePath, SourceFile, Sources};
pub use span::{FileId, LineIndex, Span};
pub use token::{Token, TokenKind};
pub use type_info::{FieldInfo, TypeInfo, TypeInfoId, TypeInfoKind, TypeInfoTable};
pub use typechecker::{capture, check, desugar, infer, infer_package};
pub use typed_ast::{
    CapturedVar, ClosureBody, EnumVariantPayload, ExportEntry, ExportKind, FieldNarrowingCheck,
    FieldNarrowingTest, ForOfKind, GenericArgument, GlobalKind, InterfaceCarrier,
    InterfaceNarrowingTest, Intrinsic, PostfixTarget, TypedArrayElement, TypedAst,
    TypedCatchClause, TypedChainPart, TypedClassAccessor, TypedClassConstructor, TypedClassDecl,
    TypedClassField, TypedClassMethod, TypedExpr, TypedExprKind, TypedFunction, TypedGlobal,
    TypedInterfaceDecl, TypedInterfaceMember, TypedNumberEnumDecl, TypedNumberEnumMember,
    TypedObjectFieldOrigin, TypedObjectFieldSource, TypedObjectLiteralField, TypedObjectMember,
    TypedParam, TypedStmt, TypedStmtKind, TypedStringEnumDecl, TypedStringEnumMember,
    TypedSwitchCase, TypedSwitchValue, TypedTypeAliasDecl, TypedTypeDecl, TypeofTagKind,
};
pub use types::{IndexSignature, ObjectField, Package, Type, TypePredicate};
