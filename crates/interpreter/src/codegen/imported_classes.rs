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

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    EntityType, FieldType, GlobalType, HeapType, ImportSection, RefType, StorageType, StructType,
    SubType, TypeSection, ValType,
};

use crate::codegen::classes::{
    OwnSlotCandidate, VtableSlot, extend_vtable_slots, method_vtable_slot, shadows_inherited_field,
};
use crate::codegen::dependency_usage::DependencyUsage;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;
use crate::codegen::{internal_failure, next_index, wasm_u32};
use crate::compiler_error::CompilerFailure;
use crate::{MangledName, Param, TypeKind, TypedAst, TypedTypeDecl};

/// The resolved full layout of an imported class, handed to [`super::classes`]
/// so a local subclass extending it lays its own fields/methods out after the
/// inherited prefix.
pub struct ImportedClassLayout {
    pub private_members: BTreeSet<String>,
    /// Full data-field names (inherited prefix then own), object-payload order.
    pub fields: Vec<String>,
    pub optional_fields: std::collections::BTreeSet<String>,
    pub over_prototype_fields: std::collections::BTreeSet<String>,
    /// Data fields implementing an inherited `abstract_property`, allocated
    /// absent so base constructors do not see them before subclass field setup.
    pub starts_absent_fields: std::collections::BTreeSet<String>,
    /// Unimplemented abstract properties on this class or its ancestors.
    pub abstract_fields: std::collections::BTreeSet<String>,
    pub narrowing_checks: BTreeMap<String, crate::FieldNarrowingCheck>,
    /// Active methods/accessors (inherited prefix then own/override), one per
    /// `(name, accessor)`.
    pub methods: Vec<ImportedSlot>,
    /// Physical vtable entries in `$C_vtable` field order.
    pub vtable_slots: Vec<VtableSlot>,
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
    pub accessor: Option<crate::AccessorKind>,
    pub absent: bool,
}

/// Per-class data gathered from the dependency `PackageDeclaration`s. `fields`
/// and `methods` are `BTreeMap`s, so iterating their keys yields the canonical
/// (sorted) own-member order the producer assigns slots in — no order metadata
/// has to cross the package boundary.
struct ClassInfo<'a> {
    generics: &'a [String],
    package: &'a str,
    compiled_class: bool,
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
/// `(name, params, ret, accessor)`. Mirrors `codegen::classes::accessor_methods`
/// so slot indices and signatures line up across the package boundary.
fn vtable_methods(
    class: &ClassInfo<'_>,
) -> BTreeMap<String, (Vec<crate::Type>, crate::Type, Option<crate::AccessorKind>)> {
    let mut out: BTreeMap<String, (Vec<crate::Type>, crate::Type, Option<crate::AccessorKind>)> =
        class
            .methods
            .iter()
            .map(|(name, sig)| {
                (
                    name.clone(),
                    (
                        sig.params.iter().map(|p| p.ty.clone()).collect(),
                        sig.ret.clone(),
                        None,
                    ),
                )
            })
            .collect();
    for acc in class.accessors {
        match acc {
            crate::AccessorSig::Getter { name, ret_ty } => {
                out.insert(
                    crate::codegen::classes::accessor_getter_name(name),
                    (Vec::new(), ret_ty.clone(), Some(crate::AccessorKind::Get)),
                );
            }
            crate::AccessorSig::Setter { name, param } => {
                out.insert(
                    crate::codegen::classes::accessor_setter_name(name),
                    (
                        vec![param.ty.clone()],
                        crate::Type::Void,
                        Some(crate::AccessorKind::Set),
                    ),
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
    let order = needed_in_topo_order(&info, ta, usage)?;

    let mut layouts: BTreeMap<MangledName, ImportedClassLayout> = BTreeMap::new();
    for mangled in &order {
        let class = class_info(&info, mangled)?;
        let layout = reconstruct_one(
            mangled,
            class,
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
            class_info(&info, mangled)?,
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
        super::check_function_parameters(
            || {
                format!(
                    "the static method `{}.{name}` of package `{}`",
                    class.name, class.package
                )
            },
            &param_types,
        )?;
        let results = symbols.wasm_result(&sig.ret)?;
        let sig_idx = next_index(next_type_idx)?;
        types.ty().function(param_types, results);
        imports.import(
            host_module_for(class),
            key.as_str(),
            EntityType::Function(sig_idx),
        );
        let idx = next_index(next_func_idx)?;
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
        symbols.record_typed_global(key, next_index(next_global_idx)?, field.ty.clone());
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
                        compiled_class: defs.runtime_types.contains_key(sym.mangled_name.as_str()),
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
) -> Result<Vec<MangledName>, CompilerFailure> {
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
            && let Some(parent) = c.parent()
            && info.contains_key(parent)
        {
            roots.insert(parent.clone());
        }
    }

    let mut order: Vec<MangledName> = Vec::new();
    let mut seen: BTreeSet<&MangledName> = BTreeSet::new();
    for root in &roots {
        push_ancestors_first(info, root, &mut seen, &mut order)?;
    }
    Ok(order)
}

/// Appends `root` and its not-yet-ordered ancestors, parents first. Unlike a
/// local class, an imported class's ancestors must all be declared: without
/// one, the subclass would be reconstructed without its field and slot prefix
/// and fail to link as `imported global type mismatch`, naming nothing.
fn push_ancestors_first<'a>(
    info: &'a BTreeMap<MangledName, ClassInfo<'_>>,
    root: &'a MangledName,
    seen: &mut BTreeSet<&'a MangledName>,
    order: &mut Vec<MangledName>,
) -> Result<(), CompilerFailure> {
    let mut chain: Vec<&MangledName> = Vec::new();
    let mut next = Some(root);
    while let Some(mangled) = next {
        if seen.contains(mangled) {
            break;
        }
        let class = info.get(mangled).ok_or_else(|| {
            internal_failure(format!(
                "the dependency closure passed to codegen has no declaration for \
                 `{mangled}`, an ancestor of imported class `{root}`"
            ))
        })?;
        if chain.contains(&mangled) {
            return Err(internal_failure(format!(
                "imported class `{mangled}` inherits from itself"
            )));
        }
        chain.push(mangled);
        next = class.extends.as_ref();
    }
    for mangled in chain.into_iter().rev() {
        seen.insert(mangled);
        order.push(mangled.clone());
    }
    Ok(())
}

fn class_info<'i, 'a>(
    info: &'i BTreeMap<MangledName, ClassInfo<'a>>,
    mangled: &MangledName,
) -> Result<&'i ClassInfo<'a>, CompilerFailure> {
    info.get(mangled).ok_or_else(|| {
        internal_failure(format!(
            "imported class `{mangled}` has no reconstructed declaration"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
fn reconstruct_one(
    mangled: &MangledName,
    class: &ClassInfo<'_>,
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
    // Parents are reconstructed first (`needed_in_topo_order`).
    let parent_layout = match &class.extends {
        Some(parent) => Some(layouts.get(parent).ok_or_else(|| {
            internal_failure(format!(
                "imported class `{mangled}` was reconstructed before its parent `{parent}`"
            ))
        })?),
        None => None,
    };
    // Full field list = inherited prefix (already reconstructed) + own *data*
    // fields. Accessor properties and abstract properties are in `fields` for
    // typing/docs but back no data slot on this class, so they're excluded —
    // matching the producer's payload layout.
    let mut fields: Vec<String> = parent_layout.map(|l| l.fields.clone()).unwrap_or_default();
    let mut optional_fields = parent_layout
        .map(|layout| layout.optional_fields.clone())
        .unwrap_or_default();
    let mut over_prototype_fields = parent_layout
        .map(|layout| layout.over_prototype_fields.clone())
        .unwrap_or_default();
    let mut starts_absent_fields = parent_layout
        .map(|layout| layout.starts_absent_fields.clone())
        .unwrap_or_default();
    let mut abstract_fields = parent_layout
        .map(|layout| layout.abstract_fields.clone())
        .unwrap_or_default();
    let mut narrowing_checks = parent_layout
        .map(|layout| layout.narrowing_checks.clone())
        .unwrap_or_default();
    for (name, field) in class.fields {
        if class.accessors.iter().any(|a| a.name() == name.as_str()) {
            continue;
        }
        if field.abstract_property {
            if !shadows_inherited_field(fields.iter().map(String::as_str), name) {
                abstract_fields.insert(name.clone());
            }
            continue;
        }
        let implements_abstract = abstract_fields.remove(name);
        if !shadows_inherited_field(fields.iter().map(String::as_str), name) {
            fields.push(name.clone());
        }
        if implements_abstract {
            starts_absent_fields.insert(name.clone());
        }
        if field.optional {
            optional_fields.insert(name.clone());
        } else {
            optional_fields.remove(name);
        }
        let shadows_parent = parent_layout.is_some_and(|parent| {
            crate::codegen::classes::has_prototype_member(
                parent
                    .methods
                    .iter()
                    .map(|slot| (slot.name.as_str(), slot.accessor)),
                name,
            )
        });
        if field.over_prototype || (!field.method && shadows_parent) {
            over_prototype_fields.insert(name.clone());
        }
    }
    narrowing_checks.extend(class.narrowing_checks.clone());
    for acc in class.accessors {
        if abstract_fields.remove(acc.name()) {
            narrowing_checks.remove(acc.name());
        }
    }
    for method in class.methods.keys() {
        if abstract_fields.remove(method) {
            narrowing_checks.remove(method);
        }
    }

    // The producer's full vtable-method set (real methods + accessor getter/setter),
    // sorted by name to match the producer's slot assignment.
    let methods = vtable_methods(class);

    // Active methods/accessors = parent's slots, then fold in own methods
    // (override → same slot/owner-of-body, new → appended owned by this class).
    let mut slots: Vec<ImportedSlot> = parent_layout
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
                    accessor: s.accessor,
                    absent: s.absent,
                })
                .collect()
        })
        .unwrap_or_default();
    for (name, (param_tys, ret_ty, accessor)) in &methods {
        let argument_metadata = match class.methods.get(name) {
            Some(sig) if accessor.is_none() => {
                super::call_arguments::metadata(sig.params.iter().map(|param| {
                    (
                        param
                            .default
                            .as_ref()
                            .or(param.optional.then_some(&crate::DefaultValue::Undefined)),
                        param.rest,
                    )
                }))
            }
            _ => None,
        };
        let generic = accessor.is_none()
            && class
                .methods
                .get(name)
                .is_some_and(|sig| !sig.generics.is_empty());
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| &s.name == name && s.accessor == *accessor)
        {
            // Override: this class supplies the body, so the slot must describe
            // it rather than the ancestor's.
            slot.owner = mangled.clone();
            slot.param_tys = param_tys.clone();
            slot.argument_metadata = argument_metadata;
            slot.ret_ty = ret_ty.clone();
            slot.generic |= generic;
            slot.absent = false;
        } else {
            slots.push(ImportedSlot {
                name: name.clone(),
                owner: mangled.clone(),
                param_tys: param_tys.clone(),
                argument_metadata,
                ret_ty: ret_ty.clone(),
                generic,
                accessor: *accessor,
                absent: false,
            });
        }
    }
    // Mirror the producer's single-half accessor rule: declaring one half
    // replaces the whole prototype descriptor, clearing any inherited other half.
    let missing_halves = crate::codegen::classes::unpaired_accessor_halves(
        class.accessors.iter().map(|acc| match acc {
            crate::AccessorSig::Getter { name, .. } => (name.as_str(), crate::AccessorKind::Get),
            crate::AccessorSig::Setter { name, .. } => (name.as_str(), crate::AccessorKind::Set),
        }),
    );
    let mut cleared_own_methods: Vec<(String, crate::AccessorKind)> = Vec::new();
    for (prop, missing) in missing_halves {
        let slot_name = match missing {
            crate::AccessorKind::Get => crate::codegen::classes::accessor_getter_name(prop),
            crate::AccessorKind::Set => crate::codegen::classes::accessor_setter_name(prop),
        };
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.accessor == Some(missing) && s.name == slot_name)
        {
            slot.owner = mangled.clone();
            slot.absent = true;
            cleared_own_methods.push((slot_name, missing));
        }
    }

    // 1. Resolve physical vtable slots (inheriting from `parent_layout`) and
    //    emit standalone fn-type sigs for originating methods and convention-
    //    changing overrides, matching `classes::ClassPlan::reserve_and_emit_types`.
    let mut vtable_slots: Vec<VtableSlot> = parent_layout
        .map(|l| l.vtable_slots.clone())
        .unwrap_or_default();
    let mut own_candidates: Vec<OwnSlotCandidate<'_>> = class
        .methods
        .iter()
        .map(|(name, sig)| OwnSlotCandidate {
            name: Cow::Borrowed(name.as_str()),
            accessor: None,
            arity: sig.params.len(),
            has_rest: sig.params.last().is_some_and(|p| p.rest),
            ret_ty: &sig.ret,
            generic: !sig.generics.is_empty(),
        })
        .chain(class.accessors.iter().map(|acc| match acc {
            crate::AccessorSig::Getter { name, ret_ty } => OwnSlotCandidate {
                name: Cow::Owned(crate::codegen::classes::accessor_getter_name(name)),
                accessor: Some(crate::AccessorKind::Get),
                arity: 0,
                has_rest: false,
                ret_ty,
                generic: false,
            },
            crate::AccessorSig::Setter { name, .. } => OwnSlotCandidate {
                name: Cow::Owned(crate::codegen::classes::accessor_setter_name(name)),
                accessor: Some(crate::AccessorKind::Set),
                arity: 1,
                has_rest: false,
                ret_ty: &crate::Type::Void,
                generic: false,
            },
        }))
        .chain(
            cleared_own_methods
                .iter()
                .map(|(name, missing)| OwnSlotCandidate {
                    name: Cow::Borrowed(name.as_str()),
                    accessor: Some(*missing),
                    arity: usize::from(*missing == crate::AccessorKind::Set),
                    has_rest: false,
                    ret_ty: if *missing == crate::AccessorKind::Get {
                        &crate::Type::Unknown
                    } else {
                        &crate::Type::Void
                    },
                    generic: false,
                }),
        )
        .collect();
    own_candidates
        .sort_by(|a, b| (a.name.as_ref(), a.accessor).cmp(&(b.name.as_ref(), b.accessor)));
    extend_vtable_slots(
        &mut vtable_slots,
        &own_candidates,
        intrinsics,
        symbols,
        types,
        next_type_idx,
        |name| {
            format!(
                "the method `{}#{name}` of package `{}`",
                class.name, class.package
            )
        },
    )?;
    for vslot in vtable_slots.iter().filter(|s| s.active) {
        symbols.record_class_method_sig(mangled.clone(), vslot.name.clone(), vslot.sig_idx);
        symbols.record_class_method_abi(mangled.clone(), vslot.name.clone(), vslot.abi.clone());
    }

    // 2. Reserve the (vtable, struct) pair and 3. emit the rec group.
    let vtable_idx = next_index(next_type_idx)?;
    let struct_idx = next_index(next_type_idx)?;
    symbols.record_class_vtable_type(mangled.clone(), vtable_idx);
    symbols.record_class_struct_type(mangled.clone(), struct_idx);

    // Parents are reconstructed first, so a missing parent type is a broken
    // order, not a root class.
    let (vtable_super, struct_super) = match &class.extends {
        Some(parent) => (
            symbols.class_vtable_type_idx(parent),
            symbols.class_struct_type_idx(parent),
        ),
        None => (Some(intrinsics.class_vtable), Some(intrinsics.object_shape)),
    };
    let (Some(vtable_super), Some(struct_super)) = (vtable_super, struct_super) else {
        return Err(internal_failure(
            "an imported class's parent types were not reconstructed first",
        ));
    };
    symbols.record_struct_supertype(vtable_idx, vtable_super);
    symbols.record_struct_supertype(struct_idx, struct_super);

    let mut vtable_fields: Vec<FieldType> = vec![
        fieldtype_ref(intrinsics.to_string_fn),
        fieldtype_ref(intrinsics.to_json_fn),
        fieldtype_ref(intrinsics.equals_fn),
        fieldtype_ref(intrinsics.hash_fn),
        fieldtype_ref_null(intrinsics.class_vtable),
        FieldType {
            element_type: StorageType::Val(ValType::I32),
            mutable: false,
        },
    ];
    for vslot in &vtable_slots {
        vtable_fields.push(fieldtype_ref(vslot.sig_idx));
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
            FieldType {
                element_type: StorageType::Val(ValType::Ref(RefType::ANYREF)),
                mutable: true,
            },
            FieldType {
                element_type: StorageType::Val(ValType::I64),
                mutable: true,
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
    symbols.record_class_vtable_global(mangled.clone(), next_index(next_global_idx)?);

    // Record field payload slots and method vtable slots (universal slots +
    // parent link precede).
    for (i, name) in fields.iter().enumerate() {
        symbols.record_class_field_slot(mangled.clone(), name.clone(), wasm_u32(i)?);
    }
    let mut own_abstract_guards = Vec::new();
    for (name, check) in &narrowing_checks {
        symbols.record_class_field_narrowing_check(mangled.clone(), name.clone(), check.clone());
        if abstract_fields.contains(name) && check.declaration.as_ref() == Some(mangled) {
            own_abstract_guards.push(name.clone());
        }
    }
    symbols.record_class_abstract_guard_slots(mangled.clone(), own_abstract_guards);
    for (i, vslot) in vtable_slots.iter().enumerate() {
        if !vslot.active {
            continue;
        }
        // Native classes keep their methods in the vtable, without the mutable
        // closure payload emitted by the compiler for guest class instances.
        if class.compiled_class && vslot.accessor.is_none() {
            symbols.record_class_payload_method(mangled.clone(), vslot.name.clone());
        }
        symbols.record_class_method_slot(
            mangled.clone(),
            vslot.name.clone(),
            method_vtable_slot(i)?,
        );
    }

    symbols
        .class_type_parameters
        .insert(mangled.clone(), class.generics.to_vec());
    let parent_guarded = match &class.extends {
        Some(parent) => {
            symbols
                .recorded_class_guard_layout(parent)?
                .has_instance_guards
        }
        None => false,
    };
    let payload_slots = slots.iter().filter(|slot| !slot.absent).count();
    let named_payload_len = fields
        .len()
        .checked_add(payload_slots)
        .ok_or_else(|| internal_failure("an imported class payload is too large"))?;
    symbols.record_class_guard_layout(
        mangled.clone(),
        class.extends.as_ref(),
        wasm_u32(named_payload_len)?,
        (!class.generics.is_empty() || !narrowing_checks.is_empty() || parent_guarded)
            && class.compiled_class,
    )?;

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
    if symbols
        .recorded_class_guard_layout(mangled)?
        .has_instance_guards
    {
        entry_params.push(ref_to(intrinsics.object_fields));
    }
    let ctor_sig = next_index(next_type_idx)?;
    types.ty().function(entry_params, [ref_to(struct_idx)]);
    let ctor_mangled = crate::mangle::extend(mangled, "constructor");
    imports.import(
        host_module_for(class),
        ctor_mangled.as_str(),
        EntityType::Function(ctor_sig),
    );
    let ctor_idx = next_index(next_func_idx)?;
    symbols.record_imported_fn(ctor_mangled, ctor_idx, ctor_param_tys, class_ref, false);
    // A host constructor can't fill its own defaults, so an argument that is
    // `undefined` takes the declared default at the call, as a host method's does.
    let host_defaults = super::is_host_package(class.package).then(|| {
        class
            .constructor
            .iter()
            .map(|param| param.default.clone())
            .collect::<Vec<_>>()
    });
    if let Some(defaults) = &host_defaults {
        symbols.record_host_parameter_defaults(ctor_idx, defaults.clone());
    }

    // Self-first ctor-init: `((ref $Object), params...) -> ()`. It takes the
    // receiver beside the declared parameters, and the entry function at
    // most one guard argument, so checking it covers both.
    let declared = ctor_params.len();
    let mut init_params = vec![ref_to(intrinsics.object)];
    init_params.extend(ctor_params);
    super::check_function_parameters(
        || {
            format!(
                "the constructor of `{}` of package `{}` ({declared} declared parameters and its receiver)",
                class.name, class.package
            )
        },
        &init_params,
    )?;
    let init_sig = next_index(next_type_idx)?;
    types.ty().function(init_params, Vec::<ValType>::new());
    let init_mangled = crate::mangle::extend(mangled, "constructor_init");
    imports.import(
        host_module_for(class),
        init_mangled.as_str(),
        EntityType::Function(init_sig),
    );
    let init_idx = next_index(next_func_idx)?;
    symbols.record_class_ctor_init_func(mangled.clone(), init_idx);
    if let Some(defaults) = host_defaults {
        symbols.record_host_parameter_defaults(
            init_idx,
            std::iter::once(None).chain(defaults).collect(),
        );
    }

    // Own method bodies — reuse the slot's `((ref $Object), params...) -> ret`
    // sig. A local subclass's vtable global `ref.func`s the inherited ones.
    for name in methods
        .keys()
        .chain(cleared_own_methods.iter().map(|(name, _)| name))
    {
        let sig = symbols
            .class_method_sig(mangled, name)
            .ok_or_else(|| internal_failure("an imported class's own method has no signature"))?;
        let method_mangled = crate::mangle::extend(mangled, name);
        imports.import(
            host_module_for(class),
            method_mangled.as_str(),
            EntityType::Function(sig),
        );
        let idx = next_index(next_func_idx)?;
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
            optional: p.optional,
            boxed: false,
            rest: p.rest,
            default: p.default.clone(),
        })
        .collect();

    let mut private_members = parent_layout
        .map(|layout| layout.private_members.clone())
        .unwrap_or_default();
    for (name, field) in class.fields {
        if !field.abstract_property && field.visibility == crate::Visibility::Private {
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
        over_prototype_fields,
        starts_absent_fields,
        abstract_fields,
        narrowing_checks,
        methods: slots,
        vtable_slots,
        ctor_params: ctor_typed_params,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{PackageSourceModule, compile_package_checked};
    use crate::compiler_error::CompilerFailure;

    fn declaration() -> crate::PackageDeclaration {
        let modules = [PackageSourceModule {
            path: crate::ModulePath::from("lib"),
            source: "export class A { x: number = 1 } export class B extends A {}",
        }];
        compile_package_checked("dep", crate::ModulePath::from("lib"), &modules, &[])
            .unwrap()
            .declaration
    }

    /// A local class extending the imported `B`, so B and its ancestors are needed.
    fn subclass_of(parent: &MangledName) -> TypedAst {
        let (mut ta, diagnostics) = crate::compile::typecheck_to_typed_ast(
            "class C {} function main(): void {}",
            crate::FileId(0),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        for decl in &mut ta.types {
            if let TypedTypeDecl::Class(class) = decl {
                class.extends = Some(crate::ClassExtends::plain(parent.clone()));
            }
        }
        ta
    }

    fn mangled(declaration: &crate::PackageDeclaration, name: &str) -> MangledName {
        declaration
            .runtime_types
            .values()
            .chain(declaration.types.values())
            .find(|symbol| symbol.name == name)
            .unwrap()
            .mangled_name
            .clone()
    }

    #[test]
    fn incomplete_or_cyclic_dependency_closures_are_internal_failures() {
        let declaration = declaration();
        let a = mangled(&declaration, "A");
        let b = mangled(&declaration, "B");
        let ta = subclass_of(&b);
        let usage = DependencyUsage::empty();

        let dependencies = [&declaration];
        let info = collect_class_info(&dependencies);
        assert_eq!(
            needed_in_topo_order(&info, &ta, &usage).unwrap(),
            [a.clone(), b.clone()]
        );

        // A missing ancestor fails rather than reconstructing a subclass
        // without its prefix, which would link as a type mismatch.
        let mut incomplete = declaration.clone();
        incomplete
            .runtime_types
            .retain(|_, symbol| symbol.mangled_name != a);
        incomplete
            .types
            .retain(|_, symbol| symbol.mangled_name != a);
        let dependencies = [&incomplete];
        let info = collect_class_info(&dependencies);
        assert!(matches!(
            needed_in_topo_order(&info, &ta, &usage),
            Err(CompilerFailure::Internal { .. })
        ));

        let mut cyclic = declaration;
        for symbol in cyclic
            .runtime_types
            .values_mut()
            .chain(cyclic.types.values_mut())
        {
            if symbol.mangled_name == a
                && let TypeKind::Class { extends, .. } = &mut symbol.kind
            {
                *extends = Some(crate::ClassExtends {
                    parent: b.clone(),
                    args: Vec::new(),
                });
            }
        }
        let dependencies = [&cyclic];
        let info = collect_class_info(&dependencies);
        assert!(matches!(
            needed_in_topo_order(&info, &ta, &usage),
            Err(CompilerFailure::Internal { .. })
        ));
    }
}
