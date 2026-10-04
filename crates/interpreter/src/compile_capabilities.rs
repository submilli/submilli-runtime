//! Capability requirement derivation for package compilation.

use std::collections::BTreeMap;

use crate::{
    DerivedCapability, Diagnostic, DocCapability, DocComment, ExprId, MangledName, MethodSig,
    PackageDeclaration, Param, TypeKind, TypedAst, TypedChainPart, TypedExprKind, ValueKind,
    derive_call_site_capability,
};

#[derive(Clone, Copy, Debug)]
struct CapabilityCallee<'a> {
    params: &'a [Param],
    capabilities: &'a [DocCapability],
}

impl<'a> CapabilityCallee<'a> {
    fn tagged(params: &'a [Param], doc: Option<&'a DocComment>) -> Option<Self> {
        let doc = doc.filter(|doc| !doc.capabilities.is_empty())?;
        Some(Self {
            params,
            capabilities: &doc.capabilities,
        })
    }
}

/// The tagged callables a package's calls can reach in other packages.
#[derive(Default)]
struct CapabilityLookup<'a> {
    /// Functions and static methods, by the name a call dispatches to.
    callees: BTreeMap<MangledName, CapabilityCallee<'a>>,
    /// Instance methods, by declaring class and method name.
    methods: BTreeMap<(&'a MangledName, &'a str), CapabilityCallee<'a>>,
    /// Every known class, the package's own among them, so a method call
    /// resolves to the class that declares the method.
    classes: BTreeMap<&'a MangledName, Class<'a>>,
}

/// `dependencies` are the packages `package` imports from and `transitive`
/// the ones those build on: a class from either can declare a method that a
/// call reaches through inheritance.
pub(crate) fn derive_package_requirements(
    package: &PackageDeclaration,
    ta: &TypedAst,
    stdlib_defs: &[PackageDeclaration],
    dependencies: &[&PackageDeclaration],
    transitive: &[&PackageDeclaration],
) -> Result<(Vec<DerivedCapability>, Vec<Diagnostic>), crate::compiler_error::CompileError> {
    let lookup = CapabilityLookup::new(
        package,
        stdlib_defs
            .iter()
            .chain(dependencies.iter().chain(transitive).copied()),
    );
    let mut required = Vec::new();
    let mut warnings = Vec::new();
    for expr_id in ta.expr_ids().map_err(|error| {
        crate::compiler_error::CompileError::from(crate::typechecker::arena_failure(error))
            .with_prior_diagnostics(&warnings)
    })? {
        let kind = &ta
            .try_expr(expr_id)
            .map_err(|error| {
                crate::compiler_error::CompileError::from(crate::typechecker::arena_failure(error))
                    .with_prior_diagnostics(&warnings)
            })?
            .kind;
        let mut derive = |callee: Option<CapabilityCallee<'_>>, args: &[ExprId]| {
            derive_call_requirements(ta, callee, args, &mut required, &mut warnings)
        };
        match kind {
            TypedExprKind::Call { mangled, args, .. } => {
                derive(lookup.callees.get(mangled).copied(), args)?;
            }
            TypedExprKind::GenericCall { mangled, args, .. } => {
                let args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
                derive(lookup.callees.get(mangled).copied(), &args)?;
            }
            TypedExprKind::MethodCall {
                iface, name, args, ..
            } => {
                derive(lookup.method(iface, &name.name), args)?;
            }
            TypedExprKind::GenericMethodCall {
                iface, name, args, ..
            } => {
                let args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
                derive(lookup.method(iface, &name.name), &args)?;
            }
            TypedExprKind::SuperMethodCall { owner, name, args } => {
                derive(lookup.method(owner, &name.name), args)?;
            }
            TypedExprKind::OptionalChain { parts, .. } => {
                for part in parts {
                    if let TypedChainPart::MethodCall {
                        iface, name, args, ..
                    } = part
                    {
                        derive(lookup.method(iface, &name.name), args)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok((required, warnings))
}

impl<'a> CapabilityLookup<'a> {
    fn new(
        package: &'a PackageDeclaration,
        others: impl Iterator<Item = &'a PackageDeclaration>,
    ) -> Self {
        let mut lookup = Self::default();
        for class in classes(package) {
            lookup.classes.insert(class.mangled_name, class);
        }
        for declaration in others {
            // A package's own calls are checked where they are made.
            if declaration.package_name != package.package_name {
                lookup.insert_package(declaration);
            }
        }
        lookup
    }

    fn insert_package(&mut self, declaration: &'a PackageDeclaration) {
        for symbol in declaration.values.values() {
            let ValueKind::Function { params, doc, .. } = &symbol.kind else {
                continue;
            };
            if let Some(callee) = CapabilityCallee::tagged(params, doc.as_ref()) {
                self.callees.insert(symbol.mangled_name.clone(), callee);
            }
        }
        for class in classes(declaration) {
            for (name, method) in class.methods {
                if let Some(callee) = CapabilityCallee::tagged(&method.params, method.doc.as_ref())
                {
                    self.methods
                        .insert((class.mangled_name, name.as_str()), callee);
                }
            }
            for (name, method) in class.statics {
                if let Some(callee) = CapabilityCallee::tagged(&method.params, method.doc.as_ref())
                {
                    self.callees.insert(
                        crate::mangle::static_member(class.mangled_name, name),
                        callee,
                    );
                }
            }
            self.classes.insert(class.mangled_name, class);
        }
    }

    /// The tags of the method a call on a `class` receiver runs: the nearest
    /// declaration up its inheritance chain. An override declared in the
    /// calling package hides an inherited tag, since its own body is checked.
    fn method(&self, class: &MangledName, name: &str) -> Option<CapabilityCallee<'a>> {
        let mut current = class;
        // Each step visits another class, so a longer walk has met a cycle.
        for _ in 0..=self.classes.len() {
            let class = self.classes.get(current)?;
            if class.methods.contains_key(name) {
                return self.methods.get(&(current, name)).copied();
            }
            current = class.parent?;
        }
        None
    }
}

/// A class a declaration holds, seen through what call resolution needs.
struct Class<'a> {
    mangled_name: &'a MangledName,
    methods: &'a BTreeMap<String, MethodSig>,
    statics: &'a BTreeMap<String, MethodSig>,
    parent: Option<&'a MangledName>,
}

/// Every class `declaration` holds. `runtime_types` covers every module's
/// classes but drops their statics; `types` keeps the statics of exported ones.
fn classes(declaration: &PackageDeclaration) -> impl Iterator<Item = Class<'_>> {
    declaration
        .runtime_types
        .values()
        .chain(declaration.types.values())
        .filter_map(|symbol| match &symbol.kind {
            TypeKind::Class {
                methods,
                statics,
                extends,
                ..
            } => Some(Class {
                mangled_name: &symbol.mangled_name,
                methods,
                statics,
                parent: extends.as_ref().map(|extends| &extends.parent),
            }),
            TypeKind::Interface { .. }
            | TypeKind::NumberEnum { .. }
            | TypeKind::StringEnum { .. }
            | TypeKind::Alias { .. } => None,
        })
}

fn derive_call_requirements(
    ta: &TypedAst,
    callee: Option<CapabilityCallee<'_>>,
    args: &[ExprId],
    required: &mut Vec<DerivedCapability>,
    warnings: &mut Vec<Diagnostic>,
) -> Result<(), crate::compiler_error::CompileError> {
    let Some(callee) = callee else {
        return Ok(());
    };
    for capability in callee.capabilities {
        let mut derived = derive_call_site_capability(capability, callee.params, ta, args)
            .map_err(|error| error.with_prior_diagnostics(warnings))?;
        warnings.append(&mut derived.warnings);
        required.push(derived);
    }
    Ok(())
}
