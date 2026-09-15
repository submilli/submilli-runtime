//! Rust implementations of the prelude's Temporal surface.

mod duration;
mod instant;
mod now;
mod plain_date;
mod plain_date_time;
mod plain_month_day;
mod plain_time;
mod plain_year_month;
pub(crate) mod shared;
mod zoned_date_time;

use wasmtime::{Engine, HeapType, Linker, RefType, Store, ValType};

use crate::PackageDeclaration;
use crate::runtime::StoreData;
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::MODULE_NAME;

pub(crate) type TemporalAbi = shared::TemporalAbi;

pub(super) struct DirectTypes {
    engine: Engine,
    intr: IntrinsicTypes,
    object: ValType,
    object_shape: ValType,
    string: ValType,
}

impl DirectTypes {
    fn new(linker: &Linker<StoreData>) -> wasmtime::Result<Self> {
        let engine = linker.engine().clone();
        let intr = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?;
        let object = ValType::Ref(RefType::new(
            true,
            HeapType::ConcreteStruct(intr.object.clone()),
        ));
        let object_shape = ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.object_shape.clone()),
        ));
        let string = ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.string.clone()),
        ));
        Ok(Self {
            engine,
            intr,
            object,
            object_shape,
            string,
        })
    }
}

pub(crate) fn install_abi(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<TemporalAbi> {
    shared::install_abi(linker, store, intr, MODULE_NAME)
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let types = DirectTypes::new(linker)?;
    duration::install(linker, &types)?;
    instant::install(linker, &types)?;
    now::install(linker, &types)?;
    plain_date::install(linker, &types)?;
    plain_time::install(linker, &types)?;
    plain_date_time::install(linker, &types)?;
    plain_year_month::install(linker, &types)?;
    plain_month_day::install(linker, &types)?;
    zoned_date_time::install(linker, &types)
}

pub fn declare(defs: &mut PackageDeclaration) {
    duration::declare(defs);
    instant::declare(defs);
    now::declare(defs);
    plain_date::declare(defs);
    plain_time::declare(defs);
    plain_date_time::declare(defs);
    plain_year_month::declare(defs);
    plain_month_day::declare(defs);
    zoned_date_time::declare(defs);
}
