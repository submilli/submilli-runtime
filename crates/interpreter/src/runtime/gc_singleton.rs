//! Helpers that declare one struct/array/func type as its own singleton rec group
//! via wasmtime's `RecGroupBuilder`. WasmGC structural canonicalization makes the
//! returned handle share the engine type-index a module emitting the same bytes
//! uses, so a host-built type flows into a guest slot of that type without trapping.

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
    Ok(builder
        .build()?
        .get_struct(id)
        .expect("singleton struct id should resolve to a struct"))
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
    Ok(builder
        .build()?
        .get_array(id)
        .expect("singleton array id should resolve to an array"))
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
    Ok(builder
        .build()?
        .get_func(id)
        .expect("singleton func id should resolve to a func"))
}
