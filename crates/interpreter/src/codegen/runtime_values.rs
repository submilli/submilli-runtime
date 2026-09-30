//! Preserve values whose inferred refinements can outlive the underlying value.
//!
//! Type checking still uses the authored types. This pass operates on a private
//! copy for emission, propagating boxed storage through uses before Wasm local
//! and function signatures are allocated.

use std::collections::{HashMap, HashSet, VecDeque};

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
}

impl Flow {
    fn edge(&mut self, from: Place, to: Place) {
        self.edges.entry(from).or_default().push(to);
    }

    fn value(&mut self, from: ExprId, to: Place) {
        self.edge(Place::Expr(from), to);
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
    !iface.as_str().starts_with("submilli:")
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
