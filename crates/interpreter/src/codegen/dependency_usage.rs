use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    MangledName, NamespaceSymbol, PackageDeclaration, Shape, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

pub struct DependencyValue<'a> {
    pub package: &'a PackageDeclaration,
    pub export_name: Cow<'a, str>,
    pub symbol: &'a ValueSymbol,
}

pub struct DependencyType<'a> {
    pub package: &'a PackageDeclaration,
    pub name: &'a str,
    pub symbol: &'a TypeSymbol,
    pub full: bool,
}

#[derive(Debug)]
pub struct DependencyUsage {
    values: BTreeSet<MangledName>,
    members: BTreeSet<MangledName>,
    types: BTreeSet<MangledName>,
    member_types: BTreeSet<MangledName>,
    shapes: BTreeSet<Shape>,
    uses_bigint: bool,
}

impl DependencyUsage {
    pub(crate) fn empty() -> Self {
        Self {
            values: BTreeSet::new(),
            members: BTreeSet::new(),
            types: BTreeSet::new(),
            member_types: BTreeSet::new(),
            shapes: BTreeSet::new(),
            uses_bigint: false,
        }
    }

    pub(crate) fn finish(mut self, dependencies: &[&PackageDeclaration]) -> Self {
        self.collect_codegen_prelude_values();
        for package in dependencies {
            for symbol in package.runtime_types.values() {
                if let TypeKind::Class {
                    narrowing_checks, ..
                } = &symbol.kind
                    && !narrowing_checks.is_empty()
                {
                    self.note_type(symbol.mangled_name.clone());
                }
            }
        }
        self.resolve_dependency_shapes(dependencies);
        self
    }

    pub fn is_value_used(&self, mangled: &MangledName) -> bool {
        self.values.contains(mangled)
    }

    pub fn is_member_used(&self, mangled: &MangledName) -> bool {
        self.members.contains(mangled)
    }

    pub fn is_type_used(&self, mangled: &MangledName) -> bool {
        self.types.contains(mangled)
    }

    pub fn is_shape_used(&self, shape: &Shape) -> bool {
        self.shapes.contains(shape)
    }

    pub fn uses_bigint(&self) -> bool {
        self.uses_bigint
    }

    pub(crate) fn note_value(&mut self, mangled: MangledName) {
        self.values.insert(mangled);
    }

    pub(crate) fn note_member(&mut self, mangled: MangledName) {
        self.members.insert(mangled);
    }

    pub(crate) fn note_type(&mut self, mangled: MangledName) {
        self.types.insert(mangled);
    }

    pub(crate) fn note_shape(&mut self, shape: Shape) {
        self.shapes.insert(shape);
    }

    pub fn is_host_value_used(&self, module: &str, name: &str) -> bool {
        self.values.contains(&crate::mangle::host(module, name))
    }

    pub fn dependency_values<'a>(
        &self,
        dependencies: &'a [&'a PackageDeclaration],
    ) -> Vec<DependencyValue<'a>> {
        let mut values = Vec::new();
        for package in dependencies {
            values.extend(package.values.iter().filter_map(|(name, symbol)| {
                self.is_value_used(&symbol.mangled_name)
                    .then_some(DependencyValue {
                        package,
                        export_name: Cow::Borrowed(name.as_str()),
                        symbol,
                    })
            }));
            collect_used_namespace_values(self, package, "", &package.namespaces, &mut values);
        }
        values
    }

    pub fn dependency_types<'a>(
        &self,
        dependencies: &'a [&'a PackageDeclaration],
    ) -> Vec<DependencyType<'a>> {
        let mut types = Vec::new();
        for package in dependencies {
            collect_used_types(self, package, &package.types, &mut types);
            for (name, symbol) in &package.runtime_types {
                if self.is_type_reachable(symbol)
                    && !package
                        .types
                        .values()
                        .any(|public| public.mangled_name == symbol.mangled_name)
                {
                    types.push(DependencyType {
                        package,
                        name,
                        symbol,
                        full: self.is_type_fully_used(symbol),
                    });
                }
            }
            collect_used_namespace_types(self, package, &package.namespaces, &mut types);
        }
        types
    }

    pub fn dependency_shapes<'a>(
        &self,
        dependencies: &'a [&'a PackageDeclaration],
    ) -> Vec<&'a Shape> {
        dependencies
            .iter()
            .flat_map(|package| package.shapes.iter())
            .filter(|shape| self.is_shape_used(shape))
            .collect()
    }

    pub fn is_type_fully_used(&self, ty: &TypeSymbol) -> bool {
        self.is_type_used(&ty.mangled_name)
    }

    pub fn is_type_reachable(&self, ty: &TypeSymbol) -> bool {
        self.is_type_used(&ty.mangled_name) || self.member_types.contains(&ty.mangled_name)
    }

    /// Reachability by mangled name alone — a class used *only* through its
    /// statics (`Calc.add(…)`, never constructed) lands in `member_types`, and
    /// still needs its WasmGC types reconstructed on the consumer side.
    pub fn is_type_reachable_by_name(&self, mangled: &MangledName) -> bool {
        self.types.contains(mangled) || self.member_types.contains(mangled)
    }

    pub fn is_interface_member_used(&self, ty: &TypeSymbol, member: &str) -> bool {
        self.is_type_used(&ty.mangled_name)
            || self.is_member_used(&crate::mangle::extend(&ty.mangled_name, member))
    }

    fn resolve_dependency_shapes(&mut self, dependencies: &[&PackageDeclaration]) {
        let mut values: BTreeMap<MangledName, &crate::ValueSymbol> = BTreeMap::new();
        let mut types: BTreeMap<MangledName, &crate::TypeSymbol> = BTreeMap::new();
        let mut members: BTreeMap<MangledName, (&crate::TypeSymbol, &str)> = BTreeMap::new();
        // Class statics keyed `Class#static#name`. Their call sites lower to
        // plain `Call`/`GlobalRef`, so they surface in `self.values` — resolved
        // in the values loop below to mark the owning class reachable.
        let mut statics: BTreeMap<MangledName, (&crate::TypeSymbol, &str)> = BTreeMap::new();

        for defs in dependencies {
            collect_values(&defs.values, &mut values);
            collect_namespace_values(&defs.namespaces, &mut values);
            for ty_sym in defs.runtime_types.values().chain(defs.types.values()) {
                types.insert(ty_sym.mangled_name.clone(), ty_sym);
                if let TypeKind::Interface {
                    methods,
                    properties,
                    ..
                } = &ty_sym.kind
                {
                    for name in methods.keys() {
                        members.insert(
                            crate::mangle::extend(&ty_sym.mangled_name, name),
                            (ty_sym, name),
                        );
                    }
                    for name in properties.keys() {
                        members.insert(
                            crate::mangle::extend(&ty_sym.mangled_name, name),
                            (ty_sym, name),
                        );
                    }
                }
                if let TypeKind::Class {
                    statics: static_methods,
                    static_fields,
                    ..
                } = &ty_sym.kind
                {
                    for name in static_methods.keys().chain(static_fields.keys()) {
                        statics.insert(
                            crate::mangle::static_member(&ty_sym.mangled_name, name),
                            (ty_sym, name),
                        );
                    }
                }
            }
        }

        let mut resolved_values = BTreeSet::new();
        let mut resolved_members = BTreeSet::new();
        let mut resolved_types = BTreeSet::new();
        loop {
            let mut changed = false;

            for mangled in self.values.clone() {
                if !resolved_values.insert(mangled.clone()) {
                    continue;
                }
                if let Some(sym) = values.get(&mangled) {
                    changed = true;
                    self.collect_value_symbol(sym);
                }
                if let Some((ty_sym, member_name)) = statics.get(&mangled) {
                    changed = true;
                    self.member_types.insert(ty_sym.mangled_name.clone());
                    self.collect_type_symbol(ty_sym);
                    if let TypeKind::Class {
                        statics: static_methods,
                        static_fields,
                        ..
                    } = &ty_sym.kind
                    {
                        if let Some(sig) = static_methods.get(*member_name) {
                            for p in &sig.params {
                                self.collect_type(&p.ty);
                            }
                            self.collect_type(&sig.ret);
                        }
                        if let Some(field) = static_fields.get(*member_name) {
                            self.collect_type(&field.ty);
                        }
                    }
                }
            }

            for mangled in self.members.clone() {
                if !resolved_members.insert(mangled.clone()) {
                    continue;
                }
                if let Some(sym) = values.get(&mangled) {
                    changed = true;
                    self.values.insert(mangled.clone());
                    self.collect_value_symbol(sym);
                }
                if let Some((ty_sym, member_name)) = members.get(&mangled) {
                    changed = true;
                    self.member_types.insert(ty_sym.mangled_name.clone());
                    self.collect_type_symbol(ty_sym);
                    if let TypeKind::Interface {
                        methods,
                        properties,
                        ..
                    } = &ty_sym.kind
                    {
                        if let Some(sig) = methods.get(*member_name) {
                            for p in &sig.params {
                                self.collect_type(&p.ty);
                            }
                            self.collect_type(&sig.ret);
                        }
                        if let Some(sig) = properties.get(*member_name) {
                            self.collect_type(&sig.ty);
                        }
                    }
                }
            }

            for mangled in self.types.clone() {
                if !resolved_types.insert(mangled.clone()) {
                    continue;
                }
                if let Some(sym) = types.get(&mangled) {
                    changed = true;
                    self.collect_type_symbol(sym);
                    // A used interface imports every member (see
                    // `is_interface_member_used`), so promote each member key too:
                    // members ported to a prelude-host value symbol must resolve to
                    // the Rust impl, not the deleted Wasm wrapper.
                    if let TypeKind::Interface {
                        methods,
                        properties,
                        ..
                    } = &sym.kind
                    {
                        for name in methods.keys().chain(properties.keys()) {
                            self.members
                                .insert(crate::mangle::extend(&sym.mangled_name, name));
                        }
                    }
                }
            }

            if !changed {
                break;
            }
        }
    }

    fn collect_value_symbol(&mut self, value: &crate::ValueSymbol) {
        match &value.kind {
            ValueKind::Function { params, ret, .. } => {
                for p in params {
                    self.collect_type(&p.ty);
                }
                self.collect_type(ret);
            }
            ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => {
                self.collect_type(ty);
            }
        }
    }

    fn collect_type_symbol(&mut self, ty_sym: &crate::TypeSymbol) {
        match &ty_sym.kind {
            TypeKind::Alias { ty, .. } => self.collect_type(ty),
            TypeKind::Interface {
                properties,
                methods,
                ..
            } => {
                for sig in properties.values() {
                    self.collect_type(&sig.ty);
                }
                for sig in methods.values() {
                    for p in &sig.params {
                        self.collect_type(&p.ty);
                    }
                    self.collect_type(&sig.ret);
                }
            }
            TypeKind::Class {
                fields,
                methods,
                statics,
                static_fields,
                constructor,
                ..
            } => {
                for sig in fields.values() {
                    self.collect_type(&sig.ty);
                }
                for sig in methods.values().chain(statics.values()) {
                    for p in &sig.params {
                        self.collect_type(&p.ty);
                    }
                    self.collect_type(&sig.ret);
                }
                for sig in static_fields.values() {
                    self.collect_type(&sig.ty);
                }
                for p in constructor {
                    self.collect_type(&p.ty);
                }
            }
            TypeKind::NumberEnum { .. } | TypeKind::StringEnum { .. } => {}
        }
    }

    /// The import half only. `CodegenAnalysis` calls this through its own
    /// `visit_type` funnel, which also collects the type's closure shapes.
    pub(crate) fn collect_type(&mut self, ty: &Type) {
        if let Some(shape) = Shape::from_type(ty) {
            if matches!(shape, Shape::Object { .. }) {
                self.collect_typed_object_stringify_host_value();
            }
            self.shapes.insert(shape);
        }
        match ty.peel() {
            Type::Object { fields, index } => {
                if let Some(index) = index {
                    self.collect_type(&index.value);
                    for helper in ["#getField", "#setField", "#recordValues", "#hasField"] {
                        self.note_member(crate::mangle::extend(
                            &crate::mangle::prelude("ObjectConstructor"),
                            helper,
                        ));
                    }
                }
                for field in fields.values() {
                    self.collect_type(&field.ty);
                }
            }
            Type::Array(elem) => self.collect_type(elem),
            Type::Tuple(elements) => {
                for elem in elements {
                    self.collect_type(elem);
                }
            }
            Type::Union(members) => {
                for member in members {
                    self.collect_type(member);
                }
            }
            Type::Function { params, ret, .. } => {
                for p in params {
                    self.collect_type(p);
                }
                self.collect_type(ret);
            }
            Type::InterfaceRef { mangled, args, .. }
            | Type::ClassRef { mangled, args, .. }
            | Type::AliasRef { mangled, args, .. } => {
                self.types.insert(mangled.clone());
                for arg in args {
                    self.collect_type(arg);
                }
            }
            Type::NumberEnum { mangled, .. } | Type::StringEnum { mangled, .. } => {
                self.types.insert(mangled.clone());
            }
            Type::Alias { ty, .. } | Type::Refined { ty, .. } | Type::Readonly(ty) => {
                self.collect_type(ty);
            }
            Type::BigInt => {
                self.uses_bigint = true;
            }
            Type::Number
            | Type::NumberLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Void
            | Type::Never
            | Type::Unknown
            | Type::Error
            | Type::TypeVar(_)
            | Type::GenericParam { .. } => {}
        }
    }

    pub(crate) fn collect_json_host_values(&mut self) {
        for name in [
            "parse",
            "parseTypedObject",
            "parseTypedValue",
            "stringify",
            "stringifyPrettyNumber",
            "stringifyPrettyString",
            "diagnosticPathField",
            "diagnosticPathIndex",
            "diagnosticMismatch",
            "diagnosticMissingFields",
            "stringifyTypedObject",
        ] {
            self.values
                .insert(crate::mangle::host(crate::runtime::JSON_MODULE_NAME, name));
        }
    }

    pub(crate) fn collect_typed_object_stringify_host_value(&mut self) {
        self.values.insert(crate::mangle::host(
            crate::runtime::JSON_MODULE_NAME,
            "stringifyTypedObject",
        ));
    }

    pub(crate) fn collect_bigint_host_value(&mut self, name: &str) {
        self.uses_bigint = true;
        self.values.insert(crate::mangle::host(
            crate::runtime::BIGINT_MODULE_NAME,
            name,
        ));
    }

    /// The string→number parse `+s` lowers to — the same host fn `Number(s)`
    /// calls.
    pub(crate) fn collect_number_coercion_host_value(&mut self) {
        self.values.insert(crate::mangle::host(
            crate::runtime::NUMBER_MODULE_NAME,
            "toNumber",
        ));
    }

    /// The bigint operator imports a `++`/`--` on a bigint target lowers to.
    /// The target's own type goes through the caller's type funnel.
    pub(crate) fn collect_postfix_bigint_ops(&mut self, ty: &Type) {
        if matches!(ty.peel(), Type::BigInt) {
            self.collect_bigint_host_value("fromNumber");
            self.collect_bigint_host_value("add");
            self.collect_bigint_host_value("sub");
        }
    }

    fn collect_codegen_prelude_values(&mut self) {
        for name in [
            "isFinite",
            "isNaN",
            "string_cmp",
            "string_concat",
            "string_eq",
            "vtable_walk_enter",
            "vtable_walk_leave",
        ] {
            self.values.insert(crate::mangle::prelude(name));
        }
        for (iface, member) in [
            ("Boolean", "toJson"),
            ("Boolean", "toString"),
            ("Number", "toJson"),
            ("Number", "toString"),
            ("RegExpConstructor", "new"),
            ("String", "toJson"),
            ("String", "toString"),
        ] {
            self.members.insert(crate::mangle::extend(
                &crate::mangle::prelude(iface),
                member,
            ));
        }
        // The internal throw sites (bounds, cast, non-null assert, `assert`)
        // call the reconstructed class constructors, so these classes must
        // reconstruct in every module.
        self.note_type(crate::mangle::prelude("Error"));
        self.note_type(crate::mangle::prelude("RangeError"));
        self.note_type(crate::mangle::prelude("TypeError"));
    }
}

fn collect_values<'a>(
    values: &'a BTreeMap<String, crate::ValueSymbol>,
    out: &mut BTreeMap<MangledName, &'a crate::ValueSymbol>,
) {
    for value in values.values() {
        out.insert(value.mangled_name.clone(), value);
    }
}

fn collect_namespace_values<'a>(
    namespaces: &'a BTreeMap<String, crate::NamespaceSymbol>,
    out: &mut BTreeMap<MangledName, &'a crate::ValueSymbol>,
) {
    for namespace in namespaces.values() {
        collect_values(&namespace.values, out);
        collect_namespace_values(&namespace.namespaces, out);
    }
}

fn collect_used_namespace_values<'a>(
    usage: &DependencyUsage,
    package: &'a PackageDeclaration,
    parent_path: &str,
    namespaces: &'a BTreeMap<String, NamespaceSymbol>,
    out: &mut Vec<DependencyValue<'a>>,
) {
    for (ns_name, namespace) in namespaces {
        let path = if parent_path.is_empty() {
            ns_name.clone()
        } else {
            format!("{parent_path}#{ns_name}")
        };
        for (local_name, symbol) in &namespace.values {
            if usage.is_value_used(&symbol.mangled_name) {
                out.push(DependencyValue {
                    package,
                    export_name: Cow::Owned(format!("{path}#{local_name}")),
                    symbol,
                });
            }
        }
        collect_used_namespace_values(usage, package, &path, &namespace.namespaces, out);
    }
}

fn collect_used_types<'a>(
    usage: &DependencyUsage,
    package: &'a PackageDeclaration,
    types: &'a BTreeMap<String, TypeSymbol>,
    out: &mut Vec<DependencyType<'a>>,
) {
    for (name, symbol) in types {
        if usage.is_type_reachable(symbol) {
            out.push(DependencyType {
                package,
                name,
                symbol,
                full: usage.is_type_fully_used(symbol),
            });
        }
    }
}

fn collect_used_namespace_types<'a>(
    usage: &DependencyUsage,
    package: &'a PackageDeclaration,
    namespaces: &'a BTreeMap<String, NamespaceSymbol>,
    out: &mut Vec<DependencyType<'a>>,
) {
    for namespace in namespaces.values() {
        collect_used_types(usage, package, &namespace.types, out);
        collect_used_namespace_types(usage, package, &namespace.namespaces, out);
    }
}
