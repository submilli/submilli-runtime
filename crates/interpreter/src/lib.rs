//! Submilli interpreter library.
//!

// The front end's contract lives in `submilli-front-end`; its modules keep
// their engine paths.
pub use submilli_front_end::{
    arena, artifact_f64, compiler_error, compiler_limits, diagnostics, doc_comment, literal_units,
    local_type_names, mangle, number_format, package_declaration, rendering, shape, source, span,
    syntax, type_rendering, type_size, type_walk, typed_ast, types, utility_names,
};

pub mod asi;
pub mod ast;
pub mod authority;
pub mod backtrace;
pub mod capability_derivation;
pub mod codegen;
pub mod compile;
pub mod compile_capabilities;
pub mod compiler_thread;
pub mod did_you_mean;
pub mod doc_directives;
pub mod docs_view;
pub mod front_end;
pub mod functions;
pub mod import_suggestions;
pub mod imports;
pub mod lexer;
pub(crate) mod lower_local_types;
pub mod lower_patterns;
pub mod package_config;
#[cfg(test)]
mod package_declaration_tests;
pub mod packages;
pub mod parser;
pub mod property_keys;
pub mod runtime;
mod source_validation;
pub mod stdlib;
pub mod token;
pub mod tree_height;
pub mod type_info;
pub mod typechecker;

pub use asi::Asi;
pub use ast::{
    AccessorKind, ArrayLiteralElement, ArrowBody, Ast, BinOp, Binding, BindingKind, CatchClause,
    ChainPart, ClassMember, ClassModifiers, EnumInitializer, EnumMember, ExportedDecl, Expr,
    ExprId, ExprKind, Ident, ImportKind, ImportSpecifier, IndexSignatureAnnotation,
    InterfaceMember, LoweredLocalTypes, ObjectLiteralField, ObjectLiteralMember,
    ObjectPatternField, ParamDecl, PatternOrigin, PostfixOp, Stmt, StmtId, StmtKind, SwitchCase,
    SwitchDefault, TypeAnnotation, TypeAnnotationField, TypeAnnotationKind,
    TypeParameterAnnotation, TypePredicateAnnotation, UnOp, Visibility,
};
pub use authority::{
    AuthorityCallable, AuthorityCallableKind, AuthorityCheck, AuthorityEdge, AuthorityEffect,
    AuthorityExposure, AuthorityMap, AuthorityPosition, AuthorityRoute, AuthorityRouteEffect,
    AuthoritySpan, AuthorityWitnessStep,
};
pub use backtrace::{BacktraceMode, failure_message, render as render_backtrace};
pub use capability_derivation::{DerivedCapability, derive_call_site_capability};
pub use codegen::{SymbolTable, codegen};
pub use compile::{
    CompiledPackage, CompiledScript, PackageSourceModule, PhaseTimings, compile_package,
    compile_package_with_transitive, compile_script, compile_script_owned_by, typecheck,
    typecheck_package_to_typed_ast, typecheck_to_typed_ast,
};
pub use compiler_error::{CompileError, CompilerFailure, CompilerStage};
pub use diagnostics::{Diagnostic, Severity};
pub use doc_comment::{
    DocCapability, DocCapabilityBinding, DocCapabilityBindingKind, DocCapabilityDiagnostic,
    DocCapabilityLiteral, DocComment, DocParam, DocReturns, DocText, DocUnknownTag, RawDoc, doc,
    parse_doc_comment,
};
pub use front_end::{EngineFrontEnd, RuntimeDeclarations, runtime_declarations, script_request};
pub use functions::{
    CompiledFunction, CompiledFunctionSet, FUNCTIONS_MODULE_PREFIX, FUNCTIONS_PACKAGE,
    FUNCTIONS_STDLIB_MODULE, FunctionCache, FunctionCompileFailure, FunctionDefinition,
    FunctionError, FunctionNameError, FunctionSet, FunctionSummary, FunctionsView,
    MAX_FUNCTION_NAME_LEN, compile_function_set, compile_function_set_for, install_functions_async,
    invoke_function_async, validate_function_name,
};
pub use imports::{ScriptImports, classify_imports};
pub use lexer::Lexer;
pub use lower_patterns::lower as lower_patterns;
pub use mangle::MangledName;
pub use package_declaration::{
    AccessorSig, ClassExtends, DefaultValue, Dispatch, EnumVariantValue, FieldSig,
    GenericParameters, MethodOverload, MethodSig, NamespaceSymbol, PackageDeclaration, Param,
    PropertySig, RuntimeFunction, TypeKind, TypeSymbol, ValueKind, ValueSymbol, VirtualDispatch,
};
pub use parser::parse;
pub use runtime::{RunResult, RuntimeConfig, dispatch_main_async, instantiate_program_async};
pub use shape::Shape;
pub use source::{ModulePath, SourceFile, Sources};
pub use span::{FileId, LineIndex, Span};
pub use stdlib::functions::Escalated;
pub use stdlib::{OptionalPackage, Stdlib};
pub use submilli_front_end::front_end::{
    CheckedPackage, CheckedScript, FrontEnd, PackageModule, PackageRequest, PackageSurface,
    PackageTest, PackageTestSurface, ParsedScript, ScriptOwner, ScriptRequest,
};
pub use token::{Token, TokenKind};
pub use type_info::{FieldInfo, TypeInfo, TypeInfoId, TypeInfoIndex, TypeInfoKind, TypeInfoTable};
pub use typechecker::{capability_binding_type, capture, check, desugar, infer, infer_package};
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
    TypeofTagKind, UnmatchedCases,
};
pub use types::{
    IndexSignature, ObjectField, Package, PredicateKind, TupleType, Type, TypePredicate,
    split_index_signatures, stored_index_signatures,
};
