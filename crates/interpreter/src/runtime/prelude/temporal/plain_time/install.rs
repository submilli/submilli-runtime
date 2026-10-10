use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use super::super::shared::*;
use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::register_host_fn;
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let obj = &types.object;
    shared::reg_plain_from(
        linker,
        engine,
        &types.string,
        obj,
        "PlainTimeConstructor",
        "PlainTime#from",
        |s, caller| {
            let time = super::parse(s).map_err(crate::runtime::host::range_error)?;
            shared::make_plain_time(
                caller,
                i32::from(time.hour()),
                i32::from(time.minute()),
                i32::from(time.second()),
                time.subsec_nanosecond(),
            )
        },
    )?;
    for method in ["toString", "toJSON"] {
        shared::reg_plain_string(
            linker,
            engine,
            obj,
            &types.string,
            "PlainTime",
            method,
            shared::plain_to_string_time,
        )?;
    }
    shared::reg_plain_equals(linker, engine, obj, "PlainTime", shared::plain_equals_time)?;
    shared::reg_plain_compare(linker, engine, obj, "PlainTimeConstructor", &[1, 2, 3, 4])?;
    for (name, field) in [("hour", 1), ("minute", 2), ("second", 3)] {
        shared::reg_plain_i32_getter(linker, engine, obj, "PlainTime", name, field)?;
    }
    shared::reg_plain_time_getters(linker, engine, obj, "PlainTime")?;
    let pair = FuncType::new(engine, [obj.clone(), obj.clone()], [obj.clone()]);
    for method in ["add", "subtract"] {
        reg_plain_time_duration_op(linker, types.intr.clone(), method, pair.clone())?;
    }
    reg_plain_time_with(
        linker,
        engine,
        FuncType::new(
            engine,
            [obj.clone(), types.object_shape.clone()],
            [obj.clone()],
        ),
    )?;
    let options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(types.intr.object.clone()),
    ));
    let pair_with_options =
        FuncType::new(engine, [obj.clone(), obj.clone(), options], [obj.clone()]);
    for method in ["until", "since"] {
        reg_plain_time_pair_op(linker, method, pair_with_options.clone())?;
    }
    Ok(())
}

fn reg_plain_time_duration_op(
    linker: &mut Linker<StoreData>,
    intr: IntrinsicTypes,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainTime.{method}");
    let add = method == "add";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainTime", method),
        ty,
        true,
        move |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let time = plain_time_from_struct(caller, st, 0, &label)?;
            let span = read_duration_like(caller, abi_arg(params, 1)?, &intr, &label)?;
            let out = if add {
                super::add(time, span)
            } else {
                super::subtract(time, span)
            };
            *abi_result(results, 0)? = make_plain_time(
                caller,
                i32::from(out.hour()),
                i32::from(out.minute()),
                i32::from(out.second()),
                out.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_time_with(
    linker: &mut Linker<StoreData>,
    _engine: &wasmtime::Engine,
    ty: FuncType,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainTime", "with"),
        ty,
        true,
        |caller, params, results| {
            let st = as_struct_val(caller, abi_arg(params, 0)?, "PlainTime.with")?;
            let time = merge_time(
                (
                    st_i32(caller, st, 1, "PlainTime.with")?,
                    st_i32(caller, st, 2, "PlainTime.with")?,
                    st_i32(caller, st, 3, "PlainTime.with")?,
                    st_i32(caller, st, 4, "PlainTime.with")?,
                ),
                object_time_bag(caller, abi_arg(params, 1)?, "PlainTime.with")?,
                "PlainTime.with",
            )?;
            *abi_result(results, 0)? = make_plain_time(
                caller,
                i32::from(time.hour()),
                i32::from(time.minute()),
                i32::from(time.second()),
                time.subsec_nanosecond(),
            )?;
            Ok(())
        },
    )
}

fn reg_plain_time_pair_op(
    linker: &mut Linker<StoreData>,
    method: &'static str,
    ty: FuncType,
) -> wasmtime::Result<()> {
    let label = format!("PlainTime.{method}");
    let until = method == "until";
    register_host_fn(
        linker,
        crate::runtime::prelude::MODULE_NAME,
        prelude_key("PlainTime", method),
        ty,
        true,
        move |caller, params, results| {
            let a_st = as_struct_val(caller, abi_arg(params, 0)?, &label)?;
            let b_st = as_struct_val(caller, abi_arg(params, 1)?, &label)?;
            let a = plain_time_from_struct(caller, a_st, 0, &label)?;
            let b = plain_time_from_struct(caller, b_st, 0, &label)?;
            let o = object_diff_options(
                caller,
                abi_arg(params, 2)?,
                if until {
                    "PlainTime.until"
                } else {
                    "PlainTime.since"
                },
            )?;
            let span = if until {
                a.until(o.time(b))
                    .map_err(temporal_err("PlainTime.until"))?
            } else {
                a.since(o.time(b))
                    .map_err(temporal_err("PlainTime.since"))?
            };
            *abi_result(results, 0)? = make_duration(caller, &span)?;
            Ok(())
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let ty = || shared::temporal_type("PlainTime");
    let receiver = || Param::new("receiver", ty());
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("PlainTimeConstructor", "from"),
        vec![Param::new("s", Type::String)],
        ty(),
    );
    for method in ["toString", "toJSON"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainTime", method),
            vec![receiver()],
            Type::String,
        );
    }
    shared::declare_direct_fn(
        defs,
        "equals",
        shared::prelude_key("PlainTime", "equals"),
        vec![receiver(), Param::new("other", ty())],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("PlainTimeConstructor", "compare"),
        vec![Param::new("a", ty()), Param::new("b", ty())],
        Type::Number,
    );
    let duration_like = || {
        Param::new(
            "duration",
            Type::Union(vec![
                shared::temporal_type("Duration"),
                shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
            ]),
        )
    };
    for method in ["add", "subtract"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainTime", method),
            vec![receiver(), duration_like()],
            ty(),
        );
    }
    for method in ["until", "since"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("PlainTime", method),
            vec![
                receiver(),
                Param::new("other", ty()),
                Param::with_default(
                    "options",
                    Type::Union(vec![shared::object_shape_type(&[]), Type::Undefined]),
                    DefaultValue::Undefined,
                ),
            ],
            shared::temporal_type("Duration"),
        );
    }
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("PlainTime", "with"),
        vec![
            receiver(),
            Param::new("fields", shared::object_shape_type(&[])),
        ],
        ty(),
    );
    for field in [
        "hour",
        "minute",
        "second",
        "millisecond",
        "microsecond",
        "nanosecond",
    ] {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("PlainTime", field),
            vec![receiver()],
            Type::Number,
        );
    }
}
