//! Repository's ordinary imported-class layout and per-store vtable.
use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, Engine, FieldType, Finality, Func, FuncType, Global, GlobalType,
    HeapType, Linker, Mutability, RecGroupBuilder, RefType, Result, Rooted, StorageType, Store,
    StructRef, StructRefPre, StructType, Val, ValType,
};

use super::MODULE_NAME;
use crate::runtime::StoreData;
use crate::runtime::host::{fatal_host_error, register_host_fn_async, write_submilli_string};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types, intrinsic_types};
use crate::runtime::prelude::iterator::as_struct;

#[derive(Clone)]
struct RepositoryClass {
    instance_type: StructType,
    vtable: Global,
    field_names: Global,
}

// Class vtable slots follow the declaration's sorted method names.
const METHODS: [&str; 14] = [
    "add",
    "addRemote",
    "branches",
    "commit",
    "createBranch",
    "diff",
    "fetch",
    "log",
    "pull",
    "remotes",
    "setRemoteUrl",
    "show",
    "status",
    "switchBranch",
];

pub(crate) fn install(linker: &mut Linker<StoreData>, store: &mut Store<StoreData>) -> Result<()> {
    let engine = store.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let key = crate::mangle::package_symbol(MODULE_NAME, "Repository");
    let mut methods = Vec::new();
    for name in METHODS {
        let symbol = crate::mangle::extend(&key, name);
        let wasmtime::Extern::Func(method) =
            linker.get(&mut *store, MODULE_NAME, symbol.as_str())?
        else {
            wasmtime::bail!("git: missing class method");
        };
        methods.push(method);
    }
    let method_types = methods
        .iter()
        .map(|method| method.ty(&*store))
        .collect::<Vec<_>>();
    let (vtable_type, instance_type) = class_types(&engine, &intr, method_types)?;
    let vtable = make_vtable(store, vtable_type, methods)?;
    let field_names = make_field_names(store, &intr)?;
    linker.define(
        &mut *store,
        MODULE_NAME,
        crate::codegen::classes::vtable_global_export_name(&key).as_str(),
        vtable,
    )?;
    let class = RepositoryClass {
        instance_type: instance_type.clone(),
        vtable,
        field_names,
    };
    install_factories(linker, &engine, &intr, &class)?;
    install_constructor(linker, &engine, &intr, class)
}

fn class_types(
    engine: &Engine,
    intr: &IntrinsicTypes,
    methods: Vec<FuncType>,
) -> Result<(StructType, StructType)> {
    let mut builder = RecGroupBuilder::new(engine);
    let vtable_label = builder.declare_struct();
    let instance_label = builder.declare_struct();
    let mut def = builder.define_struct(vtable_label);
    def.finality(Finality::NonFinal);
    def.supertype(intr.class_vtable.clone());
    for ty in [
        &intr.to_string_fn,
        &intr.to_json_fn,
        &intr.equals_fn,
        &intr.hash_fn,
    ] {
        def.field(ref_field(ty.clone().into(), false));
    }
    def.field(ref_field(intr.class_vtable.clone().into(), true));
    def.field(FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::I32),
    ));
    for ty in methods {
        def.field(ref_field(ty.into(), false));
    }
    def.finish();
    let mut def = builder.define_struct(instance_label);
    def.finality(Finality::NonFinal);
    def.supertype(intr.object_shape.clone());
    def.forward_ref_field(vtable_label)
        .mutability(Mutability::Const)
        .nullable(false)
        .finish();
    def.field(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::Ref(RefType::new(
            false,
            intr.field_names.clone().into(),
        ))),
    ));
    def.field(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::Ref(RefType::new(
            false,
            intr.object_fields.clone().into(),
        ))),
    ));
    def.field(wasmtime::FieldType::new(
        wasmtime::Mutability::Var,
        wasmtime::StorageType::ValType(ValType::Ref(RefType::ANYREF)),
    ));
    def.field(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::I64),
    ));
    def.finish();
    let group = builder.build().map_err(fatal_host_error)?;
    Ok((
        group
            .get_struct(vtable_label)
            .ok_or_else(|| fatal_host_error("git class: declared vtable type is missing"))?,
        group
            .get_struct(instance_label)
            .ok_or_else(|| fatal_host_error("git class: declared instance type is missing"))?,
    ))
}

fn ref_field(heap: HeapType, nullable: bool) -> FieldType {
    FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::Ref(RefType::new(nullable, heap))),
    )
}

fn make_vtable(store: &mut Store<StoreData>, ty: StructType, methods: Vec<Func>) -> Result<Global> {
    let opaque = store
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| fatal_host_error("git class: prelude is not installed"))?
        .opaque_vtable;
    let value = opaque.get(&mut *store);
    let Val::AnyRef(Some(value)) = value else {
        wasmtime::bail!("missing opaque vtable")
    };
    let opaque = value.unwrap_struct(&mut *store)?;
    // Like guest classes, JSON contains field data, never class identity.
    let object = store
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| fatal_host_error("git class: prelude is not installed"))?
        .object_vtable;
    let Val::AnyRef(Some(value)) = object.get(&mut *store) else {
        wasmtime::bail!("missing object vtable")
    };
    let object = value.unwrap_struct(&mut *store)?;
    let mut slots = vec![
        opaque.field(&mut *store, 0)?,
        object.field(&mut *store, 1)?,
        opaque.field(&mut *store, 2)?,
        opaque.field(&mut *store, 3)?,
        Val::AnyRef(None),
        Val::I32(1),
    ];
    slots.extend(methods.into_iter().map(|method| Val::FuncRef(Some(method))));
    let pre = StructRefPre::new(&mut *store, ty.clone());
    let vtable = StructRef::new(&mut *store, &pre, &slots)?;
    Global::new(
        &mut *store,
        GlobalType::new(
            ValType::Ref(RefType::new(false, ty.into())),
            Mutability::Const,
        ),
        Val::AnyRef(Some(vtable.to_anyref())),
    )
}

fn make_field_names(store: &mut Store<StoreData>, intr: &IntrinsicTypes) -> Result<Global> {
    let string_vtable = store
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| fatal_host_error("git class: prelude is not installed"))?
        .string_vtable;
    let vtable = string_vtable.get(&mut *store);
    let raw = write_submilli_string(&mut *store, "path")?;
    let name_type = private_field_name_type(store.engine(), intr)?;
    let pre = StructRefPre::new(&mut *store, name_type);
    let name = StructRef::new(
        &mut *store,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(raw.to_anyref())),
            Val::I64(0),
            Val::I32(1),
            Val::I32(1),
        ],
    )?;
    let pre = ArrayRefPre::new(&mut *store, intr.field_names.clone());
    let names = ArrayRef::new_fixed(&mut *store, &pre, &[Val::AnyRef(Some(name.to_anyref()))])?;
    Global::new(
        &mut *store,
        GlobalType::new(
            ValType::Ref(RefType::new(false, intr.field_names.clone().into())),
            Mutability::Const,
        ),
        Val::AnyRef(Some(names.to_anyref())),
    )
}

/// Match the compiler's marked-name layout: presence followed by privacy.
fn private_field_name_type(engine: &Engine, intr: &IntrinsicTypes) -> Result<StructType> {
    let mut fields: Vec<_> = intr.string.fields().collect();
    fields.push(FieldType::new(
        Mutability::Var,
        StorageType::ValType(ValType::I32),
    ));
    fields.push(FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::I32),
    ));
    crate::runtime::gc_singleton::singleton_struct(
        engine,
        Finality::Final,
        Some(intr.string.clone()),
        fields,
    )
}

fn install_factories(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    class: &RepositoryClass,
) -> Result<()> {
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object_shape.clone()),
    ));
    let repository_result = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(class.instance_type.clone()),
    ));
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::static_member(
            &crate::mangle::package_symbol(MODULE_NAME, "Repository"),
            "open",
        ),
        FuncType::new(engine, [string.clone()], [repository_result.clone()]),
        false,
        {
            let class = class.clone();
            move |caller, params, results| {
                let class = class.clone();
                Box::pin(async move {
                    let path = super::invoke(caller, "open", false, params).await?;
                    *abi_result(results, 0)? = new_instance(caller, &class, path)?;
                    Ok(())
                })
            }
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::static_member(
            &crate::mangle::package_symbol(MODULE_NAME, "Repository"),
            "init",
        ),
        FuncType::new(
            engine,
            [string.clone(), options.clone()],
            [repository_result.clone()],
        ),
        false,
        {
            let class = class.clone();
            move |caller, params, results| {
                let class = class.clone();
                Box::pin(async move {
                    let path = super::invoke(caller, "init", false, params).await?;
                    *abi_result(results, 0)? = new_instance(caller, &class, path)?;
                    Ok(())
                })
            }
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::static_member(
            &crate::mangle::package_symbol(MODULE_NAME, "Repository"),
            "clone",
        ),
        FuncType::new(
            engine,
            [string.clone(), string.clone(), options.clone()],
            [repository_result.clone()],
        ),
        false,
        {
            let class = class.clone();
            move |caller, params, results| {
                let class = class.clone();
                Box::pin(async move {
                    let path = super::invoke(caller, "clone", false, params).await?;
                    *abi_result(results, 0)? = new_instance(caller, &class, path)?;
                    Ok(())
                })
            }
        },
    )?;
    Ok(())
}

fn install_constructor(
    linker: &mut Linker<StoreData>,
    engine: &Engine,
    intr: &IntrinsicTypes,
    class: RepositoryClass,
) -> Result<()> {
    let key = crate::mangle::package_symbol(MODULE_NAME, "Repository");
    let string = ValType::Ref(RefType::new(false, intr.string.clone().into()));
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&key, "constructor"),
        FuncType::new(
            engine,
            [string.clone()],
            [ValType::Ref(RefType::new(
                false,
                class.instance_type.clone().into(),
            ))],
        ),
        false,
        move |caller, params, results| {
            let class = class.clone();
            Box::pin(async move {
                let path = super::invoke(caller, "open", false, params).await?;
                *abi_result(results, 0)? = new_instance(caller, &class, path)?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&key, "constructor_init"),
        FuncType::new(
            engine,
            [
                ValType::Ref(RefType::new(false, intr.object.clone().into())),
                string,
            ],
            [],
        ),
        false,
        |caller, params, _results| {
            Box::pin(async move {
                let path = super::invoke(caller, "open", false, &params[1..]).await?;
                payload(caller, abi_arg(params, 0)?)?.set(&mut *caller, 0, path)?;
                Ok(())
            })
        },
    )
}

fn payload(caller: &mut Caller<'_, StoreData>, receiver: &Val) -> Result<Rooted<ArrayRef>> {
    let instance = as_struct(caller, receiver, "Repository")?;
    let fields = instance.field(&mut *caller, 2)?;
    match fields {
        Val::AnyRef(Some(value)) => value.unwrap_array(&mut *caller),
        _ => wasmtime::bail!("git: invalid repository payload"),
    }
}

pub(super) fn path(caller: &mut Caller<'_, StoreData>, receiver: &Val) -> Result<Val> {
    payload(caller, receiver)?.get(&mut *caller, 0)
}

fn new_instance(
    caller: &mut Caller<'_, StoreData>,
    class: &RepositoryClass,
    path: Val,
) -> Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let fields = ArrayRef::new_fixed(&mut *caller, &pre, &[path])?;
    let vtable = class.vtable.get(&mut *caller);
    let names = class.field_names.get(&mut *caller);
    let pre = StructRefPre::new(&mut *caller, class.instance_type.clone());
    let instance = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            names,
            Val::AnyRef(Some(fields.to_anyref())),
            Val::AnyRef(None),
            Val::I64(0),
        ],
    )?;
    Ok(Val::AnyRef(Some(instance.to_anyref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_class_setup_without_prelude_returns_fatal_error() {
        let config = crate::RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let intr = build_intrinsic_types(&engine).unwrap();
        let mut store = config
            .store(&engine, StoreData::with_vfs(crate::runtime::Vfs::none()))
            .unwrap();
        let error = make_field_names(&mut store, &intr).unwrap_err();
        assert!(error.is::<crate::runtime::host::FatalHostError>());
        let error = make_vtable(&mut store, intr.class_vtable, vec![]).unwrap_err();
        assert!(error.is::<crate::runtime::host::FatalHostError>());
    }
}
