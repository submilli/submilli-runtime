//! Definite-assignment rule: a non-optional class field with no initializer must
//! be assigned on every path through the constructor (TS `strictPropertyInitialization`).
//! A field whose type admits `undefined` is exempt, as in TypeScript: it starts
//! as `undefined`.
//!
//! The analysis returns, per statement, the set of own fields assigned via
//! `this.x = …` on all paths that complete normally, plus whether the statement
//! diverges (returns/throws). It is sound and conservative: loops and `switch`
//! contribute no guaranteed assignment.

use std::collections::BTreeSet;

use crate::{Diagnostic, Severity, StmtId, TypedAst, TypedExprKind, TypedStmtKind, TypedTypeDecl};

pub(super) fn run(
    ta: &TypedAst,
    diags: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for decl in &ta.types {
        let TypedTypeDecl::Class(class) = decl else {
            continue;
        };
        let required: Vec<&crate::TypedClassField> = class
            .fields
            .iter()
            .filter(|f| {
                !f.optional
                    && f.initializer.is_none()
                    && !f.auto_assigned
                    && !may_start_undefined(&f.ty)
            })
            .collect();
        if required.is_empty() {
            continue;
        }
        let assigned = match &class.constructor {
            Some(ctor) => analyze(ta, ctor.body)?.assigned,
            None => BTreeSet::new(),
        };
        for field in required {
            if !assigned.contains(&field.name.name) {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    span: field.name.span,
                    message: format!(
                        "property `{}` has no initializer and is not assigned in the constructor",
                        field.name.name
                    ),
                    help: vec![format!(
                        "give it an initializer (`{}: T = …`) or assign `this.{}` in the constructor",
                        field.name.name, field.name.name
                    )],
                    notes: vec![],
                });
            }
        }
    }
    Ok(())
}

/// Whether a field of type `ty` may start out `undefined`, so it needs no
/// initializer. `unknown` holds `undefined` too, unlike the runtime-cast rule.
fn may_start_undefined(ty: &crate::Type) -> bool {
    ty.any_member(&|member| {
        matches!(
            member,
            crate::Type::Undefined | crate::Type::Void | crate::Type::Unknown
        )
    })
}

struct Flow {
    /// Own fields definitely assigned on every normally-completing path.
    assigned: BTreeSet<String>,
    /// `true` when the statement returns/throws on every path.
    diverges: bool,
}

impl Flow {
    fn empty() -> Self {
        Flow {
            assigned: BTreeSet::new(),
            diverges: false,
        }
    }

    fn diverges() -> Self {
        Flow {
            assigned: BTreeSet::new(),
            diverges: true,
        }
    }
}

fn analyze(ta: &TypedAst, id: StmtId) -> Result<Flow, crate::compiler_error::CompilerFailure> {
    Ok(
        match &ta
            .try_stmt(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            TypedStmtKind::Return(_) | TypedStmtKind::Throw { .. } => Flow::diverges(),
            TypedStmtKind::AssignField { receiver, name, .. } => {
                if matches!(
                    ta.try_expr(*receiver)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind,
                    TypedExprKind::This
                ) {
                    let mut assigned = BTreeSet::new();
                    assigned.insert(name.name.clone());
                    Flow {
                        assigned,
                        diverges: false,
                    }
                } else {
                    Flow::empty()
                }
            }
            TypedStmtKind::Block(stmts) => {
                let mut flow = Flow::empty();
                for &s in stmts {
                    if flow.diverges {
                        break;
                    }
                    let next = analyze(ta, s)?;
                    flow.assigned.extend(next.assigned);
                    flow.diverges = next.diverges;
                }
                flow
            }
            TypedStmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                let then = analyze(ta, *then_block)?;
                let Some(eb) = else_block else {
                    // No else: the false path assigns nothing, so nothing is guaranteed.
                    return Ok(Flow::empty());
                };
                let els = analyze(ta, *eb)?;
                match (then.diverges, els.diverges) {
                    (true, true) => Flow::diverges(),
                    (true, false) => els,
                    (false, true) => then,
                    (false, false) => Flow {
                        assigned: then.assigned.intersection(&els.assigned).cloned().collect(),
                        diverges: false,
                    },
                }
            }
            TypedStmtKind::NarrowRegion { body, .. } => analyze(ta, *body)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                // A `try` body may throw partway, so only fields assigned in the
                // body and every catch arm are guaranteed; `finally` always runs.
                let body_flow = analyze(ta, *body)?;
                let mut assigned = body_flow.assigned.clone();
                for c in catches {
                    let catch_flow = analyze(ta, c.body)?;
                    assigned = assigned
                        .intersection(&catch_flow.assigned)
                        .cloned()
                        .collect();
                }
                let mut diverges = false;
                if let Some(f) = finally {
                    let fin = analyze(ta, *f)?;
                    assigned.extend(fin.assigned);
                    diverges = fin.diverges;
                }
                Flow { assigned, diverges }
            }
            // Loops and `switch` contribute no guaranteed assignment (conservative).
            _ => Flow::empty(),
        },
    )
}
