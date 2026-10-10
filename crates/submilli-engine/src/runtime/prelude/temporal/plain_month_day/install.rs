use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, Linker};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::register_host_fn;
use crate::{PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let obj = &types.object;
    shared::reg_plain_from(
        linker,
        engine,
        &types.string,
        obj,
        "PlainMonthDayConstructor",
        "PlainMonthDay#from",
        |s, caller| {
            let (month, day) = super::parse(s).map_err(crate::runtime::host::range_error)?;
            shared::make_plain_month_day(caller, month, day)
        },
    )?;
    for method in ["toString", "toJSON"] {
        shared::reg_plain_string(
            linker,
            engine,
            obj,
            &types.string,
            "PlainMonthDay",
            method,
            shared::plain_to_string_month_day,
        )?;
    }
    shared::reg_plain_equals(
        linker,
        engine,
        obj,
        "PlainMonthDay",
        shared::plain_equals_month_day,
    )?;
    shared::reg_plain_i32_getter(linker, engine, obj, "PlainMonthDay", "month", 1)?;
    shared::reg_plain_i32_getter(linker, engine, obj, "PlainMonthDay", "day", 2)?;
    shared::reg_plain_month_code_getter(linker, engine, obj, &types.string, "PlainMonthDay", 1)?;
    reg_plain_month_day_with(
        linker,
        engine,
        FuncType::new(
            engine,
            [obj.clone(), types.object_shape.clone()],
            [obj.clone()],
        ),
    )?;
    reg_plain_month_day_to_plain_date(
        linker,
        FuncType::new(
            engine,
            [obj.clone(), types.object_shape.clone()],
            [obj.clone()],
        ),
    )
}

fn reg_plain_month_day_with(
    linker: &mut Linker<StoreData>,
    _engine: &wasmtime::Engine,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainMonthDay", "with"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainMonthDay.with")?;
            let m = clamp_month(
                object_i64_field(caller, abi_arg(params, 1)?, "month", "PlainMonthDay.with")?
                    .unwrap_or_else(|| i64::from(st_i32_or(caller, st, 1))),
            );
            let dim = days_in_month(1972, m, "PlainMonthDay.with")?;
            let d = object_i64_field(caller, abi_arg(params, 1)?, "day", "PlainMonthDay.with")?
                .unwrap_or_else(|| i64::from(st_i32_or(caller, st, 2)))
                .clamp(1, dim);
            let date = y_m_d_date(1972, m, d, "PlainMonthDay.with")?;
            *abi_result(results, 0)? =
                make_plain_month_day(caller, i32::from(date.month()), i32::from(date.day()))?;
            Ok(())
        },
    )
}

fn reg_plain_month_day_to_plain_date(
    linker: &mut Linker<StoreData>,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainMonthDay", "toPlainDate"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainMonthDay.toPlainDate")?;
            let month = clamp_month(i64::from(st_i32(
                caller,
                st,
                1,
                "PlainMonthDay.toPlainDate",
            )?));
            let recv_day = i64::from(st_i32(caller, st, 2, "PlainMonthDay.toPlainDate")?);
            let year = object_i64_field(
                caller,
                abi_arg(params, 1)?,
                "year",
                "PlainMonthDay.toPlainDate",
            )?
            .ok_or_else(|| {
                wasmtime::Error::msg("Temporal.PlainMonthDay.toPlainDate: `year` is required")
            })?;
            let dim = days_in_month(year, month, "PlainMonthDay.toPlainDate")?;
            let d = y_m_d_date(
                year,
                month,
                recv_day.clamp(1, dim),
                "PlainMonthDay.toPlainDate",
            )?;
            *abi_result(results, 0)? = make_plain_date(
                caller,
                i32::from(d.year()),
                i32::from(d.month()),
                i32::from(d.day()),
            )?;
            Ok(())
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let ty = || shared::temporal_type("PlainMonthDay");
    let receiver = || Param::new("receiver", ty());
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("PlainMonthDayConstructor", "from"),
        vec![Param::new("s", Type::String)],
        ty(),
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainMonthDay", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("PlainMonthDay", "equals"),
        vec![receiver(), Param::new("other", ty())],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("PlainMonthDay", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        ty(),
    );
    shared::declare_direct_fn(
        defs,
        "toPlainDate",
        shared::prelude_key("PlainMonthDay", "toPlainDate"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        shared::temporal_type("PlainDate"),
    );
    for (field, ret) in [
        ("month", Type::Number),
        ("monthCode", Type::String),
        ("day", Type::Number),
    ] {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("PlainMonthDay", field),
            vec![receiver()],
            ret,
        );
    }
}
