//! Top-level declarations of a module, indexed by symbol space.
//!
//! `PackageDeclaration` is to a Submilli module what a `.d.ts` file is to a TypeScript
//! module — a description of what the module exposes to the outside world.
//! Codegen consumes its own module's declaration as well as the declarations of
//! every imported module to translate symbolic names into Wasm import entries
//! and (post-MVP) to look up signatures for cross-module calls.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Shape, Span, Type, TypedAst};
use serde::{Deserialize, Serialize};

/// The host fn a virtual package's functions dispatch through, in place of
/// per-function Wasm imports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualDispatch {
    /// `@mcp/<server>`: `submilli:mcp.call(server, tool, argsJson)`.
    Mcp { server: String },
    /// `@tools`, the session's application tools: `submilli:tools.call(tool, argsJson)`.
    Tools,
}

impl VirtualDispatch {
    /// The source-level package name.
    pub fn package_name(&self) -> String {
        match self {
            VirtualDispatch::Mcp { server } => format!("@mcp/{server}"),
            VirtualDispatch::Tools => crate::mangle::TOOLS_PACKAGE.to_string(),
        }
    }

    /// The internal host module whose `call` every function dispatches through.
    pub fn host_module(&self) -> &'static str {
        match self {
            VirtualDispatch::Mcp { .. } => crate::mangle::MCP_MODULE_NAME,
            VirtualDispatch::Tools => crate::mangle::TOOLS_MODULE_NAME,
        }
    }
}

/// Maps are `BTreeMap` for deterministic iteration order; codegen derives stable function-index assignments from it.
///
/// Directly constructed declarations must pass [`Self::check_type_limits`] before
/// cloning, formatting, serializing, or calling type/shape accessors. Checked
/// compilation validates borrowed declarations before any such consumer. Mutation
/// invalidates that validation; namespace destruction itself needs no validation.
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PackageDeclaration {
    /// Physical user-code signatures, separate from the source API. Inferred
    /// refinements may not describe the values crossing a runtime boundary.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_functions: BTreeMap<crate::MangledName, RuntimeFunction>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_globals: BTreeMap<crate::MangledName, Type>,
    /// Also used as the Wasm import-module name when codegen imports symbols from this package.
    pub package_name: String,
    /// `Some` for a virtual package (`@mcp/<server>` or `@tools`). Its functions
    /// resolve to `TypedExprKind::VirtualCall` and dispatch through one host fn
    /// for the whole package, so codegen skips emitting per-function imports for
    /// these. Set by the catalog mapper: the structured signal that replaces
    /// matching on the package name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_dispatch: Option<VirtualDispatch>,
    pub values: BTreeMap<String, ValueSymbol>,
    /// Named declarations only (interfaces, enums, aliases). Anonymous structural shapes live on `shapes`.
    pub types: BTreeMap<String, TypeSymbol>,
    /// Compiler-only declarations used to reconstruct hidden runtime classes.
    /// These names never enter source imports or rendered package declarations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_types: BTreeMap<String, TypeSymbol>,
    /// Generic functions using the hidden runtime-descriptor argument.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub runtime_generics: BTreeSet<crate::MangledName>,
    /// Anonymous structural shapes (`Object`, `Array`, `Union`) for cross-module WasmGC subtype alignment.
    pub shapes: Vec<crate::Shape>,
    /// Prelude-declared namespaces (`Math`, `Temporal`). User `namespace {}` is a parse error; always empty for user-source modules.
    pub namespaces: BTreeMap<String, NamespaceSymbol>,
    /// Functions and static methods whose closure cache the package exports as
    /// a global (see [`crate::mangle::closure_cache`]), so that a consumer reading
    /// one as a value gets the same closure the package itself does.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub closure_caches: BTreeSet<crate::MangledName>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeFunction {
    pub params: Vec<Type>,
    pub ret: Type,
}

impl PackageDeclaration {
    /// Prefer [`Self::with_package`] when the package name is known.
    pub fn new() -> Self {
        Self::with_package(crate::mangle::USER_PACKAGE)
    }

    pub fn with_package(package_name: impl Into<String>) -> Self {
        Self {
            package_name: package_name.into(),
            ..Self::default()
        }
    }

    /// The source import surface for consumers outside this package. The engine
    /// invokes the reserved lifecycle entry point through the full declaration.
    pub fn without_finalizer(mut self) -> Self {
        self.values.remove("finalizer");
        self
    }

    /// First-occurrence wins on duplicate names — typecheck has already emitted the diagnostic.
    pub fn from_typed_ast(ast: &TypedAst) -> Self {
        let mut defs = PackageDeclaration::with_package(ast.package_name.clone());
        for f in &ast.functions {
            if !f.generics.is_empty() {
                defs.runtime_generics.insert(f.mangled_name.clone());
            }
            defs.values
                .entry(f.name.name.clone())
                .or_insert_with(|| ValueSymbol {
                    name: f.name.name.clone(),
                    mangled_name: f.mangled_name.clone(),
                    declaration_span: f.name.span,
                    kind: ValueKind::Function {
                        generics: f.generics.clone(),
                        params: f.params.iter().map(param_from_typed).collect(),
                        ret: f.return_type.clone(),
                        type_predicate: f.type_predicate.clone(),
                        doc: f.doc.clone(),
                    },
                });
        }
        for g in &ast.globals {
            defs.values
                .entry(g.name.name.clone())
                .or_insert_with(|| ValueSymbol {
                    name: g.name.name.clone(),
                    mangled_name: g.mangled_name.clone(),
                    declaration_span: g.name.span,
                    kind: match g.kind {
                        crate::GlobalKind::Let => ValueKind::Let {
                            ty: g.ty.clone(),
                            doc: g.doc.clone(),
                        },
                        crate::GlobalKind::Const => ValueKind::Const {
                            ty: g.ty.clone(),
                            doc: g.doc.clone(),
                        },
                    },
                });
        }
        // Only aliases here; interfaces and enums are consumed from ast.types directly.
        for ty_decl in &ast.types {
            if let crate::TypedTypeDecl::Alias(alias) = ty_decl {
                defs.types
                    .entry(alias.name.name.clone())
                    .or_insert_with(|| TypeSymbol {
                        name: alias.name.name.clone(),
                        mangled_name: crate::mangle::package_symbol(
                            &ast.package_name,
                            &alias.name.name,
                        ),
                        declaration_span: alias.name.span,
                        kind: TypeKind::Alias {
                            generics: alias.generics.clone(),
                            ty: alias.ty.clone(),
                            doc: alias.doc.clone(),
                        },
                    });
            }
        }
        defs.refresh_shapes();
        defs
    }

    /// The named type declared under `mangled`. `runtime_types` holds the
    /// declarations of every module under the name a type refers to them by;
    /// a type re-exported from the root module is also known by its public
    /// name, which only `types` records.
    pub fn type_symbol(&self, mangled: &crate::MangledName) -> Option<&TypeSymbol> {
        self.runtime_types
            .get(mangled.as_str())
            .or_else(|| symbol_named(self.types.values(), mangled))
            .or_else(|| {
                self.namespaces
                    .values()
                    .find_map(|namespace| namespace_type_symbol(namespace, mangled))
            })
    }

    /// The `Dispatch::Static` interface `ty` names in this package, if any.
    /// Direct calls on host constructor and namespace bindings (`Boolean`,
    /// `console`, `Temporal.Instant`) drop the binding receiver, whereas
    /// non-binding places of a satisfiable static interface (`const B:
    /// BooleanConstructor = fn`) hold live runtime values.
    pub fn static_interface_of(&self, ty: &Type) -> Option<&TypeSymbol> {
        let Type::InterfaceRef { mangled, .. } = ty.peel() else {
            return None;
        };
        self.type_symbol(mangled).filter(|symbol| {
            matches!(
                &symbol.kind,
                TypeKind::Interface {
                    dispatch: crate::Dispatch::Static,
                    ..
                }
            )
        })
    }

    /// Appends every value binding in this package whose declared type resolves
    /// to a `Dispatch::Static` interface in this package.
    pub fn collect_static_value_bindings(
        &self,
        out: &mut std::collections::BTreeSet<crate::MangledName>,
    ) {
        fn walk_values(
            values: &BTreeMap<String, ValueSymbol>,
            namespaces: &BTreeMap<String, NamespaceSymbol>,
            declaration: &PackageDeclaration,
            out: &mut std::collections::BTreeSet<crate::MangledName>,
        ) {
            for symbol in values.values() {
                let (ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. }) = &symbol.kind else {
                    continue;
                };
                if declaration.static_interface_of(ty).is_some() {
                    out.insert(symbol.mangled_name.clone());
                }
            }
            for namespace in namespaces.values() {
                walk_values(&namespace.values, &namespace.namespaces, declaration, out);
            }
        }
        walk_values(&self.values, &self.namespaces, self, out);
    }

    pub fn refresh_shapes(&mut self) {
        self.shapes = self.collect_shapes();
    }

    pub fn collect_shapes(&self) -> Vec<Shape> {
        let mut collector = PackageShapeCollector::default();
        for value in self.values.values() {
            collector.collect_value(value);
        }
        for ty in self.types.values() {
            collector.collect_type_symbol(ty);
        }
        for namespace in self.namespaces.values() {
            collector.collect_namespace(namespace);
        }
        collector.shapes
    }

    /// Rejects oversized namespace metadata and types beyond the limits in
    /// [`crate::type_size`]. Declarations read from artifact JSON are bounded
    /// by the parser's nesting limit, but an embedder can build one directly,
    /// and every recursive walk over its types trusts those limits.
    pub fn check_type_limits(&self) -> Result<(), crate::type_size::TypeTooLarge> {
        check_namespace_limits(&self.namespaces)?;
        let mut result = Ok(());
        self.for_each_type(&mut |ty| {
            if result.is_ok() {
                result = crate::type_size::check(ty);
            }
        });
        result
    }

    /// Calls `visit` on every type the declaration holds. Structures are
    /// destructured exhaustively so a new type-bearing field has to be added
    /// here, and nested namespaces are walked without recursion.
    fn for_each_type(&self, visit: &mut dyn FnMut(&Type)) {
        let Self {
            runtime_functions,
            runtime_globals,
            package_name: _,
            virtual_dispatch: _,
            values,
            types,
            runtime_types,
            runtime_generics: _,
            shapes,
            namespaces,
            closure_caches: _,
        } = self;
        for RuntimeFunction { params, ret } in runtime_functions.values() {
            params.iter().for_each(&mut *visit);
            visit(ret);
        }
        runtime_globals.values().for_each(&mut *visit);
        values.values().for_each(|value| value_types(value, visit));
        types
            .values()
            .chain(runtime_types.values())
            .for_each(|symbol| type_symbol_types(symbol, visit));
        for shape in shapes {
            match shape {
                Shape::Object {
                    fields,
                    index,
                    more_indexes,
                } => {
                    fields.values().for_each(|field| visit(&field.ty));
                    for index in crate::stored_index_signatures(index, more_indexes) {
                        visit(&index.value);
                    }
                }
                Shape::Array(element) => visit(element),
                Shape::Tuple(members) => {
                    members.iter().for_each(&mut *visit);
                }
                Shape::Union(members) => {
                    members.iter().for_each(&mut *visit);
                }
            }
        }
        let mut pending: Vec<&NamespaceSymbol> = namespaces.values().collect();
        while let Some(namespace) = pending.pop() {
            let NamespaceSymbol {
                name: _,
                mangled_prefix: _,
                declaration_span: _,
                values,
                types,
                namespaces,
                doc: _,
            } = namespace;
            values.values().for_each(|value| value_types(value, visit));
            types
                .values()
                .for_each(|symbol| type_symbol_types(symbol, visit));
            pending.extend(namespaces.values());
        }
    }
}

fn check_namespace_limits(
    namespaces: &BTreeMap<String, NamespaceSymbol>,
) -> Result<(), crate::type_size::TypeTooLarge> {
    use crate::compiler_limits::{
        MAX_NAMESPACE_DEPTH, MAX_NAMESPACE_NODES, MAX_NAMESPACE_PATH_BYTES,
    };
    use crate::type_size::TypeTooLarge;

    // Iterator frames bound the frontier by depth, not the width of public input.
    let mut pending = vec![(namespaces.iter(), 0usize)];
    let mut nodes: usize = 0;
    let mut path_bytes = 0usize;
    while let Some((siblings, prefix_bytes)) = pending.last_mut() {
        let Some((name, namespace)) = siblings.next() else {
            pending.pop();
            continue;
        };
        let path_len = prefix_bytes.saturating_add(name.len());
        nodes = nodes.saturating_add(1);
        path_bytes = path_bytes.saturating_add(path_len);
        for name in namespace.types.keys() {
            path_bytes =
                path_bytes.saturating_add(path_len.saturating_add(1).saturating_add(name.len()));
            if path_bytes > MAX_NAMESPACE_PATH_BYTES {
                return Err(TypeTooLarge::NamespaceMetadata);
            }
        }
        if nodes > MAX_NAMESPACE_NODES
            || pending.len() > MAX_NAMESPACE_DEPTH
            || path_bytes > MAX_NAMESPACE_PATH_BYTES
        {
            return Err(TypeTooLarge::NamespaceMetadata);
        }
        if !namespace.namespaces.is_empty() {
            pending.push((namespace.namespaces.iter(), path_len.saturating_add(1)));
        }
    }
    Ok(())
}

fn namespace_type_symbol<'a>(
    namespace: &'a NamespaceSymbol,
    mangled: &crate::MangledName,
) -> Option<&'a TypeSymbol> {
    symbol_named(namespace.types.values(), mangled).or_else(|| {
        namespace
            .namespaces
            .values()
            .find_map(|child| namespace_type_symbol(child, mangled))
    })
}

fn symbol_named<'a>(
    mut symbols: impl Iterator<Item = &'a TypeSymbol>,
    mangled: &crate::MangledName,
) -> Option<&'a TypeSymbol> {
    symbols.find(|symbol| symbol.mangled_name == *mangled)
}

fn value_types(value: &ValueSymbol, visit: &mut dyn FnMut(&Type)) {
    match &value.kind {
        ValueKind::Function {
            generics,
            params,
            ret,
            type_predicate,
            doc: _,
        } => {
            generics.constraints.values().for_each(&mut *visit);
            params.iter().for_each(|param| visit(&param.ty));
            visit(ret);
            if let Some(predicate) = type_predicate {
                visit(&predicate.asserted_type);
            }
        }
        ValueKind::Let { ty, doc: _ } | ValueKind::Const { ty, doc: _ } => visit(ty),
    }
}

fn type_symbol_types(symbol: &TypeSymbol, visit: &mut dyn FnMut(&Type)) {
    match &symbol.kind {
        TypeKind::Interface { generics, .. }
        | TypeKind::Class { generics, .. }
        | TypeKind::Alias { generics, .. } => generics.constraints.values().for_each(&mut *visit),
        _ => {}
    }
    match &symbol.kind {
        TypeKind::Interface {
            extends,
            generics: _,
            methods,
            properties,
            index,
            more_indexes,
            dispatch: _,
            doc: _,
        } => {
            extends.iter().for_each(&mut *visit);
            methods
                .values()
                .for_each(|method| method_types(method, visit));
            properties.values().for_each(|property| visit(&property.ty));
            for index in crate::stored_index_signatures(index, more_indexes) {
                visit(&index.value);
            }
        }
        TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } => {}
        TypeKind::Alias {
            generics: _,
            ty,
            doc: _,
        } => visit(ty),
        TypeKind::Class {
            generics: _,
            fields,
            narrowing_checks,
            methods,
            method_visibility: _,
            accessors,
            constructor,
            constructor_visibility: _,
            statics,
            static_visibility: _,
            static_fields,
            extends,
            implements: _,
            doc: _,
        } => {
            fields
                .values()
                .chain(static_fields.values())
                .for_each(|field| visit(&field.ty));
            for check in narrowing_checks.values() {
                narrowing_check_types(check, visit);
            }
            methods
                .values()
                .chain(statics.values())
                .for_each(|method| method_types(method, visit));
            for accessor in accessors {
                match accessor {
                    AccessorSig::Getter { name: _, ret_ty } => visit(ret_ty),
                    AccessorSig::Setter { name: _, param } => visit(&param.ty),
                }
            }
            constructor.iter().for_each(|param| visit(&param.ty));
            if let Some(extends) = extends {
                extends.args.iter().for_each(&mut *visit);
            }
        }
    }
}

fn narrowing_check_types(check: &crate::FieldNarrowingCheck, visit: &mut dyn FnMut(&Type)) {
    let crate::FieldNarrowingCheck {
        declaration: _,
        test,
        minimal_test_target,
        message: _,
    } = check;
    if let Some(target) = minimal_test_target {
        visit(target);
    }
    match test {
        crate::FieldNarrowingTest::Shape(ty) => visit(ty),
        crate::FieldNarrowingTest::Interface(interface) => {
            let crate::InterfaceNarrowingTest {
                index,
                more_indexes,
                members,
                methods: _,
                non_shape_carriers,
                shape_allowed: _,
                nullable: _,
                undefined: _,
            } = interface;
            for index in crate::stored_index_signatures(index, more_indexes) {
                visit(&index.value);
            }
            members.values().for_each(|member| visit(&member.ty));
            for carrier in non_shape_carriers {
                match carrier {
                    crate::InterfaceCarrier::Array(element)
                    | crate::InterfaceCarrier::Function(element)
                    | crate::InterfaceCarrier::Set(element) => visit(element),
                    crate::InterfaceCarrier::Map(key, value) => {
                        visit(key);
                        visit(value);
                    }

                    crate::InterfaceCarrier::Number
                    | crate::InterfaceCarrier::Boolean
                    | crate::InterfaceCarrier::String
                    | crate::InterfaceCarrier::BigInt
                    | crate::InterfaceCarrier::Uint8Array
                    | crate::InterfaceCarrier::ArrayAny
                    | crate::InterfaceCarrier::FunctionAny
                    | crate::InterfaceCarrier::MapAny
                    | crate::InterfaceCarrier::SetAny
                    | crate::InterfaceCarrier::RegExp
                    | crate::InterfaceCarrier::RegExpMatch
                    | crate::InterfaceCarrier::RegExpMatchArray
                    | crate::InterfaceCarrier::TemporalInstant
                    | crate::InterfaceCarrier::TemporalDuration
                    | crate::InterfaceCarrier::TemporalZonedDateTime
                    | crate::InterfaceCarrier::TemporalPlainDate
                    | crate::InterfaceCarrier::TemporalPlainTime
                    | crate::InterfaceCarrier::TemporalPlainDateTime
                    | crate::InterfaceCarrier::TemporalPlainYearMonth
                    | crate::InterfaceCarrier::TemporalPlainMonthDay
                    | crate::InterfaceCarrier::ObjectShape
                    | crate::InterfaceCarrier::Url
                    | crate::InterfaceCarrier::FsStat
                    | crate::InterfaceCarrier::FsPeek
                    | crate::InterfaceCarrier::FsDirEntry
                    | crate::InterfaceCarrier::FsInfo
                    | crate::InterfaceCarrier::FsMountInfo
                    | crate::InterfaceCarrier::FsFileWriter
                    | crate::InterfaceCarrier::HttpResponse
                    | crate::InterfaceCarrier::HttpDownloadResult
                    | crate::InterfaceCarrier::SessionEntry
                    | crate::InterfaceCarrier::SessionPage => {}
                }
            }
        }
        crate::FieldNarrowingTest::NonNull
        | crate::FieldNarrowingTest::Substituted
        | crate::FieldNarrowingTest::Representation => {}
    }
}

pub fn method_types(method: &MethodSig, visit: &mut dyn FnMut(&Type)) {
    for signature in &method.overloads {
        signature
            .generics
            .constraints
            .values()
            .for_each(&mut *visit);
        signature.params.iter().for_each(|param| visit(&param.ty));
        visit(&signature.ret);
        if let Some(predicate) = &signature.predicate {
            visit(&predicate.asserted_type);
        }
    }

    method.generics.constraints.values().for_each(&mut *visit);
    let MethodSig {
        overloads: _,
        optional: _,
        generics: _,
        params,
        ret,
        predicate,
        doc: _,
    } = method;
    params.iter().for_each(|param| visit(&param.ty));
    visit(ret);
    if let Some(predicate) = predicate {
        visit(&predicate.asserted_type);
    }
}

#[derive(Default)]
struct PackageShapeCollector {
    shapes: Vec<Shape>,
    seen: BTreeSet<Shape>,
}

impl PackageShapeCollector {
    fn collect_value(&mut self, value: &ValueSymbol) {
        match &value.kind {
            ValueKind::Function {
                params,
                ret,
                type_predicate,
                ..
            } => {
                for param in params {
                    self.collect_type(&param.ty);
                }
                self.collect_type(ret);
                if let Some(predicate) = type_predicate {
                    self.collect_type(&predicate.asserted_type);
                }
            }
            ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => self.collect_type(ty),
        }
    }

    fn collect_type_symbol(&mut self, ty: &TypeSymbol) {
        match &ty.kind {
            TypeKind::Interface {
                extends,
                methods,
                properties,
                index,
                more_indexes,
                ..
            } => {
                for base in extends {
                    self.collect_type(base);
                }
                for index in crate::stored_index_signatures(index, more_indexes) {
                    self.collect_type(&index.value);
                }
                for method in methods.values() {
                    for method in method.signatures() {
                        for param in &method.params {
                            self.collect_type(&param.ty);
                        }
                        self.collect_type(&method.ret);
                        if let Some(predicate) = &method.predicate {
                            self.collect_type(&predicate.asserted_type);
                        }
                    }
                }
                for property in properties.values() {
                    self.collect_type(&property.ty);
                }
            }
            TypeKind::Class {
                fields,
                methods,
                accessors,
                constructor,
                ..
            } => {
                for field in fields.values() {
                    self.collect_type(&field.ty);
                }
                for method in methods.values() {
                    for method in method.signatures() {
                        for param in &method.params {
                            self.collect_type(&param.ty);
                        }
                        self.collect_type(&method.ret);
                    }
                }
                for acc in accessors {
                    match acc {
                        AccessorSig::Getter { ret_ty, .. } => self.collect_type(ret_ty),
                        AccessorSig::Setter { param, .. } => self.collect_type(&param.ty),
                    }
                }
                for param in constructor {
                    self.collect_type(&param.ty);
                }
            }
            TypeKind::Alias { ty, .. } => self.collect_type(ty),
            TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } => {}
        }
    }

    fn collect_namespace(&mut self, namespace: &NamespaceSymbol) {
        for value in namespace.values.values() {
            self.collect_value(value);
        }
        for ty in namespace.types.values() {
            self.collect_type_symbol(ty);
        }
        for child in namespace.namespaces.values() {
            self.collect_namespace(child);
        }
    }

    fn collect_type(&mut self, ty: &Type) {
        match ty {
            Type::TemplateLiteral { substitutions, .. } => {
                for inner in substitutions {
                    self.collect_type(inner);
                }
            }
            Type::Object { fields, .. } => {
                self.collect_shape(ty);
                for field in fields.values() {
                    self.collect_type(&field.ty);
                }
            }
            Type::Array(elem) => {
                self.collect_shape(ty);
                self.collect_type(elem);
            }
            Type::Tuple(elements) => {
                self.collect_shape(ty);
                for elem in elements {
                    self.collect_type(elem);
                }
            }
            Type::Union(members) => {
                self.collect_shape(ty);
                for member in members {
                    self.collect_type(member);
                }
            }
            Type::Function { params, ret, .. } => {
                for param in params {
                    self.collect_type(param);
                }
                self.collect_type(ret);
            }
            Type::InterfaceRef { args, .. }
            | Type::ClassRef { args, .. }
            | Type::AliasRef { args, .. } => {
                for arg in args {
                    self.collect_type(arg);
                }
            }
            Type::Alias { ty, .. }
            | Type::Refined { ty, .. }
            | Type::Readonly(ty)
            | Type::KeyOf(ty) => {
                self.collect_type(ty);
            }
            Type::IndexedAccess { object, index } => {
                self.collect_type(object);
                self.collect_type(index);
            }
            Type::Mapped { .. } | Type::Conditional { .. } | Type::Intersection(_) => {
                crate::type_size::for_each_child(ty, |child| self.collect_type(child));
            }
            Type::Number
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::NumberLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::Never
            | Type::Unknown
            | Type::NonPrimitive
            | Type::Error
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. } => {}
        }
    }

    fn collect_shape(&mut self, ty: &Type) {
        let Some(shape) = Shape::from_type(ty) else {
            return;
        };
        if self.seen.insert(shape.clone()) {
            self.shapes.push(shape);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValueSymbol {
    /// The symbol's own (unaliased) name within its package.
    pub name: String,
    /// The key codegen uses for cross-module symbol lookup.
    pub mangled_name: crate::MangledName,
    pub declaration_span: Span,
    pub kind: ValueKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub default: Option<DefaultValue>,
    /// Whether a caller may omit the argument. Signatures the typechecker
    /// builds from source set it for `?` and for defaulted parameters alike,
    /// but a host declaration may give only `default`, so readers ask
    /// [`Param::is_omittable`], which covers both.
    #[serde(default)]
    pub optional: bool,
    /// Callee sees `T[]`; call-site collects trailing args into a fresh array.
    pub rest: bool,
}

impl Param {
    /// Whether a caller may leave the argument out: declared `?`, or defaulted.
    pub fn is_omittable(&self) -> bool {
        self.optional || self.default.is_some()
    }
}

/// Optional fixed parameters at the end of a signature, excluding its rest slot.
pub fn optional_parameter_count(params: &[Param]) -> usize {
    params
        .iter()
        .rev()
        .skip_while(|param| param.rest)
        .take_while(|param| param.is_omittable())
        .count()
}

impl Param {
    pub fn new(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
            default: None,
            optional: false,
            rest: false,
        }
    }

    pub fn with_default(name: impl Into<String>, ty: Type, default: DefaultValue) -> Self {
        Self {
            name: name.into(),
            ty,
            default: Some(default),
            optional: true,
            rest: false,
        }
    }

    /// The caller may omit this parameter, supplying `undefined`.
    pub fn optional(name: impl Into<String>, ty: Type) -> Self {
        Self {
            optional: true,
            ..Self::new(name, ty)
        }
    }

    /// `ty` is the array type `T[]`. Must be last; cannot have a default.
    pub fn rest(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
            default: None,
            optional: false,
            rest: true,
        }
    }

    /// Empty name signals "positional" to the diagnostic lifter.
    pub fn anon(ty: Type) -> Self {
        Self {
            name: String::new(),
            ty,
            default: None,
            optional: false,
            rest: false,
        }
    }
}

/// Lower a body-pass `TypedParam` to an exported `Param`, preserving `rest` and
/// the resolved default so cross-module callers can omit the argument.
pub fn param_from_typed(p: &crate::TypedParam) -> Param {
    Param {
        name: p.name.name.clone(),
        ty: p.ty.clone(),
        default: p.default.clone(),
        optional: p.optional,
        rest: p.rest,
    }
}

/// Restricted to shapes materializable without re-evaluating arbitrary source.
/// Re-evaluated per call (matching JS/TS) — a fresh `TypedExpr` is spliced at each call site.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DefaultValue {
    Undefined,
    Number(#[serde(with = "crate::artifact_f64")] f64),
    /// A number for an omitted argument that an explicit `undefined` does not
    /// share, as in JavaScript: `splice(1)` removes to the end, while
    /// `splice(1, undefined)` converts `undefined` to `0` and removes nothing.
    OmittedNumber {
        #[serde(with = "crate::artifact_f64")]
        omitted: f64,
        #[serde(with = "crate::artifact_f64")]
        undefined: f64,
    },
    /// A string for an omitted argument only: an explicit `undefined` is passed
    /// as it is, as JavaScript's `String()` is `""` while `String(undefined)`
    /// is `"undefined"`.
    OmittedString(String),
    String(String),
    Boolean(bool),
    Null,
    /// `[]` — the parameter's resolved type must be `Type::Array(_)`;
    /// signature-time validation checks this.
    EmptyArray,
    /// `{}` — the parameter's resolved type must be `Type::Object { .. }`. Synthesizes an
    /// empty object literal; absent optional fields serialize away. Used for MCP tools
    /// whose input schema marks nothing required, so a zero-arg call type-checks.
    EmptyObject,
    /// Codegen emits a `GlobalGet` at each fill-in site.
    GlobalConst(crate::MangledName),
    /// Fields mirror `TypedExprKind::{Number,String}EnumMember` so the call-site synthesizer can build that node verbatim.
    EnumVariant {
        enum_mangled: crate::MangledName,
        variant: String,
        value: EnumVariantValue,
    },
}

impl DefaultValue {
    /// Whether an explicit `undefined` argument is replaced too: by the default,
    /// as for a JavaScript default parameter, or by `OmittedNumber`'s
    /// `undefined` value.
    pub fn replaces_undefined(&self) -> bool {
        !matches!(self, Self::OmittedString(_))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EnumVariantValue {
    Number(#[serde(with = "crate::artifact_f64")] f64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ValueKind {
    Function {
        /// Names appear inside `params`/`ret` as `Type::Var(name)`, resolved per call site by `TypeParamSubstitution`.
        generics: GenericParameters,
        params: Vec<Param>,
        ret: Type,
        /// Type-guard (`x is T`); inferer copies this into `Type::Function` at each reference site.
        type_predicate: Option<crate::TypePredicate>,
        doc: Option<crate::DocComment>,
    },
    Let {
        /// Resolved type of the binding. `Type::Error` when the annotation
        /// was missing or unresolvable — the originating diagnostic is in
        /// the diagnostics list.
        ty: Type,
        doc: Option<crate::DocComment>,
    },
    Const {
        ty: Type,
        doc: Option<crate::DocComment>,
    },
}

/// Ordered type parameter names and their optional upper bounds. Keeping the
/// bounds beside the names preserves unused parameters in exported signatures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GenericParameters {
    pub names: Vec<String>,
    pub constraints: Box<BTreeMap<String, Type>>,
}

impl std::ops::Deref for GenericParameters {
    type Target = [String];

    fn deref(&self) -> &[String] {
        &self.names
    }
}

impl GenericParameters {
    /// Construct parameters known to have no bounds, such as built-in
    /// declarations. Parsed declarations must resolve and retain their bounds.
    pub fn unconstrained(names: Vec<String>) -> Self {
        Self {
            names,
            constraints: Box::default(),
        }
    }
    pub fn select_names(&self, names: &[String]) -> Self {
        Self {
            names: names.to_vec(),
            constraints: Box::new(
                names
                    .iter()
                    .filter_map(|name| {
                        self.constraints
                            .get(name)
                            .map(|bound| (name.clone(), bound.clone()))
                    })
                    .collect(),
            ),
        }
    }
}

impl<'a> IntoIterator for &'a GenericParameters {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.names.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TypeSymbol {
    pub name: String,
    /// Same role as `ValueSymbol::mangled_name`, for the type namespace.
    pub mangled_name: crate::MangledName,
    pub declaration_span: Span,
    pub kind: TypeKind,
}

// `Interface` carries an optional `DocComment` which makes
// the enum non-trivial in size; the enum is rarely instantiated in
// hot paths so the variance is acceptable.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    /// Single declaration site per name; resolved bases accompany flattened members.
    /// Codegen has no Wasm representation for the interface itself; it only drives method dispatch.
    Interface {
        /// Declared bases in source order, expressed in this interface's type parameters.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extends: Vec<Type>,
        generics: GenericParameters,
        methods: BTreeMap<String, MethodSig>,
        /// Read via `expr.name` (not `expr.name()`). Writable unless the
        /// declaration carries a `readonly` modifier (`PropertySig.readonly`).
        /// A name also present in `methods` has both the callable contract and
        /// these additional constraints on the callable value.
        properties: BTreeMap<String, PropertySig>,
        index: Option<crate::IndexSignature>,
        /// The signatures beside `index`, as on `Type::Object`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        more_indexes: Vec<crate::IndexSignature>,
        dispatch: Dispatch,
        doc: Option<crate::DocComment>,
    },
    /// Auto-numbered at bind time (source AST stores `None`; bound form holds the resolved value).
    NumberEnum {
        #[serde(with = "crate::artifact_f64::pairs")]
        variants: Vec<(String, f64)>,
        doc: Option<crate::DocComment>,
    },
    /// Every variant must have an explicit initializer; auto-numbering doesn't apply.
    StringEnum {
        variants: Vec<(String, String)>,
        doc: Option<crate::DocComment>,
    },
    /// Stored `ty` is the fully resolved alias body. At use sites the inferer wraps the result in
    /// `Type::Alias { name, args, ty }` so the alias label flows through `Display`.
    Alias {
        generics: GenericParameters,
        ty: Type,
        doc: Option<crate::DocComment>,
    },
    /// A `class` declaration. Nominal (`Type::ClassRef` keys on the symbol's mangled name).
    /// Single inheritance via `extends`; `implements` is structural conformance only.
    /// Privacy is enforced at the access site (module-scoped), not by stripping here.
    /// Methods are always vtable-dispatched (overrides slot into the inherited index),
    /// so there is no per-class `dispatch` — unlike `Interface`, where the prelude opts
    /// into direct/static dispatch.
    Class {
        /// Class-level type parameters, erased at codegen (every `T`-typed slot
        /// is a boxed value slot). Substituted at use sites from the
        /// `ClassRef`'s args.
        generics: GenericParameters,
        fields: BTreeMap<String, FieldSig>,
        /// Runtime read guards for fields whose declaration narrows an inherited
        /// slot. Carried across packages because the consumer reconstructs the
        /// class layout and emits its reads independently.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        narrowing_checks: BTreeMap<String, crate::FieldNarrowingCheck>,
        methods: BTreeMap<String, MethodSig>,
        /// Per-method visibility, keyed alongside `methods` (kept separate so
        /// `MethodSig` stays shared with interfaces, which have no visibility).
        method_visibility: BTreeMap<String, crate::Visibility>,
        /// Accessor functions (`get`/`set`) — one entry each, mirroring the typed
        /// AST. A property's read type (getter) and write type (setter) are
        /// independent; the property itself is also in `fields` (its read type) for
        /// the type system. Carries the setter's parameter name + type so the
        /// public surface (docs, `.d.ts`) renders the full signature.
        accessors: Vec<AccessorSig>,
        /// Constructor parameter signature (no return type).
        constructor: Vec<Param>,
        /// A `private` constructor may be called, and the class extended, only in
        /// the module that declares the class.
        #[serde(default, skip_serializing_if = "crate::Visibility::is_public")]
        constructor_visibility: crate::Visibility,
        /// Static methods — self-less functions dispatched by name on the class
        /// object (`Class#static#name`), never through the vtable. Inherited down
        /// the `extends` chain by name resolution at the use site.
        #[serde(default)]
        statics: BTreeMap<String, MethodSig>,
        /// Per-static-method visibility, mirroring `method_visibility`.
        #[serde(default)]
        static_visibility: BTreeMap<String, crate::Visibility>,
        /// Static fields, backed by module globals (`Class#static#name`). `readonly`
        /// is declaration-driven; a mutable one is writable through the class name.
        #[serde(default)]
        static_fields: BTreeMap<String, FieldSig>,
        /// Resolved parent class, if any.
        extends: Option<ClassExtends>,
        /// Resolved interfaces this class declares it implements.
        implements: Vec<crate::MangledName>,
        doc: Option<crate::DocComment>,
    },
    // `String` is absent — it's an intrinsic declared per-consumer via `declare_intrinsic_types`.
}

/// A class's resolved `extends` clause. Parent and type args are one fact:
/// every chain walker that hops to the parent must zip `args` against the
/// parent's generics, so they travel together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClassExtends {
    pub parent: crate::MangledName,
    /// Type args applied to the parent's generics; may mention the child's own
    /// generics. Empty for a non-generic parent.
    pub args: Vec<Type>,
}

impl ClassExtends {
    /// Extends a non-generic parent.
    pub fn plain(parent: crate::MangledName) -> Self {
        Self {
            parent,
            args: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Dispatch {
    /// Codegen pushes the receiver as the first argument before calling the prelude wrapper.
    Direct,
    /// Codegen evaluates the receiver (for side effects) and drops it, then calls with only user args.
    Static,
    /// `call_ref` via a funcref loaded from the receiver's vtable slot.
    VTable,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodSig {
    /// Additional callable signatures, in overload-resolution order after the
    /// primary signature on this method. Kept flat so metadata cannot nest overload sets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overloads: Vec<MethodOverload>,
    /// Declared `m?(…)`: the member may be absent, so a call goes through `?.`.
    #[serde(default)]
    pub optional: bool,
    pub generics: GenericParameters,
    pub params: Vec<Param>,
    pub ret: Type,
    /// Guard or assertion predicate (`x is T`, `asserts x is T`, `asserts x`);
    /// `None` for ordinary methods.
    pub predicate: Option<crate::TypePredicate>,
    pub doc: Option<crate::DocComment>,
}

/// One additional signature of an overloaded method. Optionality and member
/// documentation belong to the method, not to an individual signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MethodOverload {
    pub generics: GenericParameters,
    pub params: Vec<Param>,
    pub ret: Type,
    pub predicate: Option<crate::TypePredicate>,
}

impl MethodSig {
    pub fn signatures(&self) -> impl Iterator<Item = MethodSig> + '_ {
        std::iter::once(MethodOverload {
            generics: self.generics.clone(),
            params: self.params.clone(),
            ret: self.ret.clone(),
            predicate: self.predicate.clone(),
        })
        .chain(self.overloads.iter().cloned())
        .map(|signature| MethodSig {
            overloads: Vec::new(),
            optional: self.optional,
            generics: signature.generics,
            params: signature.params,
            ret: signature.ret,
            predicate: signature.predicate,
            doc: self.doc.clone(),
        })
    }

    pub fn from_function_type(ty: &Type, optional_member: bool) -> Option<Self> {
        if let Type::Intersection(members) = ty.peel() {
            let mut members = members.iter();
            let mut result = Self::from_function_type(members.next()?, optional_member)?;
            for member in members {
                result.append_signatures(Self::from_function_type(member, optional_member)?);
            }
            return Some(result);
        }
        let Type::Function {
            params,
            ret,
            predicate,
            has_rest,
            optional,
        } = ty.peel()
        else {
            return None;
        };
        let fixed = params.len().saturating_sub(usize::from(*has_rest));
        Some(Self {
            overloads: Vec::new(),
            optional: optional_member,
            generics: Default::default(),
            params: params
                .iter()
                .enumerate()
                .map(|(position, ty)| Param {
                    name: format!("arg{position}"),
                    ty: ty.clone(),
                    default: None,
                    optional: position < fixed && position >= fixed.saturating_sub(*optional),
                    rest: *has_rest && position == fixed,
                })
                .collect(),
            ret: (**ret).clone(),
            predicate: predicate.as_deref().cloned(),
            doc: None,
        })
    }

    pub fn try_map_types<E>(
        &mut self,
        mut map: impl FnMut(&Type) -> Result<Type, E>,
    ) -> Result<(), E> {
        for bound in self.generics.constraints.values_mut() {
            *bound = map(bound)?;
        }
        for param in &mut self.params {
            param.ty = map(&param.ty)?;
        }
        self.ret = map(&self.ret)?;
        if let Some(predicate) = &mut self.predicate {
            predicate.asserted_type = map(&predicate.asserted_type)?;
        }
        for signature in &mut self.overloads {
            for bound in signature.generics.constraints.values_mut() {
                *bound = map(bound)?;
            }
            for param in &mut signature.params {
                param.ty = map(&param.ty)?;
            }
            signature.ret = map(&signature.ret)?;
            if let Some(predicate) = &mut signature.predicate {
                predicate.asserted_type = map(&predicate.asserted_type)?;
            }
        }
        Ok(())
    }

    pub fn append_signatures(&mut self, other: MethodSig) {
        for signature in other.signatures() {
            self.overloads.push(MethodOverload {
                generics: signature.generics,
                params: signature.params,
                ret: signature.ret,
                predicate: signature.predicate,
            });
        }
        self.optional &= other.optional;
    }
}

/// One accessor function on a class property. Getter and setter are independent
/// (TS 4.3+): a property may declare a getter, a setter, or both, and their types
/// need not match. The setter carries its parameter (name + write type).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AccessorSig {
    Getter { name: String, ret_ty: Type },
    Setter { name: String, param: Param },
}

impl AccessorSig {
    pub fn name(&self) -> &str {
        match self {
            AccessorSig::Getter { name, .. } | AccessorSig::Setter { name, .. } => name,
        }
    }
}

/// `readonly` mirrors the declared modifier and forbids writes through the
/// interface (shallow — it does not affect assignability). Optional properties
/// widen reads to `T | undefined`.
///
/// `intrinsic` marks a member codegen emits inline as an instruction sequence
/// (`String#length`/`Array#length` → payload `struct.get` + `array.len`): no
/// getter exists to import, and the emitter owns its lowering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertySig {
    pub ty: Type,
    pub readonly: bool,
    pub optional: bool,
    #[serde(default)]
    pub intrinsic: bool,
    pub doc: Option<crate::DocComment>,
}

/// A class instance field. Unlike [`PropertySig`], a field may be mutable and carries
/// `visibility`; `readonly` fields are writeable only inside the declaring constructor.
/// Optional fields widen reads to `T | undefined`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldSig {
    pub ty: Type,
    pub visibility: crate::Visibility,
    pub readonly: bool,
    pub optional: bool,
    /// Declared as a bodyless optional method (`f?(): T;`), which a subclass
    /// may override with a method.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub method: bool,
    pub doc: Option<crate::DocComment>,
}

/// No runtime representation; member resolution is fully static.
/// Recursive derived operations require the containing declaration's successful
/// [`PackageDeclaration::check_type_limits`] check since its last mutation.
/// Namespaced types are also mirrored into `PackageDeclaration::types` under their full dotted key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamespaceSymbol {
    pub name: String,
    /// Shared by every export of this namespace; children extend it via `mangle::extend`.
    pub mangled_prefix: crate::MangledName,
    /// Synthetic span for prelude namespaces.
    pub declaration_span: Span,
    pub values: BTreeMap<String, ValueSymbol>,
    /// Also mirrored into `PackageDeclaration::types` under the full dotted key for `lookup_named_type`.
    pub types: BTreeMap<String, TypeSymbol>,
    pub namespaces: BTreeMap<String, NamespaceSymbol>,
    pub doc: Option<crate::DocComment>,
}

impl Drop for NamespaceSymbol {
    fn drop(&mut self) {
        drop_namespace_children(&mut self.namespaces);
    }
}

fn drop_namespace_children(namespaces: &mut BTreeMap<String, NamespaceSymbol>) {
    if namespaces.is_empty() {
        return;
    }
    let mut pending = Vec::new();
    if pending.try_reserve(1).is_err() {
        drop_namespace_children_without_allocation(namespaces);
        return;
    }
    pending.push(std::mem::take(namespaces).into_values());
    while let Some(siblings) = pending.last_mut() {
        let Some(mut child) = siblings.next() else {
            pending.pop();
            continue;
        };
        if child.namespaces.is_empty() {
            continue;
        }
        if pending.try_reserve(1).is_err() {
            drop_namespace_children_without_allocation(&mut child.namespaces);
        } else {
            pending.push(std::mem::take(&mut child.namespaces).into_values());
        }
        // Each child now owns no namespaces, so its Drop cannot recurse.
    }
}

fn drop_namespace_children_without_allocation(namespaces: &mut BTreeMap<String, NamespaceSymbol>) {
    // Under memory pressure, repeatedly find and remove a leaf. This is slower
    // than the iterator stack but needs neither allocation nor native recursion.
    while !namespaces.is_empty() {
        let mut branch = &mut *namespaces;
        // Descend through first children until one is a leaf, and remove it.
        while let Some(entry) = branch.first_entry() {
            if entry.get().namespaces.is_empty() {
                entry.remove();
                break;
            }
            branch = &mut entry.into_mut().namespaces;
        }
    }
}

#[cfg(test)]
mod type_limit_checks {
    use std::collections::BTreeMap;

    use super::*;
    use crate::compiler_limits::MAX_TYPE_DEPTH;
    use crate::type_size::TypeTooLarge;

    fn nested(depth: u32) -> Type {
        (1..depth).fold(Type::Number, |inner, _| Type::Array(Box::new(inner)))
    }

    fn namespace(name: &str) -> NamespaceSymbol {
        NamespaceSymbol {
            name: name.into(),
            mangled_prefix: crate::mangle::prelude(name),
            declaration_span: Span::at(crate::FileId(0)),
            values: BTreeMap::new(),
            types: BTreeMap::new(),
            namespaces: BTreeMap::new(),
            doc: None,
        }
    }

    fn alias(ty: Type) -> TypeSymbol {
        TypeSymbol {
            name: "Deep".into(),
            mangled_name: crate::mangle::prelude("Deep"),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Alias {
                generics: Default::default(),
                ty,
                doc: None,
            },
        }
    }

    #[test]
    fn namespace_cleanup_can_run_without_allocating_a_frontier() {
        let mut child = namespace("leaf");
        for _ in 0..1_000 {
            let mut parent = namespace("branch");
            parent.namespaces.insert("child".into(), child);
            parent
                .namespaces
                .insert("sibling".into(), namespace("sibling"));
            child = parent;
        }
        drop_namespace_children_without_allocation(&mut child.namespaces);
        assert!(child.namespaces.is_empty());
    }

    #[test]
    fn a_type_in_a_nested_namespace_is_checked() {
        crate::test_support::on_compiler_stack(|| {
            let mut inner = namespace("Inner");
            inner
                .types
                .insert("Deep".into(), alias(nested(MAX_TYPE_DEPTH + 1)));
            let mut outer = namespace("Outer");
            outer.namespaces.insert("Inner".into(), inner);
            let mut declaration = PackageDeclaration::with_package("dep");
            assert_eq!(declaration.check_type_limits(), Ok(()));
            declaration.namespaces.insert("Outer".into(), outer);
            assert_eq!(declaration.check_type_limits(), Err(TypeTooLarge::Depth));
        });
    }

    #[test]
    fn interface_base_metadata_is_checked_even_without_members() {
        crate::test_support::on_compiler_stack(|| {
            let mut declaration = PackageDeclaration::with_package("dep");
            let mut symbol = alias(Type::Unknown);
            symbol.kind = TypeKind::Interface {
                extends: vec![nested(MAX_TYPE_DEPTH + 1)],
                generics: Default::default(),
                methods: BTreeMap::new(),
                properties: BTreeMap::new(),
                index: None,
                more_indexes: Vec::new(),
                dispatch: Dispatch::VTable,
                doc: None,
            };
            declaration.types.insert("Deep".into(), symbol);
            assert_eq!(declaration.check_type_limits(), Err(TypeTooLarge::Depth));
        });
    }

    #[test]
    fn runtime_declarations_are_checked() {
        crate::test_support::on_compiler_stack(|| {
            let mut declaration = PackageDeclaration::with_package("dep");
            declaration.runtime_functions.insert(
                crate::mangle::prelude("f"),
                RuntimeFunction {
                    params: vec![nested(MAX_TYPE_DEPTH)],
                    ret: Type::Void,
                },
            );
            declaration
                .runtime_types
                .insert("Deep".into(), alias(nested(MAX_TYPE_DEPTH)));
            assert_eq!(declaration.check_type_limits(), Ok(()));
            declaration
                .runtime_globals
                .insert(crate::mangle::prelude("g"), nested(MAX_TYPE_DEPTH + 1));
            assert_eq!(declaration.check_type_limits(), Err(TypeTooLarge::Depth));
        });
    }
}

#[cfg(test)]
mod non_finite_artifact_roundtrip {
    use super::{DefaultValue, EnumVariantValue, TypeKind};
    use crate::Type;
    use crate::types::LiteralF64;

    /// The failure [`crate::artifact_f64`] exists to prevent, at the site that
    /// makes it reachable from source: `Infinity` is a valid parameter default.
    #[test]
    fn non_finite_defaults_survive_json() {
        for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let json = serde_json::to_string(&DefaultValue::Number(value)).unwrap();
            assert!(
                !json.contains("null"),
                "non-finite default serialized to null: {json}"
            );
            let DefaultValue::Number(back) = serde_json::from_str(&json).unwrap() else {
                panic!("round-trip changed the variant: {json}");
            };
            assert_eq!(back.to_bits(), value.to_bits(), "round-trip lost {value}");
        }
    }

    #[test]
    fn finite_defaults_still_serialize_as_numbers() {
        let json = serde_json::to_string(&DefaultValue::Number(1.5)).unwrap();
        assert_eq!(json, r#"{"Number":1.5}"#);
        let DefaultValue::Number(back) = serde_json::from_str(&json).unwrap() else {
            panic!("round-trip changed the variant");
        };
        assert_eq!(back, 1.5);
    }

    #[test]
    fn non_finite_literal_types_survive_json() {
        let ty = Type::NumberLiteral(LiteralF64(f64::INFINITY));
        let json = serde_json::to_string(&ty).unwrap();
        assert_eq!(serde_json::from_str::<Type>(&json).unwrap(), ty);
    }

    #[test]
    fn non_finite_enum_values_survive_json() {
        let value = EnumVariantValue::Number(f64::INFINITY);
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<EnumVariantValue>(&json).unwrap(),
            value
        );
    }

    /// The fourth persisted `f64`, reached by `1e400` in an enum initializer.
    #[test]
    fn non_finite_enum_variants_survive_json() {
        let kind = TypeKind::NumberEnum {
            variants: vec![
                ("Huge".to_string(), f64::INFINITY),
                ("One".to_string(), 1.0),
            ],
            doc: None,
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(
            json.contains(r#"["Huge","Infinity"]"#),
            "non-finite enum variant did not survive serialization: {json}"
        );
        let TypeKind::NumberEnum { variants, .. } = serde_json::from_str(&json).unwrap() else {
            panic!("round-trip changed the variant: {json}");
        };
        assert_eq!(variants[0].1.to_bits(), f64::INFINITY.to_bits());
        assert_eq!(variants[1].1, 1.0);
    }
}
