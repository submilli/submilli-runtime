use crate::runtime::effects;
use crate::runtime::host::{
    abi_arg, read_string_arg, register_host_value_async, write_submilli_string_struct,
};
use std::collections::BTreeMap;

use wasmtime::{FuncType, Linker, Val};

use super::super::{DirectTypes, shared};
use super::RecordedZoned;
use crate::runtime::StoreData;
use crate::runtime::prelude::MODULE_NAME;
use crate::{
    DefaultValue, NamespaceSymbol, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol,
};

pub(crate) fn install(linker: &mut Linker<StoreData>, types: &DirectTypes) -> wasmtime::Result<()> {
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "instant"),
        FuncType::new(&types.engine, [], [types.object.clone()]),
        |caller, _params| {
            Box::pin(async move {
                let instant = effects::execute(caller, "Temporal.Now.instant", (), |_, _, ()| {
                    Box::pin(async { Ok(super::instant()) })
                })
                .await?;
                shared::make_instant(caller, instant)
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "timeZoneId"),
        FuncType::new(&types.engine, [], [types.string.clone()]),
        |caller, _params| {
            Box::pin(async move {
                let id = effects::execute(caller, "Temporal.Now.timeZoneId", (), |_, _, ()| {
                    Box::pin(async { Ok(super::time_zone_id().to_string()) })
                })
                .await?;
                let value = write_submilli_string_struct(caller, &id)?;
                Ok(Val::AnyRef(Some(value.to_anyref())))
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "zonedDateTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let time_zone = match abi_arg(params, 0)? {
                    value if crate::runtime::prelude::undefined::is_undefined(caller, value)? => {
                        None
                    }
                    _ => Some(read_string_arg(
                        caller,
                        abi_arg(params, 0)?,
                        "Temporal.Now.zonedDateTimeISO",
                    )?),
                };
                let RecordedZoned((zoned, id)) = effects::execute(
                    caller,
                    "Temporal.Now.zonedDateTimeISO",
                    time_zone,
                    |caller, _, time_zone| {
                        Box::pin(async move {
                            super::zoned_date_time_iso(caller, time_zone.as_deref())
                                .map(RecordedZoned)
                        })
                    },
                )
                .await?;
                shared::make_zoned_date_time(caller, &zoned, &id)
            })
        },
    )?;

    // `Temporal.Now.zonedDateTime` — declared alias of `zonedDateTimeISO` (our
    // subset has no calendar argument), same body under its own dispatch key.
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "zonedDateTime"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let time_zone = match abi_arg(params, 0)? {
                    value if crate::runtime::prelude::undefined::is_undefined(caller, value)? => {
                        None
                    }
                    _ => Some(read_string_arg(
                        caller,
                        abi_arg(params, 0)?,
                        "Temporal.Now.zonedDateTime",
                    )?),
                };
                let RecordedZoned((zoned, id)) = effects::execute(
                    caller,
                    "Temporal.Now.zonedDateTime",
                    time_zone,
                    |caller, _, time_zone| {
                        Box::pin(async move {
                            super::zoned_date_time_iso(caller, time_zone.as_deref())
                                .map(RecordedZoned)
                        })
                    },
                )
                .await?;
                shared::make_zoned_date_time(caller, &zoned, &id)
            })
        },
    )?;

    // `Temporal.Now.plain{Date,Time,DateTime}ISO(tz?)` — the current zoned
    // moment projected onto the calendar-naive Plain types.
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainDateISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let z = now_zoned(caller, abi_arg(params, 0)?, "Temporal.Now.plainDateISO").await?;
                let d = z.date();
                shared::make_plain_date(
                    caller,
                    i32::from(d.year()),
                    i32::from(d.month()),
                    i32::from(d.day()),
                )
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let z = now_zoned(caller, abi_arg(params, 0)?, "Temporal.Now.plainTimeISO").await?;
                let t = z.time();
                shared::make_plain_time(
                    caller,
                    i32::from(t.hour()),
                    i32::from(t.minute()),
                    i32::from(t.second()),
                    t.subsec_nanosecond(),
                )
            })
        },
    )?;
    register_host_value_async(
        linker,
        MODULE_NAME,
        shared::prelude_key("Now", "plainDateTimeISO"),
        FuncType::new(
            &types.engine,
            [types.object.clone()],
            [types.object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let z =
                    now_zoned(caller, abi_arg(params, 0)?, "Temporal.Now.plainDateTimeISO").await?;
                let dt = z.datetime();
                shared::make_plain_date_time(
                    caller,
                    i32::from(dt.year()),
                    i32::from(dt.month()),
                    i32::from(dt.day()),
                    i32::from(dt.hour()),
                    i32::from(dt.minute()),
                    i32::from(dt.second()),
                    dt.subsec_nanosecond(),
                )
            })
        },
    )?;
    Ok(())
}

/// The current moment in the `tz` argument's zone (system zone when undefined).
async fn now_zoned(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    tz: &Val,
    label: &'static str,
) -> wasmtime::Result<jiff::Zoned> {
    let tz = match tz {
        value if crate::runtime::prelude::undefined::is_undefined(caller, value)? => None,
        _ => Some(read_string_arg(caller, tz, label)?),
    };
    let RecordedZoned((zoned, _)) = effects::execute(caller, label, tz, |caller, _, tz| {
        Box::pin(
            async move { super::zoned_date_time_iso(caller, tz.as_deref()).map(RecordedZoned) },
        )
    })
    .await?;
    Ok(zoned)
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let temporal_prefix = crate::mangle::prelude("Temporal");
    let now_prefix = crate::mangle::extend(&temporal_prefix, "Now");
    let function = |name: &str, ret: Type| ValueSymbol {
        name: name.to_string(),
        mangled_name: shared::prelude_key("Now", name),
        declaration_span: Span::at(crate::FileId::TEMPORAL),
        kind: ValueKind::Function {
            generics: Vec::new(),
            params: Vec::new(),
            ret,
            type_predicate: None,
            doc: None,
        },
    };
    let now = NamespaceSymbol {
        name: "Now".to_string(),
        mangled_prefix: now_prefix,
        declaration_span: Span::at(crate::FileId::TEMPORAL),
        values: BTreeMap::from([
            (
                "instant".to_string(),
                function("instant", shared::temporal_type("Instant")),
            ),
            (
                "timeZoneId".to_string(),
                function("timeZoneId", Type::String),
            ),
            (
                "zonedDateTimeISO".to_string(),
                ValueSymbol {
                    name: "zonedDateTimeISO".to_string(),
                    mangled_name: shared::prelude_key("Now", "zonedDateTimeISO"),
                    declaration_span: Span::at(crate::FileId::TEMPORAL),
                    kind: ValueKind::Function {
                        generics: Vec::new(),
                        params: vec![Param::with_default(
                            "timeZone",
                            Type::Union(vec![Type::String, Type::Undefined]),
                            DefaultValue::Undefined,
                        )],
                        ret: shared::temporal_type("ZonedDateTime"),
                        type_predicate: None,
                        doc: None,
                    },
                },
            ),
        ]),
        types: BTreeMap::new(),
        namespaces: BTreeMap::new(),
        doc: None,
    };
    defs.namespaces
        .entry("Temporal".to_string())
        .or_insert_with(|| NamespaceSymbol {
            name: "Temporal".to_string(),
            mangled_prefix: temporal_prefix,
            declaration_span: Span::at(crate::FileId::TEMPORAL),
            values: BTreeMap::new(),
            types: BTreeMap::new(),
            namespaces: BTreeMap::new(),
            doc: None,
        })
        .namespaces
        .insert("Now".to_string(), now);
}
