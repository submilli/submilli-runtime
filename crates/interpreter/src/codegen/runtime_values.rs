//! Preserve values whose inferred refinements can outlive the underlying value.
//!
//! Type checking still uses the authored types. This pass operates on a private
//! copy for emission, propagating boxed storage through uses before Wasm local
//! and function signatures are allocated.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::{ExprId, Ident, MangledName, StmtId, Type, TypedAst};

#[path = "runtime_values/declarations.rs"]
mod declarations;
#[path = "runtime_values/flow.rs"]
mod flow;
#[path = "runtime_values/rewrite.rs"]
mod rewrite;
pub(super) use declarations::{global_types, lower_declaration, signatures};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Place {
    Expr(ExprId),
    Local(Ident),
    Global(MangledName),
    Return(StmtId),
    ClosureReturn(ExprId),
    CallbackReturn,
    Element,
    Field(String),
    Method(String),
    Chain(ExprId, usize),
}

#[derive(Default)]
struct Flow {
    edges: HashMap<Place, Vec<Place>>,
    widened: HashSet<Place>,
    live_reads: HashMap<ExprId, ExprId>,
    binding_types: HashMap<Ident, (Type, bool)>,
    static_value_bindings: BTreeSet<MangledName>,
}

impl Flow {
    fn edge(&mut self, from: Place, to: Place) {
        self.edges.entry(from).or_default().push(to);
    }

    fn value(&mut self, from: ExprId, to: Place) {
        self.edge(Place::Expr(from), to);
    }

    fn is_static_value_binding(
        &self,
        ast: &TypedAst,
        id: ExprId,
    ) -> Result<bool, crate::compiler_error::CompilerFailure> {
        let expr = ast.try_expr(id).map_err(crate::codegen::arena_failure)?;
        Ok(matches!(
            &expr.kind,
            crate::TypedExprKind::GlobalRef { mangled, .. }
                if self.static_value_bindings.contains(mangled)
        ))
    }

    fn propagate(&mut self) {
        let mut pending: VecDeque<_> = self.widened.iter().cloned().collect();
        while let Some(place) = pending.pop_front() {
            for next in self.edges.get(&place).into_iter().flatten() {
                if self.widened.insert(next.clone()) {
                    pending.push_back(next.clone());
                }
            }
        }
    }

    fn expr_is_wide(&self, id: ExprId) -> bool {
        self.widened.contains(&Place::Expr(id))
    }
}

pub(super) fn lower(
    ast: &TypedAst,
    dependencies: &[&crate::PackageDeclaration],
) -> Result<TypedAst, crate::compiler_error::CompilerFailure> {
    let mut lowered = ast.clone();
    let locals = crate::typechecker::resolve_locals(&mut lowered)
        .map_err(|failure| failure.with_stage(crate::compiler_error::CompilerStage::Codegen))?;
    let mut flow = Flow::default();
    flow::connect(ast, &mut lowered, dependencies, &locals, &mut flow)?;
    flow.propagate();
    rewrite::rewrite(&mut lowered, ast, &flow, &locals.writes)?;
    Ok(lowered)
}

/// Host interfaces retain their native operation ABI. Core language values and
/// user-defined objects resolve members on the actual receiver.
pub(crate) fn dynamic_member_interface(iface: &MangledName) -> bool {
    !crate::codegen::is_host_package(iface.as_str())
        || [
            "String",
            "Number",
            "Boolean",
            "BigInt",
            "Array",
            "Uint8Array",
            "Object",
            "Map",
            "Set",
            "RegExp",
        ]
        .iter()
        .any(|name| iface == &crate::mangle::prelude(name))
}

/// Whether a method on `iface` called on `recv_ty` dispatches through the live
/// member helpers: a union of primitives, or a wrapper-interface receiver.
pub(crate) fn method_dispatches_live(recv_ty: &Type, iface: &MangledName) -> bool {
    crate::typechecker::infer::narrowing::strip_nullish(recv_ty)
        .primitive_union_members()
        .is_some()
        || is_wrapper_interface_receiver(recv_ty, iface)
}

/// Whether `iface` is a primitive wrapper interface (`Number`, `Boolean`,
/// `String`, `BigInt`) reached through a non-primitive receiver (`Number`,
/// `T extends Number`, or a structural user class, optionally nullable).
/// Such receivers lower to `(ref null $Object)` rather than the primitive Wasm
/// slot their direct wrapper expects, and may hold a structural object at
/// runtime, so they dispatch through the live member helpers.
pub(crate) fn is_wrapper_interface_receiver(recv_ty: &Type, iface: &MangledName) -> bool {
    let Some(wrapper) = crate::typechecker::infer::assignable::wrapper_interface_name(iface) else {
        return false;
    };
    let concrete = crate::typechecker::infer::narrowing::strip_nullish(recv_ty);
    match wrapper {
        "Number" => !matches!(
            concrete.primitive_behavior(),
            Type::Number | Type::NumberLiteral(_)
        ),
        "Boolean" => !concrete
            .all_members(&|member| matches!(member, Type::Boolean | Type::BooleanLiteral(_))),
        "String" => !concrete.is_string_shaped(),
        _ => !concrete.is_bigint(),
    }
}
