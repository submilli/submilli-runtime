//! ABI wiring for the Rust `RegExp` / `RegExpMatch` / `RegExpConstructor` and the
//! `String` regex-arm methods: registers each under its dispatch key and declares
//! the value symbols codegen routes through. The match/glue logic lives in the
//! parent module; matching itself stays in `submilli:regex`.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_array_type, intrinsic_string_type, register_host_fn, register_host_fn_async,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::{MODULE_NAME, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

fn regex_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("RegExp"), method)
}

fn match_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("RegExpMatch"), method)
}

fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("RegExpConstructor"), method)
}

fn string_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("String"), method)
}

fn ref_to(struct_ty: StructType) -> ValType {
    ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(struct_ty)))
}

/// The six boolean flag getters, as `(property, bitmask)`.
const FLAG_GETTERS: [(&str, i32); 6] = [
    ("global", 1 << 0),
    ("ignoreCase", 1 << 1),
    ("multiline", 1 << 2),
    ("dotAll", 1 << 3),
    ("unicode", 1 << 4),
    ("sticky", 1 << 5),
];

#[allow(clippy::too_many_lines)]
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    // `(ref null $Object)` — the erased receiver of every Direct-dispatch object
    // interface, and the lowering of `RegExp`/`RegExpMatch`/`string | RegExp`.
    let obj = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let string = ref_to(intrinsic_string_type(&engine)?);
    let array = ref_to(intrinsic_array_type(&engine)?);
    let num = ValType::F64;
    let boolean = ValType::I32;
    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    let m = MODULE_NAME;

    // --- RegExpConstructor#new (Static: no receiver) ----------------------
    register_host_fn(
        linker,
        m,
        ctor_key("new"),
        ft(vec![string.clone(), string.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? =
                super::construct(caller, abi_arg(params, 0)?, abi_arg(params, 1)?)?;
            Ok(())
        },
    )?;

    // --- RegExp instance methods & getters (receiver: $Object) ------------
    register_host_fn(
        linker,
        m,
        regex_key("test"),
        ft(vec![obj.clone(), string.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = Val::I32(i32::from(super::test(caller, params)?));
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        regex_key("exec"),
        ft(vec![obj.clone(), string.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::exec(caller, params)?;
            Ok(())
        },
    )?;
    for (name, field) in [("source", 3usize), ("flags", 4usize)] {
        register_host_fn(
            linker,
            m,
            regex_key(name),
            ft(vec![obj.clone()], vec![string.clone()]),
            true,
            move |caller, params, results| {
                *abi_result(results, 0)? = super::string_field(caller, params, field)?;
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        m,
        regex_key("lastIndex"),
        ft(vec![obj.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? =
                Val::F64(super::last_index_getter(caller, params)?.to_bits());
            Ok(())
        },
    )?;
    for (name, mask) in FLAG_GETTERS {
        register_host_fn(
            linker,
            m,
            regex_key(name),
            ft(vec![obj.clone()], vec![boolean.clone()]),
            true,
            move |caller, params, results| {
                *abi_result(results, 0)? = Val::I32(i32::from(super::flag(caller, params, mask)?));
                Ok(())
            },
        )?;
    }

    // --- RegExpMatch accessors (receiver: $Object) ------------------------
    for (name, field) in [("match", 1usize), ("input", 3usize)] {
        register_host_fn(
            linker,
            m,
            match_key(name),
            ft(vec![obj.clone()], vec![string.clone()]),
            true,
            move |caller, params, results| {
                *abi_result(results, 0)? = super::match_field(caller, params, field)?;
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        m,
        match_key("index"),
        ft(vec![obj.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = Val::F64(super::match_index(caller, params)?.to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        match_key("groups"),
        ft(vec![obj.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::groups(caller, params)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        m,
        match_key("namedGroups"),
        ft(vec![obj.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = super::named_groups(caller, params).await?;
                Ok(())
            })
        },
    )?;

    // --- String regex-arm methods (receiver: $string) ---------------------
    register_host_fn(
        linker,
        m,
        string_key("match"),
        ft(vec![string.clone(), obj.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::string_match(caller, params)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        string_key("search"),
        ft(vec![string.clone(), obj.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = Val::F64(super::string_search(caller, params)?.to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        string_key("matchAll"),
        ft(vec![string.clone(), obj.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::string_match_all(caller, params)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        string_key("replace"),
        ft(
            vec![string.clone(), obj.clone(), string.clone()],
            vec![string.clone()],
        ),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::string_replace(caller, params)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        string_key("replaceAll"),
        ft(
            vec![string.clone(), obj.clone(), string.clone()],
            vec![string.clone()],
        ),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::string_replace_all(caller, params)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        m,
        string_key("split"),
        ft(
            vec![string.clone(), obj.clone(), num.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::string_split(caller, params)?;
            Ok(())
        },
    )?;
    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    // Receivers/args lower per `value_type`: prelude-interface refs (and unions
    // containing one) → `(ref null $Object)`, `Type::String` → `(ref $string)`,
    // `Type::Array` → `(ref $Array)`.
    let regexp = || Type::prelude_interface("RegExp".to_string(), Vec::new());
    let regexp_match = || Type::prelude_interface("RegExpMatch".to_string(), Vec::new());
    let match_or_null = || Type::Union(vec![regexp_match(), Type::Null]);
    let re_self = || Param::new("self", regexp());
    let match_self = || Param::new("self", regexp_match());

    // RegExpConstructor#new(source, flags) -> RegExp
    declare_method(
        defs,
        "new",
        ctor_key("new"),
        vec![
            Param::new("source", Type::String),
            Param::with_default(
                "flags",
                Type::String,
                crate::DefaultValue::String(String::new()),
            ),
        ],
        regexp(),
    );

    // RegExp
    declare_method(
        defs,
        "test",
        regex_key("test"),
        vec![re_self(), Param::new("input", Type::String)],
        Type::Boolean,
    );
    declare_method(
        defs,
        "exec",
        regex_key("exec"),
        vec![re_self(), Param::new("input", Type::String)],
        match_or_null(),
    );
    for name in ["source", "flags"] {
        declare_method(defs, name, regex_key(name), vec![re_self()], Type::String);
    }
    declare_method(
        defs,
        "lastIndex",
        regex_key("lastIndex"),
        vec![re_self()],
        Type::Number,
    );
    for (name, _) in FLAG_GETTERS {
        declare_method(defs, name, regex_key(name), vec![re_self()], Type::Boolean);
    }

    // RegExpMatch
    for name in ["match", "input"] {
        declare_method(
            defs,
            name,
            match_key(name),
            vec![match_self()],
            Type::String,
        );
    }
    declare_method(
        defs,
        "index",
        match_key("index"),
        vec![match_self()],
        Type::Number,
    );
    declare_method(
        defs,
        "groups",
        match_key("groups"),
        vec![match_self()],
        Type::Array(Box::new(Type::Union(vec![Type::String, Type::Null]))),
    );
    declare_method(
        defs,
        "namedGroups",
        match_key("namedGroups"),
        vec![match_self()],
        Type::prelude_interface("Map".to_string(), vec![Type::String, Type::String]),
    );

    // String regex-arm methods
    let s = || Param::new("s", Type::String);
    let re = || Param::new("re", regexp());
    let string_or_regexp = || Type::Union(vec![Type::String, regexp()]);
    declare_method(
        defs,
        "match",
        string_key("match"),
        vec![s(), re()],
        match_or_null(),
    );
    declare_method(
        defs,
        "search",
        string_key("search"),
        vec![s(), re()],
        Type::Number,
    );
    declare_method(
        defs,
        "matchAll",
        string_key("matchAll"),
        vec![s(), re()],
        Type::Array(Box::new(regexp_match())),
    );
    for name in ["replace", "replaceAll"] {
        declare_method(
            defs,
            name,
            string_key(name),
            vec![
                s(),
                Param::new("search", string_or_regexp()),
                Param::new("replacement", Type::String),
            ],
            Type::String,
        );
    }
    declare_method(
        defs,
        "split",
        string_key("split"),
        vec![
            s(),
            Param::new("separator", string_or_regexp()),
            Param::new("limit", Type::Number),
        ],
        Type::Array(Box::new(Type::String)),
    );
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
    let regex_string_prop_doc =
        "/** The original JS-source pattern (without the leading/trailing `/`). */";
    let regex_flags_doc = "/** The flag string in source order — any subset of `gimsuy`. */";
    let regex_last_index_doc = "/** Read-only in v1 — the wrapper writes it back internally on `g`/`y` matches. Writable from user code lands in a follow-up. */";
    let regex_global_doc = "/** `true` if the regex was constructed with the `g` flag. */";
    let regex_ignore_case_doc =
        "/** `true` if the regex was constructed with the `i` flag (case-insensitive matching). */";
    let regex_multiline_doc = "/** `true` if the regex was constructed with the `m` flag (`^` / `$` match line boundaries). */";
    let regex_dot_all_doc =
        "/** `true` if the regex was constructed with the `s` flag (`.` matches newlines). */";
    let regex_unicode_doc = "/** `true` if the regex was constructed with the `u` flag. */";
    let regex_sticky_doc = "/** `true` if the regex was constructed with the `y` flag (sticky / anchored at `lastIndex`). */";
    let regex_bool_prop = |doc_text: &'static str| PropertySig {
        ty: Type::Boolean,
        readonly: true,
        intrinsic: false,
        optional: false,
        doc: doc(doc_text),
    };
    defs.types.insert(
        "RegExp".to_string(),
        TypeSymbol {
            name: "RegExp".to_string(),
            mangled_name: crate::mangle::prelude("RegExp"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "test".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("s", Type::String)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if the pattern matches anywhere in `s`. Under `g` or `y` advances `this.lastIndex` past the match (or resets it to 0 on no match).\n * @param s The string to test.\n */",
                            ),
                        },
                    ),
                    (
                        "exec".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("s", Type::String)],
                            ret: Type::Union(vec![
                                Type::prelude_interface("RegExpMatch".to_string(), Vec::new()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Find the next match in `s`. Returns the match descriptor (`match`, `index`, `input`) or `null`. Under `g` or `y` advances `this.lastIndex` past the match.\n * @param s The string to search.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    (
                        "source".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(regex_string_prop_doc),
                        },
                    ),
                    (
                        "flags".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(regex_flags_doc),
                        },
                    ),
                    (
                        "lastIndex".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(regex_last_index_doc),
                        },
                    ),
                    ("global".to_string(), regex_bool_prop(regex_global_doc)),
                    (
                        "ignoreCase".to_string(),
                        regex_bool_prop(regex_ignore_case_doc),
                    ),
                    (
                        "multiline".to_string(),
                        regex_bool_prop(regex_multiline_doc),
                    ),
                    ("dotAll".to_string(), regex_bool_prop(regex_dot_all_doc)),
                    ("unicode".to_string(), regex_bool_prop(regex_unicode_doc)),
                    ("sticky".to_string(), regex_bool_prop(regex_sticky_doc)),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Compiled regular expression. Lookaround and backreferences are unsupported. */",
                ),
            },
        },
    );

    defs.types.insert(
        "RegExpMatch".to_string(),
        TypeSymbol {
            name: "RegExpMatch".to_string(),
            mangled_name: crate::mangle::prelude("RegExpMatch"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties: BTreeMap::from([
                    (
                        "match".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The full match text. */"),
                        },
                    ),
                    (
                        "index".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(
                                "/** Offset of the match's first code unit within `input`. */",
                            ),
                        },
                    ),
                    (
                        "input".to_string(),
                        PropertySig {
                            ty: Type::String,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The full string the match ran against. */"),
                        },
                    ),
                    (
                        "groups".to_string(),
                        PropertySig {
                            ty: Type::Array(Box::new(Type::Union(vec![
                                Type::String,
                                Type::Null,
                            ]))),
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(
                                "/** Numbered capture groups (1..n). `null` entries represent groups that did not participate in the match (e.g. alternation arms). */",
                            ),
                        },
                    ),
                    (
                        "namedGroups".to_string(),
                        PropertySig {
                            ty: Type::prelude_interface("Map".to_string(), vec![Type::String, Type::String]),
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc(
                                "/** Named capture groups as a `Map<string, string>`. Unmatched named captures are omitted (no entry in the map). */",
                            ),
                        },
                    ),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Result of a successful `RegExp.exec`. Properties only. */",
                ),
            },
        },
    );

    defs.types.insert(
        "RegExpConstructor".to_string(),
        TypeSymbol {
            name: "RegExpConstructor".to_string(),
            mangled_name: crate::mangle::prelude("RegExpConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "new".to_string(),
                    MethodSig {
                        generics: Vec::new(),
                        params: vec![
                            Param::new("source", Type::String),
                            Param::with_default("flags", Type::String, crate::DefaultValue::String(String::new())),
                        ],
                        ret: Type::prelude_interface("RegExp".to_string(), Vec::new()),
                        predicate: None,
                        doc: doc(
                            "/**\n * Construct a new `RegExp` from `source` and `flags`. Throws on invalid pattern or unsupported feature (lookaround / backreference).\n * @param source The JS regex pattern (without delimiters).\n * @param flags Any subset of `gimsuy`; defaults to an empty string when omitted.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `RegExp`. Accessed via the global `RegExp` binding — call `new RegExp(source, flags)`. */",
                ),
            },
        },
    );

    defs.values.insert(
        "RegExp".to_string(),
        ValueSymbol {
            name: "RegExp".to_string(),
            mangled_name: crate::mangle::prelude("RegExp"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("RegExpConstructor".to_string(), Vec::new()),
                doc: doc("/** The `RegExp` constructor. */"),
            },
        },
    );
}
