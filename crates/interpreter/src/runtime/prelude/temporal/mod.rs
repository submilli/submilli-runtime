//! Rust implementations of the prelude's Temporal surface.

mod duration;
mod instant;
pub(crate) mod like;
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
    structural_object: ValType,
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
        let structural_object = ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.object.clone()),
        ));
        let string = ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.string.clone()),
        ));
        Ok(Self {
            engine,
            intr,
            object,
            structural_object,
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
    zoned_date_time::prepare_zones()?;
    now::prepare_system_zone()?;
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
    for name in JSON_TYPES {
        shared::declare_direct_fn(
            defs,
            "fromJson",
            shared::prelude_key(&format!("{name}Constructor"), "fromJson"),
            vec![crate::Param::new("value", crate::Type::Unknown)],
            shared::temporal_type(name),
        );
    }
}

pub(crate) const JSON_TYPES: [&str; 8] = [
    "Instant",
    "Duration",
    "ZonedDateTime",
    "PlainDate",
    "PlainTime",
    "PlainDateTime",
    "PlainYearMonth",
    "PlainMonthDay",
];

/// The decoder is selected by nominal prelude identity, never a display name.
pub(crate) fn from_json_function(ty: &crate::Type) -> Option<crate::MangledName> {
    let crate::Type::InterfaceRef { mangled, .. } = ty.peel() else {
        return None;
    };
    JSON_TYPES
        .into_iter()
        .find(|name| *mangled == crate::mangle::extend(&crate::mangle::prelude("Temporal"), name))
        .map(|name| shared::prelude_key(&format!("{name}Constructor"), "fromJson"))
}
