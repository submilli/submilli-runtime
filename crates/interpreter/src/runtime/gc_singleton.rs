//! Helpers that declare one struct/array/func type as its own singleton rec group
//! via wasmtime's `RecGroupBuilder`. WasmGC structural canonicalization makes the
//! returned handle share the engine type-index a module emitting the same bytes
//! uses, so a host-built type flows into a guest slot of that type without trapping.

use crate::runtime::host::fatal_host_error;

use wasmtime::{
    ArrayType, Engine, FieldType, Finality, FuncType, RecGroupBuilder, StructType, ValType,
};

pub(crate) fn singleton_struct(
    engine: &Engine,
    finality: Finality,
    supertype: Option<StructType>,
    fields: Vec<FieldType>,
) -> wasmtime::Result<StructType> {
    let mut builder = RecGroupBuilder::new(engine);
    let id = builder.declare_struct();
    let mut def = builder.define_struct(id);
    def.finality(finality);
    if let Some(supertype) = supertype {
        def.supertype(supertype);
    }
    for field in fields {
        def.field(field);
    }
    def.finish();
    // This builder declared and defined id with this kind. Successful build
    // preserves its ID and kind, with no intervening mutation.
    Ok(builder
        .build()
        .map_err(fatal_host_error)?
        .get_struct(id)
        .expect("declared singleton struct retains its kind"))
}

pub(crate) fn singleton_array(
    engine: &Engine,
    finality: Finality,
    field: FieldType,
) -> wasmtime::Result<ArrayType> {
    let mut builder = RecGroupBuilder::new(engine);
    let id = builder.declare_array();
    let mut def = builder.define_array(id);
    def.finality(finality);
    def.element(field);
    def.finish();
    // This builder declared and defined id with this kind. Successful build
    // preserves its ID and kind, with no intervening mutation.
    Ok(builder
        .build()
        .map_err(fatal_host_error)?
        .get_array(id)
        .expect("declared singleton array retains its kind"))
}

/// Declare a non-final function type with no supertype — the shape codegen emits
/// for closure and method func signatures.
pub(crate) fn singleton_func(
    engine: &Engine,
    params: Vec<ValType>,
    results: Vec<ValType>,
) -> wasmtime::Result<FuncType> {
    let mut builder = RecGroupBuilder::new(engine);
    let id = builder.declare_func();
    let mut def = builder.define_func(id);
    def.finality(Finality::NonFinal);
    for param in params {
        def.param(param);
    }
    for result in results {
        def.result(result);
    }
    def.finish();
    // This builder declared and defined id with this kind. Successful build
    // preserves its ID and kind, with no intervening mutation.
    Ok(builder
        .build()
        .map_err(fatal_host_error)?
        .get_func(id)
        .expect("declared singleton func retains its kind"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::host::FatalHostError;

    #[test]
    fn invalid_singleton_layout_is_fatal_and_does_not_break_next_build() {
        let engine = crate::RuntimeConfig::default().engine().unwrap();
        let parent = singleton_struct(&engine, Finality::Final, None, vec![]).unwrap();
        let error =
            singleton_struct(&engine, Finality::NonFinal, Some(parent), vec![]).unwrap_err();
        assert!(error.is::<FatalHostError>(), "{error:#}");
        singleton_struct(&engine, Finality::NonFinal, None, vec![]).unwrap();
        singleton_array(
            &engine,
            Finality::Final,
            FieldType::new(wasmtime::Mutability::Var, wasmtime::StorageType::I16),
        )
        .unwrap();
        singleton_func(&engine, vec![ValType::I32], vec![]).unwrap();
    }
}
