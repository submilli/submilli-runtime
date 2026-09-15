//! Maps [`MangledName`] to Wasm indices for types, functions, and globals —
//! both imported symbols and consumer-local declarations.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use wasm_encoder::{HeapType, RefType, ValType};

use crate::codegen::closures::ClosureSig;
use crate::codegen::intrinsics::{IntrinsicTypeIndices, intrinsic_supertypes};
use crate::{Dispatch, ExprId, MangledName, Type};

/// One step of a constructor's field-setup sequence, run after `super(...)`
/// (or at the top of a base class's init fn): own-field initializers and
/// parameter-property copies, in execution order.
#[derive(Clone, Debug)]
pub enum FieldSetup {
    /// `this.field = <initializer expr>`.
    Init { field: String, value: ExprId },
    /// `this.field = <constructor param>` for a parameter property. `param_local`
    /// is the init fn's local index of the param (self is local 0).
    ParamCopy {
        field: String,
        param_local: u32,
        ty: Type,
    },
    /// `this.field = null` for an optional field that redeclares an inherited
    /// one with no initializer of its own. The slot is the parent's, so without
    /// this it would keep the parent's initialized value.
    Reset { field: String },
}

#[derive(Default, Clone, Debug)]
pub struct SymbolTable {
    types: BTreeMap<MangledName, u32>,
    funcs: BTreeMap<MangledName, u32>,
    globals: BTreeMap<MangledName, u32>,
    intrinsic_type_indices: Option<IntrinsicTypeIndices>,
    // Per-class WasmGC artifacts, keyed by the class's mangled name. Classes are
    // nominal (one struct type each), unlike arity-shared object subtypes.
    class_struct_type_idx: BTreeMap<MangledName, u32>,
    class_vtable_type_idx: BTreeMap<MangledName, u32>,
    class_vtable_global_idx: BTreeMap<MangledName, u32>,
    class_getter_global_idx: BTreeMap<MangledName, u32>,
    class_setter_global_idx: BTreeMap<MangledName, u32>,
    class_method_func_idx: BTreeMap<(MangledName, String), u32>,
    /// Wasm fn index of a class's constructor *init* function — the body that
    /// initializes fields on an already-allocated instance (self-first ABI), as
    /// distinct from the allocating entry constructor. `super(...)` calls it.
    class_ctor_init_func_idx: BTreeMap<MangledName, u32>,
    /// Wasm fn index of a class method's closure-ABI adapter, for the per-instance
    /// method closures placed in the object-fields payload (interface-typed dispatch).
    class_method_adapter_func_idx: BTreeMap<(MangledName, String), u32>,
    /// Vtable slot index (4 + declaration position) of a class method, for static
    /// `call_ref` dispatch; keyed for every class in the method's inheritance chain.
    class_method_slot: BTreeMap<(MangledName, String), u32>,
    /// Wasm fn-type index of a class method's signature, for the `call_ref`.
    class_method_sig: BTreeMap<(MangledName, String), u32>,
    /// Physical Wasm signature of a class method's vtable slot, keyed for every
    /// class in the chain like `class_method_sig`.
    class_method_abi: BTreeMap<(MangledName, String), MethodSlotAbi>,
    /// Wasm parameter types of a class's constructor (implicit subclass ctors
    /// inherit the parent's).
    class_ctor_abi: BTreeMap<MangledName, Vec<ValType>>,
    /// Struct slot index (4 + declaration position) of a class field, for
    /// `struct.get`/`struct.set` on a class-typed receiver.
    class_field_slot: BTreeMap<(MangledName, String), u32>,
    /// Read guards, keyed by `(class, field)`. See [`crate::FieldNarrowingCheck`].
    class_field_narrowing_check: BTreeMap<(MangledName, String), crate::FieldNarrowingCheck>,
    /// Constructor field-setup steps (parameter-property copies then own-field
    /// initializers) emitted at the post-`super()` field-setup point.
    class_field_setup: BTreeMap<MangledName, Vec<FieldSetup>>,
    vtable_global_idx: BTreeMap<Type, u32>,
    field_names_global_idx: BTreeMap<Vec<String>, u32>,
    field_name_string_global_idx: BTreeMap<String, u32>,
    // Keyed by ValType so language types sharing a Wasm representation share one box.
    box_type_idx: HashMap<ValType, u32>,
    /// Declared Wasm supertype of each struct type the module emits, child →
    /// parent, for [`ref_fits_slot`](Self::ref_fits_slot).
    struct_supertype: BTreeMap<u32, u32>,
    closure_func_type_idx: BTreeMap<ClosureSig, u32>,
    closure_struct_type_idx: BTreeMap<ClosureSig, u32>,
    closure_coercions: BTreeMap<ClosureSig, u32>,
    closure_coercion_vtable_type: Option<u32>,
    env_type_idx: BTreeMap<ExprId, u32>,
    closure_func_idx: BTreeMap<ExprId, u32>,
    closure_vtable_global_idx: Option<u32>,
    /// The sole exception tag's index, recorded at import emission.
    error_tag_idx: Option<u32>,
    adapter_func_idx: BTreeMap<MangledName, u32>,
    // Per-alias recursive validator helpers, keyed by the `Type::AliasRef` back-edge.
    cast_validator_idx: BTreeMap<Type, u32>,
    // Closures, adapters, and direct-dispatch wrappers are NOT in this map.
    top_level_fns: BTreeMap<MangledName, TopLevelFn>,
    iface_dispatch: BTreeMap<MangledName, Dispatch>,
    /// Physical signature of each Direct/Static-dispatch interface wrapper,
    /// keyed by its `iface#method` dispatch key.
    iface_method_abi: BTreeMap<MangledName, MethodSlotAbi>,
    // Members declared `intrinsic` — codegen emits them inline; nothing imports.
    intrinsic_members: BTreeSet<MangledName>,
}

#[derive(Clone, Debug)]
pub struct TopLevelFn {
    pub wasm_idx: u32,
    pub params: Vec<Type>,
    pub ret: Type,
    /// `true` for host modules whose Wasm ABI uses raw `(ref $rawString)` instead of
    /// `(ref $string)`; drives per-arg extract and post-call rewrap in `emit_direct_call`.
    pub is_host: bool,
}

/// The physical Wasm signature of a dispatched method — a class-method vtable
/// slot, or a Direct/Static-dispatch interface wrapper — captured where the
/// signature is built rather than re-derived at each call site.
///
/// A class slot's signature belongs to the ancestor that introduced it, and
/// erases class types and type variables (see
/// [`slot_value_type`](SymbolTable::slot_value_type)), so it can differ from
/// what an overriding method declares; an interface wrapper's signature erases
/// the interface's type parameters the same way. Call sites coerce arguments
/// into [`params`](Self::params) and cast [`ret`](Self::ret) back to the
/// call-site type; an overriding body binds `params` and returns `ret`
/// whatever its own annotations say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MethodSlotAbi {
    /// User parameters; the leading `self` is excluded.
    pub params: Vec<ValType>,
    /// `None` for a void slot.
    pub ret: Option<ValType>,
}

/// Whether a type occupies the erased (boxed) object slot rather than its own
/// concrete lowering. Class types are included: an erased slot must lower the
/// same way regardless of whether the class's struct type has been recorded yet
/// — imported classes are reconstructed before local slot signatures are built,
/// local classes after, and box cells are collected before either — and
/// regardless of which package is compiling it. A union erases whole if any
/// member does; a fully concrete union already lowers identically everywhere.
pub fn is_erased(ty: &Type) -> bool {
    match ty.peel() {
        Type::TypeVar(_) | Type::GenericParam { .. } | Type::ClassRef { .. } => true,
        Type::Union(members) => members.iter().any(is_erased),
        _ => false,
    }
}

/// The *lowering* question — whether a Wasm slot of this type can hold null.
/// See [`may_hold_null`] for the language-level one.
pub(crate) fn is_nullable_ref(val: ValType) -> bool {
    matches!(val, ValType::Ref(RefType { nullable: true, .. }))
}

/// True iff `ty`'s value space contains `null`.
///
/// The *semantic* question, deliberately not `is_nullable_ref(value_type(ty))`:
/// an `InterfaceRef` and an unrecorded `ClassRef` lower to a nullable
/// `(ref null $Object)` without admitting `null` at the language level, and the
/// `ClassRef` lowering is phase-dependent besides. Codegen reads this wherever
/// it has to decide "can this value be null at runtime" — the null-aware `===`
/// dispatch, `JSON.stringify`, and the object-shape `toJson` emitter.
pub(crate) fn may_hold_null(ty: &Type) -> bool {
    match ty.peel() {
        // A recursion back-edge is a name codegen has no body for, and the alias
        // it names often does list `null` (`type J = number | null | Wrap[]`).
        Type::Null
        | Type::Unknown
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::AliasRef { .. } => true,
        Type::Union(members) => members.iter().any(may_hold_null),
        _ => false,
    }
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn type_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.types.get(mangled).copied()
    }

    pub fn func_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.funcs.get(mangled).copied()
    }

    pub fn global_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.globals.get(mangled).copied()
    }

    pub fn prelude_func_idx(&self, symbol: &str) -> Option<u32> {
        self.func_idx(&crate::mangle::prelude(symbol))
    }

    pub fn prelude_global_idx(&self, symbol: &str) -> Option<u32> {
        self.global_idx(&crate::mangle::prelude(symbol))
    }

    pub fn string_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.string)
    }

    pub fn raw_string_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.raw_string)
    }

    pub fn bigint_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.bigint)
    }

    pub fn raw_bigint_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.raw_bigint)
    }

    pub fn regex_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.regex)
    }

    pub fn regex_match_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.regex_match)
    }

    pub fn regex_capture_array_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.regex_capture_array)
    }

    pub fn regex_match_box_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.regex_match_box)
    }

    pub fn boxed_number_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.boxed_number)
    }

    pub fn boxed_boolean_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.boxed_boolean)
    }

    pub fn raw_array_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.raw_array)
    }

    pub fn field_names_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.field_names)
    }

    pub fn object_shape_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.object_shape)
    }

    pub fn array_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.array)
    }

    pub fn raw_uint8_array_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.raw_uint8_array)
    }

    pub fn uint8_array_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.uint8_array)
    }

    pub fn closure_type_idx(&self) -> Option<u32> {
        self.intrinsic_type_indices.map(|i| i.closure)
    }

    pub fn intrinsic_type_indices(&self) -> Option<IntrinsicTypeIndices> {
        self.intrinsic_type_indices
    }

    pub fn object_subtype_idx(&self, ty: &Type) -> Option<u32> {
        match ty {
            Type::Object { .. } => self.object_shape_type_idx(),
            _ => None,
        }
    }

    pub fn class_struct_type_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.class_struct_type_idx.get(mangled).copied()
    }

    pub fn class_vtable_type_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.class_vtable_type_idx.get(mangled).copied()
    }

    pub fn class_vtable_global_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.class_vtable_global_idx.get(mangled).copied()
    }

    pub fn class_getter_global_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.class_getter_global_idx.get(mangled).copied()
    }

    pub fn class_setter_global_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.class_setter_global_idx.get(mangled).copied()
    }

    pub fn class_method_func_idx(&self, class: &MangledName, method: &str) -> Option<u32> {
        self.class_method_func_idx
            .get(&(class.clone(), method.to_string()))
            .copied()
    }

    pub fn class_ctor_init_func_idx(&self, class: &MangledName) -> Option<u32> {
        self.class_ctor_init_func_idx.get(class).copied()
    }

    pub fn class_method_adapter_func_idx(&self, class: &MangledName, method: &str) -> Option<u32> {
        self.class_method_adapter_func_idx
            .get(&(class.clone(), method.to_string()))
            .copied()
    }

    pub fn class_method_slot(&self, class: &MangledName, method: &str) -> Option<u32> {
        self.class_method_slot
            .get(&(class.clone(), method.to_string()))
            .copied()
    }

    pub fn class_method_sig(&self, class: &MangledName, method: &str) -> Option<u32> {
        self.class_method_sig
            .get(&(class.clone(), method.to_string()))
            .copied()
    }

    pub fn class_method_abi(&self, class: &MangledName, method: &str) -> Option<&MethodSlotAbi> {
        self.class_method_abi
            .get(&(class.clone(), method.to_string()))
    }

    pub fn class_ctor_abi(&self, class: &MangledName) -> Option<&[ValType]> {
        self.class_ctor_abi.get(class).map(Vec::as_slice)
    }

    pub fn class_field_slot(&self, class: &MangledName, field: &str) -> Option<u32> {
        self.class_field_slot
            .get(&(class.clone(), field.to_string()))
            .copied()
    }

    /// `None` for every ordinary field. See [`crate::FieldNarrowingCheck`].
    pub fn class_field_narrowing_check(
        &self,
        class: &MangledName,
        field: &str,
    ) -> Option<&crate::FieldNarrowingCheck> {
        self.class_field_narrowing_check
            .get(&(class.clone(), field.to_string()))
    }

    pub fn class_field_setup(&self, class: &MangledName) -> Option<&[FieldSetup]> {
        self.class_field_setup.get(class).map(Vec::as_slice)
    }

    pub fn vtable_global_idx(&self, ty: &Type) -> Option<u32> {
        self.vtable_global_idx.get(ty).copied()
    }

    pub fn field_names_global_idx(&self, field_names: &[String]) -> Option<u32> {
        self.field_names_global_idx.get(field_names).copied()
    }

    pub fn field_name_string_global_idx(&self, name: &str) -> Option<u32> {
        self.field_name_string_global_idx.get(name).copied()
    }

    /// Box cells are keyed on [`slot_value_type`](Self::slot_value_type) so the
    /// key a binding registers during collection still resolves once class
    /// struct types are recorded. A cell for an erased type therefore holds
    /// `(ref null $Object)`; reads out of it need `emit_unerase`.
    pub fn box_type_idx(&self, ty: &Type) -> Option<u32> {
        self.box_type_idx.get(&self.slot_value_type(ty)).copied()
    }

    /// A box struct is the `(ref $box_T)` wrapper a captured-and-mutated binding
    /// is stored behind. Returns the payload type of `idx`, or `None` if `idx`
    /// is not one — so codegen can recognise a slot that holds a box rather than
    /// the value, and know what a `struct.get` off it leaves on the stack.
    ///
    /// The reverse scan is unambiguous because `box_types::collect` dedups by
    /// payload before recording, so one index is registered per payload type.
    pub fn box_payload_type(&self, idx: u32) -> Option<ValType> {
        self.box_type_idx
            .iter()
            .find(|&(_, &v)| v == idx)
            .map(|(payload, _)| *payload)
    }

    pub fn closure_func_type_idx(&self, sig: ClosureSig) -> Option<u32> {
        self.closure_func_type_idx.get(&sig).copied()
    }

    pub fn record_closure_coercion_vtable_type(&mut self, index: u32) {
        self.closure_coercion_vtable_type = Some(index);
    }

    pub fn closure_coercion_vtable_type(&self) -> u32 {
        self.closure_coercion_vtable_type
            .expect("closure coercion vtable declared")
    }

    pub fn closure_signatures(&self) -> impl Iterator<Item = ClosureSig> + '_ {
        self.closure_struct_type_idx.keys().copied()
    }

    pub fn record_closure_coercion(&mut self, target: ClosureSig, index: u32) {
        self.closure_coercions.insert(target, index);
    }

    pub fn closure_coercion(&self, target: ClosureSig) -> Option<u32> {
        self.closure_coercions.get(&target).copied()
    }

    pub fn closure_struct_type_idx(&self, sig: ClosureSig) -> Option<u32> {
        self.closure_struct_type_idx.get(&sig).copied()
    }

    pub fn env_type_idx(&self, expr_id: ExprId) -> Option<u32> {
        self.env_type_idx.get(&expr_id).copied()
    }

    pub fn closure_func_idx(&self, expr_id: ExprId) -> Option<u32> {
        self.closure_func_idx.get(&expr_id).copied()
    }

    pub fn closure_vtable_global_idx(&self) -> Option<u32> {
        self.closure_vtable_global_idx
    }

    pub fn adapter_func_idx(&self, mangled: &MangledName) -> Option<u32> {
        self.adapter_func_idx.get(mangled).copied()
    }

    /// Whether a `value`-typed ref already satisfies a `slot` by WasmGC
    /// subtyping, so a coercion needs no instruction.
    ///
    /// Answers `false` whenever the relation isn't known — an unrecorded
    /// supertype edge, an abstract heap type — leaving the caller to emit a
    /// `ref.cast`, which is valid for any two types sharing a hierarchy. Wrong
    /// in that direction costs an always-succeeding cast; wrong in the other
    /// direction is a module the validator rejects.
    pub fn ref_fits_slot(&self, value: RefType, slot: RefType) -> bool {
        if value.nullable && !slot.nullable {
            return false;
        }
        let (HeapType::Concrete(value_idx), HeapType::Concrete(slot_idx)) =
            (value.heap_type, slot.heap_type)
        else {
            return value.heap_type == slot.heap_type;
        };
        let mut current = value_idx;
        while current != slot_idx {
            // Wasm requires a supertype to be declared before its subtypes, so
            // the walk strictly descends and terminates whatever the map holds.
            match self.struct_supertype.get(&current) {
                Some(&parent) if parent < current => current = parent,
                _ => return false,
            }
        }
        true
    }

    pub fn record_type(&mut self, mangled: MangledName, idx: u32) {
        self.types.insert(mangled, idx);
    }

    /// Also seeds the intrinsic hierarchy: those edges are fixed by the indices,
    /// so they can neither be known before this call nor go stale after it.
    pub fn set_intrinsic_type_indices(&mut self, indices: IntrinsicTypeIndices) {
        self.intrinsic_type_indices = Some(indices);
        for (child, parent) in intrinsic_supertypes(indices) {
            self.record_struct_supertype(child, parent);
        }
    }

    pub fn record_struct_supertype(&mut self, child: u32, parent: u32) {
        self.struct_supertype.insert(child, parent);
    }

    pub fn record_class_struct_type(&mut self, mangled: MangledName, idx: u32) {
        self.class_struct_type_idx.insert(mangled, idx);
    }

    pub fn record_class_vtable_type(&mut self, mangled: MangledName, idx: u32) {
        self.class_vtable_type_idx.insert(mangled, idx);
    }

    pub fn record_class_vtable_global(&mut self, mangled: MangledName, idx: u32) {
        self.class_vtable_global_idx.insert(mangled, idx);
    }

    pub fn record_class_getter_global(&mut self, mangled: MangledName, idx: u32) {
        self.class_getter_global_idx.insert(mangled, idx);
    }

    pub fn record_class_setter_global(&mut self, mangled: MangledName, idx: u32) {
        self.class_setter_global_idx.insert(mangled, idx);
    }

    pub fn record_class_method_func(&mut self, class: MangledName, method: String, idx: u32) {
        self.class_method_func_idx.insert((class, method), idx);
    }

    pub fn record_class_ctor_init_func(&mut self, class: MangledName, idx: u32) {
        self.class_ctor_init_func_idx.insert(class, idx);
    }

    pub fn record_class_method_adapter_func(
        &mut self,
        class: MangledName,
        method: String,
        idx: u32,
    ) {
        self.class_method_adapter_func_idx
            .insert((class, method), idx);
    }

    pub fn record_class_method_slot(&mut self, class: MangledName, method: String, slot: u32) {
        self.class_method_slot.insert((class, method), slot);
    }

    pub fn record_class_method_sig(&mut self, class: MangledName, method: String, sig: u32) {
        self.class_method_sig.insert((class, method), sig);
    }

    pub fn record_class_method_abi(
        &mut self,
        class: MangledName,
        method: String,
        abi: MethodSlotAbi,
    ) {
        self.class_method_abi.insert((class, method), abi);
    }

    pub fn record_class_ctor_abi(&mut self, class: MangledName, params: Vec<ValType>) {
        self.class_ctor_abi.insert(class, params);
    }

    pub fn record_class_field_narrowing_check(
        &mut self,
        class: MangledName,
        field: String,
        check: crate::FieldNarrowingCheck,
    ) {
        self.class_field_narrowing_check
            .insert((class, field), check);
    }

    pub fn record_class_field_slot(&mut self, class: MangledName, field: String, slot: u32) {
        self.class_field_slot.insert((class, field), slot);
    }

    pub fn record_class_field_setup(&mut self, class: MangledName, setup: Vec<FieldSetup>) {
        self.class_field_setup.insert(class, setup);
    }

    pub fn record_vtable_global(&mut self, ty: Type, idx: u32) {
        self.vtable_global_idx.insert(ty, idx);
    }

    pub fn record_field_names_global(&mut self, field_names: Vec<String>, idx: u32) {
        self.field_names_global_idx.insert(field_names, idx);
    }

    pub fn record_field_name_string_global(&mut self, name: String, idx: u32) {
        self.field_name_string_global_idx.insert(name, idx);
    }

    pub fn record_box_type(&mut self, value_type: ValType, idx: u32) {
        self.box_type_idx.insert(value_type, idx);
    }

    pub fn record_closure_func_type(&mut self, sig: ClosureSig, idx: u32) {
        self.closure_func_type_idx.insert(sig, idx);
    }

    pub fn record_closure_struct_type(&mut self, sig: ClosureSig, idx: u32) {
        self.closure_struct_type_idx.insert(sig, idx);
    }

    pub fn record_env_type(&mut self, expr_id: ExprId, idx: u32) {
        self.env_type_idx.insert(expr_id, idx);
    }

    pub fn record_closure_func_idx(&mut self, expr_id: ExprId, idx: u32) {
        self.closure_func_idx.insert(expr_id, idx);
    }

    pub fn set_closure_vtable_global(&mut self, idx: u32) {
        self.closure_vtable_global_idx = Some(idx);
    }

    // Kept as Option so call sites pattern-match consistently with other optional resources.
    pub fn error_tag_idx(&self) -> Option<u32> {
        self.error_tag_idx
    }

    pub fn set_error_tag_idx(&mut self, idx: u32) {
        self.error_tag_idx = Some(idx);
    }

    pub fn record_adapter_func_idx(&mut self, mangled: MangledName, idx: u32) {
        self.adapter_func_idx.insert(mangled, idx);
    }

    pub fn record_cast_validator(&mut self, key: Type, idx: u32) {
        self.cast_validator_idx.insert(key, idx);
    }

    /// Func index of the `as`-cast recursive validator for an `AliasRef` back-edge.
    pub fn cast_validator_idx(&self, key: &Type) -> Option<u32> {
        self.cast_validator_idx.get(key).copied()
    }

    pub fn record_func(&mut self, mangled: MangledName, idx: u32) {
        self.funcs.insert(mangled, idx);
    }

    pub fn record_global(&mut self, mangled: MangledName, idx: u32) {
        self.globals.insert(mangled, idx);
    }

    /// Writes both the wasm function index and the static-call registry entry;
    /// splitting them invited callers to drop one half by accident.
    pub fn record_imported_fn(
        &mut self,
        mangled: MangledName,
        idx: u32,
        params: Vec<Type>,
        ret: Type,
        is_host: bool,
    ) {
        self.funcs.insert(mangled.clone(), idx);
        self.top_level_fns.insert(
            mangled,
            TopLevelFn {
                wasm_idx: idx,
                params,
                ret,
                is_host,
            },
        );
    }

    pub fn record_local_fn(
        &mut self,
        mangled: MangledName,
        idx: u32,
        params: Vec<Type>,
        ret: Type,
    ) {
        // Guard against an accidental mangle/source-name swap.
        debug_assert!(
            mangled.as_str().contains(crate::mangle::SEP),
            "local fn mangled name must contain the separator: {}",
            mangled.as_str(),
        );
        self.funcs.insert(mangled.clone(), idx);
        self.top_level_fns.insert(
            mangled,
            TopLevelFn {
                wasm_idx: idx,
                params,
                ret,
                is_host: false,
            },
        );
    }

    pub fn top_level_fn(&self, mangled: &MangledName) -> Option<&TopLevelFn> {
        self.top_level_fns.get(mangled)
    }

    pub fn record_iface_dispatch(&mut self, iface: MangledName, dispatch: Dispatch) {
        self.iface_dispatch.insert(iface, dispatch);
    }

    pub fn iface_dispatch(&self, iface: &MangledName) -> Option<Dispatch> {
        self.iface_dispatch.get(iface).copied()
    }

    pub fn record_iface_method_abi(&mut self, method_key: MangledName, abi: MethodSlotAbi) {
        self.iface_method_abi.insert(method_key, abi);
    }

    pub fn iface_method_abi(&self, method_key: &MangledName) -> Option<&MethodSlotAbi> {
        self.iface_method_abi.get(method_key)
    }

    pub fn record_intrinsic_member(&mut self, mangled: MangledName) {
        self.intrinsic_members.insert(mangled);
    }

    pub fn is_intrinsic_member(&self, mangled: &MangledName) -> bool {
        self.intrinsic_members.contains(mangled)
    }

    pub fn value_type(&self, ty: &Type) -> ValType {
        let ty = ty.peel();
        match ty {
            Type::Number | Type::NumberLiteral(_) => ValType::F64,
            Type::Boolean => ValType::I32,
            Type::String | Type::StringLiteral(_) => {
                let idx = self.string_type_idx().expect(
                    "Type::String requires the intrinsic types to be declared (declare_intrinsic_types)",
                );
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::BigInt => {
                let idx = self.bigint_type_idx().expect(
                    "Type::BigInt requires the intrinsic types to be declared (declare_intrinsic_types)",
                );
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Object { .. } => {
                // All objects lower to `$ObjectShape` (not the arity-specific subtype):
                // sibling per-arity subtypes are not Wasm subtypes of each other, so
                // width subtyping and object-only unions require `$ObjectShape` as the
                // common supertype.
                let idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object_shape;
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Array(_) => {
                let idx = self.array_type_idx().expect(
                    "Type::Array requires the intrinsic types to be declared (declare_intrinsic_types)",
                );
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Tuple(_) => {
                // Tuples lower to `$Array` — they are arrays at runtime; positional
                // element types are tracked by the typechecker only.
                let idx = self.array_type_idx().expect(
                    "Type::Tuple requires the intrinsic types to be declared (declare_intrinsic_types)",
                );
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Uint8Array => {
                let idx = self.uint8_array_type_idx().expect(
                    "Type::Uint8Array requires the intrinsic types to be declared (declare_intrinsic_types)",
                );
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::TypeVar(_) | Type::GenericParam { .. } | Type::Unknown => {
                // Erased-generic and dynamic slots lower to nullable `(ref null $Object)`;
                // nullable so `null` literals can flow in via WasmGC subtyping.
                let object_idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object;
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(object_idx),
                })
            }
            Type::Function { .. } => {
                let sig = crate::codegen::closures::classify(ty);
                let idx = self.closure_struct_type_idx(sig).unwrap_or_else(|| {
                    panic!(
                        "ClosureSig {sig:?} (from {ty:?}) not registered — \
                         closures::emit_func_and_struct_types should have recorded it"
                    )
                });
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Null => {
                let object_idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object;
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(object_idx),
                })
            }
            Type::Void | Type::Error => {
                // `void` is a return type only; `Error` only follows a reported typecheck
                // failure. Either arm here is a compiler bug.
                unreachable!("value_type called on {ty:?}: void/error never occupy a value slot")
            }
            Type::Never => {
                // The surrounding expression always diverges, but a `: never` function's
                // result slot still needs a concrete Wasm type for the validator. Share
                // the `Unknown` lowering — the call always throws before any value reaches
                // the slot.
                let object_idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object;
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(object_idx),
                })
            }
            Type::Union(members) => {
                let lowered: Vec<ValType> = members.iter().map(|m| self.value_type(m)).collect();
                let first_lowering = *lowered.first().expect("Type::Union holds ≥2 members");
                if lowered.iter().all(|&lowering| lowering == first_lowering) {
                    return first_lowering;
                }
                // Object-only union → `$ObjectShape`; mixed union → `$Object`.
                let all_object_or_null = members
                    .iter()
                    .all(|m| matches!(m.peel(), Type::Object { .. } | Type::Null));
                let heap_idx = if all_object_or_null {
                    self.intrinsic_type_indices()
                        .expect("intrinsics declared by codegen entry")
                        .object_shape
                } else {
                    self.intrinsic_type_indices()
                        .expect("intrinsics declared by codegen entry")
                        .object
                };
                // Nullable iff some member *lowers* nullable, not iff one is
                // spelled `null`: a `TypeVar`, `GenericParam`, `unknown`, or an
                // unrecorded `ClassRef` can hold null after instantiation
                // without naming it, and a non-null slot would trap the write.
                let nullable = lowered.iter().copied().any(is_nullable_ref);
                ValType::Ref(RefType {
                    nullable,
                    heap_type: HeapType::Concrete(heap_idx),
                })
            }
            // ClassRef lowers to the universal `(ref null $Object)` in this
            // typecheck-only slice; the concrete `(ref $Foo)` lands with class
            // codegen (SUB-480).
            // A locally-known class lowers to its concrete `(ref $Foo)` (non-null);
            // `struct.new`/`struct.get`/`struct.set` need the concrete type. A class
            // not in this module's rec group (cross-package, SUB-488) falls back to
            // the universal `(ref null $Object)`.
            Type::ClassRef { mangled, .. } => match self.class_struct_type_idx(mangled) {
                Some(idx) => ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                }),
                None => ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(
                        self.intrinsic_type_indices()
                            .expect("intrinsics declared by codegen entry")
                            .object,
                    ),
                }),
            },
            Type::InterfaceRef { .. } => {
                let object_idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object;
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(object_idx),
                })
            }
            Type::AliasRef { .. } => {
                // A recursion back-edge's runtime value is a heap object
                // (object-shaped alias) or a boxed/erased union member —
                // both lower to the universal `(ref null $Object)`, the
                // same as `InterfaceRef`. The structural type is tracked
                // only at the typechecker layer.
                let object_idx = self
                    .intrinsic_type_indices()
                    .expect("intrinsics declared by codegen entry")
                    .object;
                ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(object_idx),
                })
            }
            Type::NumberEnum { .. } => {
                let idx = self
                    .boxed_number_type_idx()
                    .expect("Type::NumberEnum requires the intrinsic types to be declared");
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::StringEnum { .. } => {
                let idx = self
                    .string_type_idx()
                    .expect("Type::StringEnum requires the intrinsic types to be declared");
                ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(idx),
                })
            }
            Type::Alias { .. } => unreachable!("peel guarantees no alias here (SUB-242)"),
        }
    }

    pub fn wasm_result(&self, ty: &Type) -> Vec<ValType> {
        if ty.is_void() {
            return vec![];
        }
        vec![self.value_type(ty)]
    }

    /// Wasm value type of a class-method **vtable-slot** parameter or result,
    /// of a **constructor** parameter, or of a **box cell**'s payload.
    ///
    /// These slots erase every class type and type variable to the boxed
    /// object slot. [`value_type`](Self::value_type) cannot serve here because
    /// its `ClassRef` lowering is phase-dependent: it yields the concrete
    /// struct once that struct type is recorded, and imported classes are
    /// reconstructed *before* local slot signatures are built while local
    /// classes are reserved *after* — so one declaration would lower two ways.
    /// Erasing unconditionally keeps a slot's ABI stable across phases and
    /// identical between a producer and a cross-package consumer, and keeps it
    /// in step with the types recorded for each slot ([`MethodSlotAbi`] for a
    /// method, `class_ctor_abi` for a constructor).
    pub fn slot_value_type(&self, ty: &Type) -> ValType {
        if is_erased(ty) {
            let object = self
                .intrinsic_type_indices()
                .expect("intrinsics declared before class slot signatures")
                .object;
            return ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(object),
            });
        }
        self.value_type(ty)
    }

    pub fn slot_wasm_result(&self, ty: &Type) -> Vec<ValType> {
        if ty.is_void() {
            return vec![];
        }
        vec![self.slot_value_type(ty)]
    }

    pub fn host_value_type(&self, ty: &Type) -> ValType {
        if matches!(ty, Type::String | Type::StringLiteral(_)) {
            let idx = self.raw_string_type_idx().expect(
                "Type::String requires the intrinsic types to be declared (declare_intrinsic_types)",
            );
            return ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(idx),
            });
        }
        if matches!(ty, Type::Uint8Array) {
            let idx = self.raw_uint8_array_type_idx().expect(
                "Type::Uint8Array requires the intrinsic types to be declared (declare_intrinsic_types)",
            );
            return ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(idx),
            });
        }
        self.value_type(ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn func_idx_round_trip_across_name_shapes() {
        let mut map = SymbolTable::default();
        map.record_local_fn(
            crate::mangle::package_symbol("main", "main"),
            10,
            Vec::new(),
            Type::Void,
        );
        map.record_imported_fn(
            crate::mangle::prelude("string_concat"),
            11,
            vec![Type::String, Type::String],
            Type::String,
            false,
        );
        map.record_func(
            crate::mangle::extend(&crate::mangle::prelude("Number"), "toString"),
            12,
        );

        assert_eq!(
            map.func_idx(&crate::mangle::package_symbol("main", "main")),
            Some(10)
        );
        assert_eq!(map.prelude_func_idx("string_concat"), Some(11));
        assert_eq!(
            map.func_idx(&crate::mangle::extend(
                &crate::mangle::prelude("Number"),
                "toString",
            )),
            Some(12),
        );

        assert_eq!(
            map.func_idx(&crate::mangle::package_symbol("main", "string_concat")),
            None,
        );
        assert_eq!(map.prelude_func_idx("toString"), None);
    }
}
