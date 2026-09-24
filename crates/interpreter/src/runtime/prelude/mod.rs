//! The prelude's implementation: every built-in method, static, constant, and
//! vtable as Rust host fns, registered under `submilli:prelude`.
//!
//! Each method is declared (via [`declare_method`]) as a value symbol whose
//! mangled name is the method's *dispatch key* (e.g.
//! `submilli:prelude#String#repeat`) — the same string is the linker field the
//! host fn registers under, so codegen's method lookup resolves straight to the
//! Rust impl. `codegen::prelude::merged_prelude_declaration` folds these value
//! symbols into the single `submilli:prelude` package next to the type surface;
//! selected top-level bindings (`isNaN`, `NaN`, …) are loaded into user scope by
//! the typechecker.

mod arguments;
pub mod array;
pub mod bigint;
pub mod boolean;
pub(crate) mod closure;
pub(crate) mod collection;
pub(crate) mod console;
pub mod declaration;
pub(crate) mod error;
pub(crate) mod iterator;
pub mod map;
pub mod math;
pub(crate) mod member;
pub mod number;
pub mod object;
pub mod regex;
pub mod set;
pub mod string;
pub mod temporal;
pub mod textcodec;
pub mod uint8array;
pub(crate) mod uri;
pub(crate) mod value;
pub(crate) mod vtable;

use wasmtime::{Linker, Store};

use crate::runtime::StoreData;
use crate::{MangledName, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub use declaration::{
    MODULE_NAME, cached_runtime_package_declarations, prelude_package_declaration,
};

/// Register every ported host fn. Store-less, so it goes into the reusable base
/// linker; the returned `$string`/`$Array` values read the prelude's vtables at
/// call time, not here. The store-bound vtable globals install separately via
/// [`install_vtables`].
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    vtable::install_walk_guards(linker)?;
    console::install(linker)?;
    error::install(linker)?;
    string::install(linker)?;
    array::install(linker)?;
    map::install(linker)?;
    set::install(linker)?;
    number::install(linker)?;
    object::install(linker)?;
    boolean::install(linker)?;
    uint8array::install(linker)?;
    textcodec::install(linker)?;
    bigint::install(linker)?;
    math::install(linker)?;
    temporal::install(linker)?;
    uri::install(linker)?;
    regex::install(linker)?;
    value::install(linker)?;
    member::install(linker)
}

/// Define the host-owned object vtable globals. Unlike [`install`], this is
/// store-bound (it allocates `$VTable` GC structs) and must run per-store before
/// the prelude instantiates — the prelude imports these globals — so the runtime
/// calls it at the top of `install_prelude_async`.
pub(crate) fn install_vtables(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<(vtable::HostVtables, error::ErrorHost)> {
    let vtables = vtable::install_vtable_module(linker, store)?;
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(store.engine())?;
    let error_host = error::install_store_bound(linker, store, &intr, &vtables.string)?;
    number::install_constants(linker, store)?;
    math::install_constants(linker, store)?;
    Ok((vtables, error_host))
}

/// The codegen-facing declaration: the value symbols that route ported methods
/// to their Rust impls (see the module doc).
pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    vtable::declare_walk_guards(&mut defs);
    console::declare(&mut defs);
    string::declare(&mut defs);
    array::declare(&mut defs);
    map::declare(&mut defs);
    set::declare(&mut defs);
    number::declare(&mut defs);
    object::declare(&mut defs);
    boolean::declare(&mut defs);
    math::declare(&mut defs);
    uint8array::declare(&mut defs);
    textcodec::declare(&mut defs);
    bigint::declare(&mut defs);
    temporal::declare(&mut defs);
    uri::declare(&mut defs);
    regex::declare(&mut defs);
    value::declare(&mut defs);
    member::declare(&mut defs);
    defs
}

/// Declare a ported method's value symbol. Its mangled name is the method's
/// dispatch key, and its first param is the (otherwise implicit) receiver —
/// `import_value_symbol` emits no receiver of its own.
pub(crate) fn declare_method(
    defs: &mut PackageDeclaration,
    name: &str,
    mangled_name: MangledName,
    params: Vec<Param>,
    ret: Type,
) {
    // Key by the dispatch key, not the bare method name: String and Array share
    // method names (`at`, `slice`, `concat`, …), so a name-keyed map would have
    // one type's declaration clobber the other's. Codegen routes on
    // `mangled_name`, not this key.
    defs.values.insert(
        mangled_name.as_str().to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name,
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: None,
            },
        },
    );
}
