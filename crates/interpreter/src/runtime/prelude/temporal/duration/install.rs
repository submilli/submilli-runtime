use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use super::super::{DirectTypes, shared};
use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn, write_submilli_string_struct};
use crate::runtime::prelude::MODULE_NAME;
use crate::{DefaultValue, PackageDeclaration, Param, Type};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let engine = &types.engine;
    let object = &types.object;
    let object_result = [object.clone()];
    let optional_options = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(types.intr.object.clone()),
    ));

    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("DurationConstructor", "new"),
        FuncType::new(engine, [types.object_shape.clone()], object_result.clone()),
        true,
        move |caller, params, results| {
            let span = shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration")?;
            *abi_result(results, 0)? = shared::make_duration(caller, &span)?;
            Ok(())
        },
    )?;
    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("DurationConstructor", "from"),
        FuncType::new(engine, [object.clone()], object_result.clone()),
        true,
        move |caller, params, results| {
            let span = if crate::runtime::prelude::collection::is_a(
                caller,
                abi_arg(params, 0)?,
                &intr.string,
            )? {
                let input = read_string_arg(caller, abi_arg(params, 0)?, "Temporal.Duration.from")?;
                super::parse(&input).map_err(crate::runtime::host::range_error)?
            } else {
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.from")?
            };
            *abi_result(results, 0)? = shared::make_duration(caller, &span)?;
            Ok(())
        },
    )?;

    let intr = types.intr.clone();
    for (method, add) in [("add", true), ("subtract", false)] {
        let intr = intr.clone();
        let label = if add {
            "Duration.add"
        } else {
            "Duration.subtract"
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("Duration", method),
            FuncType::new(
                engine,
                [object.clone(), object.clone()],
                object_result.clone(),
            ),
            true,
            move |caller, params, results| {
                let a = shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, label)?;
                let b = shared::read_duration_like(caller, abi_arg(params, 1)?, &intr, label)?;
                let out = if add {
                    super::add(a, b)
                } else {
                    super::subtract(a, b)
                }
                .map_err(crate::runtime::host::range_error)?;
                *abi_result(results, 0)? = shared::make_duration(caller, &out)?;
                Ok(())
            },
        )?;
    }

    let intr = types.intr.clone();
    for (method, transform) in [
        ("negated", super::negated as fn(jiff::Span) -> jiff::Span),
        ("abs", super::abs as fn(jiff::Span) -> jiff::Span),
    ] {
        let intr = intr.clone();
        let label = if method == "negated" {
            "Duration.negated"
        } else {
            "Duration.abs"
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("Duration", method),
            FuncType::new(engine, [object.clone()], object_result.clone()),
            true,
            move |caller, params, results| {
                let span = shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, label)?;
                *abi_result(results, 0)? = shared::make_duration(caller, &transform(span))?;
                Ok(())
            },
        )?;
    }

    for (index, field) in shared::DURATION_FIELD_NAMES.iter().enumerate() {
        let label = match *field {
            "years" => "Duration.years",
            "months" => "Duration.months",
            "weeks" => "Duration.weeks",
            "days" => "Duration.days",
            "hours" => "Duration.hours",
            "minutes" => "Duration.minutes",
            "seconds" => "Duration.seconds",
            "milliseconds" => "Duration.milliseconds",
            "microseconds" => "Duration.microseconds",
            "nanoseconds" => "Duration.nanoseconds",
            _ => {
                return Err(crate::runtime::host::invariant_trap(
                    "Temporal.Duration: unknown registered field",
                ));
            }
        };
        register_host_fn(
            linker,
            MODULE_NAME,
            shared::prelude_key("Duration", field),
            FuncType::new(engine, [object.clone()], [ValType::F64]),
            true,
            move |caller, params, results| {
                let value =
                    shared::duration_field_from_val(caller, abi_arg(params, 0)?, index, label)?;
                *abi_result(results, 0)? = Val::F64((value as f64).to_bits());
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "sign"),
        FuncType::new(engine, [object.clone()], [ValType::F64]),
        true,
        |caller, params, results| {
            let sign =
                shared::duration_sign_from_val(caller, abi_arg(params, 0)?, "Duration.sign")?;
            *abi_result(results, 0)? = Val::F64(f64::from(sign).to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "blank"),
        FuncType::new(engine, [object.clone()], [ValType::I32]),
        true,
        |caller, params, results| {
            let sign =
                shared::duration_sign_from_val(caller, abi_arg(params, 0)?, "Duration.blank")?;
            *abi_result(results, 0)? = Val::I32((sign == 0) as i32);
            Ok(())
        },
    )?;

    install_with(linker, types)?;
    install_round_total_compare(linker, types, optional_options.clone())?;
    install_strings(linker, types, optional_options)?;
    Ok(())
}

fn install_with(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "with"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [types.object.clone()],
        ),
        true,
        move |caller, params, results| {
            let base =
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.with")?;
            let mut fields = shared::span_fields(&base);
            if crate::runtime::prelude::collection::is_a(
                caller,
                abi_arg(params, 1)?,
                &intr.temporal_duration,
            )? {
                fields = shared::span_fields(&shared::read_duration_like(
                    caller,
                    abi_arg(params, 1)?,
                    &intr,
                    "Duration.with",
                )?);
            } else {
                for (index, name) in shared::DURATION_FIELD_NAMES.iter().enumerate() {
                    if let Some(value) = shared::object_duration_field(
                        caller,
                        abi_arg(params, 1)?,
                        name,
                        index,
                        "Duration.with",
                    )? {
                        fields[index] = value;
                    }
                }
            }
            let out = super::from_fields(fields, "Duration.with")
                .map_err(crate::runtime::host::range_error)?;
            *abi_result(results, 0)? = shared::make_duration(caller, &out)?;
            Ok(())
        },
    )
}

fn install_round_total_compare(
    linker: &mut Linker<StoreData>,
    types: &DirectTypes,
    options: ValType,
) -> wasmtime::Result<()> {
    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "round"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [types.object.clone()],
        ),
        true,
        move |caller, params, results| {
            let span =
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.round")?;
            let (smallest, largest, mode, increment, anchor) =
                if crate::runtime::prelude::collection::is_a(
                    caller,
                    abi_arg(params, 1)?,
                    &intr.string,
                )? {
                    let unit =
                        read_string_arg(caller, abi_arg(params, 1)?, "Temporal.Duration.round")?;
                    (
                        Some(shared::unit_from_str(&unit, "Duration.round")?),
                        None,
                        None,
                        None,
                        None,
                    )
                } else {
                    let smallest = shared::object_string_field(
                        caller,
                        abi_arg(params, 1)?,
                        "smallestUnit",
                        "Duration.round",
                    )?
                    .map(|unit| shared::unit_from_str(&unit, "Duration.round"))
                    .transpose()?;
                    let largest = shared::object_string_field(
                        caller,
                        abi_arg(params, 1)?,
                        "largestUnit",
                        "Duration.round",
                    )?
                    .map(|unit| shared::unit_from_str(&unit, "Duration.round"))
                    .transpose()?;
                    let mode = shared::object_string_field(
                        caller,
                        abi_arg(params, 1)?,
                        "roundingMode",
                        "Duration.round",
                    )?
                    .map(|mode| shared::round_mode_from_str(&mode, "Duration.round"))
                    .transpose()?;
                    let increment = shared::object_i64_field(
                        caller,
                        abi_arg(params, 1)?,
                        "roundingIncrement",
                        "Duration.round",
                    )?;
                    let anchor =
                        shared::object_relative_to(caller, abi_arg(params, 1)?, "Duration.round")?;
                    (smallest, largest, mode, increment, anchor)
                };
            let out = super::round(span, smallest, largest, mode, increment, anchor)
                .map_err(crate::runtime::host::range_error)?;
            *abi_result(results, 0)? = shared::make_duration(caller, &out)?;
            Ok(())
        },
    )?;

    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "total"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone()],
            [ValType::F64],
        ),
        true,
        move |caller, params, results| {
            let span =
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.total")?;
            let (unit, anchor) = if crate::runtime::prelude::collection::is_a(
                caller,
                abi_arg(params, 1)?,
                &intr.string,
            )? {
                (
                    read_string_arg(caller, abi_arg(params, 1)?, "Temporal.Duration.total")?,
                    None,
                )
            } else {
                let unit = shared::object_string_field(
                    caller,
                    abi_arg(params, 1)?,
                    "unit",
                    "Duration.total",
                )?
                .ok_or_else(|| wasmtime::Error::msg("Temporal.Duration.total: missing `unit`"))?;
                let anchor =
                    shared::object_relative_to(caller, abi_arg(params, 1)?, "Duration.total")?;
                (unit, anchor)
            };
            *abi_result(results, 0)? =
                Val::F64(shared::span_total(&span, &unit, anchor)?.to_bits());
            Ok(())
        },
    )?;

    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("DurationConstructor", "compare"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), types.object.clone(), options],
            [ValType::F64],
        ),
        true,
        move |caller, params, results| {
            let a =
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.compare")?;
            let b =
                shared::read_duration_like(caller, abi_arg(params, 1)?, &intr, "Duration.compare")?;
            let anchor =
                shared::object_relative_to(caller, abi_arg(params, 2)?, "Duration.compare")?;
            *abi_result(results, 0)? = Val::F64(
                super::compare(a, b, anchor)
                    .map_err(crate::runtime::host::range_error)?
                    .to_bits(),
            );
            Ok(())
        },
    )
}

fn install_strings(
    linker: &mut Linker<StoreData>,
    types: &DirectTypes,
    options: ValType,
) -> wasmtime::Result<()> {
    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "toString"),
        FuncType::new(
            &types.engine,
            [types.object.clone(), options],
            [types.string.clone()],
        ),
        true,
        move |caller, params, results| {
            let span = shared::read_duration_like(
                caller,
                abi_arg(params, 0)?,
                &intr,
                "Duration.toString",
            )?;
            let smallest = shared::object_string_field(
                caller,
                abi_arg(params, 1)?,
                "smallestUnit",
                "Duration.toString",
            )?;
            let mode = shared::object_string_field(
                caller,
                abi_arg(params, 1)?,
                "roundingMode",
                "Duration.toString",
            )?;
            let digits = shared::object_i64_field(
                caller,
                abi_arg(params, 1)?,
                "fractionalSecondDigits",
                "Duration.toString",
            )?
            .map(|value| {
                u8::try_from(value).ok().filter(|value| *value <= 9).ok_or_else(|| {
                    crate::runtime::host::range_error(
                        "Temporal.Duration.toString: fractionalSecondDigits must be an integer from 0 through 9",
                    )
                })
            })
            .transpose()?;
            let text = shared::duration_to_string_with_options(
                &span,
                smallest.as_deref(),
                mode.as_deref(),
                digits,
            )?;
            *abi_result(results, 0)? = Val::AnyRef(Some(
                write_submilli_string_struct(caller, &text)?.to_anyref(),
            ));
            Ok(())
        },
    )?;

    let intr = types.intr.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        shared::prelude_key("Duration", "toJSON"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.string.clone()],
        ),
        true,
        move |caller, params, results| {
            let span =
                shared::read_duration_like(caller, abi_arg(params, 0)?, &intr, "Duration.toJSON")?;
            *abi_result(results, 0)? = Val::AnyRef(Some(
                write_submilli_string_struct(caller, &span.to_string())?.to_anyref(),
            ));
            Ok(())
        },
    )
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let duration = || shared::temporal_type("Duration");
    let receiver = || Param::new("receiver", duration());
    let duration_like = || {
        Param::new(
            "duration",
            Type::Union(vec![
                duration(),
                shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
            ]),
        )
    };
    let optional_options = || Type::Union(vec![shared::object_shape_type(&[]), Type::Undefined]);

    shared::declare_direct_fn(
        defs,
        "new",
        shared::prelude_key("DurationConstructor", "new"),
        vec![Param::new(
            "fields",
            shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
        )],
        duration(),
    );
    shared::declare_direct_fn(
        defs,
        "from",
        shared::prelude_key("DurationConstructor", "from"),
        vec![Param::new(
            "item",
            Type::Union(vec![
                Type::String,
                duration(),
                shared::object_shape_type(&shared::DURATION_FIELD_NAMES),
            ]),
        )],
        duration(),
    );
    shared::declare_direct_fn(
        defs,
        "compare",
        shared::prelude_key("DurationConstructor", "compare"),
        vec![
            duration_like(),
            duration_like(),
            Param::with_default("options", optional_options(), DefaultValue::Undefined),
        ],
        Type::Number,
    );
    for method in ["add", "subtract"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Duration", method),
            vec![receiver(), duration_like()],
            duration(),
        );
    }
    for method in ["negated", "abs"] {
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Duration", method),
            vec![receiver()],
            duration(),
        );
    }
    for field in shared::DURATION_FIELD_NAMES {
        shared::declare_direct_fn(
            defs,
            field,
            shared::prelude_key("Duration", field),
            vec![receiver()],
            Type::Number,
        );
    }
    shared::declare_direct_fn(
        defs,
        "sign",
        shared::prelude_key("Duration", "sign"),
        vec![receiver()],
        Type::Number,
    );
    shared::declare_direct_fn(
        defs,
        "blank",
        shared::prelude_key("Duration", "blank"),
        vec![receiver()],
        Type::Boolean,
    );
    shared::declare_direct_fn(
        defs,
        "with",
        shared::prelude_key("Duration", "with"),
        vec![receiver(), duration_like()],
        duration(),
    );
    for method in ["round", "total"] {
        let options = if method == "round" {
            shared::temporal_type("DurationRoundOptions")
        } else {
            shared::temporal_type("DurationTotalOptions")
        };
        shared::declare_direct_fn(
            defs,
            method,
            shared::prelude_key("Duration", method),
            vec![
                receiver(),
                Param::new("options", Type::Union(vec![Type::String, options])),
            ],
            if method == "total" {
                Type::Number
            } else {
                duration()
            },
        );
    }
    shared::declare_direct_fn(
        defs,
        "toString",
        shared::prelude_key("Duration", "toString"),
        vec![
            receiver(),
            Param::with_default("options", optional_options(), DefaultValue::Undefined),
        ],
        Type::String,
    );
    shared::declare_direct_fn(
        defs,
        "toJSON",
        shared::prelude_key("Duration", "toJSON"),
        vec![receiver()],
        Type::String,
    );
}
