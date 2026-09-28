//! WasmGC type emission and body lowering for user `class` declarations.
//!
//! Each class becomes one rec group holding a struct type
//! `$Foo (sub $ObjectShape …)` and a vtable type `$Foo_vtable (sub $VTable …)`,
//! with subclasses subtyping their parent's pair. Alongside those: the
//! per-class singleton globals (vtable instance, field-names array), the
//! allocating constructor and its self-first init counterpart, real method
//! bodies, one closure-ABI adapter per non-generic vtable *slot* for
//! interface-typed dispatch (inherited slots included — a subclass reuses its
//! ancestor's adapter where one exists locally),
//! and the four universal vtable bodies (`toString`/`toJson`/`equals`/`hash`).
//!
//! Data fields live in the shared object-fields payload rather than as named
//! struct fields, so a class's struct only ever references its own vtable and
//! the intrinsics — which is what lets a cross-package consumer rebuild just
//! the classes it uses (see [`super::imported_classes`]).

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    BlockType, CodeSection, CompositeInnerType, CompositeType, ConstExpr, FieldType, Function,
    FunctionSection, GlobalSection, GlobalType, HeapType, Instruction, RefType, StorageType,
    StructType, SubType, TypeSection, ValType,
};

use crate::codegen::CodegenCtx;
use crate::codegen::closures::ClosureSig;
use crate::codegen::function_emitter::FunctionEmitter;
use crate::codegen::imported_classes::ImportedClassLayout;
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::{FieldSetup, MethodSlotAbi, SymbolTable};
use crate::{MangledName, TypedAst, TypedClassDecl, TypedTypeDecl};

/// The four universal vtable slots (toString/toJson/equals/hash) every vtable
/// carries, each backed by a shared stub function.
pub(crate) const UNIVERSAL_STUB_COUNT: usize = 4;
/// `$ClassVTable` field index of the nominal-identity parent link.
pub(crate) const VTABLE_PARENT_SLOT: u32 = UNIVERSAL_STUB_COUNT as u32;
/// Class-vtable fields preceding method slots: the universal funcrefs plus the
/// parent link.
pub(crate) const VTABLE_METHOD_SLOT_BASE: u32 = VTABLE_PARENT_SLOT + 1;

/// One method slot in a class vtable. `name` is the method name; `owner` is the
/// most-derived class that supplies the body (the class itself for an override,
/// else the nearest ancestor); `sig_idx` is the Wasm fn-type the slot holds
/// (allocated by the base declarer and reused by overrides).
struct MethodSlot {
    name: String,
    owner: MangledName,
    sig_idx: u32,
    /// Declared parameter/return types of the body backing this slot (the
    /// *owner's*). Needed to pick the closure signature for the slot's payload
    /// adapter; its body uses the recorded ABI for physical representations.
    param_tys: Vec<crate::Type>,
    argument_metadata: Option<String>,
    ret_ty: crate::Type,
    /// A generic method has no adapter and never enters the instance payload.
    generic: bool,
}

struct ClassLayout {
    private_members: BTreeSet<String>,
    generics: Vec<String>,
    mangled: MangledName,
    name: String,
    parent: Option<MangledName>,
    /// Full field list (inherited prefix then own), in object payload order.
    fields: Vec<FieldLayout>,
    /// Full method slot list (inherited prefix then own/new), in vtable order.
    methods: Vec<MethodSlot>,
    /// Methods declared on *this* class (own or override) — each gets a real body.
    own_methods: Vec<OwnMethod>,
    /// Indices into `methods` whose payload adapter *this* class emits. A slot
    /// inherited from a local ancestor reuses that ancestor's adapter (topo
    /// order guarantees it exists); an imported owner has no adapter in this
    /// module, so one is emitted here.
    adapter_slots: Vec<usize>,
    /// `this`-class's own constructor params + body. `None` body = no declared
    /// constructor (synthesize a default that just allocates + returns).
    ctor_params: Vec<crate::TypedParam>,
    ctor_body: Option<crate::StmtId>,
    /// Parameter-property copies then own-field initializers, in execution order.
    /// Emitted at the post-`super()` field-setup point — after the parent is
    /// initialized, before the rest of the constructor body.
    field_setup: Vec<FieldSetup>,
    /// Accessor property names (own). These are not data fields, so they need a
    /// per-name `$string` global for the runtime property scan even though they
    /// never appear in any shape's field-names array.
    accessor_names: Vec<String>,
    struct_type_idx: u32,
    vtable_type_idx: u32,
    ctor_func_idx: u32,
    ctor_sig_idx: u32,
    /// Constructor *init* fn: self-first body that initializes fields on an
    /// already-allocated instance (`super(...)` calls it; the entry constructor
    /// delegates to it after allocation).
    ctor_init_func_idx: u32,
    ctor_init_sig_idx: u32,
    getter_func_idx: u32,
    setter_func_idx: u32,
    /// Per-class `equals` body (vtable universal slot 2): nominal exact-class
    /// guard + structural compare of the data-field payload slots.
    equals_func_idx: u32,
    /// Per-class universal slot 0 body: a user `toString(): string` method
    /// thunk, else the `"[object Object]"` default.
    to_string_func_idx: u32,
    /// Per-class universal slot 1 body: a user `toJson(): string` method
    /// thunk, else data-field JSON in canonical sorted-key order.
    to_json_func_idx: u32,
    /// Per-class universal slot 3 body: FNV-1a over the data-field payload
    /// slots, consistent with the `equals` contract.
    hash_func_idx: u32,
}

impl ClassLayout {
    /// The non-generic vtable slots that enter the instance payload, in slot
    /// order — every method in the inheritance chain, not just the declared
    /// ones. An interface-typed receiver resolves a method by scanning this
    /// payload, and `implements` conformance may be satisfied by an inherited
    /// member, so a subclass instance has to carry the whole chain.
    fn payload_slots(&self) -> impl Iterator<Item = &MethodSlot> {
        self.methods.iter().filter(|s| !s.generic)
    }

    /// Instance payload names: the full data fields (inherited prefix then own,
    /// folded in by [`ClassPlan::collect`]) followed by the payload method
    /// names. Single source of truth for the payload built in
    /// [`ClassPlan::emit_ctor_body`] and the field-names global that indexes it.
    fn payload_field_names(&self) -> Vec<super::field_names::FieldName> {
        let mut names: Vec<_> = self
            .fields
            .iter()
            .map(|f| super::field_names::FieldName {
                name: f.name.clone(),
                optional: f.optional,
                is_accessor: false,
                is_private: self.private_members.contains(&f.name),
            })
            .collect();
        names.extend(self.payload_slots().map(|s| super::field_names::FieldName {
            name: s.name.clone(),
            optional: false,
            is_accessor: s.name.starts_with("get ") || s.name.starts_with("set "),
            is_private: self.private_members.contains(&s.name),
        }));
        names
    }
}

#[derive(Clone)]
struct FieldLayout {
    name: String,
    optional: bool,
    /// Inherited along the layout prefix, so a subclass that does not redeclare
    /// still reads at the narrowed type. See [`crate::FieldNarrowingCheck`].
    narrowing_check: Option<Box<crate::FieldNarrowingCheck>>,
}

/// A method declared on the class itself (own or override) — gets a real Wasm body.
#[derive(Clone)]
struct OwnMethod {
    name: String,
    params: Vec<crate::TypedParam>,
    body: crate::StmtId,
    return_type: crate::Type,
    /// A method declaring its own type parameters is rejected at bind time; the
    /// flag only exists so codegen can emit an unreachable stub for its slot.
    generic: bool,
}

/// The class-emission plan, computed once and threaded through the codegen phases.
pub struct ClassPlan {
    package_name: String,
    classes: Vec<ClassLayout>,
    /// Shared `unreachable` stubs for the universal vtable slots. No vtable
    /// references them anymore (every class emits its own four real bodies);
    /// they're kept only so function numbering stays stable.
    universal_stubs: [u32; UNIVERSAL_STUB_COUNT],
}

impl ClassPlan {
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Phase 0: gather class declarations from the typed AST, topologically
    /// ordered by `extends` (parents before children), and resolve each class's
    /// full field + method slot layout across the inheritance chain.
    pub fn collect(
        ta: &TypedAst,
        imported: &BTreeMap<MangledName, ImportedClassLayout>,
    ) -> Result<Self, crate::compiler_error::CompilerFailure> {
        let by_mangled: BTreeMap<&MangledName, &TypedClassDecl> = ta
            .types
            .iter()
            .filter_map(|d| match d {
                TypedTypeDecl::Class(c) => Some((&c.mangled_name, c)),
                _ => None,
            })
            .collect();

        let order = topo_order(&by_mangled);
        let mut private_members: BTreeMap<MangledName, BTreeSet<String>> = imported
            .iter()
            .map(|(name, layout)| (name.clone(), layout.private_members.clone()))
            .collect();

        // Build layouts in topo order so a parent's resolved layout is available
        // when its child is processed.
        let mut layouts: BTreeMap<MangledName, ResolvedLayout> = BTreeMap::new();
        let mut ordered: Vec<MangledName> = Vec::new();
        // Which of each class's data-field slots came from its parent prefix —
        // read back below to decide which own fields need a construction-time
        // reset (see [`field_setup_steps`]).
        let mut inherited_fields: BTreeMap<MangledName, BTreeSet<String>> = BTreeMap::new();

        for mangled in &order {
            let decl = by_mangled[mangled];
            let mut private = decl
                .extends
                .as_ref()
                .and_then(|parent| private_members.get(parent))
                .cloned()
                .unwrap_or_default();
            for field in &decl.fields {
                if field.visibility == crate::Visibility::Private {
                    private.insert(field.name.name.clone());
                }
            }
            for accessor in &decl.accessors {
                if accessor.visibility() == crate::Visibility::Private {
                    private.insert(accessor_getter_name(&accessor.name().name));
                    private.insert(accessor_setter_name(&accessor.name().name));
                }
            }
            private_members.insert(mangled.clone(), private);
            // Parent prefix: a local parent (resolved earlier this pass) or an
            // imported parent (its full layout reconstructed in `imported_classes`).
            let (parent_fields, parent_methods) = decl
                .extends
                .as_ref()
                .and_then(|p| {
                    layouts
                        .get(p)
                        .map(|(f, m, _)| (f.clone(), m.clone()))
                        .or_else(|| {
                            imported.get(p).map(|l| {
                                let fields = l
                                    .fields
                                    .iter()
                                    .map(|name| FieldLayout {
                                        name: name.clone(),
                                        optional: l.optional_fields.contains(name),
                                        narrowing_check: l
                                            .narrowing_checks
                                            .get(name)
                                            .cloned()
                                            .map(Box::new),
                                    })
                                    .collect();
                                let methods = l
                                    .methods
                                    .iter()
                                    .map(|s| SlotDraft {
                                        name: s.name.clone(),
                                        owner: s.owner.clone(),
                                        param_tys: s.param_tys.clone(),
                                        argument_metadata: s.argument_metadata.clone(),
                                        ret_ty: s.ret_ty.clone(),
                                        generic: s.generic,
                                    })
                                    .collect();
                                (fields, methods)
                            })
                        })
                })
                .unwrap_or_default();

            // Slot order is an internal ABI, not source order: own members sort
            // by name and append after the inherited prefix. A cross-package
            // consumer derives the identical order from the (sorted) public
            // `PackageDeclaration` maps, so no order metadata crosses the boundary.
            // Serialization sorts the full set itself (spec.md §JSON), so payload
            // order stays free.
            let mut own_fields: Vec<&crate::TypedClassField> = decl.fields.iter().collect();
            own_fields.sort_by(|a, b| a.name.name.cmp(&b.name.name));
            // Accessors lower to internal getter/setter methods here in codegen —
            // they are not methods in the typed AST or `PackageDeclaration`.
            let synth_methods = accessor_methods(decl);
            let mut sorted_methods: Vec<&crate::TypedClassMethod> =
                decl.methods.iter().chain(synth_methods.iter()).collect();
            sorted_methods.sort_by(|a, b| a.name.name.cmp(&b.name.name));

            inherited_fields.insert(
                mangled.clone(),
                parent_fields.iter().map(|f| f.name.clone()).collect(),
            );
            let mut fields: Vec<FieldLayout> = parent_fields;
            for f in &own_fields {
                // A redeclaration reuses the parent's slot. Its guard replaces the
                // inherited one only if it has its own: redeclaring at the *same*
                // type as the parent narrows nothing and records none, and the
                // guard the ancestor installed still describes this slot.
                if let Some(slot) = fields.iter_mut().find(|e| e.name == f.name.name) {
                    slot.optional = f.optional;
                    if let Some(check) = f.narrowing_check.clone() {
                        slot.narrowing_check = Some(check);
                    }
                    continue;
                }
                fields.push(FieldLayout {
                    name: f.name.name.clone(),
                    optional: f.optional,
                    narrowing_check: f.narrowing_check.clone(),
                });
            }

            let mut methods: Vec<SlotDraft> = parent_methods;
            for m in &sorted_methods {
                let param_tys: Vec<crate::Type> = m.params.iter().map(|p| p.ty.clone()).collect();
                let argument_metadata = super::call_arguments::typed_metadata(&m.params);
                if let Some(slot) = methods.iter_mut().find(|s| s.name == m.name.name) {
                    // Override: same slot, body now supplied by this class — so
                    // the slot must describe *this* body, not the ancestor's.
                    slot.owner = decl.mangled_name.clone();
                    slot.param_tys = param_tys;
                    slot.argument_metadata = argument_metadata;
                    slot.ret_ty = m.return_type.clone();
                    slot.generic = !m.generics.is_empty();
                } else {
                    methods.push(SlotDraft {
                        name: m.name.name.clone(),
                        owner: decl.mangled_name.clone(),
                        param_tys,
                        argument_metadata,
                        ret_ty: m.return_type.clone(),
                        generic: !m.generics.is_empty(),
                    });
                }
            }
            let own_methods: Vec<OwnMethod> = sorted_methods
                .iter()
                .map(|m| OwnMethod {
                    name: m.name.name.clone(),
                    params: m.params.clone(),
                    body: m.body,
                    return_type: m.return_type.clone(),
                    generic: !m.generics.is_empty(),
                })
                .collect();
            layouts.insert(mangled.clone(), (fields, methods, own_methods));
            ordered.push(mangled.clone());
        }

        // Effective ctor params per class (topo order, parents first), so an
        // implicit constructor (no body, has a parent) inherits the nearest
        // ancestor's params — what the entry constructor takes and forwards.
        // Seeded with imported parents so a local subclass of one inherits its
        // params across the package boundary.
        let mut ctor_params_by_mangled: BTreeMap<MangledName, Vec<crate::TypedParam>> = imported
            .iter()
            .map(|(m, l)| (m.clone(), l.ctor_params.clone()))
            .collect();
        let mut classes: Vec<ClassLayout> = Vec::with_capacity(ordered.len());
        for mangled in ordered {
            let decl = by_mangled[&mangled];
            let (fields, method_drafts, own_methods) = layouts.remove(&mangled).unwrap();
            // An inherited signature comes from the typed AST with the
            // `extends` clause's type arguments substituted, matching what the
            // declaration exports and a consumer imports; re-deriving the
            // parent's unsubstituted parameters here would emit a module that
            // cannot link.
            let ctor_params = decl.effective_ctor_params().to_vec();
            let ctor_body = decl.constructor.as_ref().map(|c| c.body);
            ctor_params_by_mangled.insert(mangled.clone(), ctor_params.clone());
            let inherited = inherited_fields
                .remove(&mangled)
                .expect("inherited field names recorded for every class in topo order");
            let field_setup = field_setup_steps(decl, &inherited);
            // get + set share a property name; dedup for the per-name string globals.
            let accessor_names: Vec<String> = decl
                .accessors
                .iter()
                .map(|a| a.name().name.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            classes.push(ClassLayout {
                private_members: private_members.remove(&mangled).unwrap_or_default(),
                generics: ta
                    .runtime_class_parameters
                    .get(&mangled)
                    .cloned()
                    .unwrap_or_default(),
                mangled: mangled.clone(),
                name: decl.name.name.clone(),
                parent: decl.extends.clone(),
                fields,
                field_setup,
                accessor_names,
                methods: method_drafts
                    .into_iter()
                    .map(|d| MethodSlot {
                        name: d.name,
                        owner: d.owner,
                        sig_idx: 0, // filled in reserve_types
                        param_tys: d.param_tys,
                        argument_metadata: d.argument_metadata,
                        ret_ty: d.ret_ty,
                        generic: d.generic,
                    })
                    .collect(),
                own_methods,
                adapter_slots: Vec::new(),
                ctor_params,
                ctor_body,
                struct_type_idx: 0,
                vtable_type_idx: 0,
                ctor_func_idx: 0,
                ctor_sig_idx: 0,
                ctor_init_func_idx: 0,
                ctor_init_sig_idx: 0,
                getter_func_idx: 0,
                setter_func_idx: 0,
                equals_func_idx: 0,
                to_string_func_idx: 0,
                to_json_func_idx: 0,
                hash_func_idx: 0,
            });
        }

        Ok(ClassPlan {
            package_name: ta.package_name.to_string(),
            classes,
            universal_stubs: [0; UNIVERSAL_STUB_COUNT],
        })
    }

    /// The class's instance `Type::ClassRef`, for constructor return types.
    fn class_ref_ty(&self, class: &ClassLayout) -> crate::Type {
        crate::Type::class_ref(
            crate::Package(self.package_name.clone()),
            class.name.clone(),
            class.mangled.clone(),
            Vec::new(),
        )
    }

    /// The ordered list of class field-name vectors, for the shared
    /// field-names-array global emission (`field_names::emit`). Built from the
    /// same helper the instance payload is, so the two cannot drift.
    pub fn field_name_lists(&self) -> Vec<Vec<super::field_names::FieldName>> {
        self.classes
            .iter()
            .map(ClassLayout::payload_field_names)
            .collect()
    }

    /// Accessor property names across all classes. They need per-name `$string`
    /// globals for the runtime property scan, but are not data fields, so they're
    /// excluded from `field_name_lists` (no shape stores them).
    pub fn accessor_property_names(&self) -> Vec<String> {
        self.classes
            .iter()
            .flat_map(|c| c.accessor_names.iter().cloned())
            .collect()
    }

    /// Phase: emit standalone method fn-type sigs, then reserve struct/vtable type
    /// indices, then emit the class rec group. `types`/`next_type_idx` advance in
    /// lockstep with the wasm-encoder type section.
    pub fn reserve_and_emit_types(
        &mut self,
        types: &mut TypeSection,
        next_type_idx: &mut u32,
        symbols: &mut SymbolTable,
        _ta: &TypedAst,
        intrinsics: IntrinsicTypeIndices,
    ) {
        if self.classes.is_empty() {
            return;
        }
        // 1. A standalone fn-type sig per method slot first introduced (base
        //    method or subclass-new method); overrides reuse the parent's sig.
        //    Each sig carries its erasure shape (derived from the originating
        //    declaration's types) — inherited/override slots copy it with the
        //    sig, so a call through any class in the chain sees the physical
        //    slot's boxing needs.
        let mut sig_by_name: BTreeMap<(MangledName, String), (u32, MethodSlotAbi)> =
            BTreeMap::new();
        for class in &self.classes {
            for slot in &class.methods {
                // Only allocate when this class is where the slot's sig originates:
                // i.e. the method is declared here AND the parent has no such slot.
                let parent_has = class.parent.as_ref().and_then(|p| {
                    sig_by_name
                        .get(&(p.clone(), slot.name.clone()))
                        .cloned()
                        // An imported parent's slot sigs live in the symbol table
                        // (recorded by `imported_classes`), not `sig_by_name`.
                        .or_else(|| {
                            symbols.class_method_sig(p, &slot.name).map(|sig| {
                                (
                                    sig,
                                    symbols
                                        .class_method_abi(p, &slot.name)
                                        .cloned()
                                        .unwrap_or_default(),
                                )
                            })
                        })
                });
                if let Some(parent_entry) = parent_has {
                    sig_by_name.insert((class.mangled.clone(), slot.name.clone()), parent_entry);
                    continue;
                }
                // Slot originates here: build its sig from the declaring method.
                // `own_methods` includes the synthetic accessor getter/setter.
                let method = class
                    .own_methods
                    .iter()
                    .find(|m| m.name == slot.name)
                    .expect("originating slot is declared on this class");
                let mut params: Vec<ValType> = vec![ref_to(intrinsics.object)];
                // Overrides may widen parameters beyond the ancestor's representation.
                // Every method argument therefore crosses a nullable boxed slot.
                params.extend(
                    method
                        .params
                        .iter()
                        .map(|_| symbols.value_type(&crate::Type::Unknown)),
                );
                let results = symbols.slot_wasm_result(&method.return_type);
                let abi = MethodSlotAbi {
                    params: params[1..].to_vec(),
                    ret: results.first().copied(),
                };
                types.ty().function(params, results);
                let sig_idx = take(next_type_idx);
                sig_by_name.insert((class.mangled.clone(), slot.name.clone()), (sig_idx, abi));
            }
        }
        // Stamp each slot's sig_idx (resolving inherited slots to their origin sig).
        for class in &mut self.classes {
            for slot in &mut class.methods {
                slot.sig_idx = sig_by_name
                    .get(&(class.mangled.clone(), slot.name.clone()))
                    .expect("every slot has a sig")
                    .0;
            }
        }

        // 2. Reserve struct + vtable type indices: one (vtable, struct) pair per
        //    class, interleaved, in topo order (parents precede children).
        for class in &mut self.classes {
            class.vtable_type_idx = take(next_type_idx);
            class.struct_type_idx = take(next_type_idx);
            symbols.record_class_vtable_type(class.mangled.clone(), class.vtable_type_idx);
            symbols.record_class_struct_type(class.mangled.clone(), class.struct_type_idx);
            let vtable_super = vtable_supertype_idx(class, intrinsics, symbols);
            let struct_super = struct_supertype_idx(class, intrinsics, symbols);
            symbols.record_struct_supertype(class.vtable_type_idx, vtable_super);
            symbols.record_struct_supertype(class.struct_type_idx, struct_super);
        }

        // 3. Emit one rec group per class: `(rec $Foo_vtable $Foo)`. A subclass's
        //    group references its parent's types from an earlier rec group — valid
        //    because there are no cross-class type cycles (single inheritance; a
        //    class's fields live in the object-fields payload, not as named struct
        //    fields, so the struct only references its own vtable + intrinsics).
        //    Per-class groups let a cross-package consumer reconstruct just the
        //    classes it uses, byte-identically.
        for class in &self.classes {
            let vtable = self.vtable_subtype(class, intrinsics, symbols);
            let strukt = self.struct_subtype(class, intrinsics, symbols);
            types.ty().rec([vtable, strukt]);
        }

        // Record each field's payload index inside ObjectShape.fields.
        for class in &self.classes {
            symbols
                .class_type_parameters
                .insert(class.mangled.clone(), class.generics.clone());
            symbols.record_class_guard_layout(
                class.mangled.clone(),
                class.parent.as_ref(),
                class.payload_field_names().len() as u32,
                !class.generics.is_empty()
                    || class.parent.as_ref().is_some_and(|parent| {
                        symbols.class_guard_layout(parent).has_instance_guards
                    })
                    || class
                        .fields
                        .iter()
                        .any(|field| field.narrowing_check.is_some()),
            );
            for (i, f) in class.fields.iter().enumerate() {
                symbols.record_class_field_slot(class.mangled.clone(), f.name.clone(), i as u32);
                if let Some(check) = &f.narrowing_check {
                    symbols.record_class_field_narrowing_check(
                        class.mangled.clone(),
                        f.name.clone(),
                        (**check).clone(),
                    );
                }
            }
            // Method vtable slot (universal slots + parent link precede user
            // methods) + its sig, for every slot in the chain so a parent-typed
            // receiver resolves too.
            for (i, slot) in class.methods.iter().enumerate() {
                symbols.record_class_method_slot(
                    class.mangled.clone(),
                    slot.name.clone(),
                    VTABLE_METHOD_SLOT_BASE + i as u32,
                );
                symbols.record_class_method_sig(
                    class.mangled.clone(),
                    slot.name.clone(),
                    slot.sig_idx,
                );
                if let Some((_, abi)) = sig_by_name.get(&(class.mangled.clone(), slot.name.clone()))
                {
                    symbols.record_class_method_abi(
                        class.mangled.clone(),
                        slot.name.clone(),
                        abi.clone(),
                    );
                }
            }
        }

        // 4. Constructor fn-type sigs — standalone, emitted *after* the rec group
        //    so they can reference `(ref $Foo)` (a rec-group type) as the return.
        //    Each class gets two: the allocating entry `(params) -> (ref $Foo)`,
        //    and the self-first init `((ref $Object), params) -> ()`. Parameters
        //    erase like method slots so a cross-package consumer reconstructs
        //    the same signature whatever order it rebuilds the classes in.
        let object_idx = intrinsics.object;
        for class in &mut self.classes {
            let params: Vec<ValType> = class
                .ctor_params
                .iter()
                .map(|p| symbols.slot_value_type(&p.ty))
                .collect();
            symbols.record_class_ctor_abi(class.mangled.clone(), params.clone());
            let ret = ref_to(class.struct_type_idx);
            let mut entry_params = params.clone();
            if symbols
                .class_guard_layout(&class.mangled)
                .has_instance_guards
            {
                entry_params.push(ref_to(intrinsics.object_fields));
            }
            types.ty().function(entry_params, vec![ret]);
            class.ctor_sig_idx = take(next_type_idx);

            let mut init_params = vec![ref_to(object_idx)];
            init_params.extend(params);
            types.ty().function(init_params, Vec::new());
            class.ctor_init_sig_idx = take(next_type_idx);
        }
    }

    fn vtable_subtype(
        &self,
        class: &ClassLayout,
        intrinsics: IntrinsicTypeIndices,
        symbols: &SymbolTable,
    ) -> SubType {
        let supertype = vtable_supertype_idx(class, intrinsics, symbols);
        let mut fields: Vec<FieldType> = vec![
            fieldtype_ref(intrinsics.to_string_fn),
            fieldtype_ref(intrinsics.to_json_fn),
            fieldtype_ref(intrinsics.equals_fn),
            fieldtype_ref(intrinsics.hash_fn),
            fieldtype_ref_null(intrinsics.class_vtable),
        ];
        for slot in &class.methods {
            fields.push(fieldtype_ref(slot.sig_idx));
        }
        substruct(fields, Some(supertype))
    }

    fn struct_subtype(
        &self,
        class: &ClassLayout,
        intrinsics: IntrinsicTypeIndices,
        symbols: &SymbolTable,
    ) -> SubType {
        let supertype = struct_supertype_idx(class, intrinsics, symbols);
        // Header slots 0-2: vtable (refined to this class's vtable),
        // field-names, object-fields. Wasm requires the full supertype prefix
        // to be re-listed.
        let fields: Vec<FieldType> = vec![
            fieldtype_ref(class.vtable_type_idx),
            FieldType {
                mutable: true,
                ..fieldtype_ref(intrinsics.field_names)
            },
            FieldType {
                mutable: true,
                ..fieldtype_ref(intrinsics.object_fields)
            },
        ];
        substruct(fields, Some(supertype))
    }

    /// Phase: reserve function indices — 4 shared universal stubs, then per-class
    /// method stubs (one per own method) + getter + setter.
    pub fn allocate_funcs(&mut self, next_func_idx: &mut u32, symbols: &mut SymbolTable) {
        if self.classes.is_empty() {
            return;
        }
        self.universal_stubs = [
            take(next_func_idx),
            take(next_func_idx),
            take(next_func_idx),
            take(next_func_idx),
        ];
        // Borrow-checker: build the (mangled, ctor params, ret) tuples first so we
        // can call the `&self` helper `class_ref_ty` before the `&mut` loop.
        let ctor_infos: Vec<(MangledName, Vec<crate::Type>, crate::Type)> = self
            .classes
            .iter()
            .map(|c| {
                (
                    crate::mangle::extend(&c.mangled, "constructor"),
                    c.ctor_params.iter().map(|p| p.ty.clone()).collect(),
                    self.class_ref_ty(c),
                )
            })
            .collect();
        for (class, (ctor_mangled, ctor_param_tys, ret)) in self.classes.iter_mut().zip(ctor_infos)
        {
            // Constructor first (its index backs the `new Foo(...)` direct call),
            // then its init body (backs `super(...)` and the entry's delegation).
            class.ctor_func_idx = take(next_func_idx);
            symbols.record_local_fn(ctor_mangled, class.ctor_func_idx, ctor_param_tys, ret);
            class.ctor_init_func_idx = take(next_func_idx);
            symbols.record_class_ctor_init_func(class.mangled.clone(), class.ctor_init_func_idx);
            symbols.record_class_field_setup(class.mangled.clone(), class.field_setup.clone());
            for method in &class.own_methods {
                let idx = take(next_func_idx);
                symbols.record_class_method_func(class.mangled.clone(), method.name.clone(), idx);
            }
            // One closure-ABI adapter per non-generic *slot* — inherited ones
            // included, since an interface-typed receiver scans the instance
            // payload, which carries the whole chain. An override rewrites the
            // slot's owner to this class, so a declared method still allocates
            // here; a slot inherited from a local ancestor reuses that
            // ancestor's adapter, which is valid because the adapter takes its
            // receiver as `(ref $Object)` and casts.
            let mut adapter_slots: Vec<usize> = Vec::new();
            for (i, slot) in class.methods.iter().enumerate() {
                if slot.generic {
                    continue;
                }
                let reused = (slot.owner != class.mangled)
                    .then(|| symbols.class_method_adapter_func_idx(&slot.owner, &slot.name))
                    .flatten();
                let idx = reused.unwrap_or_else(|| take(next_func_idx));
                if reused.is_none() {
                    adapter_slots.push(i);
                    // Also key it by the owner, so descendants of this class
                    // find it. Without this an import-owned slot re-emits an
                    // identical adapter at every level of a local chain: the
                    // owner is the imported class all the way down, and only
                    // the emitting class was ever recorded.
                    symbols.record_class_method_adapter_func(
                        slot.owner.clone(),
                        slot.name.clone(),
                        idx,
                    );
                }
                symbols.record_class_method_adapter_func(
                    class.mangled.clone(),
                    slot.name.clone(),
                    idx,
                );
            }
            class.adapter_slots = adapter_slots;
            // Retained temporarily for stable function/global numbering while
            // class field dispatch migrates to the object-fields payload.
            class.getter_func_idx = take(next_func_idx);
            class.setter_func_idx = take(next_func_idx);
            class.equals_func_idx = take(next_func_idx);
            class.to_string_func_idx = take(next_func_idx);
            class.to_json_func_idx = take(next_func_idx);
            class.hash_func_idx = take(next_func_idx);
            *next_func_idx += 3; // Guarded equals, JSON, and hash implementation bodies.
        }
    }

    /// Phase: function-section entries for every emitted function, in the same
    /// order as `allocate_funcs` / `emit_bodies`.
    pub fn emit_function_entries(
        &self,
        functions: &mut FunctionSection,
        symbols: &SymbolTable,
        intrinsics: IntrinsicTypeIndices,
    ) {
        if self.classes.is_empty() {
            return;
        }
        functions.function(intrinsics.to_string_fn);
        functions.function(intrinsics.to_json_fn);
        functions.function(intrinsics.equals_fn);
        functions.function(intrinsics.hash_fn);
        for class in &self.classes {
            functions.function(class.ctor_sig_idx);
            functions.function(class.ctor_init_sig_idx);
            for method in &class.own_methods {
                let sig_idx = class
                    .methods
                    .iter()
                    .find(|s| s.name == method.name)
                    .expect("own method has a slot")
                    .sig_idx;
                functions.function(sig_idx);
            }
            for &i in &class.adapter_slots {
                let ty = symbols
                    .closure_func_type_idx(slot_closure_sig(&class.methods[i]))
                    .expect("class method closure sig registered (closures::class_member_sigs)");
                functions.function(ty);
            }
            functions.function(intrinsics.field_getter);
            functions.function(intrinsics.field_setter);
            functions.function(intrinsics.equals_fn);
            functions.function(intrinsics.to_string_fn);
            functions.function(intrinsics.to_json_fn);
            functions.function(intrinsics.hash_fn);
            functions.function(intrinsics.equals_fn);
            functions.function(intrinsics.to_json_fn);
            functions.function(intrinsics.hash_fn);
        }
    }

    /// Phase: vtable instance, getter, and setter globals (the field-names array
    /// global is emitted separately via `field_names::emit`).
    pub fn emit_globals(
        &self,
        globals: &mut GlobalSection,
        symbols: &mut SymbolTable,
        next_global_idx: &mut u32,
        intrinsics: IntrinsicTypeIndices,
    ) {
        for class in &self.classes {
            // Vtable instance: the four per-class universal bodies + the
            // parent-vtable link + a ref.func per method slot (resolved to the
            // owning class's body), then struct.new $Foo_vtable. The parent
            // link is a global.get of the parent's singleton — an earlier
            // defined global (topo order, parents first) or an imported one
            // (cross-package / `Error`); both are valid in a const expr.
            let parent_link = match &class.parent {
                Some(p) => Instruction::GlobalGet(
                    symbols
                        .class_vtable_global_idx(p)
                        .expect("parent vtable global recorded before child"),
                ),
                None => Instruction::RefNull(HeapType::Concrete(intrinsics.class_vtable)),
            };
            let mut instrs: Vec<Instruction<'_>> = vec![
                Instruction::RefFunc(class.to_string_func_idx),
                Instruction::RefFunc(class.to_json_func_idx),
                Instruction::RefFunc(class.equals_func_idx),
                Instruction::RefFunc(class.hash_func_idx),
                parent_link,
            ];
            for slot in &class.methods {
                let func = symbols
                    .class_method_func_idx(&slot.owner, &slot.name)
                    .expect("method stub allocated");
                instrs.push(Instruction::RefFunc(func));
            }
            instrs.push(Instruction::StructNew(class.vtable_type_idx));
            globals.global(
                GlobalType {
                    val_type: ref_to(class.vtable_type_idx),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::extended(instrs),
            );
            symbols.record_class_vtable_global(class.mangled.clone(), take(next_global_idx));

            // Getter + setter globals are no longer part of ObjectShape, but keep
            // emitting them for now so existing numbering remains stable.
            globals.global(
                GlobalType {
                    val_type: ref_to(intrinsics.field_getter),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::extended([Instruction::RefFunc(class.getter_func_idx)]),
            );
            symbols.record_class_getter_global(class.mangled.clone(), take(next_global_idx));

            globals.global(
                GlobalType {
                    val_type: ref_to(intrinsics.field_setter),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::extended([Instruction::RefFunc(class.setter_func_idx)]),
            );
            symbols.record_class_setter_global(class.mangled.clone(), take(next_global_idx));
        }
    }

    /// Phase: function bodies — constructor + init, method bodies and their
    /// closure adapters, then the retired getter/setter stubs (kept for stable
    /// numbering) and the four per-class universal vtable bodies
    /// (equals/toString/toJson/hash).
    pub fn emit_bodies(
        &self,
        code: &mut CodeSection,
        ctx: &crate::codegen::CodegenCtx<'_>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if self.classes.is_empty() {
            return Ok(());
        }
        // The slot-2 stub is unreferenced (each class emits its own equals
        // body) but kept so function numbering stays in lockstep with
        // `allocate_funcs` / `emit_function_entries`.
        for _ in 0..UNIVERSAL_STUB_COUNT {
            code.function(&stub_body());
        }
        for class in &self.classes {
            code.function(&self.emit_ctor_body(class, ctx));
            code.function(&self.emit_ctor_init(class, ctx)?);
            for method in &class.own_methods {
                code.function(&self.emit_method_body(class, method, ctx)?);
            }
            for &i in &class.adapter_slots {
                code.function(&self.emit_method_adapter_body(class, &class.methods[i], ctx)?);
            }
            // getter, then setter.
            code.function(&stub_body());
            code.function(&stub_body());
            let intrinsics = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared by codegen entry");
            code.function(&super::vtable_walk::guarded_body(
                class.hash_func_idx + 1,
                2,
                ValType::I32,
                ctx.symbols,
            ));
            let string_vtable_global_idx = ctx
                .symbols
                .prelude_global_idx("string_vtable")
                .expect("string_vtable imported from prelude");
            // A user `toString`/`toJson` method fills the universal slot (the
            // universal slots require strings even when the authored method's
            // physical return has widened to preserve live values).
            let user_method_thunk =
                |name: &str| -> Result<Option<Function>, crate::compiler_error::CompilerFailure> {
                    class
                        .methods
                        .iter()
                        .find(|s| s.name == name)
                        .map(|slot| {
                            let idx = ctx
                                .symbols
                                .class_method_func_idx(&slot.owner, &slot.name)
                                .expect("slot owner's method func allocated");
                            emit_universal_slot_thunk(ctx, idx)
                        })
                        .transpose()
                };
            code.function(&user_method_thunk("toString")?.unwrap_or_else(|| {
                emit_class_to_string_body(intrinsics, string_vtable_global_idx)
            }));
            code.function(&super::vtable_walk::guarded_body(
                class.hash_func_idx + 2,
                1,
                ref_to(intrinsics.string),
                ctx.symbols,
            ));
            code.function(&super::vtable_walk::guarded_body(
                class.hash_func_idx + 3,
                1,
                ValType::I32,
                ctx.symbols,
            ));
            code.function(&emit_class_equals_body(
                class.fields.len() as u32,
                intrinsics,
            ));
            code.function(
                &user_method_thunk("toJson")?
                    .unwrap_or_else(|| emit_class_to_json_body(ctx.symbols)),
            );
            code.function(&emit_class_hash_body(class.fields.len() as u32, intrinsics));
        }
        Ok(())
    }

    /// Closure-ABI adapter for one vtable slot: forward the boxed args, call the
    /// body backing the slot (self-first), and box a primitive slot return. `env` (slot 0) is the receiver — cast it to `(ref $Object)` for
    /// the body's self param, which is why one adapter serves every subclass.
    /// Mirrors [`function_adapters::emit_bodies`].
    ///
    /// `class` is used only for the ABI lookup; every other input comes from
    /// the slot. That is sound because a slot's `MethodSlotAbi` is
    /// chain-invariant — `reserve_types` propagates the declarer's verbatim,
    /// and the one place a slot's types are rewritten (an override) rewrites
    /// `owner` too, so an overridden slot never shares an adapter. Classes
    /// sharing an adapter therefore necessarily share its ABI. Make a slot's
    /// ABI class-dependent and that stops holding, with no validation error to
    /// catch it — reuse would box against the wrong shape.
    fn emit_method_adapter_body(
        &self,
        class: &ClassLayout,
        slot: &MethodSlot,
        ctx: &crate::codegen::CodegenCtx<'_>,
    ) -> Result<Function, crate::compiler_error::CompilerFailure> {
        use crate::codegen::function_emitter::{FunctionEmitter, cast};

        let intrinsics = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared by codegen entry");
        let object_ref_null = ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(intrinsics.object),
        });
        let any_ref = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::ANY,
        });
        let mut wasm_params: Vec<(crate::Ident, ValType)> = vec![(
            crate::Ident {
                name: "$__env__".to_string(),
                span: crate::Span::at(ctx.file),
            },
            any_ref,
        )];
        for (i, _) in slot.param_tys.iter().enumerate() {
            wasm_params.push((
                crate::Ident {
                    name: format!("$__arg{i}__"),
                    span: crate::Span::at(ctx.file),
                },
                object_ref_null,
            ));
        }
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params);
        // The owner supplies the body — a local ancestor's, or an imported
        // one's, both recorded under the owner's mangled name.
        let method_func = ctx
            .symbols
            .class_method_func_idx(&slot.owner, &slot.name)
            .expect("method body func allocated or imported");

        // self = env, cast to the method body's `(ref $Object)` self param.
        // The callee's physical signature is the slot's. Arguments are boxed;
        // reference results already carry the value the closure ABI wants.
        let abi = ctx
            .symbols
            .class_method_abi(&class.mangled, &slot.name)
            .cloned()
            .unwrap_or_default();
        emitter.instruction(Instruction::LocalGet(0));
        if slot.argument_metadata.is_some() {
            super::call_arguments::unwrap(&mut emitter, ctx);
        }
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            intrinsics.object,
        )));
        for (i, ty) in slot.param_tys.iter().enumerate() {
            emitter.instruction(Instruction::LocalGet((i + 1) as u32));
            if abi.params.get(i) != Some(&object_ref_null) {
                crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
                    &mut emitter,
                    ctx,
                    &crate::Type::Unknown,
                    ty,
                )?;
            }
        }
        emitter.instruction(Instruction::Call(method_func));
        match abi.ret {
            Some(ValType::F64) => cast::emit_box(&mut emitter, ctx, &crate::Type::Number),
            Some(ValType::I32) => cast::emit_box(&mut emitter, ctx, &crate::Type::Boolean),
            // A never-returning override can inherit a void slot. Its closure
            // convention has a result, but the inherited call cannot return.
            None if matches!(slot.ret_ty.peel(), crate::Type::Never) => {
                emitter.instruction(Instruction::Unreachable);
            }
            _ => {}
        }
        Ok(emitter.build())
    }

    /// Method body: bind `this` (`local.get 0 ; ref.cast (ref $Foo)` — the vtable
    /// ABI's self param is `(ref $Object)`), then run the typed body. A method
    /// with its own type parameters is rejected by the typechecker, so its
    /// stub body is unreachable defence rather than pending work.
    ///
    /// The function's Wasm signature is the slot's — the originating ancestor's,
    /// which an override's own annotations may differ from in either direction:
    /// wider, where a generic class's params/returns erase to
    /// `(ref null $Object)`, or narrower, where the ancestor spelled a
    /// structural type the override reaches through an interface. So each param
    /// binds through a shadow local cast to the body's own type, and each
    /// `return` coerces back into the slot. Byte-identical when own == origin.
    fn emit_method_body(
        &self,
        class: &ClassLayout,
        method: &OwnMethod,
        ctx: &crate::codegen::CodegenCtx<'_>,
    ) -> Result<Function, crate::compiler_error::CompilerFailure> {
        use crate::codegen::function_emitter::{FunctionEmitter, ReturnTarget, stmt};

        if method.generic {
            return Ok(stub_body());
        }
        let object_idx = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared by codegen entry")
            .object;
        let abi = ctx
            .symbols
            .class_method_abi(&class.mangled, &method.name)
            .cloned()
            .unwrap_or_default();
        let mut wasm_params: Vec<(crate::Ident, ValType)> = vec![(
            crate::Ident {
                name: "this".to_string(),
                span: crate::Span::at(ctx.file),
            },
            ref_to(object_idx),
        )];
        wasm_params.extend(slot_params(ctx, &method.params, &abi.params));
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params);
        let param_slots = rebind_erased_params(&mut emitter, ctx, &method.params, &abi.params)?;
        // Param prologue boxes captured-mutated method params.
        emitter.emit_boxed_param_prologue(&method.params, &param_slots);

        // `this` = ref.cast of the self param to the concrete class.
        let this_slot = emitter.add_anonymous_local(ref_to(class.struct_type_idx));
        emitter.instruction(Instruction::LocalGet(0));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            class.struct_type_idx,
        )));
        emitter.instruction(Instruction::LocalSet(this_slot));
        emitter.set_this_local(this_slot);
        super::field_guards::bind_receiver(&mut emitter, ctx, this_slot, &class.mangled);

        if !method.return_type.is_void() {
            emitter.set_return_target(ReturnTarget::Slot(
                abi.ret
                    .unwrap_or_else(|| ctx.symbols.slot_value_type(&method.return_type)),
            ));
        }
        stmt::emit_statement(&mut emitter, ctx, method.body)?;
        if !method.return_type.is_void() {
            // Keep the function statically total even when control flow can't be proven to terminate.
            emitter.instruction(Instruction::Unreachable);
        }
        Ok(emitter.build())
    }

    /// Entry constructor (`new Foo(...)`): `struct.new $Foo` (header globals +
    /// default field values + method closures) into a `this` local, delegate
    /// field initialization to the self-first init fn, then return the instance.
    /// Identical for base and derived classes — the struct is allocated once at
    /// the most-derived type, and `super(...)` chains run inside the init fns.
    fn emit_ctor_body(
        &self,
        class: &ClassLayout,
        ctx: &crate::codegen::CodegenCtx<'_>,
    ) -> Function {
        use crate::codegen::function_emitter::FunctionEmitter;

        // Parameters stay at their (possibly erased) slot types: this body only
        // forwards them to the init fn, which takes the same slots.
        let ctor_slots = ctor_slot_types(ctx, &class.mangled);
        let mut wasm_params = slot_params(ctx, &class.ctor_params, &ctor_slots);
        let guarded = ctx
            .symbols
            .class_guard_layout(&class.mangled)
            .has_instance_guards;
        if guarded {
            let intr = ctx
                .symbols
                .intrinsic_type_indices()
                .expect("intrinsics declared");
            wasm_params.push((
                crate::Ident {
                    name: "$field_guards".into(),
                    span: crate::Span::at(ctx.file),
                },
                ref_to(intr.object_fields),
            ));
        }
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params);

        let this_slot = emitter.add_anonymous_local(ref_to(class.struct_type_idx));
        emitter.set_this_local(this_slot);

        // struct.new $Foo: vtable + field-names + object-fields payload. The
        // payload holds the data fields (slots `0..N_fields`, written by the init
        // fn) then one closure per non-generic method (slots `N_fields..`,
        // populated by the prologue below). The field-names list must match
        // `class_field_names` so the dynamic field-name scan resolves both.
        let dynamic_methods: Vec<&MethodSlot> = class.payload_slots().collect();
        let n_fields = class.fields.len();
        let field_names = class.payload_field_names();
        let push_global =
            |em: &mut FunctionEmitter, idx: u32| em.instruction(Instruction::GlobalGet(idx));
        push_global(
            &mut emitter,
            ctx.symbols
                .class_vtable_global_idx(&class.mangled)
                .expect("vtable global"),
        );
        super::field_names::emit_instance_names(&mut emitter, ctx, &field_names, |_| false);
        let intrinsics = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared by codegen entry");
        // Instance validators live after the named payload, indexed by data
        // slot. Keeping them out of field_names preserves object enumeration.
        let layout = ctx.symbols.class_guard_layout(&class.mangled);
        let depth = layout.inheritance_depth;
        let guarded = layout.has_instance_guards;
        let guard_slots = if guarded {
            (depth as usize + 1) * (field_names.len() + 1)
        } else {
            0
        };
        let payload_len = field_names.len() + guard_slots;
        for _ in 0..payload_len {
            let default = default_object_value_instr(intrinsics.object);
            emitter.instruction(default);
        }
        emitter.instruction(Instruction::ArrayNewFixed {
            array_type_index: intrinsics.object_fields,
            array_size: payload_len as u32,
        });
        emitter.instruction(Instruction::StructNew(class.struct_type_idx));
        emitter.instruction(Instruction::LocalSet(this_slot));

        if guarded {
            emitter.instruction(Instruction::LocalGet(this_slot));
            emitter.instruction(Instruction::StructGet {
                struct_type_index: class.struct_type_idx,
                field_index: 2,
            });
            emitter.instruction(Instruction::I32Const(field_names.len() as i32));
            emitter.instruction(Instruction::LocalGet(class.ctor_params.len() as u32));
            emitter.instruction(Instruction::I32Const(0));
            emitter.instruction(Instruction::I32Const(guard_slots as i32));
            emitter.instruction(Instruction::ArrayCopy {
                array_type_index_dst: intrinsics.object_fields,
                array_type_index_src: intrinsics.object_fields,
            });
        }

        // Method-closure prologue: store `$closure_<sig>{ closure_vtable, adapter,
        // this }` into each method's payload slot, so an interface-typed receiver
        // dispatches it through the shared field-name scan.
        if !dynamic_methods.is_empty() {
            let closure_vtable = ctx
                .symbols
                .closure_vtable_global_idx()
                .expect("closure vtable global emitted when class methods exist");
            for (j, method) in dynamic_methods.iter().enumerate() {
                let closure_struct = ctx
                    .symbols
                    .closure_struct_type_idx(slot_closure_sig(method))
                    .expect("class method closure struct registered");
                let adapter = ctx
                    .symbols
                    .class_method_adapter_func_idx(&class.mangled, &method.name)
                    .expect("class method adapter allocated or reused from the owner");
                emitter.instruction(Instruction::LocalGet(this_slot));
                emitter.instruction(Instruction::StructGet {
                    struct_type_index: class.struct_type_idx,
                    field_index: 2,
                });
                emitter.instruction(Instruction::I32Const((n_fields + j) as i32));
                emitter.instruction(Instruction::GlobalGet(closure_vtable));
                emitter.instruction(Instruction::RefFunc(adapter));
                emitter.instruction(Instruction::LocalGet(this_slot));
                if let Some(metadata) = &method.argument_metadata {
                    super::call_arguments::wrap(&mut emitter, ctx, metadata);
                }
                emitter.instruction(Instruction::StructNew(closure_struct));
                emitter.instruction(Instruction::ArraySet(intrinsics.object_fields));
            }
        }

        // Delegate to the self-first init fn: `Foo#ctor_init(this, params...)`.
        // It runs the constructor body (or forwards to the parent for an implicit
        // constructor), where `super(...)` chains up to the parent init.
        emitter.instruction(Instruction::LocalGet(this_slot));
        for i in 0..class.ctor_params.len() as u32 {
            emitter.instruction(Instruction::LocalGet(i));
        }
        emitter.instruction(Instruction::Call(class.ctor_init_func_idx));

        // Implicit `return this`. (Explicit `return;` inside a constructor is not
        // supported in this slice — the fixtures don't use it.)
        emitter.instruction(Instruction::LocalGet(this_slot));
        emitter.build()
    }

    /// Constructor init fn: self-first ABI `((ref $Object), params...) -> ()` that
    /// initializes fields on an already-allocated instance. `super(...)` lowers to
    /// a direct call of the parent's init fn (see [`expr`] codegen). A class with
    /// no declared constructor but a parent (implicit constructor) forwards all
    /// params to the parent init.
    fn emit_ctor_init(
        &self,
        class: &ClassLayout,
        ctx: &crate::codegen::CodegenCtx<'_>,
    ) -> Result<Function, crate::compiler_error::CompilerFailure> {
        use crate::codegen::function_emitter::{FunctionEmitter, cast, stmt};

        let object_idx = ctx
            .symbols
            .intrinsic_type_indices()
            .expect("intrinsics declared by codegen entry")
            .object;
        let mut wasm_params: Vec<(crate::Ident, ValType)> = vec![(
            crate::Ident {
                name: "this".to_string(),
                span: crate::Span::at(ctx.file),
            },
            ref_to(object_idx),
        )];
        let ctor_slots = ctor_slot_types(ctx, &class.mangled);
        wasm_params.extend(slot_params(ctx, &class.ctor_params, &ctor_slots));
        let mut emitter = FunctionEmitter::new(ctx, &wasm_params);
        let param_slots = rebind_erased_params(&mut emitter, ctx, &class.ctor_params, &ctor_slots)?;
        // Box captured-mutated ctor params.
        emitter.emit_boxed_param_prologue(&class.ctor_params, &param_slots);

        // `this` = ref.cast of the self param to the concrete class.
        let this_slot = emitter.add_anonymous_local(ref_to(class.struct_type_idx));
        emitter.instruction(Instruction::LocalGet(0));
        emitter.instruction(Instruction::RefCastNonNull(HeapType::Concrete(
            class.struct_type_idx,
        )));
        emitter.instruction(Instruction::LocalSet(this_slot));
        emitter.set_this_local(this_slot);
        super::field_guards::bind_receiver(&mut emitter, ctx, this_slot, &class.mangled);
        emitter.set_ctor_class(class.mangled.clone());

        if let Some(body) = class.ctor_body {
            // Base class: no `super(...)`, so the field setup runs at the top.
            // Derived class: the `super(...)` call inside the body emits it.
            if class.parent.is_none() {
                crate::codegen::function_emitter::expr::emit_class_field_setup(
                    &mut emitter,
                    ctx,
                    &class.mangled,
                )?;
            }
            stmt::emit_statement(&mut emitter, ctx, body)?;
        } else {
            // Implicit constructor: forward all params to the parent init (if
            // any), then run this class's field setup.
            if let Some(parent) = &class.parent {
                let parent_init = ctx
                    .symbols
                    .class_ctor_init_func_idx(parent)
                    .expect("parent ctor init allocated");
                // This class's parameters carry the `extends` clause's
                // substituted types, while the parent's own signature may
                // still erase them, so each one is coerced on the way through.
                // Forward from the rebound local, whose Wasm type is `p.ty`'s —
                // what the coercion takes as its source.
                let parent_slots = ctor_slot_types(ctx, parent);
                emitter.instruction(Instruction::LocalGet(this_slot));
                for (i, p) in class.ctor_params.iter().enumerate() {
                    emitter.instruction(Instruction::LocalGet(param_slots[i]));
                    if let Some(slot) = parent_slots.get(i).copied() {
                        cast::emit_coerce_to_wasm_slot(&mut emitter, ctx, &p.ty, slot);
                    }
                }
                emitter.instruction(Instruction::Call(parent_init));
            }
            crate::codegen::function_emitter::expr::emit_class_field_setup(
                &mut emitter,
                ctx,
                &class.mangled,
            )?;
        }
        Ok(emitter.build())
    }

    /// `(export name, func index)` pairs for every class function a cross-package
    /// consumer may import: the entry constructor, the self-first ctor-init, and
    /// every method body. Restricted to classes the package actually exports
    /// (`is_exported`). Method bodies include private methods — Wasm-level
    /// visibility is wider than source so a cross-package subclass can fill its
    /// inherited vtable slots (docs/classes.md §2, §6). Generic methods are
    /// excluded (still emitted as stubs this slice).
    pub fn exported_funcs(
        &self,
        symbols: &SymbolTable,
        is_exported: impl Fn(&MangledName) -> bool,
    ) -> Vec<(MangledName, u32)> {
        let mut out: Vec<(MangledName, u32)> = Vec::new();
        for class in &self.classes {
            if !is_exported(&class.mangled) {
                continue;
            }
            out.push((
                crate::mangle::extend(&class.mangled, "constructor"),
                class.ctor_func_idx,
            ));
            out.push((
                crate::mangle::extend(&class.mangled, "constructor_init"),
                class.ctor_init_func_idx,
            ));
            for method in &class.own_methods {
                if method.generic {
                    continue;
                }
                let idx = symbols
                    .class_method_func_idx(&class.mangled, &method.name)
                    .expect("method body func allocated");
                out.push((crate::mangle::extend(&class.mangled, &method.name), idx));
            }
        }
        out
    }

    /// Each exported class's vtable-singleton global, for the export section —
    /// the class's nominal identity, imported by consumers for `instanceof`'s
    /// ref.eq comparison and as a local subclass's parent link.
    pub fn exported_vtable_globals(
        &self,
        symbols: &SymbolTable,
        is_exported: impl Fn(&MangledName) -> bool,
    ) -> Vec<(MangledName, u32)> {
        self.classes
            .iter()
            .filter(|class| is_exported(&class.mangled))
            .map(|class| {
                let global_idx = symbols
                    .class_vtable_global_idx(&class.mangled)
                    .expect("vtable global emitted before exports");
                (vtable_global_export_name(&class.mangled), global_idx)
            })
            .collect()
    }

    /// Function indices that appear in a `ref.func` const-expr and so must be
    /// covered by the declarative element section.
    pub fn declared_funcs(&self, symbols: &SymbolTable) -> Vec<u32> {
        if self.classes.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<u32> = self.universal_stubs.to_vec();
        for class in &self.classes {
            // Every vtable slot funcref the vtable global stores — own bodies,
            // overrides, and inherited slots whose body lives on an ancestor
            // (possibly imported from another package).
            for slot in &class.methods {
                if let Some(idx) = symbols.class_method_func_idx(&slot.owner, &slot.name) {
                    out.push(idx);
                }
            }
            // Method-closure adapters are `ref.func`'d in the constructor
            // prologue — one per payload slot, inherited ones included. The
            // dedup below absorbs the indices shared with an ancestor.
            for slot in class.payload_slots() {
                if let Some(idx) = symbols.class_method_adapter_func_idx(&class.mangled, &slot.name)
                {
                    out.push(idx);
                }
            }
            out.push(class.getter_func_idx);
            out.push(class.setter_func_idx);
            out.push(class.equals_func_idx);
            out.push(class.to_string_func_idx);
            out.push(class.to_json_func_idx);
            out.push(class.hash_func_idx);
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Constructor field-setup steps for a class: parameter-property copies (in
/// constructor-param order) followed by own-field initializers (in declaration
/// order). Parameter-property fields are the auto-assigned own fields; their copy
/// reads the matching constructor param (init-fn local `position + 1`; self is 0).
///
/// A field that only redeclares an inherited one contributes a
/// [`FieldSetup::Reset`] instead — see [`redeclares_uninitialized_slot`].
/// `inherited` names the slots the parent prefix already owns.
fn field_setup_steps(decl: &TypedClassDecl, inherited: &BTreeSet<String>) -> Vec<FieldSetup> {
    let mut steps = Vec::new();
    if let Some(ctor) = &decl.constructor {
        for field in decl.fields.iter().filter(|f| f.auto_assigned) {
            if let Some(pos) = ctor
                .params
                .iter()
                .position(|p| p.name.name == field.name.name)
            {
                steps.push(FieldSetup::ParamCopy {
                    field: field.name.name.clone(),
                    param_local: pos as u32 + 1,
                    ty: ctor.params[pos].ty.clone(),
                });
            }
        }
    }
    for field in &decl.fields {
        if let Some(value) = field.initializer {
            steps.push(FieldSetup::Init {
                field: field.name.name.clone(),
                value,
            });
        } else if redeclares_uninitialized_slot(field, inherited) {
            steps.push(FieldSetup::Reset {
                field: field.name.name.clone(),
            });
        }
    }
    steps
}

/// Whether this own field shares an inherited slot without writing it. Such a
/// field would otherwise read back whatever the *parent's* initializer left
/// there — a value the redeclaration's (possibly narrower) type need not admit.
/// Only the optional case arises: a non-optional field with no initializer must
/// be assigned in the constructor (`definite_assignment`), which runs after this
/// setup.
fn redeclares_uninitialized_slot(
    field: &crate::TypedClassField,
    inherited: &BTreeSet<String>,
) -> bool {
    field.optional && !field.auto_assigned && inherited.contains(&field.name.name)
}

/// Export/linker field name of a class's vtable-singleton global. The space in
/// the suffix keeps it out of the member namespace (same trick as
/// `get foo`/`set foo`), so no user method can collide with it.
pub(crate) fn vtable_global_export_name(mangled: &MangledName) -> MangledName {
    crate::mangle::extend(mangled, "vtable global")
}

/// Internal name of the vtable method an accessor property's getter lowers to.
/// The space keeps it disjoint from any user identifier, so it never collides
/// with a real method; it is also the runtime key under which the getter closure
/// is stored in the object payload. Codegen-only — not a mangled symbol name.
pub(crate) fn accessor_getter_name(prop: &str) -> String {
    format!("get {prop}")
}

/// Internal name of the vtable method an accessor property's setter lowers to.
pub(crate) fn accessor_setter_name(prop: &str) -> String {
    format!("set {prop}")
}

/// The closure shape an accessor's payload slot holds: a getter takes no
/// argument and returns the property (never `void`), a setter takes one and
/// returns nothing. Producer (`analysis::note_shaped_property_access`, which
/// collects the sig) and consumer (`emit_accessor_slot_present`, which looks up
/// its struct index) must agree exactly, so they read it from here.
pub(crate) fn accessor_closure_sig(kind: crate::AccessorKind) -> ClosureSig {
    match kind {
        crate::AccessorKind::Get => ClosureSig {
            arity: 0,
            is_void: false,
        },
        crate::AccessorKind::Set => ClosureSig {
            arity: 1,
            is_void: true,
        },
    }
}

/// Whether an own data field redeclares one already in the inherited prefix —
/// a shadow, so it reuses the parent's slot rather than appending a second one.
/// The slot has to stay at the *parent's* index: a child's data region is a
/// prefix-compatible extension of its parent's, which is what lets a
/// parent-typed receiver read a child instance at all.
///
/// Both layout builders must agree on this rule, or the same class gets
/// different slot numbers on the two sides of a package boundary. This is the
/// `imported_classes` reconstruction's copy; [`ClassPlan::collect`] answers the
/// same question with `fields.iter_mut().find(…)`, because it also needs the
/// slot itself to carry the redeclaration's narrowing guard across.
pub(crate) fn shadows_inherited_field<'a>(
    inherited: impl IntoIterator<Item = &'a str>,
    name: &str,
) -> bool {
    inherited.into_iter().any(|f| f == name)
}

/// Internal getter/setter methods synthesized from a class's accessors. These
/// reuse the ordinary method machinery (vtable slots, closures, bodies, dynamic
/// dispatch) but exist only inside codegen — never in the typed AST or
/// `PackageDeclaration`. Their names (`get <p>` / `set <p>`) are the runtime ABI
/// key under which the getter/setter closure is stored in the object payload.
fn accessor_methods(decl: &TypedClassDecl) -> Vec<crate::TypedClassMethod> {
    decl.accessors
        .iter()
        .map(|acc| {
            let (name, params, return_type) = match acc {
                crate::TypedClassAccessor::Getter { name, ret_ty, .. } => {
                    (accessor_getter_name(&name.name), Vec::new(), ret_ty.clone())
                }
                crate::TypedClassAccessor::Setter { name, param, .. } => (
                    accessor_setter_name(&name.name),
                    vec![param.clone()],
                    crate::Type::Void,
                ),
            };
            crate::TypedClassMethod {
                name: crate::Ident {
                    name,
                    span: acc.name().span,
                },
                generics: Vec::new(),
                params,
                return_type,
                body: acc.body(),
                visibility: acc.visibility(),
                doc: None,
            }
        })
        .collect()
}

fn slot_closure_sig(slot: &MethodSlot) -> crate::codegen::closures::ClosureSig {
    let sig = crate::Type::Function {
        params: slot.param_tys.clone(),
        ret: Box::new(slot.ret_ty.clone()),
        predicate: None,
        has_rest: false,
    };
    crate::codegen::closures::classify(&sig)
}

#[derive(Clone)]
struct SlotDraft {
    name: String,
    owner: MangledName,
    param_tys: Vec<crate::Type>,
    argument_metadata: Option<String>,
    ret_ty: crate::Type,
    generic: bool,
}

/// A class's resolved (fields, method slots, own-method names) during the
/// inheritance-folding pass in [`ClassPlan::collect`].
type ResolvedLayout = (Vec<FieldLayout>, Vec<SlotDraft>, Vec<OwnMethod>);

/// Topological order of classes by `extends` (parents before children). Classes
/// whose parent is not in this module (cross-package) are treated as roots.
fn topo_order(by_mangled: &BTreeMap<&MangledName, &TypedClassDecl>) -> Vec<MangledName> {
    let mut visited: std::collections::BTreeSet<MangledName> = std::collections::BTreeSet::new();
    let mut out: Vec<MangledName> = Vec::new();
    let mut keys: Vec<&MangledName> = by_mangled.keys().copied().collect();
    keys.sort();
    for k in keys {
        visit(k, by_mangled, &mut visited, &mut out);
    }
    out
}

fn visit(
    mangled: &MangledName,
    by_mangled: &BTreeMap<&MangledName, &TypedClassDecl>,
    visited: &mut std::collections::BTreeSet<MangledName>,
    out: &mut Vec<MangledName>,
) {
    if visited.contains(mangled) {
        return;
    }
    let Some(decl) = by_mangled.get(mangled) else {
        return;
    };
    visited.insert(mangled.clone());
    if let Some(parent) = &decl.extends {
        visit(parent, by_mangled, visited, out);
    }
    out.push(mangled.clone());
}

/// The Wasm type of a body's parameter `i` — the recorded slot's, not the
/// declaration's own: an override shares the signature of the ancestor that
/// introduced the slot, and a constructor's signature erases class types so
/// producer and cross-package consumer agree.
fn slot_param_type(
    ctx: &CodegenCtx<'_>,
    slot_types: &[ValType],
    i: usize,
    p: &crate::TypedParam,
) -> ValType {
    slot_types
        .get(i)
        .copied()
        .unwrap_or_else(|| ctx.symbols.slot_value_type(&p.ty))
}

/// The constructor parameter slots recorded when the class's ctor fn-types were
/// emitted — the same signature the entry ctor, the init fn, and every call site
/// must agree on. An unrecorded class falls back to per-param `slot_value_type`
/// in [`slot_param_type`], which is what recording ran in the first place.
fn ctor_slot_types(ctx: &CodegenCtx<'_>, class: &MangledName) -> Vec<ValType> {
    ctx.symbols
        .class_ctor_abi(class)
        .map(<[ValType]>::to_vec)
        .unwrap_or_default()
}

/// Named Wasm parameters for a method or constructor body, at their slot types.
fn slot_params(
    ctx: &CodegenCtx<'_>,
    params: &[crate::TypedParam],
    slot_types: &[ValType],
) -> Vec<(crate::Ident, ValType)> {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), slot_param_type(ctx, slot_types, i, p)))
        .collect()
}

/// Unbox every param whose slot is erased but whose declared type is concrete
/// (an override of a generic method, a class-typed constructor param) into a
/// shadow local, and rebind the name to it so the body reads its own type.
/// Returns the local index backing each parameter, for the boxed-param
/// prologue. Every body this serves is self-first, so parameter `i` is local
/// `i + 1`.
fn rebind_erased_params(
    emitter: &mut FunctionEmitter<'_>,
    ctx: &CodegenCtx<'_>,
    params: &[crate::TypedParam],
    slot_types: &[ValType],
) -> Result<Vec<u32>, crate::compiler_error::CompilerFailure> {
    let mut param_slots: Vec<u32> = (1..=params.len() as u32).collect();
    for (i, p) in params.iter().enumerate() {
        let own_vt = ctx.symbols.value_type(&p.ty);
        if slot_param_type(ctx, slot_types, i, p) == own_vt {
            continue;
        }
        let shadow = emitter.add_anonymous_local(own_vt);
        emitter.instruction(Instruction::LocalGet((i + 1) as u32));
        crate::codegen::cast_check::emit_checked_parameter_cast_on_stack(
            emitter,
            ctx,
            &crate::Type::Unknown,
            &p.ty,
        )?;
        emitter.instruction(Instruction::LocalSet(shadow));
        emitter.rebind_in_innermost_scope(&p.name.name, shadow, own_vt);
        param_slots[i] = shadow;
    }
    Ok(param_slots)
}

fn stub_body() -> Function {
    let mut f = Function::new([]);
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);
    f
}

/// The class's `equals` vtable body: nominal exact-class guard + structural
/// compare of the data-field payload slots.
///
/// The guard is a `ref.eq` of the two operands' vtable refs — self's vtable
/// *is* this class's singleton (only this class's vtable references this
/// body), so no `global.get` is needed, and any other operand (sibling,
/// subclass, structural shape, boxed primitive) fails it. Only slots
/// `0..n_fields` are compared: the payload's trailing slots hold per-instance
/// method closures, whose reference-identity equals would break field-equal
/// instances.
fn emit_class_equals_body(n_fields: u32, intrinsics: IntrinsicTypeIndices) -> Function {
    // Locals layout (after 2 params):
    //   2: $elem_a    (ref null $Object)
    //   3: $elem_b    (ref null $Object)
    //   4: $elem_a_nn (ref $Object)
    //   5: $eq_fn     (ref $equalsFn)
    //   6: $a_fields  (ref $objectFields)
    //   7: $b_fields  (ref $objectFields)
    let locals: Vec<(u32, ValType)> = if n_fields == 0 {
        Vec::new()
    } else {
        vec![
            (
                2,
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(intrinsics.object),
                }),
            ),
            (1, ref_to(intrinsics.object)),
            (1, ref_to(intrinsics.equals_fn)),
            (2, ref_to(intrinsics.object_fields)),
        ]
    };
    let mut f = Function::new(locals);
    let (a, b) = (0u32, 1u32);
    let (a_fields, b_fields) = (6u32, 7u32);

    f.instruction(&Instruction::LocalGet(a));
    f.instruction(&Instruction::LocalGet(b));
    f.instruction(&Instruction::RefEq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    for operand in [a, b] {
        f.instruction(&Instruction::LocalGet(operand));
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object,
            field_index: 0,
        });
    }
    f.instruction(&Instruction::RefEq);
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    if n_fields > 0 {
        // Casts are guaranteed: the guard passed, so both operands are
        // instances of this class.
        for (operand, dest) in [(a, a_fields), (b, b_fields)] {
            f.instruction(&Instruction::LocalGet(operand));
            f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
                intrinsics.object_shape,
            )));
            f.instruction(&Instruction::StructGet {
                struct_type_index: intrinsics.object_shape,
                field_index: 2,
            });
            f.instruction(&Instruction::LocalSet(dest));
        }
        for slot in 0..n_fields {
            emit_payload_slot_equals(&mut f, slot, intrinsics);
        }
    }

    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::End);
    f
}

/// One payload-slot comparison for [`emit_class_equals_body`], null-aware
/// (optional fields store `null`): both null → equal, one null → return 0,
/// else dispatch the element's own `vtable.equals`. Uses the fixed locals
/// documented there.
fn emit_payload_slot_equals(f: &mut Function, slot: u32, intrinsics: IntrinsicTypeIndices) {
    let (elem_a, elem_b, elem_a_nn, eq_fn) = (2u32, 3u32, 4u32, 5u32);
    let (a_fields, b_fields) = (6u32, 7u32);

    for (fields, dest) in [(a_fields, elem_a), (b_fields, elem_b)] {
        f.instruction(&Instruction::LocalGet(fields));
        f.instruction(&Instruction::I32Const(slot as i32));
        f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
        f.instruction(&Instruction::LocalSet(dest));
    }

    f.instruction(&Instruction::LocalGet(elem_a));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(elem_b));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(elem_b));
    f.instruction(&Instruction::RefIsNull);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::LocalGet(elem_a));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalTee(elem_a_nn));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object,
        field_index: 0,
    });
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 2,
    });
    f.instruction(&Instruction::LocalSet(eq_fn));

    f.instruction(&Instruction::LocalGet(elem_a_nn));
    f.instruction(&Instruction::LocalGet(elem_b));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(eq_fn));
    f.instruction(&Instruction::CallRef(intrinsics.equals_fn));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
}

/// Bridge an authored conversion method to the universal string-returning slot.
/// Direct user calls still preserve the method's actual return value.
fn emit_universal_slot_thunk(
    ctx: &CodegenCtx,
    method_func_idx: u32,
) -> Result<Function, crate::compiler_error::CompilerFailure> {
    let mut emitter = super::function_emitter::FunctionEmitter::new(
        ctx,
        &[(
            crate::Ident {
                name: "self".into(),
                span: crate::Span::at(crate::FileId(0)),
            },
            ctx.symbols.value_type(&crate::Type::Unknown),
        )],
    );
    emitter.instruction(Instruction::LocalGet(0));
    emitter.instruction(Instruction::Call(method_func_idx));
    super::cast_check::emit_operation_cast_on_stack(
        &mut emitter,
        ctx,
        &crate::Type::Unknown,
        &crate::Type::String,
    )?;
    Ok(emitter.build())
}

fn emit_class_to_string_body(
    intrinsics: IntrinsicTypeIndices,
    string_vtable_global_idx: u32,
) -> Function {
    let mut f = Function::new([]);
    crate::codegen::intrinsics::push_string_literal(
        &mut f,
        intrinsics,
        string_vtable_global_idx,
        "[object Object]",
    );
    f.instruction(&Instruction::End);
    f
}

/// Class serialization reads the runtime property metadata, including visibility
/// and getters, so structural views and dynamically added fields use the same rules.
fn emit_class_to_json_body(symbols: &SymbolTable) -> Function {
    let mut f = Function::new([]);
    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::Call(
        symbols
            .prelude_func_idx("ObjectConstructor##toJson")
            .expect("object serializer imported"),
    ));
    f.instruction(&Instruction::End);
    f
}

/// Default class `hash`: FNV-1a over the data-field payload slots, each value
/// hashed through its own `vtable.hash` (null slot → 0) — the same recipe as
/// the structural `emit_subtype_hash_body`, so the `equals`-implies-equal-hash
/// contract holds: `equals` compares exactly these slots by vtable dispatch.
fn emit_class_hash_body(n_fields: u32, intrinsics: IntrinsicTypeIndices) -> Function {
    const FNV_BASIS: i32 = 0x811c9dc5_u32 as i32;
    const FNV_PRIME: i32 = 0x01000193;

    if n_fields == 0 {
        let mut f = Function::new([]);
        f.instruction(&Instruction::I32Const(FNV_BASIS));
        f.instruction(&Instruction::End);
        return f;
    }

    // Locals (after 1 param self=0):
    //   1: $fields  (ref $objectFields)
    //   2: $hash    (i32)
    //   3: $elem    (ref null $Object)
    //   4: $elem_nn (ref $Object)
    //   5: $hash_fn (ref $hashFn)
    let locals: Vec<(u32, ValType)> = vec![
        (1, ref_to(intrinsics.object_fields)),
        (1, ValType::I32),
        (
            1,
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(intrinsics.object),
            }),
        ),
        (1, ref_to(intrinsics.object)),
        (1, ref_to(intrinsics.hash_fn)),
    ];
    let mut f = Function::new(locals);
    let (fields_arr, hash, elem, elem_nn, hash_fn) = (1u32, 2u32, 3u32, 4u32, 5u32);

    f.instruction(&Instruction::LocalGet(0));
    f.instruction(&Instruction::RefCastNonNull(HeapType::Concrete(
        intrinsics.object_shape,
    )));
    f.instruction(&Instruction::StructGet {
        struct_type_index: intrinsics.object_shape,
        field_index: 2,
    });
    f.instruction(&Instruction::LocalSet(fields_arr));

    f.instruction(&Instruction::I32Const(FNV_BASIS));
    f.instruction(&Instruction::LocalSet(hash));

    for slot in 0..n_fields {
        f.instruction(&Instruction::LocalGet(fields_arr));
        f.instruction(&Instruction::I32Const(slot as i32));
        f.instruction(&Instruction::ArrayGet(intrinsics.object_fields));
        f.instruction(&Instruction::LocalSet(elem));

        f.instruction(&Instruction::LocalGet(elem));
        f.instruction(&Instruction::RefIsNull);
        f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::LocalGet(elem));
        f.instruction(&Instruction::RefAsNonNull);
        f.instruction(&Instruction::LocalTee(elem_nn));
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.object,
            field_index: 0,
        });
        f.instruction(&Instruction::StructGet {
            struct_type_index: intrinsics.vtable,
            field_index: 3,
        });
        f.instruction(&Instruction::LocalSet(hash_fn));
        f.instruction(&Instruction::LocalGet(elem_nn));
        f.instruction(&Instruction::LocalGet(hash_fn));
        f.instruction(&Instruction::CallRef(intrinsics.hash_fn));
        f.instruction(&Instruction::End);

        // hash = (hash XOR slot_hash) * FNV prime
        f.instruction(&Instruction::LocalGet(hash));
        f.instruction(&Instruction::I32Xor);
        f.instruction(&Instruction::I32Const(FNV_PRIME));
        f.instruction(&Instruction::I32Mul);
        f.instruction(&Instruction::LocalSet(hash));
    }

    f.instruction(&Instruction::LocalGet(hash));
    f.instruction(&Instruction::End);
    f
}

fn ref_to(idx: u32) -> ValType {
    ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::Concrete(idx),
    })
}

fn default_object_value_instr(object_type_idx: u32) -> Instruction<'static> {
    Instruction::RefNull(HeapType::Concrete(object_type_idx))
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

/// The Wasm supertype of a class's vtable: the parent class's vtable, or
/// `$ClassVTable` at the root of the chain.
fn vtable_supertype_idx(
    class: &ClassLayout,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
) -> u32 {
    match &class.parent {
        Some(p) => symbols
            .class_vtable_type_idx(p)
            .expect("parent vtable type reserved before child"),
        None => intrinsics.class_vtable,
    }
}

/// The Wasm supertype of a class's instance struct: the parent class's struct,
/// or `$ObjectShape` at the root of the chain.
fn struct_supertype_idx(
    class: &ClassLayout,
    intrinsics: IntrinsicTypeIndices,
    symbols: &SymbolTable,
) -> u32 {
    match &class.parent {
        Some(p) => symbols
            .class_struct_type_idx(p)
            .expect("parent struct type reserved before child"),
        None => intrinsics.object_shape,
    }
}

fn substruct(fields: Vec<FieldType>, supertype: Option<u32>) -> SubType {
    SubType {
        is_final: false,
        supertype_idx: supertype,
        composite_type: CompositeType {
            inner: CompositeInnerType::Struct(StructType {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::intrinsics::declare_intrinsic_types;
    use crate::runtime::intrinsic_types::build_intrinsic_types;
    use wasm_encoder::{
        ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType, HeapType, Module, RefType,
        TypeSection, ValType,
    };
    use wasmtime::Config;

    /// Seam guard for the intrinsic `(rec $Error_vtable $Error)` pair: this
    /// emitter's output for a parentless, method-less `{message, name}` class must
    /// canonicalize to the intrinsic types, so `class MyError extends Error`
    /// reconstructs the parent through the ordinary class path and WasmGC
    /// canonicalization unifies them.
    #[test]
    fn field_only_class_rec_group_canonicalizes_to_intrinsic_error() {
        let mut types = TypeSection::new();
        let intrinsics = declare_intrinsic_types(&mut types);
        let vtable_type_idx = crate::codegen::intrinsics::INTRINSIC_TYPE_COUNT;
        let struct_type_idx = vtable_type_idx + 1;
        let layout = ClassLayout {
            private_members: BTreeSet::new(),
            generics: Vec::new(),
            mangled: crate::mangle::prelude("ErrorWitness"),
            name: "ErrorWitness".to_string(),
            parent: None,
            fields: vec![
                FieldLayout {
                    name: "message".to_string(),
                    optional: false,
                    narrowing_check: None,
                },
                FieldLayout {
                    name: "name".to_string(),
                    optional: false,
                    narrowing_check: None,
                },
            ],
            methods: Vec::new(),
            own_methods: Vec::new(),
            adapter_slots: Vec::new(),
            ctor_params: Vec::new(),
            ctor_body: None,
            field_setup: Vec::new(),
            accessor_names: Vec::new(),
            struct_type_idx,
            vtable_type_idx,
            ctor_func_idx: 0,
            ctor_sig_idx: 0,
            ctor_init_func_idx: 0,
            ctor_init_sig_idx: 0,
            getter_func_idx: 0,
            setter_func_idx: 0,
            equals_func_idx: 0,
            to_string_func_idx: 0,
            to_json_func_idx: 0,
            hash_func_idx: 0,
        };
        let plan = ClassPlan {
            package_name: "witness".to_string(),
            classes: Vec::new(),
            universal_stubs: [0; UNIVERSAL_STUB_COUNT],
        };
        let symbols = SymbolTable::default();
        let vtable = plan.vtable_subtype(&layout, intrinsics, &symbols);
        let strukt = plan.struct_subtype(&layout, intrinsics, &symbols);
        types.ty().rec([vtable, strukt]);

        let mut module = Module::new();
        module.section(&types);
        let mut globals = GlobalSection::new();
        let mut exports = ExportSection::new();
        for (name, idx) in [("vtable", vtable_type_idx), ("struct", struct_type_idx)] {
            globals.global(
                GlobalType {
                    val_type: ValType::Ref(RefType {
                        nullable: true,
                        heap_type: HeapType::Concrete(idx),
                    }),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::ref_null(HeapType::Concrete(idx)),
            );
            exports.export(name, ExportKind::Global, idx - vtable_type_idx);
        }
        module.section(&globals);
        module.section(&exports);

        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = wasmtime::Engine::new(&config).unwrap();
        let intr = build_intrinsic_types(&engine).unwrap();
        let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
        let recovered = |name: &str| {
            module
                .get_export(name)
                .unwrap()
                .global()
                .unwrap()
                .content()
                .as_ref()
                .map(|r| r.heap_type().clone())
                .unwrap()
                .as_concrete_struct()
                .unwrap()
                .clone()
        };
        assert!(wasmtime::StructType::eq(
            &intr.error_vtable,
            &recovered("vtable")
        ));
        assert!(wasmtime::StructType::eq(&intr.error, &recovered("struct")));
    }
}
