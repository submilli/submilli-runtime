//! Host-owned `Error` machinery: the shared exception tag, the `$Error` and
//! subclass vtable and field-names singletons, and the ported
//! constructor/`isError` surface.
//!
//! The tag is created per store (`Tag::new`) and defined in the linker under
//! `submilli:prelude`; every user module imports it via a
//! fixed unconditional import (there is exactly one tag — only `Error`s are
//! throwable — so it's fixed codegen plumbing, not a declaration entry), and a
//! throw raised anywhere unwinds to any module's `try_table`.
//! The engine matches tag imports by exact canonical type identity, so the
//! payload `FuncType` is built from the canonical intrinsic `$Error` struct type.
//!
//! `$Error` is class-shaped (see `codegen/intrinsics.rs`): the four
//! `$ObjectShape` header slots plus a mutable identity ID, with `message` at payload slot 0
//! and `name` at slot 1. Construction is host-only — guests call the imported
//! constructor; a user subclass's `super(...)` calls the self-first ctor-init.
//!
//! `RangeError`, `TypeError`, and `SyntaxError` are the host-implemented
//! `Error` subclasses:
//! same layout, one shared `(rec $Subclass_vtable $Subclass)` pair subtyping
//! the `$Error` pair (canonical identity includes the supertype, so consumers'
//! imported-class reconstruction produces the same engine types — and all
//! subclasses canonicalize to the same pair), and per-class vtable singletons
//! whose parent link is the `Error` vtable — the nominal-identity chain
//! `instanceof` and typed catch walk.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, Engine, Finality, Func, FuncType, Global, GlobalType, HeapType,
    Linker, Mutability, RecGroupBuilder, RefType, Rooted, Store, StructRef, StructRefPre,
    StructType, Tag, TagType, Val, ValType,
};

use super::MODULE_NAME;
use super::vtable::{as_struct, read_string_units};
use crate::runtime::StoreData;
use crate::runtime::fuel::host_func_async;
use crate::runtime::host::{
    fatal_host_error, register_host_fn, write_submilli_string, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::IntrinsicTypes;

const MESSAGE_SLOT: u32 = 0;
const NAME_SLOT: u32 = 1;
/// First payload slot for a subclass's own fields, after the inherited
/// `message`/`name`.
const OWN_SLOT_BASE: u32 = 2;

/// The host-implemented error classes. Selects the vtable singleton, struct
/// type, and `name` field text when the host constructs or throws an error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BuiltinErrorClass {
    Error,
    Range,
    QuotaExceeded,
    Type,
    Syntax,
    PermissionDenied,
}

impl BuiltinErrorClass {
    const SUBCLASSES: [Self; 5] = [
        Self::Range,
        Self::QuotaExceeded,
        Self::Type,
        Self::Syntax,
        Self::PermissionDenied,
    ];

    pub(crate) fn name_text(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Range => "RangeError",
            Self::QuotaExceeded => "QuotaExceededError",
            Self::Type => "TypeError",
            Self::Syntax => "SyntaxError",
            Self::PermissionDenied => "PermissionDeniedError",
        }
    }

    /// Own data fields beyond the inherited `message`/`name`, in payload-slot
    /// order — which must match the consumer's reconstruction: parent fields
    /// first, then own fields in `BTreeMap` (name) order. The constructor's
    /// trailing params follow the same order.
    fn own_fields(self) -> &'static [&'static str] {
        match self {
            Self::Error | Self::Range | Self::QuotaExceeded | Self::Type | Self::Syntax => &[],
            Self::PermissionDenied => &["caller", "capability", "reason"],
        }
    }

    fn vtable(self, handles: &crate::runtime::host::HostAbiHandles) -> Global {
        match self {
            Self::Error => handles.error_vtable,
            Self::Range => handles.range_error_vtable,
            Self::QuotaExceeded => handles.quota_exceeded_vtable,
            Self::Type => handles.type_error_vtable,
            Self::Syntax => handles.syntax_error_vtable,
            Self::PermissionDenied => handles.permission_denied_vtable,
        }
    }

    fn field_names(self, handles: &crate::runtime::host::HostAbiHandles) -> Global {
        match self {
            Self::Error | Self::Range | Self::QuotaExceeded | Self::Type | Self::Syntax => {
                handles.error_field_names
            }
            Self::PermissionDenied => handles.permission_denied_field_names,
        }
    }

    fn struct_type(self, handles: &crate::runtime::host::HostAbiHandles) -> StructType {
        match self {
            Self::Error => handles.error_type.clone(),
            Self::Range
            | Self::QuotaExceeded
            | Self::Type
            | Self::Syntax
            | Self::PermissionDenied => handles.error_subclass_type.clone(),
        }
    }
}

/// Store-bound handles for the host-owned Error machinery, cached in `HostAbi`.
#[derive(Clone, Copy)]
pub(crate) struct ErrorHost {
    pub vtable: Global,
    pub range_vtable: Global,
    pub quota_exceeded_vtable: Global,
    pub type_vtable: Global,
    pub syntax_vtable: Global,
    pub permission_denied_vtable: Global,
    pub field_names: Global,
    pub permission_denied_field_names: Global,
    pub tag: Tag,
}

fn tag_name() -> crate::MangledName {
    crate::mangle::prelude("__error_tag")
}

fn class_key(class: BuiltinErrorClass, member: &str) -> crate::MangledName {
    crate::mangle::extend(&crate::mangle::prelude(class.name_text()), member)
}

/// The `(rec $Subclass_vtable $Subclass)` pair shared by every built-in
/// subclass: same shape as the `$Error` pair but subtyping it, mirroring
/// the consumer-side imported-class reconstruction so the host's globals and
/// ctor signatures canonicalize with the guests' imports. All subclasses
/// canonicalize to this one pair — nominal identity lives in the per-class
/// vtable singletons, not the struct type. Not part of [`IntrinsicTypes`] —
/// it is a class pair, reconstructed by consumers only when used, not an
/// intrinsic every module declares.
pub(crate) fn build_error_subclass_types(
    engine: &Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<(StructType, StructType)> {
    let imm = Mutability::Const;
    let mut b = RecGroupBuilder::new(engine);
    let vtable_label = b.declare_struct();
    let struct_label = b.declare_struct();

    let mut def = b.define_struct(vtable_label);
    def.finality(Finality::NonFinal);
    def.supertype(intr.error_vtable.clone());
    for slot_fn in [
        &intr.to_string_fn,
        &intr.to_json_fn,
        &intr.equals_fn,
        &intr.hash_fn,
    ] {
        def.field(wasmtime::FieldType::new(
            imm,
            wasmtime::StorageType::ValType(ValType::Ref(RefType::new(
                false,
                (*slot_fn).clone().into(),
            ))),
        ));
    }
    def.field(wasmtime::FieldType::new(
        imm,
        wasmtime::StorageType::ValType(ValType::Ref(RefType::new(
            true,
            intr.class_vtable.clone().into(),
        ))),
    ));
    def.field(wasmtime::FieldType::new(
        imm,
        wasmtime::StorageType::ValType(ValType::I32),
    ));
    def.finish();

    let mut def = b.define_struct(struct_label);
    def.finality(Finality::NonFinal);
    def.supertype(intr.error.clone());
    def.forward_ref_field(vtable_label)
        .mutability(imm)
        .nullable(false)
        .finish();
    def.field(wasmtime::FieldType::new(
        wasmtime::Mutability::Var,
        wasmtime::StorageType::ValType(ValType::Ref(RefType::new(
            false,
            intr.field_names.clone().into(),
        ))),
    ));
    def.field(wasmtime::FieldType::new(
        wasmtime::Mutability::Var,
        wasmtime::StorageType::ValType(ValType::Ref(RefType::new(
            false,
            intr.object_fields.clone().into(),
        ))),
    ));
    def.field(wasmtime::FieldType::new(
        wasmtime::Mutability::Var,
        wasmtime::StorageType::ValType(ValType::Ref(RefType::ANYREF)),
    ));
    def.field(wasmtime::FieldType::new(
        wasmtime::Mutability::Var,
        wasmtime::StorageType::ValType(ValType::I64),
    ));
    def.finish();

    let g = b.build().map_err(fatal_host_error)?;
    let vtable = g
        .get_struct(vtable_label)
        .ok_or_else(|| fatal_host_error("error subclass vtable should be a struct"))?;
    let struct_ty = g
        .get_struct(struct_label)
        .ok_or_else(|| fatal_host_error("error subclass should be a struct"))?;
    Ok((vtable, struct_ty))
}

/// Create the tag, the `$Error_vtable` and subclass vtable singletons, and
/// the field-names singleton. Store-bound; runs from `install_vtables` after
/// the shared vtables exist (the field-name strings reuse the `$string` vtable
/// singleton).
pub(crate) fn install_store_bound(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    string_vtable: &Global,
) -> wasmtime::Result<ErrorHost> {
    let payload = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.error.clone()),
    ));
    let func = FuncType::new(store.engine(), [payload], []);
    let tag = Tag::new(&mut *store, &TagType::new(func))?;
    linker.define(&mut *store, MODULE_NAME, tag_name().as_str(), tag)?;

    let string_vtable_val = string_vtable.get(&mut *store);
    let field_names_global =
        |store: &mut Store<StoreData>, names: &[&str]| -> wasmtime::Result<Global> {
            let mut name_vals = Vec::with_capacity(names.len());
            for text in names {
                let raw = write_submilli_string(&mut *store, text)?;
                let pre = StructRefPre::new(&mut *store, intr.string.clone());
                let st = StructRef::new(
                    &mut *store,
                    &pre,
                    &[
                        string_vtable_val,
                        Val::AnyRef(Some(raw.to_anyref())),
                        Val::I64(0),
                    ],
                )?;
                name_vals.push(Val::AnyRef(Some(st.to_anyref())));
            }
            let pre = ArrayRefPre::new(&mut *store, intr.field_names.clone());
            let names = ArrayRef::new_fixed(&mut *store, &pre, &name_vals)?;
            Global::new(
                &mut *store,
                GlobalType::new(
                    ValType::Ref(RefType::new(
                        false,
                        HeapType::ConcreteArray(intr.field_names.clone()),
                    )),
                    Mutability::Const,
                ),
                Val::AnyRef(Some(names.to_anyref())),
            )
        };
    let field_names = field_names_global(store, &["message", "name"])?;
    let mut pd_names = vec!["message", "name"];
    pd_names.extend(BuiltinErrorClass::PermissionDenied.own_fields());
    let permission_denied_field_names = field_names_global(store, &pd_names)?;

    let slots = build_error_vtable(store, intr)?;
    let pre = StructRefPre::new(&mut *store, intr.error_vtable.clone());
    let vtable_struct = StructRef::new(
        &mut *store,
        &pre,
        &[
            Val::FuncRef(Some(slots[0])),
            Val::FuncRef(Some(slots[1])),
            Val::FuncRef(Some(slots[2])),
            Val::FuncRef(Some(slots[3])),
            // Nominal-identity parent link: Error is the chain root.
            Val::AnyRef(None),
            Val::I32(0),
        ],
    )?;
    let vtable = Global::new(
        &mut *store,
        GlobalType::new(
            ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.error_vtable.clone()),
            )),
            Mutability::Const,
        ),
        Val::AnyRef(Some(vtable_struct.to_anyref())),
    )?;
    // Resolve consumers' ordinary imported-class vtable import for `Error` —
    // the nominal-identity singleton `instanceof` walks up to.
    linker.define(
        &mut *store,
        MODULE_NAME,
        crate::codegen::classes::vtable_global_export_name(&crate::mangle::prelude("Error"))
            .as_str(),
        vtable,
    )?;

    let (subclass_vtable_ty, _) = build_error_subclass_types(store.engine(), intr)?;
    let [
        range_vtable,
        quota_exceeded_vtable,
        type_vtable,
        syntax_vtable,
        permission_denied_vtable,
    ] = BuiltinErrorClass::SUBCLASSES.map(|class| {
        install_subclass_vtable(
            linker,
            store,
            intr,
            &subclass_vtable_ty,
            vtable_struct,
            class,
        )
    });

    Ok(ErrorHost {
        vtable,
        range_vtable: range_vtable?,
        quota_exceeded_vtable: quota_exceeded_vtable?,
        type_vtable: type_vtable?,
        syntax_vtable: syntax_vtable?,
        permission_denied_vtable: permission_denied_vtable?,
        field_names,
        permission_denied_field_names,
        tag,
    })
}

/// Build one subclass's vtable singleton (parent-linked to the `Error`
/// vtable) and define it in the linker under the class's ordinary
/// imported-class vtable name.
fn install_subclass_vtable(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
    vtable_ty: &StructType,
    parent: Rooted<StructRef>,
    class: BuiltinErrorClass,
) -> wasmtime::Result<Global> {
    let slots = build_error_vtable(store, intr)?;
    let pre = StructRefPre::new(&mut *store, vtable_ty.clone());
    let vtable_struct = StructRef::new(
        &mut *store,
        &pre,
        &[
            Val::FuncRef(Some(slots[0])),
            Val::FuncRef(Some(slots[1])),
            Val::FuncRef(Some(slots[2])),
            Val::FuncRef(Some(slots[3])),
            // Parent link: the subclass extends Error.
            Val::AnyRef(Some(parent.to_anyref())),
            Val::I32(0),
        ],
    )?;
    let vtable = Global::new(
        &mut *store,
        GlobalType::new(
            ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(vtable_ty.clone()),
            )),
            Mutability::Const,
        ),
        Val::AnyRef(Some(vtable_struct.to_anyref())),
    )?;
    linker.define(
        &mut *store,
        MODULE_NAME,
        crate::codegen::classes::vtable_global_export_name(&crate::mangle::prelude(
            class.name_text(),
        ))
        .as_str(),
        vtable,
    )?;
    Ok(vtable)
}

/// The four universal slots for `$Error`/subclass instances. `toString`
/// follows JS `Error.prototype.toString` ("name: message", eliding the
/// separator when either side is empty); `toJson` is `"{}"`. Equality and
/// hashing use reference identity, as do generated Error subclass hooks.
fn build_error_vtable(
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<[Func; 4]> {
    let to_string = host_func_async(
        &mut *store,
        intr.to_string_fn.clone(),
        |mut caller, params, results| {
            Box::new(async move {
                let name = payload_units(
                    &mut caller,
                    abi_arg(params, 0)?,
                    NAME_SLOT,
                    "Error#toString",
                )?;
                let message = payload_units(
                    &mut caller,
                    abi_arg(params, 0)?,
                    MESSAGE_SLOT,
                    "Error#toString",
                )?;
                let text = match (name.is_empty(), message.is_empty()) {
                    (true, _) => message,
                    (_, true) => name,
                    (false, false) => {
                        let mut out = name;
                        out.extend(": ".encode_utf16());
                        out.extend_from_slice(&message);
                        out
                    }
                };
                let st = write_submilli_string_struct_units(&mut caller, &text)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let to_json = host_func_async(
        &mut *store,
        intr.to_json_fn.clone(),
        |mut caller, _params, results| {
            Box::new(async move {
                let units: Vec<u16> = "{}".encode_utf16().collect();
                let st = write_submilli_string_struct_units(&mut caller, &units)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    );

    let equals = host_func_async(
        &mut *store,
        intr.equals_fn.clone(),
        move |mut caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? = Val::I32(error_equals(&mut caller, params)? as i32);
                Ok(())
            })
        },
    );

    let hash = host_func_async(
        &mut *store,
        intr.hash_fn.clone(),
        |mut caller, params, results| {
            Box::new(async move {
                *abi_result(results, 0)? = Val::I32(super::vtable::identity_hash(
                    &mut caller,
                    abi_arg(params, 0)?,
                )? as i32);
                Ok(())
            })
        },
    );

    Ok([to_string, to_json, equals, hash])
}

/// Register the ported Error surface. Store-less (base linker); bodies read the
/// store-bound singletons from `HostAbi` at call time.
pub(crate) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?;
    let string_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let error_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.error.clone()),
    ));
    let object_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    // The allocating constructors declare the concrete `(ref $Error)` /
    // `(ref $Subclass)` results — import checking is by exact canonical
    // type, and consumers derive the concrete class type for each.
    register_host_fn(
        linker,
        MODULE_NAME,
        class_key(BuiltinErrorClass::Error, "constructor"),
        FuncType::new(&engine, [string_ref.clone()], [error_ref.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? =
                construct(caller, BuiltinErrorClass::Error, abi_arg(params, 0)?, &[])?;
            Ok(())
        },
    )?;

    let (_, subclass_ty) = build_error_subclass_types(&engine, &intr)?;
    let subclass_ref = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(subclass_ty)));
    for class in BuiltinErrorClass::SUBCLASSES {
        let ctor_params = vec![string_ref.clone(); 1 + class.own_fields().len()];
        register_host_fn(
            linker,
            MODULE_NAME,
            class_key(class, "constructor"),
            FuncType::new(&engine, ctor_params, [subclass_ref.clone()]),
            true,
            move |caller, params, results| {
                *abi_result(results, 0)? =
                    construct(caller, class, abi_arg(params, 0)?, &params[1..])?;
                Ok(())
            },
        )?;
    }

    // Self-first ctor-init: `super(message)` from a user `class MyError extends
    // Error` (or a built-in subclass) initializes the parent slots of an
    // already-allocated instance.
    for class in [
        BuiltinErrorClass::Error,
        BuiltinErrorClass::Range,
        BuiltinErrorClass::QuotaExceeded,
        BuiltinErrorClass::Type,
        BuiltinErrorClass::Syntax,
        BuiltinErrorClass::PermissionDenied,
    ] {
        let mut init_params = vec![object_ref.clone()];
        init_params.extend(vec![string_ref.clone(); 1 + class.own_fields().len()]);
        register_host_fn(
            linker,
            MODULE_NAME,
            class_key(class, "constructor_init"),
            FuncType::new(&engine, init_params, []),
            true,
            move |caller, params, _results| {
                let payload = payload_array(caller, abi_arg(params, 0)?, "Error#constructor_init")?;
                payload.set(&mut *caller, MESSAGE_SLOT, *abi_arg(params, 1)?)?;
                let name = name_string(caller, class)?;
                payload.set(&mut *caller, NAME_SLOT, name)?;
                for (i, own) in params[2..].iter().enumerate() {
                    payload.set(&mut *caller, OWN_SLOT_BASE + i as u32, *own)?;
                }
                Ok(())
            },
        )?;
    }

    let class_vtable = intr.class_vtable.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::static_member(&crate::mangle::prelude("Error"), "isError"),
        FuncType::new(&engine, [nullable_object.clone()], [ValType::I32]),
        true,
        move |caller, params, results| {
            *abi_result(results, 0)? =
                Val::I32(is_error(caller, abi_arg(params, 0)?, &class_vtable)? as i32);
            Ok(())
        },
    )?;

    Ok(())
}

/// Errors compare by identity regardless of mutable payload fields.
/// Generated Error subclass hooks use the same reference comparison.
fn error_equals(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<bool> {
    let (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) = (abi_arg(params, 0)?, abi_arg(params, 1)?)
    else {
        return Ok(false);
    };
    Rooted::ref_eq(&*caller, a, b)
}

/// Which of an Error instance's inherited `message`/`name` slots JSON leaves
/// out, as JavaScript does: `message` is not an own enumerable property, and
/// `name` is one only when the instance assigned it. An assigned `name` can't
/// be told from the one the constructor stored, so a `name` equal to a built-in
/// error class's is taken as the constructor's. Likewise a subclass that
/// redeclares `message` as a class field still has it left out. `None` for a
/// non-Error value.
pub(crate) fn json_hidden_slots(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Option<ErrorJsonSlots>> {
    let class_vtable = super::super::intrinsic_types::intrinsic_types(&mut *caller)?
        .class_vtable
        .clone();
    if !is_error(caller, value, &class_vtable)? {
        return Ok(None);
    }
    let name = payload_units(caller, value, NAME_SLOT, "JSON.stringify")?;
    let name_is_builtin = std::iter::once(BuiltinErrorClass::Error)
        .chain(BuiltinErrorClass::SUBCLASSES)
        .any(|class| name.iter().copied().eq(class.name_text().encode_utf16()));
    Ok(Some(ErrorJsonSlots {
        hides_name: name_is_builtin,
    }))
}

/// The payload slots of an Error instance that JSON serialization skips.
pub(crate) struct ErrorJsonSlots {
    hides_name: bool,
}

impl ErrorJsonSlots {
    pub(crate) fn hides(&self, slot: u32) -> bool {
        slot == MESSAGE_SLOT || (self.hides_name && slot == NAME_SLOT)
    }
}

/// Host-side twin of codegen's nominal `instanceof` walk
/// (`emit_nominal_instance_test`): read the value's vtable and, while it is a
/// `$ClassVTable`, walk the parent chain comparing by identity against the
/// store's `Error` vtable singleton. Shape can't carry the answer —
/// same-layout classes canonicalize to one WasmGC struct type.
fn is_error(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    class_vtable: &wasmtime::StructType,
) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(any)) = value else {
        return Ok(false);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(false);
    };
    let target = match abi(caller)?.error_vtable.get(&mut *caller) {
        Val::AnyRef(Some(target)) => target,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "Error.isError: malformed $Error_vtable global {other:?}"
            )));
        }
    };
    let mut link = st.field(&mut *caller, 0)?;
    loop {
        let Val::AnyRef(Some(vt)) = link else {
            return Ok(false);
        };
        let Some(vt_st) = vt.as_struct(&mut *caller)? else {
            return Ok(false);
        };
        if !vt_st.matches_ty(&mut *caller, class_vtable)? {
            return Ok(false);
        }
        if Rooted::ref_eq(&*caller, &vt, &target)? {
            return Ok(true);
        }
        link = vt_st.field(
            &mut *caller,
            crate::codegen::classes::VTABLE_PARENT_SLOT as usize,
        )?;
    }
}

fn abi(caller: &Caller<'_, StoreData>) -> wasmtime::Result<crate::runtime::host::HostAbiHandles> {
    crate::runtime::host::error_abi(caller)
}

/// Allocate an instance of `class` carrying `message` plus the class's own
/// fields (for the direct host-throw path). `own_fields` are the string texts
/// in payload-slot order and must match `class.own_fields()` in count.
pub(crate) fn construct_from_message(
    caller: &mut Caller<'_, StoreData>,
    class: BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
) -> wasmtime::Result<Rooted<StructRef>> {
    validate_own_field_count(class, own_fields.len())?;
    let message_units: Vec<u16> = message.encode_utf16().collect();
    let message = write_submilli_string_struct_units(&mut *caller, &message_units)?;
    let mut vals = Vec::with_capacity(own_fields.len());
    for text in own_fields {
        let units: Vec<u16> = text.encode_utf16().collect();
        let st = write_submilli_string_struct_units(&mut *caller, &units)?;
        vals.push(Val::AnyRef(Some(st.to_anyref())));
    }
    construct_struct(
        caller,
        class,
        &Val::AnyRef(Some(message.to_anyref())),
        &vals,
    )
}

fn validate_own_field_count(class: BuiltinErrorClass, actual: usize) -> wasmtime::Result<()> {
    let expected = class.own_fields().len();
    if actual != expected {
        return Err(fatal_host_error(format!(
            "{} construction expected {expected} own fields, got {actual}",
            class.name_text()
        )));
    }
    Ok(())
}

/// Allocate an instance of `class` with `message`, the class's `name`, and its
/// own fields in payload-slot order. The concrete struct type matters: a typed
/// catch arm `ref.cast`s to the annotation class after the brand walk matches,
/// so a subclass must be allocated as `$Subclass`, not
/// `$Error`-with-a-subclass-vtable.
fn construct(
    caller: &mut Caller<'_, StoreData>,
    class: BuiltinErrorClass,
    message: &Val,
    own_fields: &[Val],
) -> wasmtime::Result<Val> {
    let error = construct_struct(caller, class, message, own_fields)?;
    Ok(Val::AnyRef(Some(error.to_anyref())))
}

/// [`construct`], keeping the generation-stamped handle that `to_anyref` drops.
fn construct_struct(
    caller: &mut Caller<'_, StoreData>,
    class: BuiltinErrorClass,
    message: &Val,
    own_fields: &[Val],
) -> wasmtime::Result<Rooted<StructRef>> {
    let handles = abi(caller)?;
    let name = name_string(caller, class)?;
    let mut payload_vals = vec![*message, name];
    payload_vals.extend_from_slice(own_fields);
    let pre = ArrayRefPre::new(&mut *caller, handles.object_fields_type.clone());
    let payload = ArrayRef::new_fixed(&mut *caller, &pre, &payload_vals)?;
    let vtable = class.vtable(&handles).get(&mut *caller);
    let field_names = class.field_names(&handles).get(&mut *caller);
    let pre = StructRefPre::new(&mut *caller, class.struct_type(&handles));
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            field_names,
            Val::AnyRef(Some(payload.to_anyref())),
            Val::AnyRef(None),
            Val::I64(0),
        ],
    )?;
    Ok(st)
}

fn name_string(
    caller: &mut Caller<'_, StoreData>,
    class: BuiltinErrorClass,
) -> wasmtime::Result<Val> {
    let units: Vec<u16> = class.name_text().encode_utf16().collect();
    let st = write_submilli_string_struct_units(&mut *caller, &units)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Read the receiver's object-fields payload array (header slot 2).
fn payload_array(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    name: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let st = as_struct(caller, receiver, name)?;
    match st.field(&mut *caller, 2)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "{name}: malformed $Error payload {other:?}"
        ))),
    }
}

/// Read one payload string's code units.
fn payload_units(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    slot: u32,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let payload = payload_array(caller, receiver, name)?;
    let val = payload.get(&mut *caller, slot)?;
    read_string_units(caller, &val, name)
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{MethodSig, Param, Span, Type, TypeKind, TypeSymbol};
    use std::collections::BTreeMap;
    defs.types.insert(
        "Error".to_string(),
        TypeSymbol {
            name: "Error".to_string(),
            mangled_name: crate::mangle::prelude("Error"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                fields: BTreeMap::from([
                    (
                        "message".to_string(),
                        crate::FieldSig {
                            ty: Type::String,
                            visibility: crate::Visibility::Public,
                            readonly: false,
                            optional: false,
                            doc: doc(
                                "/** The human-readable message passed to `new Error(message)`. */",
                            ),
                        },
                    ),
                    (
                        "name".to_string(),
                        crate::FieldSig {
                            ty: Type::String,
                            visibility: crate::Visibility::Public,
                            readonly: false,
                            optional: false,
                            doc: doc(
                                "/** The error class name. Defaults to `\"Error\"`; subclasses conventionally set `this.name` in their constructor. */",
                            ),
                        },
                    ),
                ]),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![Param::new("message", Type::String)],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::from([(
                    "isError".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![Param::new("value", Type::Unknown)],
                        ret: Type::Boolean,
                        predicate: Some(crate::TypePredicate {
                            parameter_index: 0,
                            asserted_type: Type::prelude_error_class(),
                        }),
                        doc: doc(
                            "/**\n * Returns `true` when `value` is an `Error` instance (including subclasses). The static-typed true branch narrows `value` to `Error`.\n * @param value The value to test.\n */",
                        ),
                    },
                )]),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: None,
                implements: Vec::new(),
                doc: doc(
                    "/** The built-in error class. `throw` accepts `Error` and its subclasses; declare custom errors with `class MyError extends Error { ... }`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "RangeError".to_string(),
        TypeSymbol {
            name: "RangeError".to_string(),
            mangled_name: crate::mangle::prelude("RangeError"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                // No own fields — `message`/`name` are inherited from `Error`.
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![Param::new("message", Type::String)],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: Some(crate::ClassExtends::plain(crate::mangle::prelude("Error"))),
                implements: Vec::new(),
                doc: doc(
                    "/** The built-in range-error class (`extends Error`, `name` = `\"RangeError\"`). Thrown by the runtime for out-of-range values: array index out of range, bigint division by zero, `String.repeat` with a negative count, invalid Temporal values, and similar. Catch selectively with `catch (e: RangeError)`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "QuotaExceededError".to_string(),
        TypeSymbol {
            name: "QuotaExceededError".to_string(),
            mangled_name: crate::mangle::prelude("QuotaExceededError"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                // No own fields — `message`/`name` are inherited from `Error`.
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![Param::new("message", Type::String)],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: Some(crate::ClassExtends::plain(crate::mangle::prelude("Error"))),
                implements: Vec::new(),
                doc: doc(
                    "/** A budget refusal (`extends Error`): filesystem space, model tokens, or session state. Free space, reduce the request, or ask the operator for a larger budget. */",
                ),
            },
        },
    );

    defs.types.insert(
        "TypeError".to_string(),
        TypeSymbol {
            name: "TypeError".to_string(),
            mangled_name: crate::mangle::prelude("TypeError"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                // No own fields — `message`/`name` are inherited from `Error`.
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![Param::new("message", Type::String)],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: Some(crate::ClassExtends::plain(crate::mangle::prelude("Error"))),
                implements: Vec::new(),
                doc: doc(
                    "/** The built-in type-error class (`extends Error`, `name` = `\"TypeError\"`). Thrown by the runtime when a value fails a type-shaped runtime check: a non-null assertion (`x!`) applied to `null`, a runtime-checked `as` cast that doesn't match (including `JSON.parse` result shapes), `TextDecoder.decode` of invalid UTF-8, or an invalid URL. Catch selectively with `catch (e: TypeError)`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "PermissionDeniedError".to_string(),
        TypeSymbol {
            name: "PermissionDeniedError".to_string(),
            mangled_name: crate::mangle::prelude("PermissionDeniedError"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                // Own fields beyond the inherited `message`/`name`. BTreeMap
                // (name) order fixes the payload slots — keep
                // `BuiltinErrorClass::PermissionDenied.own_fields()` in sync.
                fields: BTreeMap::from([
                    (
                        "caller".to_string(),
                        crate::FieldSig {
                            ty: Type::String,
                            visibility: crate::Visibility::Public,
                            readonly: false,
                            optional: false,
                            doc: doc(
                                "/** The package the denied call was attributed to (e.g. `\"main\"`). */",
                            ),
                        },
                    ),
                    (
                        "capability".to_string(),
                        crate::FieldSig {
                            ty: Type::String,
                            visibility: crate::Visibility::Public,
                            readonly: false,
                            optional: false,
                            doc: doc(
                                "/** The denied capability name (e.g. `\"fs.read\"`, `\"http.get\"`). */",
                            ),
                        },
                    ),
                    (
                        "reason".to_string(),
                        crate::FieldSig {
                            ty: Type::String,
                            visibility: crate::Visibility::Public,
                            readonly: false,
                            optional: false,
                            doc: doc("/** The policy-supplied denial reason. */"),
                        },
                    ),
                ]),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![
                    Param::new("message", Type::String),
                    Param::new("caller", Type::String),
                    Param::new("capability", Type::String),
                    Param::new("reason", Type::String),
                ],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: Some(crate::ClassExtends::plain(crate::mangle::prelude("Error"))),
                implements: Vec::new(),
                doc: doc(
                    "/** The built-in permission-denial class (`extends Error`, `name` = `\"PermissionDeniedError\"`). Thrown when a gated operation is denied (an explicit `security.check` or a gated stdlib call such as `fs.read` or `http.get`) — usually by the operator's security policy, and for a few capabilities by the runtime itself ahead of any policy, which no blueprint can override. The structured `capability`/`caller`/`reason` fields let recovery code react to the specific denial; the denial itself is final — do not retry through another route. Catch selectively with `catch (e: PermissionDeniedError)`. */",
                ),
            },
        },
    );

    defs.types.insert(
        "SyntaxError".to_string(),
        TypeSymbol {
            name: "SyntaxError".to_string(),
            mangled_name: crate::mangle::prelude("SyntaxError"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Class {
                generics: Vec::new(),
                // No own fields — `message`/`name` are inherited from `Error`.
                fields: BTreeMap::new(),
                narrowing_checks: BTreeMap::new(),
                methods: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: Vec::new(),
                constructor: vec![Param::new("message", Type::String)],
                constructor_visibility: crate::Visibility::Public,
                statics: BTreeMap::new(),
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: Some(crate::ClassExtends::plain(crate::mangle::prelude("Error"))),
                implements: Vec::new(),
                doc: doc(
                    "/** The built-in syntax-error class (`extends Error`, `name` = `\"SyntaxError\"`). Thrown by the runtime when text fails to parse: `JSON.parse` of malformed JSON, `BigInt()` of an invalid literal string, `Uint8Array.fromHex`/`fromBase64` of malformed input, `new RegExp()` of an invalid pattern or flags. Catch selectively with `catch (e: SyntaxError)`. */",
                ),
            },
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_error_field_count_returns_fatal_error() {
        for (class, invalid) in [
            (BuiltinErrorClass::Error, 1),
            (BuiltinErrorClass::PermissionDenied, 0),
            (BuiltinErrorClass::PermissionDenied, 4),
        ] {
            let error = validate_own_field_count(class, invalid).unwrap_err();
            assert!(error.is::<crate::runtime::host::FatalHostError>());
            assert!(error.to_string().contains(class.name_text()));
            validate_own_field_count(class, class.own_fields().len()).unwrap();
        }
    }
}
