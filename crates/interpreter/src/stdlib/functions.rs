//! `submilli:functions` — the function-authoring package available only to
//! functions compiled into `@functions`.
//!
//! Like [`crate::stdlib::test`], this module is deliberately absent from
//! [`crate::stdlib::stdlib_package_declarations`], [`crate::Stdlib`] discovery
//! (`packages.search` / `packages.docs`), and
//! [`crate::stdlib::install_host_functions`]. Only function compilation passes
//! [`package_declaration`] into the compiler, and only function installation or
//! direct invocation installs [`install`] into the linker, so ordinary scripts
//! and packages neither see it in `packages.docs` nor can import it. Harnesses
//! that prompt a model to write a function include [`docs`] in that prompt.
//!
//! `escalate(reason)` raises an uncatchable [`Escalated`] run-ending host
//! error so guest `try`/`catch` blocks cannot swallow an escalation intended
//! for the host harness. An escalated function terminates immediately and is
//! never resumed, so functions should call `escalate` before performing any
//! external side effects.

use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::packages::{DocAudience, ModuleDoc, render_declarations_for};
use crate::runtime::StoreData;
use crate::runtime::host::{abi_arg, read_string_arg, register_host_fn};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:functions";

const MODULE_DESCRIPTION: &str =
    "Function-only control flow (`escalate`) for functions compiled into `@functions`.";

/// Raised by `submilli:functions.escalate(reason)`. Recognized by
/// [`crate::runtime::host::ends_the_run`] so guest `try`/`catch` blocks cannot
/// intercept it and effect history does not commit an in-flight effect around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalated {
    pub reason: String,
}

impl std::fmt::Display for Escalated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "escalated: {}", self.reason)
    }
}

impl std::error::Error for Escalated {}

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    defs.values.insert(
        "escalate".to_string(),
        ValueSymbol {
            name: "escalate".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "escalate"),
            declaration_span: Span::at(crate::FileId::FUNCTIONS),
            kind: ValueKind::Function {
                generics: Default::default(),
                params: vec![Param::new("reason", Type::String)],
                ret: Type::Never,
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::FUNCTIONS,
                    "/**\n * Abort the current function run and hand control back to the host with `reason`.\n *\n * Call `escalate` before performing external side effects (such as HTTP writes, messages, or state mutations): an escalated function run terminates immediately and will not be resumed.\n * @param reason Why the function cannot safely complete without host or model intervention.\n */",
                ),
            },
        },
    );
    defs
}

/// Rendered documentation for `submilli:functions`, for embedders to include in
/// prompts and playbooks that author `@functions` sources.
pub fn docs() -> ModuleDoc {
    ModuleDoc {
        name: MODULE_NAME.to_string(),
        description: MODULE_DESCRIPTION.to_string(),
        declarations: render_declarations_for(&package_declaration(), DocAudience::Agent),
    }
}

/// Register the `submilli:functions` host functions on `linker`.
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    // `: never` lowers to the `Unknown` slot (`(ref null $Object)`) in codegen;
    // `escalate` always traps before any value reaches the return slot.
    let nullable_object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "escalate"),
        FuncType::new(&engine, [string], [nullable_object]),
        |caller, params, _results| {
            let reason = read_string_arg(
                &mut *caller,
                abi_arg(params, 0)?,
                "functions.escalate (reason)",
            )?;
            Err(wasmtime::Error::new(Escalated { reason }))
        },
    )?;
    Ok(())
}
