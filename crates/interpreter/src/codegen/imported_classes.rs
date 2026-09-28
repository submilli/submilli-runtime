//! Cross-package class reconstruction (SUB-488).
//!
//! A consumer that imports a `class` from another package never sees that
//! class's Wasm module — only its public `PackageDeclaration`. To use the class
//! (construct it, read fields, dispatch methods, `instanceof`, or `extends` it)
//! the consumer must rebuild the class's WasmGC types in its own module so that
//! structural canonicalization unifies them with the producer's.
//!
//! Each class is emitted as its own rec group `(rec $C_vtable $C)` — matching the
//! producer ([`super::classes`]) — so the consumer reconstructs only the classes
//! it transitively needs, parents first. There are no cross-class type
//! references (a class's fields live in the shared object-fields payload, not as
//! named struct fields), so per-class groups always canonicalize. The consumer
//! also imports each class's constructor, self-first ctor-init, and method bodies
//! (exported by the producer in [`super::classes::ClassPlan::exported_funcs`]) so
//! `new`, `super(...)`, and a local subclass's inherited vtable slots resolve.

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    EntityType, FieldType, GlobalType, HeapType, ImportSection, RefType, StorageType, StructType,
    SubType, TypeSection, ValType,
};

use crate::codegen::classes::shadows_inherited_field;
use crate::codegen::dependency_usage::DependencyUsage;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::{MethodSlotAbi, SymbolTable};
use crate::{MangledName, Param, TypeKind, TypedAst, TypedTypeDecl};

/// The resolved full layout of an imported class, handed to [`super::classes`]
/// so a local subclass extending it lays its own fields/methods out after the
/// inherited prefix.
pub struct ImportedClassLayout {
    pub private_members: BTreeSet<String>,
    /// Full data-field names (inherited prefix then own), object-payload order.
    pub fields: Vec<String>,
    pub optional_fields: std::collections::BTreeSet<String>,
    pub narrowing_checks: BTreeMap<String, crate::FieldNarrowingCheck>,
    /// Full vtable method slots (inherited prefix then own/override), in order.
    pub methods: Vec<ImportedSlot>,
    /// The class's own constructor params, so a local subclass with an *implicit*
    /// constructor extending this class inherits its parameter signature.
    pub ctor_params: Vec<crate::TypedParam>,
}

pub struct ImportedSlot {
    pub name: String,
    /// The class whose body backs this slot (the declarer, or nearest ancestor
    /// for an inherited slot) — the key under which its method func was imported.
    pub owner: MangledName,
    /// Declared parameter/return types of the owner's body, so a local subclass
    /// can emit its own payload adapter for this slot.
    pub param_tys: Vec<crate::Type>,
    pub argument_metadata: Option<String>,
    pub ret_ty: crate::Type,
    pub generic: bool,
}

/// Per-class data gathered from the dependency `PackageDeclaration`s. `fields`
/// and `methods` are `BTreeMap`s, so iterating their keys yields the canonical
/// (sorted) own-member order the producer assigns slots in — no order metadata
/// has to cross the package boundary.
struct ClassInfo<'a> {
    generics: &'a [String],
    package: &'a str,
    supports_instance_guards: bool,
    runtime_generics: &'a BTreeSet<MangledName>,
    name: &'a str,
    extends: Option<MangledName>,
    fields: &'a BTreeMap<String, crate::FieldSig>,
    narrowing_checks: &'a BTreeMap<String, crate::FieldNarrowingCheck>,
    methods: &'a BTreeMap<String, crate::MethodSig>,
    constructor: &'a [Param],
    /// Accessor functions; codegen reconstructs the same synthetic getter/setter
    /// vtable methods the producer emitted.
    accessors: &'a [crate::AccessorSig],
    statics: &'a BTreeMap<String, crate::MethodSig>,
    static_visibility: &'a BTreeMap<String, crate::Visibility>,
    static_fields: &'a BTreeMap<String, crate::FieldSig>,
}

impl<'a> ClassInfo<'a> {
    /// Private statics are not exported by the producer, so the consumer must
    /// not import them.
    fn public_statics(&self) -> impl Iterator<Item = (&'a String, &'a crate::MethodSig)> {
        self.statics.iter().filter(|(name, _)| {
            self.static_visibility.get(*name) != Some(&crate::Visibility::Private)
        })
    }

    fn public_static_fields(&self) -> impl Iterator<Item = (&'a String, &'a crate::FieldSig)> {
        self.static_fields
            .iter()
            .filter(|(_, field)| field.visibility != crate::Visibility::Private)
    }
}

/// The producer's vtable-method set for an imported class: real methods plus the
/// synthetic getter/setter methods derived from its accessors, each as
/// `(name, params, ret)`. Mirrors `codegen::classes::accessor_methods` so slot
/// indices and signatures line up across the package boundary.
fn vtable_methods(class: &ClassInfo<'_>) -> BTreeMap<String, (Vec<crate::Type>, crate::Type)> {
    let mut out: BTreeMap<String, (Vec<crate::Type>, crate::Type)> = class
        .methods
        .iter()
        .map(|(name, sig)| {
            (
                name.clone(),
                (
                    sig.params.iter().map(|p| p.ty.clone()).collect(),
                    sig.ret.clone(),
                ),
            )
        })
        .collect();
    for acc in class.accessors {
        match acc {
            crate::AccessorSig::Getter { name, ret_ty } => {
                out.insert(
                    crate::codegen::classes::accessor_getter_name(name),
                    (Vec::new(), ret_ty.clone()),
                );
            }
            crate::AccessorSig::Setter { name, param } => {
                out.insert(
                    crate::codegen::classes::accessor_setter_name(name),
                    (vec![param.ty.clone()], crate::Type::Void),
                );
            }
        }
    }
    out
}

/// The linker module a class's function bodies import from — the owning
/// package for user classes and prelude-declared (host-implemented) classes
/// alike: `Error`'s constructor/ctor-init are Rust host fns registered under
/// `submilli:prelude`.
fn host_module_for<'a>(class: &ClassInfo<'a>) -> &'a str {
    class.package
}

/// Reconstruct every imported class reachable from this module: emit its rec
/// group, record type/slot indices, and import its functions. Returns the
/// resolved layout of each, keyed by mangled name, for `classes::ClassPlan`.
#[allow(clippy::too_many_arguments)]
pub fn reconstruct(
    dependencies: &[&crate::PackageDeclaration],
    ta: &TypedAst,
    usage: &DependencyUsage,
    intrinsics: IntrinsicTypeIndices,
    file: crate::FileId,
    types: &mut TypeSection,
    next_type_idx: &mut u32,
    imports: &mut ImportSection,
    next_func_idx: &mut u32,
    next_global_idx: &mut u32,
    symbols: &mut SymbolTable,
) -> Result<BTreeMap<MangledName, ImportedClassLayout>, crate::compiler_error::CompilerFailure> {
    let info = collect_class_info(dependencies);
    let order = needed_in_topo_order(&info, ta, usage);

    let mut layouts: BTreeMap<MangledName, ImportedClassLayout> = BTreeMap::new();
    for mangled in &order {
        let class = &info[mangled];
        let layout = reconstruct_one(
            mangled,
            class,
            &info,
            &layouts,
            intrinsics,
            file,
            types,
            next_type_idx,
            imports,
            next_func_idx,
            next_global_idx,
            symbols,
        )?;
        layouts.insert(mangled.clone(), layout);
    }
    // Statics come last — see `import_statics`.
    for mangled in &order {
        import_statics(
            mangled,
            &info[mangled],
            usage,
            types,
            next_type_idx,
            imports,
            next_func_idx,
            next_global_idx,
            symbols,
        )?;
    }
    Ok(layouts)
}

/// Import the statics this module actually uses: self-less functions and
/// backing globals, keyed `Class#static#name`.
///
/// Unlike methods and constructors, these signatures are **unerased** — the
/// producer emits statics through the ordinary function/global path, where
/// `value_type` resolves a class to its concrete struct. So every class one
/// names must already be reconstructed, or `value_type` takes the
/// `(ref null $Object)` fallback and the import fails to link.
///
/// Two things keep that true. This runs in a pass after all reconstruction, so
/// declaration order can't matter. And the usage gate holds the rest: resolving
/// a static marks every class its signature names, so one that reaches here has
/// already pulled the classes it names *from this module's dependency closure*
/// into the reconstructed set. A class from outside that closure is marked but
/// never reconstructed, and still takes the fallback — the residual hole is a
/// consumer that declares fewer dependencies than the statics it uses reach.
///
/// Importing *unused* statics is what would break the invariant outright: a
/// class reconstructed only as someone's ancestor carries statics nothing
/// prepared types for, and they need not even be expressible here — a
/// function-typed static wants a closure struct this module may never have
/// registered.
#[allow(clippy::too_many_arguments)]
fn import_statics(
    mangled: &MangledName,
    class: &ClassInfo<'_>,
    usage: &DependencyUsage,
    types: &mut TypeSection,
    next_type_idx: &mut u32,
    imports: &mut ImportSection,
    next_func_idx: &mut u32,
    next_global_idx: &mut u32,
    symbols: &mut SymbolTable,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for (name, sig) in class.public_statics() {
        let key = crate::mangle::static_member(mangled, name);
        if !usage.is_value_used(&key) {
            continue;
        }
        let mut param_types: Vec<ValType> = sig
            .params
            .iter()
            .map(|p| symbols.value_type(&p.ty))
            .collect::<Result<_, _>>()?;
        if class.runtime_generics.contains(&key) {
            symbols.runtime_generic_functions.insert(key.clone());
            param_types.push(super::runtime_descriptors::environment_type(symbols)?);
        }
        let results = symbols.wasm_result(&sig.ret)?;
        types.ty().function(param_types, results);
        let sig_idx = take(next_type_idx);
        imports.import(
            host_module_for(class),
            key.as_str(),
            EntityType::Function(sig_idx),
        );
        let idx = take(next_func_idx);
        symbols.record_imported_fn(
            key,
            idx,
            sig.params.iter().map(|p| p.ty.clone()).collect(),
            sig.ret.clone(),
            false,
        );
    }
    for (name, field) in class.public_static_fields() {
        let key = crate::mangle::static_member(mangled, name);
        if !usage.is_value_used(&key) {
            continue;
        }
        // Mutable like every exported value-global — the producer's `_start`
        // initializes it; const-ness is typechecker-enforced.
        imports.import(
            host_module_for(class),
            key.as_str(),
            EntityType::Global(GlobalType {
                val_type: super::global_val_type(&field.ty, symbols)?,
                mutable: true,
                shared: false,
            }),
        );
        symbols.record_typed_global(key, take(next_global_idx), field.ty.clone());
    }
    Ok(())
}

fn collect_class_info<'a>(
    dependencies: &'a [&'a crate::PackageDeclaration],
) -> BTreeMap<MangledName, ClassInfo<'a>> {
    let mut info = BTreeMap::new();
    for defs in dependencies {
        for sym in defs.runtime_types.values().chain(defs.types.values()) {
            if let TypeKind::Class {
                generics,
                extends,
                fields,
                narrowing_checks,
                methods,
                accessors,
                constructor,
                statics,
                static_visibility,
                static_fields,
                ..
            } = &sym.kind
            {
                info.insert(
                    sym.mangled_name.clone(),
                    ClassInfo {
                        generics,
                        runtime_generics: &defs.runtime_generics,
                        package: defs.package_name.as_str(),
                        supports_instance_guards: defs
                            .runtime_types
                            .contains_key(sym.mangled_name.as_str()),
                        name: sym.name.as_str(),
                        // Layout and vtables depend on the parent's name;
                        // concrete arguments travel in the instance context.
                        extends: extends.as_ref().map(|e| e.parent.clone()),
                        fields,
                        narrowing_checks,
                        methods,
                        constructor,
                        accessors,
                        statics,
                        static_visibility,
                        static_fields,
                    },
                );
            }
        }
    }
    info
}

/// Imported classes the module needs: every imported class referenced by the
/// program plus the imported parents of local classes, transitively closed over
/// `extends`, ordered parents-first.
fn needed_in_topo_order(
    info: &BTreeMap<MangledName, ClassInfo<'_>>,
    ta: &TypedAst,
    usage: &DependencyUsage,
) -> Vec<MangledName> {
    let mut roots: BTreeSet<MangledName> = BTreeSet::new();
    for mangled in info.keys() {
        // `is_type_reachable_by_name` also catches classes used only through
        // their statics (never constructed, never named as a type).
        if usage.is_type_reachable_by_name(mangled) {
            roots.insert(mangled.clone());
        }
    }
    // Imported parents of local classes (the parent itself may not be "used").
    for decl in &ta.types {
        if let TypedTypeDecl::Class(c) = decl
            && let Some(parent) = &c.extends
            && info.contains_key(parent)
        {
            roots.insert(parent.clone());
        }
    }

    let mut order: Vec<MangledName> = Vec::new();
    let mut seen: BTreeSet<MangledName> = BTreeSet::new();
    for root in &roots {
        visit(root, info, &mut seen, &mut order);
    }
    order
}

fn visit(
    mangled: &MangledName,
    info: &BTreeMap<MangledName, ClassInfo<'_>>,
    seen: &mut BTreeSet<MangledName>,
    order: &mut Vec<MangledName>,
) {
    if seen.contains(mangled) || !info.contains_key(mangled) {
        return;
    }
    seen.insert(mangled.clone());
    if let Some(parent) = &info[mangled].extends {
        debug_assert!(
            info.contains_key(parent),
            "no declaration for `{parent}`, the parent of imported class `{mangled}`: the \
             dependency closure passed to codegen is incomplete. Reconstructing `{mangled}` \
             without its ancestor's field and slot prefix links as `imported global type \
             mismatch`, with nothing naming the missing package.",
        );
        visit(parent, info, seen, order);
    }
    order.push(mangled.clone());
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_one(
    mangled: &MangledName,
    class: &ClassInfo<'_>,
    info: &BTreeMap<MangledName, ClassInfo<'_>>,
    layouts: &BTreeMap<MangledName, ImportedClassLayout>,
    intrinsics: IntrinsicTypeIndices,
    file: crate::FileId,
    types: &mut TypeSection,
    next_type_idx: &mut u32,
    imports: &mut ImportSection,
    next_func_idx: &mut u32,
    next_global_idx: &mut u32,
    symbols: &mut SymbolTable,
) -> Result<ImportedClassLayout, crate::compiler_error::CompilerFailure> {
    // Full field list = inherited prefix (already reconstructed) + own *data*
    // fields. Accessor properties are in `fields` for typing/docs but back no data
    // slot, so they're excluded — matching the producer's payload layout.
    let mut fields: Vec<String> = class
        .extends
        .as_ref()
        .and_then(|p| layouts.get(p))
        .map(|l| l.fields.clone())
        .unwrap_or_default();
    for name in class
        .fields
        .keys()
        .filter(|name| !class.accessors.iter().any(|a| a.name() == name.as_str()))
    {
        if shadows_inherited_field(fields.iter().map(String::as_str), name) {
            continue;
        }
        fields.push(name.clone());
    }
    let mut optional_fields = class
        .extends
        .as_ref()
        .and_then(|parent| layouts.get(parent))
        .map(|layout| layout.optional_fields.clone())
        .unwrap_or_default();
    for (name, field) in class.fields {
        if field.optional {
            optional_fields.insert(name.clone());
        } else {
            optional_fields.remove(name);
        }
    }
    let mut narrowing_checks = class
        .extends
        .as_ref()
        .and_then(|parent| layouts.get(parent))
        .map(|layout| layout.narrowing_checks.clone())
        .unwrap_or_default();
    narrowing_checks.extend(class.narrowing_checks.clone());

    // The producer's full vtable-method set (real methods + accessor getter/setter),
    // sorted by name to match the producer's slot assignment.
    let methods = vtable_methods(class);

    // Full vtable slots = parent's slots, then fold in own methods (override →
    // same slot/owner-of-body, new → appended owned by this class).
    let mut slots: Vec<ImportedSlot> = class
        .extends
        .as_ref()
        .and_then(|p| layouts.get(p))
        .map(|l| {
            l.methods
                .iter()
                .map(|s| ImportedSlot {
                    name: s.name.clone(),
                    owner: s.owner.clone(),
                    param_tys: s.param_tys.clone(),
                    argument_metadata: s.argument_metadata.clone(),
                    ret_ty: s.ret_ty.clone(),
                    generic: s.generic,
                })
                .collect()
        })
        .unwrap_or_default();
    for (name, (param_tys, ret_ty)) in &methods {
        let argument_metadata = class.methods.get(name).and_then(|sig| {
            super::call_arguments::metadata(
                sig.params
                    .iter()
                    .map(|param| (param.default.as_ref(), param.rest)),
            )
        });
        let generic = class
            .methods
            .get(name)
            .is_some_and(|sig| !sig.generics.is_empty());
        if let Some(slot) = slots.iter_mut().find(|s| &s.name == name) {
            // Override: this class supplies the body, so the slot must describe
            // it rather than the ancestor's.
            slot.owner = mangled.clone();
            slot.param_tys = param_tys.clone();
            slot.argument_metadata = argument_metadata;
            slot.ret_ty = ret_ty.clone();
            slot.generic = generic;
        } else {
            slots.push(ImportedSlot {
                name: name.clone(),
                owner: mangled.clone(),
                param_tys: param_tys.clone(),
                argument_metadata,
                ret_ty: ret_ty.clone(),
                generic,
            });
        }
    }

    // 1. Method-signature fn-types for slots that *originate* on this class
    //    (newly introduced). Overrides/inherited slots reuse the originating
    //    ancestor's sig, looked up from the symbol table (parents come first).
    for (name, (param_tys, ret)) in &methods {
        let originates = class
            .extends
            .as_ref()
            .and_then(|p| symbols.class_method_sig(p, name))
            .is_none();
        if !originates {
            continue;
        }
        let mut params: Vec<ValType> = vec![ref_to(intrinsics.object)];
        params.extend(
            param_tys
                .iter()
                .map(|_| symbols.value_type(&crate::Type::Unknown))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let results = symbols.slot_wasm_result(ret)?;
        let abi = MethodSlotAbi {
            params: params[1..].to_vec(),
            ret: results.first().copied(),
        };
        types.ty().function(params, results);
        let sig_idx = take(next_type_idx);
        symbols.record_class_method_sig(mangled.clone(), name.clone(), sig_idx);
        symbols.record_class_method_abi(mangled.clone(), name.clone(), abi);
    }
    // Inherited slots: copy the originating ancestor's sig (and its erasure
    // shape) under this class's key.
    for slot in &slots {
        if symbols.class_method_sig(mangled, &slot.name).is_none() {
            let sig = symbols
                .class_method_sig(&slot.owner, &slot.name)
                .or_else(|| ancestor_sig(info, symbols, class, &slot.name))
                .expect("inherited slot's originating sig reconstructed earlier");
            symbols.record_class_method_sig(mangled.clone(), slot.name.clone(), sig);
        }
        if symbols.class_method_abi(mangled, &slot.name).is_none()
            && let Some(abi) = symbols
                .class_method_abi(&slot.owner, &slot.name)
                .cloned()
                .or_else(|| {
                    class
                        .extends
                        .as_ref()
                        .and_then(|p| symbols.class_method_abi(p, &slot.name).cloned())
                })
        {
            symbols.record_class_method_abi(mangled.clone(), slot.name.clone(), abi);
        }
    }

    // 2. Reserve the (vtable, struct) pair and 3. emit the rec group.
    let vtable_idx = take(next_type_idx);
    let struct_idx = take(next_type_idx);
    symbols.record_class_vtable_type(mangled.clone(), vtable_idx);
    symbols.record_class_struct_type(mangled.clone(), struct_idx);

    let vtable_super = class
        .extends
        .as_ref()
        .and_then(|p| symbols.class_vtable_type_idx(p))
        .unwrap_or(intrinsics.class_vtable);
    let struct_super = class
        .extends
        .as_ref()
        .and_then(|p| symbols.class_struct_type_idx(p))
        .unwrap_or(intrinsics.object_shape);
    symbols.record_struct_supertype(vtable_idx, vtable_super);
    symbols.record_struct_supertype(struct_idx, struct_super);

    let mut vtable_fields: Vec<FieldType> = vec![
        fieldtype_ref(intrinsics.to_string_fn),
        fieldtype_ref(intrinsics.to_json_fn),
        fieldtype_ref(intrinsics.equals_fn),
        fieldtype_ref(intrinsics.hash_fn),
        fieldtype_ref_null(intrinsics.class_vtable),
    ];
    for slot in &slots {
        let sig = symbols
            .class_method_sig(mangled, &slot.name)
            .expect("slot sig recorded above");
        vtable_fields.push(fieldtype_ref(sig));
    }
    let vtable = substruct(vtable_fields, Some(vtable_super));

    let struct_ty = substruct(
        vec![
            fieldtype_ref(vtable_idx),
            FieldType {
                mutable: true,
                ..fieldtype_ref(intrinsics.field_names)
            },
            FieldType {
                mutable: true,
                ..fieldtype_ref(intrinsics.object_fields)
            },
        ],
        Some(struct_super),
    );
    types.ty().rec([vtable, struct_ty]);

    // Import the producer's vtable-singleton global — the class's nominal
    // identity. `instanceof` compares against it by ref.eq, and a local
    // subclass's vtable wires it in as the parent link. For host classes
    // (`Error`) the producer is the Rust runtime, which `linker.define`s it.
    imports.import(
        host_module_for(class),
        crate::codegen::classes::vtable_global_export_name(mangled).as_str(),
        EntityType::Global(GlobalType {
            val_type: ref_to(vtable_idx),
            mutable: false,
            shared: false,
        }),
    );
    symbols.record_class_vtable_global(mangled.clone(), take(next_global_idx));

    // Record field payload slots and method vtable slots (universal slots +
    // parent link precede).
    for (i, name) in fields.iter().enumerate() {
        symbols.record_class_field_slot(mangled.clone(), name.clone(), i as u32);
    }
    for (name, check) in &narrowing_checks {
        symbols.record_class_field_narrowing_check(mangled.clone(), name.clone(), check.clone());
    }
    for (i, slot) in slots.iter().enumerate() {
        symbols.record_class_method_slot(
            mangled.clone(),
            slot.name.clone(),
            crate::codegen::classes::VTABLE_METHOD_SLOT_BASE + i as u32,
        );
    }

    symbols
        .class_type_parameters
        .insert(mangled.clone(), class.generics.to_vec());
    symbols.record_class_guard_layout(
        mangled.clone(),
        class.extends.as_ref(),
        (fields.len() + slots.iter().filter(|slot| !slot.generic).count()) as u32,
        (!class.generics.is_empty()
            || !narrowing_checks.is_empty()
            || class
                .extends
                .as_ref()
                .is_some_and(|parent| symbols.class_guard_layout(parent).has_instance_guards))
            && class.supports_instance_guards,
    );

    // 4. Import the constructor, ctor-init, and own method bodies.
    let ctor_params: Vec<ValType> = class
        .constructor
        .iter()
        .map(|p| symbols.slot_value_type(&p.ty))
        .collect::<Result<_, _>>()?;
    let ctor_param_tys: Vec<crate::Type> = class.constructor.iter().map(|p| p.ty.clone()).collect();
    symbols.record_class_ctor_abi(mangled.clone(), ctor_params.clone());
    let class_ref = crate::Type::class_ref(
        crate::Package(class.package.to_string()),
        class.name.to_string(),
        mangled.clone(),
        Vec::new(),
    );

    // Entry constructor: `(params) -> (ref $C)`.
    let mut entry_params = ctor_params.clone();
    if symbols.class_guard_layout(mangled).has_instance_guards {
        entry_params.push(ref_to(intrinsics.object_fields));
    }
    types.ty().function(entry_params, [ref_to(struct_idx)]);
    let ctor_sig = take(next_type_idx);
    let ctor_mangled = crate::mangle::extend(mangled, "constructor");
    imports.import(
        host_module_for(class),
        ctor_mangled.as_str(),
        EntityType::Function(ctor_sig),
    );
    let ctor_idx = take(next_func_idx);
    symbols.record_imported_fn(ctor_mangled, ctor_idx, ctor_param_tys, class_ref, false);

    // Self-first ctor-init: `((ref $Object), params...) -> ()`.
    let mut init_params = vec![ref_to(intrinsics.object)];
    init_params.extend(ctor_params);
    types.ty().function(init_params, Vec::<ValType>::new());
    let init_sig = take(next_type_idx);
    let init_mangled = crate::mangle::extend(mangled, "constructor_init");
    imports.import(
        host_module_for(class),
        init_mangled.as_str(),
        EntityType::Function(init_sig),
    );
    let init_idx = take(next_func_idx);
    symbols.record_class_ctor_init_func(mangled.clone(), init_idx);

    // Own method bodies — reuse the slot's `((ref $Object), params...) -> ret`
    // sig. A local subclass's vtable global `ref.func`s the inherited ones.
    for name in methods.keys() {
        let sig = symbols
            .class_method_sig(mangled, name)
            .expect("own method sig recorded");
        let method_mangled = crate::mangle::extend(mangled, name);
        imports.import(
            host_module_for(class),
            method_mangled.as_str(),
            EntityType::Function(sig),
        );
        let idx = take(next_func_idx);
        symbols.record_class_method_func(mangled.clone(), name.clone(), idx);
    }

    let ctor_typed_params = class
        .constructor
        .iter()
        .map(|p| crate::TypedParam {
            name: crate::Ident {
                name: p.name.clone(),
                span: crate::Span::at(file),
            },
            ty: p.ty.clone(),
            boxed: false,
            rest: p.rest,
            default: p.default.clone(),
        })
        .collect();

    let mut private_members = class
        .extends
        .as_ref()
        .and_then(|parent| layouts.get(parent))
        .map(|layout| layout.private_members.clone())
        .unwrap_or_default();
    for (name, field) in class.fields {
        if field.visibility == crate::Visibility::Private {
            private_members.insert(name.clone());
        }
    }
    for accessor in class.accessors {
        if class
            .fields
            .get(accessor.name())
            .is_some_and(|field| field.visibility == crate::Visibility::Private)
        {
            private_members.insert(super::classes::accessor_getter_name(accessor.name()));
            private_members.insert(super::classes::accessor_setter_name(accessor.name()));
        }
    }
    Ok(ImportedClassLayout {
        private_members,
        fields,
        optional_fields,
        narrowing_checks,
        methods: slots,
        ctor_params: ctor_typed_params,
    })
}

/// Walk the `extends` chain to find the class that originates `method`, returning
/// the sig recorded under it. Used only as a fallback for inherited slots.
fn ancestor_sig(
    info: &BTreeMap<MangledName, ClassInfo<'_>>,
    symbols: &SymbolTable,
    class: &ClassInfo<'_>,
    method: &str,
) -> Option<u32> {
    let mut cur = class.extends.clone();
    while let Some(m) = cur {
        if let Some(sig) = symbols.class_method_sig(&m, method) {
            return Some(sig);
        }
        cur = info.get(&m).and_then(|c| c.extends.clone());
    }
    None
}

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
}

fn fieldtype_ref(idx: u32) -> FieldType {
    FieldType {
        element_type: StorageType::Val(ref_to(idx)),
        mutable: false,
    }
}

fn fieldtype_ref_null(idx: u32) -> FieldType {
    FieldType {
        element_type: StorageType::Val(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(idx),
        })),
        mutable: false,
    }
}

fn substruct(fields: Vec<FieldType>, supertype: Option<u32>) -> SubType {
    SubType {
        is_final: false,
        supertype_idx: supertype,
        composite_type: wasm_encoder::CompositeType {
            inner: wasm_encoder::CompositeInnerType::Struct(StructType {
                fields: fields.into_boxed_slice(),
            }),
            shared: false,
            descriptor: None,
            describes: None,
        },
    }
}

fn take(counter: &mut u32) -> u32 {
    let v = *counter;
    *counter += 1;
    v
}
