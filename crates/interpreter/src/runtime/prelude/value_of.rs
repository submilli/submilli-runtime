//! Primitive `valueOf` methods return the receiver without copying its payload.

use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{abi_arg, abi_result, register_host_fn};
use crate::{MethodSig, PackageDeclaration, Param, Type};

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?;
    for (name, ty) in [
        ("Number", ValType::F64),
        ("Boolean", ValType::I32),
        (
            "String",
            ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.string.clone()),
            )),
        ),
        (
            "BigInt",
            ValType::Ref(RefType::new(
                false,
                HeapType::ConcreteStruct(intr.bigint.clone()),
            )),
        ),
    ] {
        register_host_fn(
            linker,
            super::MODULE_NAME,
            crate::mangle::extend(&crate::mangle::prelude(name), "valueOf"),
            FuncType::new(&engine, [ty.clone()], [ty]),
            |_caller, params, results| {
                *abi_result(results, 0)? = *abi_arg(params, 0)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

pub(super) fn declare(defs: &mut PackageDeclaration) {
    for (name, ty) in [
        ("Number", Type::Number),
        ("Boolean", Type::Boolean),
        ("String", Type::String),
        ("BigInt", Type::BigInt),
    ] {
        super::declare_method(
            defs,
            "valueOf",
            crate::mangle::extend(&crate::mangle::prelude(name), "valueOf"),
            vec![Param::new("value", ty.clone())],
            ty,
        );
    }
}

pub(super) fn signature(ret: Type) -> MethodSig {
    MethodSig {
        overloads: Vec::new(),
        optional: false,
        generics: Default::default(),
        params: Vec::new(),
        ret,
        predicate: None,
        doc: super::declaration::doc("/** Returns this primitive value unchanged. */"),
    }
}
