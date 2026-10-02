//! ABI wiring for the Rust `String` methods: marshals `$string` ↔ [`Str`],
//! registers each method under its dispatch key, and declares the value symbols
//! codegen routes through. The string operations themselves live in the parent
//! module — this file is the wasmtime boundary they're kept clear of.

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, Caller, FuncType, HeapType, Linker, RefType, StructRef,
    StructRefPre, StructType, Val, ValType,
};

use super::{
    RangeError, Str, at, char_at, char_code_at, cmp, code_point_at, concat, ends_with, eq,
    from_char_code, from_code_point, includes, index_of, is_well_formed, last_index_of, normalize,
    pad_end, pad_start, repeat, slice, starts_with, substring, to_lower_case, to_upper_case,
    to_well_formed, trim, trim_end, trim_start,
};
use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    intrinsic_string_type, read_code_units, register_host_fn, register_host_fn_async,
    string_array_type, write_submilli_string_struct, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// The dispatch key codegen looks up for `String#<method>`: the prelude's
/// `String` interface mangled name extended by the method. The host fn's linker
/// field and the value symbol's mangled name must both be exactly this.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("String"), method)
}

/// The dispatch key for a `StringConstructor` static (`String.fromCharCode`,
/// `String.fromCodePoint`) — the `StringConstructor` interface mangled name
/// extended by the method.
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("StringConstructor"), method)
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let abi = StringAbi::recover(&engine)?;

    reg_str_num_to_str(linker, &engine, &abi, "charAt", char_at)?;
    reg_str_num_to_str_or_null(linker, &engine, &abi, "at", at)?;
    reg_str_num_to_num(linker, &engine, &abi, "charCodeAt", char_code_at)?;
    reg_str_num_to_num(linker, &engine, &abi, "codePointAt", code_point_at)?;

    reg_str_2num_to_str(linker, &engine, &abi, "slice", slice)?;
    reg_str_2num_to_str(linker, &engine, &abi, "substring", substring)?;

    reg_str_str_num_to_num(linker, &engine, &abi, "indexOf", index_of)?;
    reg_str_str_num_to_num(linker, &engine, &abi, "lastIndexOf", last_index_of)?;
    register_string_search_predicate(linker, &engine, &abi, "includes", includes)?;
    register_string_search_predicate(linker, &engine, &abi, "startsWith", starts_with)?;
    register_string_search_predicate(linker, &engine, &abi, "endsWith", ends_with)?;

    reg_str_str_to_str(linker, &engine, &abi, "concat", concat)?;
    reg_str_num_str_to_str(linker, &engine, &abi, "padStart", pad_start)?;
    reg_str_num_str_to_str(linker, &engine, &abi, "padEnd", pad_end)?;

    reg_str_to_bool(linker, &engine, &abi, "isWellFormed", is_well_formed)?;
    reg_str_to_str(linker, &engine, &abi, "toWellFormed", to_well_formed)?;

    // Case / whitespace / normalization — formerly the `submilli:string`
    // module; they decode to UTF-8 for the Unicode crates (the sanctioned
    // round-trip) and now live alongside the rest of the String surface.
    reg_str_to_str(linker, &engine, &abi, "toUpperCase", to_upper_case)?;
    reg_str_to_str(linker, &engine, &abi, "toLowerCase", to_lower_case)?;
    reg_str_to_str(linker, &engine, &abi, "trim", trim)?;
    reg_str_to_str(linker, &engine, &abi, "trimStart", trim_start)?;
    reg_str_to_str(linker, &engine, &abi, "trimEnd", trim_end)?;
    reg_str_str_to_str_fallible(linker, &engine, &abi, "normalize", normalize)?;

    reg_str_num_to_str_fallible(linker, &engine, &abi, "repeat", repeat)?;

    // StringConstructor statics: `Dispatch::Static`, so the receiver is dropped
    // and the variadic args arrive packed into one `$Array` of boxed numbers.
    let array_ref = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(build_intrinsic_types(&engine)?.array),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("fromCharCode"),
        FuncType::new(&engine, [array_ref.clone()], [abi.value_type()]),
        true,
        move |caller, params, results| {
            let codes = read_number_array(caller, &params[0], "String.fromCharCode")?;
            let out = from_char_code(&codes);
            let st = write_submilli_string_struct_units(caller, out.units())?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("fromCodePoint"),
        FuncType::new(&engine, [array_ref], [abi.value_type()]),
        true,
        move |caller, params, results| {
            let codes = read_number_array(caller, &params[0], "String.fromCodePoint")?;
            let out = from_code_point(&codes).map_err(throw)?;
            let st = write_submilli_string_struct_units(caller, out.units())?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    // Identity/serialization/comparison members (their vtable-slot twins live
    // in `vtable.rs`; these are the direct-dispatch method keys).
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toString"),
        FuncType::new(&engine, [abi.value_type()], [abi.value_type()]),
        true,
        |_caller, params, results| {
            results[0] = params[0];
            Ok(())
        },
    )?;
    let json_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toJson"),
        FuncType::new(&engine, [abi.value_type()], [abi.value_type()]),
        true,
        move |caller, params, results| {
            let recv = json_abi.read(caller, &params[0], "String#toJson")?;
            let escaped = crate::runtime::prelude::vtable::json_escape_units(recv.value.units());
            let st = write_submilli_string_struct_units(caller, &escaped)?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;
    let equals_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("equals"),
        FuncType::new(
            &engine,
            [abi.value_type(), abi.value_type()],
            [ValType::I32],
        ),
        true,
        move |caller, params, results| {
            let a = equals_abi.read(caller, &params[0], "String#equals")?;
            let b = equals_abi.read(caller, &params[1], "String#equals")?;
            results[0] = Val::I32(eq(&a.value, &b.value) as i32);
            Ok(())
        },
    )?;
    let lc_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("localeCompare"),
        FuncType::new(
            &engine,
            [abi.value_type(), abi.value_type()],
            [ValType::F64],
        ),
        true,
        move |caller, params, results| {
            let a = lc_abi.read(caller, &params[0], "String#localeCompare")?;
            let b = lc_abi.read(caller, &params[1], "String#localeCompare")?;
            results[0] = Val::F64(f64::from(cmp(&a.value, &b.value)).to_bits());
            Ok(())
        },
    )?;

    // `String(value)` — the conversion call-signature (`Dispatch::Static`, so
    // only the value arrives). A bigint renders its decimal text directly (its
    // vtable slots are host-owned but the Wasm path special-cased it, and the
    // dispatch is cheaper); everything else re-enters its `toString` slot.
    let obj_param = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(build_intrinsic_types(&engine)?.object),
    ));
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&crate::mangle::prelude("StringConstructor"), "@call"),
        FuncType::new(&engine, [obj_param], [abi.value_type()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                results[0] = string_ctor_call(caller, &params[0]).await?;
                Ok(())
            })
        },
    )?;

    // `String#iterator` — the for-of cursor over code points.
    let obj_ret = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(build_intrinsic_types(&engine)?.object),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("iterator"),
        FuncType::new(&engine, [abi.value_type()], [obj_ret]),
        true,
        |caller, params, results| {
            results[0] =
                crate::runtime::prelude::iterator::make_string_iterator(caller, params[0])?;
            Ok(())
        },
    )?;

    // Operator primitives — value symbols, not method keys: codegen emits
    // direct calls to these for string `+`, `===`/`==`, and relational compares.
    let s = abi.value_type();
    let concat_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("string_concat"),
        FuncType::new(&engine, [s.clone(), s.clone()], [s.clone()]),
        true,
        move |caller, params, results| {
            let recv = concat_abi.read(caller, &params[0], "string_concat")?;
            let other = concat_abi.read(caller, &params[1], "string_concat")?;
            results[0] =
                concat_abi.write(caller, recv.vtable, &concat(&recv.value, &other.value))?;
            Ok(())
        },
    )?;
    let eq_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("string_eq"),
        FuncType::new(&engine, [s.clone(), s.clone()], [ValType::I32]),
        true,
        move |caller, params, results| {
            let a = eq_abi.read(caller, &params[0], "string_eq")?;
            let b = eq_abi.read(caller, &params[1], "string_eq")?;
            results[0] = Val::I32(eq(&a.value, &b.value) as i32);
            Ok(())
        },
    )?;
    let cmp_abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::prelude("string_cmp"),
        FuncType::new(&engine, [s.clone(), s], [ValType::I32]),
        true,
        move |caller, params, results| {
            let a = cmp_abi.read(caller, &params[0], "string_cmp")?;
            let b = cmp_abi.read(caller, &params[1], "string_cmp")?;
            results[0] = Val::I32(cmp(&a.value, &b.value));
            Ok(())
        },
    )?;
    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    let m = |defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type| {
        declare_method(defs, name, method_key(name), params, ret);
    };
    let s = || Param::new("s", Type::String);

    m(
        defs,
        "charAt",
        vec![s(), Param::new("index", Type::Number)],
        Type::String,
    );
    m(
        defs,
        "at",
        vec![s(), Param::new("index", Type::Number)],
        Type::Union(vec![Type::String, Type::Null]),
    );
    m(
        defs,
        "charCodeAt",
        vec![s(), Param::new("index", Type::Number)],
        Type::Number,
    );
    m(
        defs,
        "codePointAt",
        vec![s(), Param::new("index", Type::Number)],
        Type::Number,
    );
    m(
        defs,
        "slice",
        vec![
            s(),
            Param::new("start", Type::Number),
            Param::new("end", Type::Number),
        ],
        Type::String,
    );
    m(
        defs,
        "substring",
        vec![
            s(),
            Param::new("start", Type::Number),
            Param::new("end", Type::Number),
        ],
        Type::String,
    );
    m(
        defs,
        "indexOf",
        vec![
            s(),
            Param::new("search", Type::String),
            Param::new("fromIndex", Type::Number),
        ],
        Type::Number,
    );
    m(
        defs,
        "lastIndexOf",
        vec![
            s(),
            Param::new("search", Type::String),
            Param::new("fromIndex", Type::Number),
        ],
        Type::Number,
    );
    m(
        defs,
        "includes",
        vec![
            s(),
            Param::new("search", Type::Unknown),
            Param::new("fromIndex", Type::Unknown),
        ],
        Type::Boolean,
    );
    m(
        defs,
        "startsWith",
        vec![
            s(),
            Param::new("search", Type::Unknown),
            Param::new("position", Type::Unknown),
        ],
        Type::Boolean,
    );
    m(
        defs,
        "endsWith",
        vec![
            s(),
            Param::new("search", Type::Unknown),
            Param::new("endPosition", Type::Unknown),
        ],
        Type::Boolean,
    );
    m(
        defs,
        "concat",
        vec![s(), Param::new("other", Type::String)],
        Type::String,
    );
    m(
        defs,
        "padStart",
        vec![
            s(),
            Param::new("targetLength", Type::Number),
            Param::new("padString", Type::String),
        ],
        Type::String,
    );
    m(
        defs,
        "padEnd",
        vec![
            s(),
            Param::new("targetLength", Type::Number),
            Param::new("padString", Type::String),
        ],
        Type::String,
    );
    m(defs, "isWellFormed", vec![s()], Type::Boolean);
    m(defs, "toWellFormed", vec![s()], Type::String);
    m(defs, "toUpperCase", vec![s()], Type::String);
    m(defs, "toLowerCase", vec![s()], Type::String);
    m(defs, "trim", vec![s()], Type::String);
    m(defs, "trimStart", vec![s()], Type::String);
    m(defs, "trimEnd", vec![s()], Type::String);
    m(
        defs,
        "normalize",
        vec![s(), Param::new("form", Type::String)],
        Type::String,
    );
    m(
        defs,
        "repeat",
        vec![s(), Param::new("count", Type::Number)],
        Type::String,
    );

    declare_method(
        defs,
        "fromCharCode",
        ctor_key("fromCharCode"),
        vec![Param::rest("codes", Type::Array(Box::new(Type::Number)))],
        Type::String,
    );
    declare_method(
        defs,
        "fromCodePoint",
        ctor_key("fromCodePoint"),
        vec![Param::rest(
            "codePoints",
            Type::Array(Box::new(Type::Number)),
        )],
        Type::String,
    );

    m(
        defs,
        "iterator",
        vec![s()],
        Type::prelude_interface("Iterator".to_string(), vec![Type::String]),
    );
    m(defs, "toString", vec![s()], Type::String);
    m(defs, "toJson", vec![s()], Type::String);
    m(
        defs,
        "equals",
        vec![s(), Param::new("other", Type::String)],
        Type::Boolean,
    );
    m(
        defs,
        "localeCompare",
        vec![s(), Param::new("other", Type::String)],
        Type::Number,
    );
    declare_method(
        defs,
        "@call",
        crate::mangle::extend(&crate::mangle::prelude("StringConstructor"), "@call"),
        vec![Param::new("value", Type::Unknown)],
        Type::String,
    );

    // Operator primitives (value symbols; `string_cmp`'s boolean return is an
    // i32 sign, matching the prelude declaration it overrides).
    let two_strings = || vec![Param::anon(Type::String), Param::anon(Type::String)];
    declare_method(
        defs,
        "string_concat",
        crate::mangle::prelude("string_concat"),
        two_strings(),
        Type::String,
    );
    declare_method(
        defs,
        "string_eq",
        crate::mangle::prelude("string_eq"),
        two_strings(),
        Type::Boolean,
    );
    declare_method(
        defs,
        "string_cmp",
        crate::mangle::prelude("string_cmp"),
        two_strings(),
        Type::Boolean,
    );
}

/// `(string, f64) -> string`: `charAt`.
fn reg_str_num_to_str(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64) -> Str,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), ValType::F64], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let arg = number(&params[1], name)?;
            results[0] = abi.write(caller, recv.vtable, &op(&recv.value, arg))?;
            Ok(())
        },
    )
}

/// `(string, f64) -> string | null`: `at`, whose out-of-range answer is `null`.
/// The result slot is the union lowering `(ref null $Object)`; a `$string` is an
/// `$Object` subtype, so the hit case needs no extra boxing.
fn reg_str_num_to_str_or_null(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64) -> Option<Str>,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(build_intrinsic_types(engine)?.object),
    ));
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s, ValType::F64], [object]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let arg = number(&params[1], name)?;
            results[0] = match op(&recv.value, arg) {
                Some(hit) => abi.write(caller, recv.vtable, &hit)?,
                None => Val::AnyRef(None),
            };
            Ok(())
        },
    )
}

/// `(string, f64) -> string` for a builder that can reject its input: `repeat`.
fn reg_str_num_to_str_fallible(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64) -> super::Result<Str>,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), ValType::F64], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let arg = number(&params[1], name)?;
            let out = op(&recv.value, arg).map_err(throw)?;
            results[0] = abi.write(caller, recv.vtable, &out)?;
            Ok(())
        },
    )
}

/// `(string, f64) -> f64`: `charCodeAt`, `codePointAt`.
fn reg_str_num_to_num(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64) -> f64,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s, ValType::F64], [ValType::F64]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let arg = number(&params[1], name)?;
            results[0] = Val::F64(op(&recv.value, arg).to_bits());
            Ok(())
        },
    )
}

/// `(string, f64, f64) -> string`: `slice`, `substring`.
fn reg_str_2num_to_str(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64, f64) -> Str,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), ValType::F64, ValType::F64], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let a = number(&params[1], name)?;
            let b = number(&params[2], name)?;
            results[0] = abi.write(caller, recv.vtable, &op(&recv.value, a, b))?;
            Ok(())
        },
    )
}

/// `(string, string, f64) -> f64`: `indexOf`, `lastIndexOf`.
fn reg_str_str_num_to_num(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, &Str, f64) -> f64,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), s, ValType::F64], [ValType::F64]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let search = abi.read(caller, &params[1], name)?;
            let from = number(&params[2], name)?;
            results[0] = Val::F64(op(&recv.value, &search.value, from).to_bits());
            Ok(())
        },
    )
}

/// Search and position stay boxed so RegExp rejection and coercions run in order.
fn register_string_search_predicate(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, &Str, f64) -> bool,
) -> wasmtime::Result<()> {
    let intr = build_intrinsic_types(engine)?;
    let value = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let abi = abi.clone();
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(
            engine,
            [abi.value_type(), value.clone(), value],
            [ValType::I32],
        ),
        true,
        move |caller, params, results| {
            let abi = abi.clone();
            Box::pin(async move {
                let recv = abi.read(caller, &params[0], name)?;
                let search = super::super::value::search_string(caller, &params[1]).await?;
                let from = super::super::value::to_number(caller, &params[2]).await?;
                results[0] = Val::I32(op(&recv.value, &Str::from_units(search), from) as i32);
                Ok(())
            })
        },
    )
}

/// `(string, string) -> string`: `concat`.
fn reg_str_str_to_str(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, &Str) -> Str,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), s.clone()], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let other = abi.read(caller, &params[1], name)?;
            results[0] = abi.write(caller, recv.vtable, &op(&recv.value, &other.value))?;
            Ok(())
        },
    )
}

/// `(string, f64, string) -> string`: `padStart`, `padEnd`.
fn reg_str_num_str_to_str(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, f64, &Str) -> super::Result<Str>,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), ValType::F64, s.clone()], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let target = number(&params[1], name)?;
            let pad = abi.read(caller, &params[2], name)?;
            let out = op(&recv.value, target, &pad.value).map_err(throw)?;
            results[0] = abi.write(caller, recv.vtable, &out)?;
            Ok(())
        },
    )
}

/// `(string, string) -> string` for a builder that can reject its input:
/// `normalize` (invalid form throws).
fn reg_str_str_to_str_fallible(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str, &Str) -> super::Result<Str>,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone(), s.clone()], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            let arg = abi.read(caller, &params[1], name)?;
            let out = op(&recv.value, &arg.value).map_err(throw)?;
            results[0] = abi.write(caller, recv.vtable, &out)?;
            Ok(())
        },
    )
}

/// `(string) -> boolean`: `isWellFormed`.
fn reg_str_to_bool(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str) -> bool,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s], [ValType::I32]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            results[0] = Val::I32(op(&recv.value) as i32);
            Ok(())
        },
    )
}

/// `(string) -> string`: `toWellFormed`.
fn reg_str_to_str(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    abi: &StringAbi,
    name: &'static str,
    op: fn(&Str) -> Str,
) -> wasmtime::Result<()> {
    let s = abi.value_type();
    let abi = abi.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key(name),
        FuncType::new(engine, [s.clone()], [s]),
        true,
        move |caller, params, results| {
            let recv = abi.read(caller, &params[0], name)?;
            results[0] = abi.write(caller, recv.vtable, &op(&recv.value))?;
            Ok(())
        },
    )
}

/// The canonical `$string` ABI types, recovered once at install and reused to
/// build every result. `$string` is `(struct (field vtable) (field $rawString))`.
#[derive(Clone)]
struct StringAbi {
    string_ty: StructType,
    payload_ty: ArrayType,
}

/// A `$string` receiver split into its vtable and code units. The result is
/// built with the *same* vtable: it's the one shared `$string` vtable singleton,
/// so reusing the receiver's avoids a prelude-instance lookup.
struct Receiver {
    vtable: Val,
    value: Str,
}

impl StringAbi {
    fn recover(engine: &wasmtime::Engine) -> wasmtime::Result<Self> {
        Ok(Self {
            string_ty: intrinsic_string_type(engine)?,
            payload_ty: string_array_type(engine),
        })
    }

    fn value_type(&self) -> ValType {
        ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(self.string_ty.clone()),
        ))
    }

    fn read(
        &self,
        caller: &mut Caller<'_, StoreData>,
        val: &Val,
        name: &str,
    ) -> wasmtime::Result<Receiver> {
        let Val::AnyRef(Some(any)) = val else {
            return Err(wasmtime::Error::msg(format!(
                "{name} expects a string, got {val:?}"
            )));
        };
        let st = any
            .as_struct(&mut *caller)?
            .ok_or_else(|| wasmtime::Error::msg(format!("{name}: expected a $string struct")))?;
        let vtable = st.field(&mut *caller, 0)?;
        let payload = match st.field(&mut *caller, 1)? {
            Val::AnyRef(Some(arr)) => arr.unwrap_array(&mut *caller)?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: malformed $string payload {other:?}"
                )));
            }
        };
        Ok(Receiver {
            vtable,
            value: Str::from_units(read_code_units(&mut *caller, payload, name)?),
        })
    }

    fn write(
        &self,
        caller: &mut Caller<'_, StoreData>,
        vtable: Val,
        s: &Str,
    ) -> wasmtime::Result<Val> {
        fuel::charge(&mut *caller, fuel::COPY, s.units().len() as u64)?;
        let pre = ArrayRefPre::new(&mut *caller, self.payload_ty.clone());
        let payload = ArrayRef::new_from_i16_slice(&mut *caller, &pre, s.units())?;
        let pre = StructRefPre::new(&mut *caller, self.string_ty.clone());
        let st = StructRef::new(
            &mut *caller,
            &pre,
            &[vtable, Val::AnyRef(Some(payload.to_anyref()))],
        )?;
        Ok(Val::AnyRef(Some(st.to_anyref())))
    }
}

/// Read a `$Array` of boxed numbers (the packed rest args of a `fromCharCode` /
/// `fromCodePoint` call) into their `f64` payloads.
/// `String(value)`: a bigint renders its decimal text; anything else re-enters
/// its vtable `toString` slot. Null mirrors the Wasm wrapper's trap as a
/// catchable error.
async fn string_ctor_call(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Val> {
    let Val::AnyRef(Some(any)) = value else {
        return Err(wasmtime::Error::msg("String(value): value is null"));
    };
    let intr = intrinsic_types(&mut *caller)?;
    if let Some(st) = any.as_struct(&mut *caller)?
        && StructType::eq(&st.ty(&*caller)?, &intr.bigint)
    {
        let (sign, limbs) = crate::runtime::prelude::bigint::ops::read_bigint_struct(
            caller,
            value,
            "String(bigint)",
        )?;
        let text =
            crate::runtime::prelude::bigint::ops::limbs_to_bigint(sign, &limbs).to_str_radix(10);
        let st = write_submilli_string_struct(caller, &text)?;
        return Ok(Val::AnyRef(Some(st.to_anyref())));
    }
    crate::runtime::prelude::vtable::dispatch_vtable_slot(caller, value, 0, &[]).await
}

fn read_number_array(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<f64>> {
    let storage = crate::runtime::array_storage::ArrayStorage::read(caller, val)?;
    let backing = storage.backing;
    let len = storage.len;
    let mut nums = Vec::new();
    nums.try_reserve_exact(len as usize)
        .map_err(crate::runtime::host::fatal_host_error)?;
    for i in 0..len {
        let boxed = match backing.get(&mut *caller, i)? {
            Val::AnyRef(Some(b)) => b
                .as_struct(&mut *caller)?
                .ok_or_else(|| wasmtime::Error::msg(format!("{name}: arg {i} not a struct")))?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: arg {i} is {other:?}, not a boxed number"
                )));
            }
        };
        match boxed.field(&mut *caller, 1)? {
            Val::F64(bits) => nums.push(f64::from_bits(bits)),
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: arg {i} payload {other:?} is not f64"
                )));
            }
        }
    }
    Ok(nums)
}

fn number(val: &Val, name: &str) -> wasmtime::Result<f64> {
    match val {
        Val::F64(bits) => Ok(f64::from_bits(*bits)),
        other => Err(wasmtime::Error::msg(format!(
            "{name} expects f64, got {other:?}"
        ))),
    }
}

fn throw(e: RangeError) -> wasmtime::Error {
    crate::runtime::host::range_error(e.message())
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{
        Dispatch, MethodSig, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
        ValueSymbol,
    };
    use std::collections::BTreeMap;
    defs.values.insert(
        "string_concat".to_string(),
        ValueSymbol {
            name: "string_concat".to_string(),
            mangled_name: crate::mangle::prelude("string_concat"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::anon(Type::String), Param::anon(Type::String)],
                ret: Type::String,
                type_predicate: None,
                doc: doc("/** Internal: concatenate two strings. */"),
            },
        },
    );
    defs.values.insert(
        "string_eq".to_string(),
        ValueSymbol {
            name: "string_eq".to_string(),
            mangled_name: crate::mangle::prelude("string_eq"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::anon(Type::String), Param::anon(Type::String)],
                ret: Type::Boolean,
                type_predicate: None,
                doc: doc("/** Internal: structural string equality. */"),
            },
        },
    );
    defs.values.insert(
        "string_length".to_string(),
        ValueSymbol {
            name: "string_length".to_string(),
            mangled_name: crate::mangle::prelude("string_length"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::anon(Type::String)],
                ret: Type::Number,
                type_predicate: None,
                doc: doc("/** Internal: UTF-16 code-unit length. */"),
            },
        },
    );
    // `ret: Boolean` only to get an i32 result; the value is a comparison sign,
    // never read as a boolean and never user-callable.
    defs.values.insert(
        "string_cmp".to_string(),
        ValueSymbol {
            name: "string_cmp".to_string(),
            mangled_name: crate::mangle::prelude("string_cmp"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::anon(Type::String), Param::anon(Type::String)],
                ret: Type::Boolean,
                type_predicate: None,
                doc: doc("/** Internal: lexicographic compare; i32 sign result. */"),
            },
        },
    );

    for name in [
        "string_true",
        "string_false",
        "string_comma",
        "string_object_object",
        "string_object_function",
        "string_null",
    ] {
        defs.values.insert(
            name.to_string(),
            ValueSymbol {
                name: name.to_string(),
                mangled_name: crate::mangle::prelude(name),
                declaration_span: Span::at(crate::FileId::PRELUDE),
                kind: ValueKind::Const {
                    ty: Type::String,
                    doc: None,
                },
            },
        );
    }
    defs.types.insert(
        "String".to_string(),
        TypeSymbol {
            name: "String".to_string(),
            mangled_name: crate::mangle::prelude("String"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns this string unchanged (identity). */"),
                        },
                    ),
                    (
                        "toJson".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns this string wrapped in `\"…\"` with JSON escapes applied (`\\\"`, `\\\\`, `\\b`, `\\f`, `\\n`, `\\r`, `\\t`, and `\\u00XX` for control codepoints). */",
                            ),
                        },
                    ),
                    (
                        "concat".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", Type::String)],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with `other` appended.\n * The receiver is not modified.\n * @param other String to append.\n */",
                            ),
                        },
                    ),
                    (
                        "equals".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", Type::String)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` when both strings have identical code units.\n * @param other String to compare against.\n */",
                            ),
                        },
                    ),
                    (
                        "localeCompare".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", Type::String)],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Compares this string with `other` in UTF-16 code-unit order.\n * @param other The string to compare against.\n * @returns A negative number if this sorts before `other`, `0` if equal, a positive number if after. No locale awareness — pure code-unit order.\n */",
                            ),
                        },
                    ),
                    (
                        "iterator".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::String]),
                            predicate: None,
                            doc: doc(
                                "/** Returns an `Iterator<string>` of CODE POINTS — surrogate pairs arrive as one two-unit string. Drives `for-of` over a string. */",
                            ),
                        },
                    ),
                    (
                        "isWellFormed".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when the string contains no lone surrogates (it round-trips losslessly through UTF-8). */",
                            ),
                        },
                    ),
                    (
                        "toWellFormed".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns a copy with every lone surrogate replaced by U+FFFD (`\u{fffd}`); well-formed strings come back unchanged. */",
                            ),
                        },
                    ),
                    (
                        "charAt".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the UTF-16 code unit at `index` as a single-character string.\n * Returns `\"\"` if `index` is negative or beyond the string length.\n * @param index Zero-based index of the code unit.\n */",
                            ),
                        },
                    ),
                    (
                        "at".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::Union(vec![Type::String, Type::Null]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the code unit at `index` as a single-character string. Negative `index` counts from the end.\n * Returns `null` on out-of-range (JS returns `undefined`; Submilli has no `undefined`). Use `charAt` for the `\"\"`-on-miss form.\n * @param index Zero-based index; negatives count from the end.\n */",
                            ),
                        },
                    ),
                    (
                        "charCodeAt".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the UTF-16 code unit at `index` as an integer (0..65535), or `NaN` if out of range.\n * @param index Zero-based index of the code unit.\n */",
                            ),
                        },
                    ),
                    (
                        "codePointAt".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the Unicode code point starting at `index`, decoding surrogate pairs into values up to 0x10FFFF.\n * Returns `NaN` if `index` is out of range.\n * @param index Zero-based index of the first code unit of the code point.\n */",
                            ),
                        },
                    ),
                    (
                        "slice".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::with_default(
                                    "start",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                                // `end` omitted means "to the end".
                                Param::with_default(
                                    "end",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string containing the code units from `start` (inclusive) to `end` (exclusive).\n * Negative values count from the end. The receiver is not modified.\n * @param start First index to copy. Defaults to `0`.\n * @param end Index one past the last to copy. Defaults to the string length.\n */",
                            ),
                        },
                    ),
                    (
                        "substring".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::with_default(
                                    "start",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                                Param::with_default(
                                    "end",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Like `slice`, but negative values clamp to `0`, and `start` and `end` are swapped when `start > end`.\n * @param start First index to copy. Defaults to `0`.\n * @param end Index one past the last to copy. Defaults to the string length.\n */",
                            ),
                        },
                    ),
                    (
                        "indexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("search", Type::String),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the first occurrence of `search` at or after `fromIndex`, or `-1` if none.\n * An empty `search` returns `min(fromIndex, length)`.\n * @param search Substring to search for.\n * @param fromIndex Index to begin searching from.\n */",
                            ),
                        },
                    ),
                    (
                        "lastIndexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("search", Type::String),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the last occurrence of `search` at or before `fromIndex`, or `-1` if none.\n * @param search Substring to search for.\n * @param fromIndex Highest valid start position to consider. Defaults to the string length.\n */",
                            ),
                        },
                    ),
                    (
                        "includes".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("search", Type::String),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if `search` occurs at or after `fromIndex`.\n * @param search Substring to search for.\n * @param fromIndex Index to begin searching from.\n */",
                            ),
                        },
                    ),
                    (
                        "startsWith".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("search", Type::String),
                                Param::with_default(
                                    "position",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if the substring starting at `position` begins with `search`.\n * @param search Prefix to test for.\n * @param position Index at which to start the comparison.\n */",
                            ),
                        },
                    ),
                    (
                        "endsWith".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("search", Type::String),
                                Param::with_default(
                                    "endPosition",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if the substring ending at `endPosition` ends with `search`.\n * @param search Suffix to test for.\n * @param endPosition Index marking the end (exclusive) of the substring considered.\n */",
                            ),
                        },
                    ),
                    (
                        "repeat".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("count", Type::Number)],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string containing `count` copies of this string concatenated.\n * Zero `count` (or an empty receiver) returns `\"\"`; a negative `count` throws a `RangeError`.\n * @param count Number of repetitions.\n */",
                            ),
                        },
                    ),
                    (
                        "padStart".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("targetLength", Type::Number),
                                Param::with_default(
                                    "padString",
                                    Type::String,
                                    crate::DefaultValue::String(" ".to_string()),
                                ),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Pads this string with `padString` on the left until the result reaches `targetLength` code units. Returns this string unchanged when no padding is needed or when `padString` is empty.\n * @param targetLength Length of the padded result.\n * @param padString Filler repeated to reach the target.\n */",
                            ),
                        },
                    ),
                    (
                        "padEnd".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("targetLength", Type::Number),
                                Param::with_default(
                                    "padString",
                                    Type::String,
                                    crate::DefaultValue::String(" ".to_string()),
                                ),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Pads this string with `padString` on the right until the result reaches `targetLength` code units. Returns this string unchanged when no padding is needed or when `padString` is empty.\n * @param targetLength Length of the padded result.\n * @param padString Filler repeated to reach the target.\n */",
                            ),
                        },
                    ),
                    (
                        "toUpperCase".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with every character converted to its Unicode uppercase form.\n */",
                            ),
                        },
                    ),
                    (
                        "toLowerCase".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with every character converted to its Unicode lowercase form.\n */",
                            ),
                        },
                    ),
                    (
                        "trim".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with Unicode whitespace removed from both ends.\n */",
                            ),
                        },
                    ),
                    (
                        "trimStart".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with Unicode whitespace removed from the start.\n */",
                            ),
                        },
                    ),
                    (
                        "trimEnd".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string with Unicode whitespace removed from the end.\n */",
                            ),
                        },
                    ),
                    (
                        "normalize".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "form",
                                Type::String,
                                crate::DefaultValue::String("NFC".to_string()),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new string in the specified Unicode normalization form.\n * @param form One of `\"NFC\"`, `\"NFD\"`, `\"NFKC\"`, `\"NFKD\"`; any other value throws `Error`. Defaults to `\"NFC\"`.\n */",
                            ),
                        },
                    ),
                    // regex-arm String overloads. `match`
                    // and `search` re-use the `RegExp.exec` host fn;
                    // `matchAll` / `split` / `replace` / `replaceAll`
                    // are tracked separately.
                    (
                        "match".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "re",
                                Type::prelude_interface("RegExp".to_string(), Vec::new()),
                            )],
                            ret: Type::Union(vec![
                                Type::prelude_interface("RegExpMatch".to_string(), Vec::new()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Find the next match of `re` in this string. Equivalent to `re.exec(this)` (v1; the `g`-flag array shape is a follow-up).\n * @param re The pattern to search for.\n */",
                            ),
                        },
                    ),
                    (
                        "search".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "re",
                                Type::prelude_interface("RegExp".to_string(), Vec::new()),
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Index of the first match of `re` in this string, or `-1` if no match.\n * @param re The pattern to search for.\n */",
                            ),
                        },
                    ),
                    (
                        "replace".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new(
                                    "search",
                                    Type::Union(vec![
                                        Type::String,
                                        Type::prelude_interface("RegExp".to_string(), Vec::new()),
                                    ]),
                                ),
                                Param::new("replacement", Type::String),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Replace the first match of `search` with `replacement`. For a `RegExp` `search`, the replacement may reference numbered captures (`$1`, `$2`) and named captures (`$<name>`); for both `string` and `RegExp` arms, the special tokens `$$` (literal `$`), `$&` (matched substring), `` $` `` (substring before the match), and `$'` (substring after the match) are honoured.\n * @param search Substring or pattern to match.\n * @param replacement The replacement string.\n */",
                            ),
                        },
                    ),
                    (
                        "replaceAll".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new(
                                    "search",
                                    Type::Union(vec![
                                        Type::String,
                                        Type::prelude_interface("RegExp".to_string(), Vec::new()),
                                    ]),
                                ),
                                Param::new("replacement", Type::String),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Replace every non-overlapping match of `search` with `replacement`. Replacement-token semantics match `replace`.\n * @param search Substring or pattern to match.\n * @param replacement The replacement string.\n */",
                            ),
                        },
                    ),
                    (
                        "split".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new(
                                    "separator",
                                    Type::Union(vec![
                                        Type::String,
                                        Type::prelude_interface("RegExp".to_string(), Vec::new()),
                                    ]),
                                ),
                                Param::with_default(
                                    "limit",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::String)),
                            predicate: None,
                            doc: doc(
                                "/**\n * Split this string into pieces by matches of `separator`. For an empty string `separator`, returns each code unit as a separate part. For a `RegExp` `separator`, captured groups are NOT inserted between parts (JS divergence; documented).\n * @param separator Substring or pattern to split on.\n * @param limit Optional cap on the number of returned parts (default: no limit).\n */",
                            ),
                        },
                    ),
                    (
                        "matchAll".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "re",
                                Type::prelude_interface("RegExp".to_string(), Vec::new()),
                            )],
                            ret: Type::Array(Box::new(Type::prelude_interface("RegExpMatch".to_string(), Vec::new()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Find every non-overlapping match of `re` in this string. Zero-length matches advance by one to avoid infinite loops, per ECMA-262.\n * @param re The pattern to match.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([(
                    "length".to_string(),
                    PropertySig {
                        ty: Type::Number,
                        readonly: true,
                        intrinsic: true,
                        optional: false,
                        doc: doc("/** The number of UTF-16 code units in the string. */"),
                    },
                )]),
                dispatch: Dispatch::Direct,
                doc: doc("/** The UTF-16 string type. */"),
            },
        },
    );
    defs.types.insert(
        "StringConstructor".to_string(),
        TypeSymbol {
            name: "StringConstructor".to_string(),
            mangled_name: crate::mangle::prelude("StringConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "@call".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::Unknown)],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Convert any value to its string representation via the value's `toString()` method. Throws on `null` at runtime.\n */",
                            ),
                        },
                    ),
                    (
                        "fromCharCode".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::rest("codes", Type::Array(Box::new(Type::Number)))],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Builds a string from UTF-16 code units — one unit per argument.\n * Values truncate to 16 bits (negatives saturate to 0, matching the `Uint8Array` constructor's divergence from JS's modulo wrap).\n * @param codes Zero or more UTF-16 code units (0–65535).\n */",
                            ),
                        },
                    ),
                    (
                        "fromCodePoint".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::rest(
                                "codePoints",
                                Type::Array(Box::new(Type::Number)),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Builds a string from Unicode code points; astral code points (> U+FFFF) encode as surrogate pairs.\n * Throws a `RangeError` on a negative, non-integer, or > U+10FFFF input.\n * @param codePoints Zero or more Unicode code points (0–0x10FFFF).\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `string`. Accessed via the global `String` binding. */",
                ),
            },
        },
    );
    defs.values.insert(
        "String".to_string(),
        ValueSymbol {
            name: "String".to_string(),
            mangled_name: crate::mangle::prelude("String"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("StringConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `String` constructor — call `String(x)` to convert any value to a string via its `toString()` method. */",
                ),
            },
        },
    );
}
