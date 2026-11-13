//! The contract between a Submilli front end and the engine.
//!
//! A front end parses and checks source and hands the engine a [`TypedAst`],
//! and for a package a [`PackageDeclaration`]. This crate holds those types,
//! the types and diagnostics they carry, and the helpers both sides must
//! compute the same way. It is shared by the engine's own front end and by
//! ts-rust.
#![forbid(unsafe_code)]
// The safety lints in `Cargo.toml` hold for product code; tests may panic.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::string_slice,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )
)]

pub mod arena;
#[doc(hidden)]
pub mod artifact_f64;
pub mod compiler_error;
pub mod compiler_limits;
pub mod diagnostics;
pub mod doc_comment;
pub mod front_end;
pub mod literal_units;
pub mod local_type_names;
pub mod mangle;
pub mod narrowing;
pub mod number_format;
pub mod package_declaration;
pub mod rendering;
pub mod shape;
pub mod source;
pub mod span;
pub mod syntax;
#[cfg(test)]
mod test_support;
#[doc(hidden)]
pub mod type_rendering;
pub mod type_size;
#[doc(hidden)]
pub mod type_walk;
pub mod typed_ast;
pub mod types;
pub mod utility_names;

pub use compiler_error::{CompileError, CompilerFailure, CompilerStage};
pub use diagnostics::{Diagnostic, Severity};
pub use doc_comment::{
    DocCapability, DocCapabilityBinding, DocCapabilityBindingKind, DocCapabilityDiagnostic,
    DocCapabilityLiteral, DocComment, DocParam, DocReturns, DocText, DocUnknownTag, RawDoc, doc,
    parse_doc_comment,
};
pub use front_end::{
    CheckedPackage, CheckedScript, FrontEnd, PackageModule, PackageRequest, PackageSurface,
    PackageTest, PackageTestSurface, ParsedScript, ScriptOwner, ScriptRequest,
};
pub use mangle::MangledName;
pub use package_declaration::{
    AccessorSig, ClassExtends, DefaultValue, Dispatch, EnumVariantValue, FieldSig,
    GenericParameters, MethodOverload, MethodSig, NamespaceSymbol, PackageDeclaration, Param,
    PropertySig, RuntimeFunction, TypeKind, TypeSymbol, ValueKind, ValueSymbol, VirtualDispatch,
};
pub use shape::Shape;
pub use source::{ModulePath, SourceFile, Sources};
pub use span::{FileId, LineIndex, Span};
pub use syntax::{BinOp, BindingKind, ExprId, Ident, PostfixOp, StmtId, UnOp, Visibility};
pub use typed_ast::{
    CapturedVar, ClosureBody, EnumVariantPayload, ExportEntry, ExportKind, FieldNarrowingCheck,
    FieldNarrowingTest, FinalizerArgumentCheck, ForOfKind, FunctionArgumentCheck,
    FunctionInvokePlan, GenericArgument, GlobalKind, InterfaceCarrier, InterfaceNarrowingTest,
    InternalModuleExports, Intrinsic, MaybeMissingLocal, PostfixTarget, TypedArrayElement,
    TypedAst, TypedCatchClause, TypedChainPart, TypedClassAccessor, TypedClassConstructor,
    TypedClassDecl, TypedClassField, TypedClassMethod, TypedExpr, TypedExprKind, TypedFunction,
    TypedGlobal, TypedInterfaceDecl, TypedInterfaceMember, TypedNumberEnumDecl,
    TypedNumberEnumMember, TypedObjectFieldOrigin, TypedObjectFieldSource, TypedObjectLiteralField,
    TypedObjectMember, TypedParam, TypedStmt, TypedStmtKind, TypedStringEnumDecl,
    TypedStringEnumMember, TypedSwitchCase, TypedSwitchValue, TypedTypeAliasDecl, TypedTypeDecl,
    TypeofTagKind,
};
pub use types::{
    IndexSignature, ObjectField, Package, PredicateKind, TupleType, Type, TypePredicate,
    split_index_signatures, stored_index_signatures,
};
