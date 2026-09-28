//! Capability requirement derivation for package compilation.

use std::collections::BTreeMap;

use crate::{
    DerivedCapability, Diagnostic, DocCapability, ExprId, MangledName, PackageDeclaration, Param,
    TypedAst, TypedExprKind, ValueKind, derive_call_site_capability,
};

#[derive(Clone, Debug)]
struct CapabilityCallee {
    params: Vec<Param>,
    capabilities: Vec<DocCapability>,
}

pub(crate) fn derive_package_requirements(
    package_name: &str,
    ta: &TypedAst,
    stdlib_defs: &[PackageDeclaration],
    dependencies: &[&PackageDeclaration],
) -> Result<(Vec<DerivedCapability>, Vec<Diagnostic>), crate::compiler_error::CompileError> {
    let lookup = dependency_capability_lookup(package_name, stdlib_defs, dependencies);
    let mut required = Vec::new();
    let mut warnings = Vec::new();
    for expr_id in ta.expr_ids().map_err(|error| {
        crate::compiler_error::CompileError::from(crate::typechecker::arena_failure(error))
            .with_prior_diagnostics(&warnings)
    })? {
        match &ta
            .try_expr(expr_id)
            .map_err(|error| {
                crate::compiler_error::CompileError::from(crate::typechecker::arena_failure(error))
                    .with_prior_diagnostics(&warnings)
            })?
            .kind
        {
            TypedExprKind::Call { mangled, args, .. } => {
                derive_call_requirements(ta, &lookup, mangled, args, &mut required, &mut warnings)?;
            }
            TypedExprKind::GenericCall { mangled, args, .. } => {
                let args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
                derive_call_requirements(
                    ta,
                    &lookup,
                    mangled,
                    &args,
                    &mut required,
                    &mut warnings,
                )?;
            }
            _ => {}
        }
    }
    Ok((required, warnings))
}

fn dependency_capability_lookup(
    package_name: &str,
    stdlib_defs: &[PackageDeclaration],
    dependencies: &[&PackageDeclaration],
) -> BTreeMap<MangledName, CapabilityCallee> {
    let mut lookup = BTreeMap::new();
    for declaration in stdlib_defs {
        insert_capability_callees(package_name, declaration, &mut lookup);
    }
    for declaration in dependencies {
        insert_capability_callees(package_name, declaration, &mut lookup);
    }
    lookup
}

fn insert_capability_callees(
    package_name: &str,
    declaration: &PackageDeclaration,
    lookup: &mut BTreeMap<MangledName, CapabilityCallee>,
) {
    if declaration.package_name == package_name {
        return;
    }
    for symbol in declaration.values.values() {
        let ValueKind::Function { params, doc, .. } = &symbol.kind else {
            continue;
        };
        let Some(doc) = doc else { continue };
        if doc.capabilities.is_empty() {
            continue;
        }
        lookup.insert(
            symbol.mangled_name.clone(),
            CapabilityCallee {
                params: params.clone(),
                capabilities: doc.capabilities.clone(),
            },
        );
    }
}

fn derive_call_requirements(
    ta: &TypedAst,
    lookup: &BTreeMap<MangledName, CapabilityCallee>,
    mangled: &MangledName,
    args: &[ExprId],
    required: &mut Vec<DerivedCapability>,
    warnings: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompileError> {
    let Some(callee) = lookup.get(mangled) else {
        return Ok(());
    };
    for capability in &callee.capabilities {
        let mut derived = derive_call_site_capability(capability, &callee.params, ta, args)
            .map_err(|error| error.with_prior_diagnostics(warnings))?;
        warnings.append(&mut derived.warnings);
        required.push(derived);
    }
    Ok(())
}
