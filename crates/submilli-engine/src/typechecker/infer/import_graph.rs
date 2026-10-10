use crate::compiler_error::CompilerFailure;

use std::collections::BTreeMap;

use crate::{Ast, Diagnostic, ModulePath, Span};

pub(super) fn topo_order(
    modules: &BTreeMap<ModulePath, (crate::FileId, &Ast)>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<Vec<ModulePath>>, CompilerFailure> {
    let mut edges: BTreeMap<ModulePath, Vec<(ModulePath, Span)>> = BTreeMap::new();
    for (module, (_file, ast)) in modules {
        for stmt_id in &ast.top_level {
            let stmt = ast.try_stmt(*stmt_id).map_err(super::arena_failure)?;
            match &stmt.kind {
                crate::StmtKind::Import {
                    module: specifier,
                    module_span,
                    ..
                } if crate::source::is_relative_specifier(specifier) => {
                    add_graph_edge(
                        modules,
                        &mut edges,
                        module,
                        specifier,
                        *module_span,
                        diagnostics,
                    );
                }
                crate::StmtKind::ExportFrom {
                    source: Some((specifier, span)),
                    ..
                } if crate::source::is_relative_specifier(specifier) => {
                    add_graph_edge(modules, &mut edges, module, specifier, *span, diagnostics);
                }
                _ => {}
            }
        }
    }
    if diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Error)
    {
        return Ok(None);
    }

    let mut order = Vec::new();
    let mut marks = BTreeMap::new();
    let mut stack = Vec::new();
    for node in modules.keys() {
        if !visit(
            node,
            &edges,
            &mut marks,
            &mut stack,
            &mut order,
            diagnostics,
        ) {
            return Ok(None);
        }
    }
    Ok(Some(order))
}

pub(super) fn available_modules_help_from_paths<'a>(
    paths: impl Iterator<Item = &'a ModulePath>,
) -> Vec<String> {
    let names: Vec<String> = paths.map(|p| format!("`{}`", p.as_str())).collect();
    if names.is_empty() {
        Vec::new()
    } else {
        vec![format!("modules in this package: {}", names.join(", "))]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Done,
}

fn visit(
    node: &ModulePath,
    edges: &BTreeMap<ModulePath, Vec<(ModulePath, Span)>>,
    marks: &mut BTreeMap<ModulePath, Mark>,
    stack: &mut Vec<ModulePath>,
    order: &mut Vec<ModulePath>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    match marks.get(node) {
        Some(Mark::Done) => return true,
        Some(Mark::Visiting) => {
            let start = stack.iter().position(|m| m == node).unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(node.clone());
            let names = cycle
                .iter()
                .map(crate::source::ModulePath::as_str)
                .collect::<Vec<_>>()
                .join(" -> ");
            diagnostics.push(Diagnostic {
                severity: crate::Severity::Error,
                span: Span::at(crate::FileId(0)),
                message: format!("circular import: {names}"),
                help: vec![
                    "break the cycle by inlining one import or extracting shared declarations into a third module"
                        .to_string(),
                ],
                notes: Vec::new(),
            });
            return false;
        }
        None => {}
    }
    marks.insert(node.clone(), Mark::Visiting);
    stack.push(node.clone());
    for (dep, _span) in edges.get(node).into_iter().flatten() {
        if !visit(dep, edges, marks, stack, order, diagnostics) {
            return false;
        }
    }
    stack.pop();
    marks.insert(node.clone(), Mark::Done);
    order.push(node.clone());
    true
}

fn add_graph_edge(
    modules: &BTreeMap<ModulePath, (crate::FileId, &Ast)>,
    edges: &mut BTreeMap<ModulePath, Vec<(ModulePath, Span)>>,
    module: &ModulePath,
    specifier: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let resolved = match module.resolve_relative(specifier) {
        Ok(path) => path,
        Err(crate::source::RelativeImportError::EscapesRoot) => {
            diagnostics.push(Diagnostic {
                severity: crate::Severity::Error,
                span,
                message: format!(
                    "import escapes package root: `{specifier}` climbs above the package root"
                ),
                help: vec![
                    "a relative import may not use `..` to leave the package; import a package specifier or a module at or below the root instead"
                        .to_string(),
                ],
                notes: Vec::new(),
            });
            return;
        }
    };
    if !modules.contains_key(&resolved) {
        diagnostics.push(Diagnostic {
            severity: crate::Severity::Error,
            span,
            message: format!("no such module `{specifier}`"),
            help: available_modules_help_from_paths(modules.keys()),
            notes: Vec::new(),
        });
        return;
    }
    edges
        .entry(module.clone())
        .or_default()
        .push((resolved, span));
}
