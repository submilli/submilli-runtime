//! Discovery + planning for recursive alias and data-interface validators.
//!
//! `x as T` lowers to a structural walk of `T`. When `T` is
//! recursive the walk would loop forever at codegen time, so recursive alias
//! and interface back-edges are compiled to *calls* into generated helpers.
//! Each helper validates the declaration's expanded shape or runtime interface
//! descriptor, and recursion terminates on the finite input value instead.
//!
//! This module finds every back-edge reachable from any runtime structural
//! check (casts and narrowed-field/interface guards),
//! records the expanded body each helper must emit, and collects the object
//! shapes those bodies need so the subtype/vtable pre-pass emits them even when
//! no literal of that shape appears in the program.

use std::collections::{BTreeMap, BTreeSet};

use crate::codegen::dependency_usage::DependencyType;
use crate::type_size::{TypeLimits, TypeTooLarge};
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{
    MangledName, ObjectField, Shape, Type, TypeKind, TypedAst, TypedExprKind, TypedInterfaceMember,
    TypedTypeDecl,
};

/// Resolves a recursion back-edge to its body, for both `Type::AliasRef`
/// (→ alias body) and `Type::InterfaceRef` (→ structural object of a
/// **data-only** interface's properties). Imported declarations are keyed by
/// nominal identity; local declarations use their source names because the
/// typed declaration nodes do not retain their mangled identities.
pub struct ValidatorBodies {
    class_fields: BTreeMap<Type, Type>,
    imported_aliases: BTreeMap<MangledName, (Vec<String>, Type)>,
    local_aliases: BTreeMap<String, (Vec<String>, Type)>,
    /// Interface identity → (generics, property field map). Method-bearing
    /// interfaces are omitted — they can't be constructed as plain objects, so
    /// the typechecker rejects them as JSON/cast targets before codegen.
    imported_interfaces: BTreeMap<MangledName, (Vec<String>, BTreeMap<String, ObjectField>)>,
    local_interfaces: BTreeMap<String, (Vec<String>, BTreeMap<String, ObjectField>)>,
    /// Codegen's allowance for expanding generic declarations.
    limits: TypeLimits,
}

impl ValidatorBodies {
    pub fn collect<'a>(
        ta: &TypedAst,
        dependency_types: impl IntoIterator<Item = &'a DependencyType<'a>>,
    ) -> Result<Self, TypeTooLarge> {
        let limits = TypeLimits::default();
        let mut imported_aliases = BTreeMap::new();
        let mut imported_interfaces = BTreeMap::new();
        for dependency_type in dependency_types {
            let sym = dependency_type.symbol;
            match &sym.kind {
                TypeKind::Alias { generics, ty, .. } => {
                    imported_aliases
                        .entry(sym.mangled_name.clone())
                        .or_insert_with(|| (generics.to_vec(), ty.clone()));
                }
                TypeKind::Interface {
                    generics,
                    methods,
                    properties,
                    ..
                } => {
                    imported_interfaces
                        .entry(sym.mangled_name.clone())
                        .or_insert_with(|| {
                            let mut fields: BTreeMap<String, ObjectField> = properties
                                .iter()
                                .map(|(field, sig)| {
                                    (
                                        field.clone(),
                                        ObjectField {
                                            ty: sig.ty.clone(),
                                            optional: sig.optional,
                                            readonly: sig.readonly,
                                            method: false,
                                        },
                                    )
                                })
                                .collect();
                            for name in methods.keys() {
                                fields.insert(name.clone(), inherited_method());
                            }
                            (generics.to_vec(), fields)
                        });
                }
                _ => {}
            }
        }
        let local_aliases = ta
            .types
            .iter()
            .filter_map(|decl| match decl {
                TypedTypeDecl::Alias(alias) => Some((
                    alias.name.name.clone(),
                    (alias.generics.to_vec(), alias.ty.clone()),
                )),
                _ => None,
            })
            .collect();
        let mut pending: BTreeMap<_, _> = ta
            .types
            .iter()
            .filter_map(|decl| match decl {
                TypedTypeDecl::Interface(iface) => Some((iface.name.name.clone(), iface)),
                _ => None,
            })
            .collect();
        let mut local_interfaces = BTreeMap::new();
        // Resolve each declaration once, after its bases. Reusing flattened
        // contracts avoids exponential walks through diamond inheritance.
        while !pending.is_empty() {
            let mut ready = None;
            for (name, iface) in &pending {
                if !limits.spend_work(1) {
                    return Err(TypeTooLarge::Work);
                }
                if let Some(fields) = inherited_fields(
                    iface,
                    &local_interfaces,
                    &imported_interfaces,
                    &ta.package_name,
                    &limits,
                )? {
                    ready = Some((name.clone(), iface.generics.to_vec(), fields));
                    break;
                }
            }
            let Some((name, generics, fields)) = ready else {
                // Source binding rejects cycles; public TypedAst callers may
                // still supply cyclic or unresolved heritage metadata.
                return Err(TypeTooLarge::Depth);
            };
            pending.remove(&name);
            local_interfaces.insert(name, (generics, fields));
        }
        local_interfaces.retain(|_, (_, fields)| !fields.values().any(|field| field.method));
        imported_interfaces.retain(|_, (_, fields)| !fields.values().any(|field| field.method));
        Ok(Self {
            class_fields: ta.runtime_class_fields.clone(),
            imported_aliases,
            local_aliases,
            imported_interfaces,
            local_interfaces,
            limits,
        })
    }

    /// One helper per generic declaration, with its arguments supplied as predicates.
    pub fn generic_key(&self, ty: &Type) -> Option<Type> {
        let generics = match ty.peel() {
            Type::AliasRef { mangled, name, .. } => {
                &self
                    .imported_aliases
                    .get(mangled)
                    .or_else(|| self.local_aliases.get(name))?
                    .0
            }
            Type::InterfaceRef { mangled, name, .. } => {
                &self
                    .imported_interfaces
                    .get(mangled)
                    .or_else(|| self.local_interfaces.get(name))?
                    .0
            }
            _ => return None,
        };
        if generics.is_empty() {
            return None;
        }
        let arguments = generics.iter().cloned().map(Type::TypeVar).collect();
        let key = match ty.peel() {
            Type::AliasRef {
                package,
                mangled,
                name,
                ..
            } => Type::AliasRef {
                package: package.clone(),
                mangled: mangled.clone(),
                name: name.clone(),
                args: arguments,
            },
            Type::InterfaceRef {
                package,
                mangled,
                name,
                ..
            } => Type::InterfaceRef {
                package: package.clone(),
                mangled: mangled.clone(),
                name: name.clone(),
                args: arguments,
            },
            _ => return None,
        };
        Some(key)
    }

    /// Expand one level: a `Type::AliasRef` becomes its alias body, a
    /// `Type::InterfaceRef` becomes its data-only property shape (both peeled,
    /// with generic args substituted); any other type returns `ty.peel()` cloned.
    /// Fails when substituting the arguments passes a type limit.
    pub fn expand_one(&self, ty: &Type) -> Result<Type, TypeTooLarge> {
        Ok(match ty.peel() {
            Type::ClassRef { .. } => self
                .class_fields
                .get(ty.peel())
                .cloned()
                .unwrap_or_else(|| ty.peel().clone()),
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                let Some((generics, body)) = self
                    .imported_aliases
                    .get(mangled)
                    .or_else(|| self.local_aliases.get(name))
                else {
                    return Ok(ty.peel().clone());
                };
                let expanded = if generics.is_empty() {
                    body.clone()
                } else {
                    TypeParamSubstitution::instantiation(generics, args)
                        .apply(body, &self.limits)?
                };
                expanded.peel().clone()
            }
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let Some((generics, fields)) = self
                    .imported_interfaces
                    .get(mangled)
                    .or_else(|| self.local_interfaces.get(name))
                else {
                    return Ok(ty.peel().clone());
                };
                let sub = (!generics.is_empty())
                    .then(|| TypeParamSubstitution::instantiation(generics, args));
                let fields = fields
                    .iter()
                    .map(|(field, f)| {
                        let ty = match &sub {
                            Some(sub) => sub.apply(&f.ty, &self.limits)?,
                            None => f.ty.clone(),
                        };
                        Ok((
                            field.clone(),
                            ObjectField {
                                ty,
                                optional: f.optional,
                                readonly: f.readonly,
                                method: false,
                            },
                        ))
                    })
                    .collect::<Result<_, TypeTooLarge>>()?;
                Type::Object {
                    index: None,
                    fields,
                }
            }
            other => other.clone(),
        })
    }
}

/// Expand only heritage edges against already flattened contracts. Property
/// types stay nominal so recursive fields remain validator back-edges.
fn inherited_fields(
    iface: &crate::TypedInterfaceDecl,
    local: &BTreeMap<String, (Vec<String>, BTreeMap<String, ObjectField>)>,
    imported: &BTreeMap<MangledName, (Vec<String>, BTreeMap<String, ObjectField>)>,
    package: &str,
    limits: &TypeLimits,
) -> Result<Option<BTreeMap<String, ObjectField>>, TypeTooLarge> {
    let mut fields = BTreeMap::new();
    let mut budget = limits.budget();
    for base in &iface.extends {
        let Some(inherited) = base_fields(base, local, imported, package, limits, &mut budget, 1)?
        else {
            return Ok(None);
        };
        fields.extend(inherited);
    }
    // Keep method markers until all overrides have been applied. A derived
    // property can replace an inherited method (and vice versa).
    for member in &iface.members {
        budget.charge(1)?;
        if let TypedInterfaceMember::Property { ty, .. } = member {
            budget.charge_copy(ty, 2)?;
        }
        let (name, field) = match member {
            TypedInterfaceMember::Method { name, .. } => (name, inherited_method()),
            TypedInterfaceMember::Property {
                name,
                ty,
                optional,
                readonly,
                ..
            } => (
                name,
                ObjectField {
                    ty: ty.clone(),
                    optional: *optional,
                    readonly: *readonly,
                    method: false,
                },
            ),
        };
        fields.insert(name.name.clone(), field);
    }
    crate::type_size::check(&Type::Object {
        fields: fields.clone(),
        index: None,
    })?;
    Ok(Some(fields))
}

/// Local references use cached contracts; recursion here follows only the
/// finite type structure of an intersection, never the inheritance graph.
#[allow(clippy::too_many_arguments)]
fn base_fields(
    ty: &Type,
    local: &BTreeMap<String, (Vec<String>, BTreeMap<String, ObjectField>)>,
    imported: &BTreeMap<MangledName, (Vec<String>, BTreeMap<String, ObjectField>)>,
    package: &str,
    limits: &TypeLimits,
    budget: &mut crate::type_size::TypeBudget<'_>,
    depth: u32,
) -> Result<Option<BTreeMap<String, ObjectField>>, TypeTooLarge> {
    budget.charge_copy(ty, depth)?;
    match ty.peel() {
        Type::Object { fields, .. } => Ok(Some(fields.clone())),
        Type::InterfaceRef {
            package: owner,
            mangled,
            name,
            args,
        } => {
            let contract = imported.get(mangled).or_else(|| {
                (owner.as_str() == package)
                    .then(|| local.get(name))
                    .flatten()
            });
            let Some((generics, fields)) = contract else {
                return Ok(None);
            };
            let body = TypeParamSubstitution::instantiation(generics, args).apply(
                &Type::Object {
                    fields: fields.clone(),
                    index: None,
                },
                limits,
            )?;
            base_fields(&body, local, imported, package, limits, budget, depth + 1)
        }
        Type::Intersection(bases) => {
            let mut fields = BTreeMap::new();
            for base in bases {
                let Some(other) =
                    base_fields(base, local, imported, package, limits, budget, depth + 1)?
                else {
                    return Ok(None);
                };
                intersect_fields(&mut fields, other, budget, depth + 1)?;
            }
            Ok(Some(fields))
        }
        _ => Ok(None),
    }
}

fn intersect_fields(
    fields: &mut BTreeMap<String, ObjectField>,
    other: BTreeMap<String, ObjectField>,
    budget: &mut crate::type_size::TypeBudget<'_>,
    depth: u32,
) -> Result<(), TypeTooLarge> {
    for (name, field) in other {
        if let Some(existing) = fields.get_mut(&name) {
            existing.ty = intersect_field_types(&existing.ty, &field.ty, budget, depth)?;
            existing.optional &= field.optional;
            existing.readonly &= field.readonly;
            existing.method |= field.method;
        } else {
            fields.insert(name, field);
        }
    }
    Ok(())
}

/// Meet the concrete data shapes carried by raw external heritage metadata.
/// Named recursive properties stay finite references; equal ones deduplicate.
fn intersect_field_types(
    left: &Type,
    right: &Type,
    budget: &mut crate::type_size::TypeBudget<'_>,
    depth: u32,
) -> Result<Type, TypeTooLarge> {
    budget.charge_copy(left, depth)?;
    budget.charge_copy(right, depth)?;
    if let Type::Union(members) = left.peel() {
        return members
            .iter()
            .map(|member| intersect_field_types(member, right, budget, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::union);
    }
    if let Type::Union(members) = right.peel() {
        return members
            .iter()
            .map(|member| intersect_field_types(left, member, budget, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::union);
    }
    if let (
        Type::Object {
            fields: left,
            index: None,
        },
        Type::Object {
            fields: right,
            index: None,
        },
    ) = (left.peel(), right.peel())
    {
        let mut fields = left.clone();
        intersect_fields(&mut fields, right.clone(), budget, depth + 1)?;
        return Ok(Type::Object {
            fields,
            index: None,
        });
    }
    Ok(Type::intersection(vec![left.clone(), right.clone()]))
}

/// Method types are irrelevant to data validation; the marker keeps override
/// precedence without copying signatures that cannot be checked as plain data.
fn inherited_method() -> ObjectField {
    ObjectField {
        ty: Type::Unknown,
        optional: false,
        readonly: false,
        method: true,
    }
}

/// One generated validator: the back-edge it keys on and the expanded body its
/// function emits. `func_idx`/`type_idx` are filled in during index allocation.
#[derive(Clone, Debug)]
pub struct ValidatorPlan {
    pub key: Type,
    pub body: Type,
    pub rejects_polymorphic_edge: bool,
}

/// Generated runtime validators and the extra object shapes their bodies need.
pub struct RecursiveValidators {
    pub plans: Vec<ValidatorPlan>,
    pub extra_shapes: Vec<Shape>,
    pub descriptor_types: BTreeSet<Type>,
}

pub fn discover<'a>(
    ta: &TypedAst,
    dependency_types: impl IntoIterator<Item = &'a DependencyType<'a>>,
    bodies: &ValidatorBodies,
) -> Result<RecursiveValidators, crate::compiler_error::CompilerFailure> {
    let mut discovery = Discovery::new(bodies);
    discovery.scan_expressions(ta)?;
    discovery.scan_runtime_tests(ta)?;
    discovery.scan_local_class_fields(ta)?;
    discovery.scan_dependency_class_fields(dependency_types)?;
    discovery.finish()
}

struct Discovery<'a> {
    bodies: &'a ValidatorBodies,
    shapes: BTreeSet<Shape>,
    plain_objects: BTreeSet<Type>,
    expanded_keys: BTreeSet<Type>,
    rejected_keys: BTreeSet<Type>,
    descriptor_types: BTreeSet<Type>,
}

impl<'a> Discovery<'a> {
    fn new(bodies: &'a ValidatorBodies) -> Self {
        Self {
            bodies,
            shapes: BTreeSet::new(),
            plain_objects: BTreeSet::new(),
            expanded_keys: BTreeSet::new(),
            rejected_keys: BTreeSet::new(),
            descriptor_types: BTreeSet::new(),
        }
    }

    fn walk(&mut self, ty: &Type) -> Result<(), crate::compiler_error::CompilerFailure> {
        collect_descriptor_arguments(ty, &mut self.descriptor_types);
        Traversal::new(
            self.bodies,
            &mut self.expanded_keys,
            &mut self.rejected_keys,
            &mut self.shapes,
            &mut self.plain_objects,
        )
        .walk(ty)?;
        Ok(())
    }

    fn scan_expressions(
        &mut self,
        ta: &TypedAst,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for i in ta.expr_ids().map_err(crate::codegen::arena_failure)? {
            let expr = ta.try_expr(i).map_err(crate::codegen::arena_failure)?;
            if let TypedExprKind::Cast {
                check: Some(shape), ..
            } = &expr.kind
            {
                self.walk(shape)?;
            }
        }
        if let Some(argument) = &ta.finalizer_argument {
            self.walk(&argument.check)?;
        }
        if let Some(plan) = &ta.function_invoke {
            for argument in &plan.arguments {
                if let crate::FunctionArgumentCheck::Checked(check) = argument {
                    self.walk(&check.check)?;
                }
            }
        }
        Ok(())
    }

    fn scan_runtime_tests(
        &mut self,
        ta: &TypedAst,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for (key, test) in &ta.runtime_type_tests {
            match test {
                crate::FieldNarrowingTest::Shape(shape) => self.walk(shape)?,
                crate::FieldNarrowingTest::Interface(interface) => {
                    if let Some(index) = &interface.index {
                        self.walk(&index.value)?;
                    }
                    for member in interface.members.values() {
                        self.walk(&member.ty)?;
                    }
                    if is_generic_interface_key(key)
                        || interface_test_is_recursive(ta, key, interface)
                    {
                        self.expanded_keys.insert(key.clone());
                    }
                    self.scan_interface_carriers(interface)?;
                }
                crate::FieldNarrowingTest::NonNull
                | crate::FieldNarrowingTest::Substituted
                | crate::FieldNarrowingTest::Representation => {}
            }
        }
        Ok(())
    }

    fn scan_interface_carriers(
        &mut self,
        interface: &crate::InterfaceNarrowingTest,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for carrier in &interface.non_shape_carriers {
            match carrier {
                crate::InterfaceCarrier::Array(ty)
                | crate::InterfaceCarrier::Function(ty)
                | crate::InterfaceCarrier::Set(ty) => {
                    self.walk(ty)?;
                }
                crate::InterfaceCarrier::Map(key, value) => {
                    self.walk(key)?;
                    self.walk(value)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn scan_local_class_fields(
        &mut self,
        ta: &TypedAst,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for decl in &ta.types {
            let TypedTypeDecl::Class(class) = decl else {
                continue;
            };
            for check in class
                .fields
                .iter()
                .filter_map(|field| field.narrowing_check.as_deref())
            {
                if let crate::FieldNarrowingTest::Shape(shape) = &check.test {
                    self.walk(shape)?;
                }
            }
        }
        Ok(())
    }

    fn scan_dependency_class_fields<'b>(
        &mut self,
        dependency_types: impl IntoIterator<Item = &'b DependencyType<'b>>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for dependency_type in dependency_types {
            let TypeKind::Class {
                narrowing_checks, ..
            } = &dependency_type.symbol.kind
            else {
                continue;
            };
            for check in narrowing_checks.values() {
                if let crate::FieldNarrowingTest::Shape(shape) = &check.test {
                    self.walk(shape)?;
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<RecursiveValidators, crate::compiler_error::CompilerFailure> {
        let mut all_keys = self.expanded_keys.clone();
        all_keys.extend(self.rejected_keys.iter().cloned());
        let plans: Vec<_> = all_keys
            .into_iter()
            .map(|key| {
                let rejects_polymorphic_edge =
                    self.rejected_keys.contains(&key) && !self.expanded_keys.contains(&key);
                let body = expanded(self.bodies, &key)?;
                Ok(ValidatorPlan {
                    key,
                    body,
                    rejects_polymorphic_edge,
                })
            })
            .collect::<Result<_, crate::compiler_error::CompilerFailure>>()?;
        let mut descriptor_types = self.descriptor_types;
        for plan in &plans {
            collect_descriptor_arguments(&plan.body, &mut descriptor_types);
        }
        for shape in &self.shapes {
            if let Shape::Object { fields, index } = shape {
                if let Some(index) = index {
                    collect_descriptor_arguments(&index.value, &mut descriptor_types);
                }
                for field in fields.values() {
                    collect_descriptor_arguments(&field.ty, &mut descriptor_types);
                }
            }
        }
        Ok(RecursiveValidators {
            descriptor_types,
            plans,
            extra_shapes: self.shapes.into_iter().collect(),
        })
    }
}

fn interface_test_is_recursive(
    ta: &TypedAst,
    key: &Type,
    interface: &crate::InterfaceNarrowingTest,
) -> bool {
    let mut visited = BTreeSet::from([key.clone()]);
    interface
        .index
        .as_ref()
        .is_some_and(|index| type_reaches_interface(&index.value, key.peel(), ta, &mut visited))
        || interface
            .members
            .values()
            .any(|member| type_reaches_interface(&member.ty, key.peel(), ta, &mut visited))
}

fn type_reaches_interface(
    ty: &Type,
    target: &Type,
    ta: &TypedAst,
    visited: &mut BTreeSet<Type>,
) -> bool {
    let ty = ty.peel();
    if ty == target {
        return true;
    }
    match ty {
        Type::Array(element) => type_reaches_interface(element, target, ta, visited),
        Type::Tuple(crate::types::TupleType { elements, .. }) | Type::Union(elements) => elements
            .iter()
            .any(|element| type_reaches_interface(element, target, ta, visited)),
        Type::Object { fields, index } => {
            index
                .as_ref()
                .is_some_and(|i| type_reaches_interface(&i.value, target, ta, visited))
                || fields
                    .values()
                    .any(|field| type_reaches_interface(&field.ty, target, ta, visited))
        }
        Type::Function { params, ret, .. } => {
            params
                .iter()
                .any(|param| type_reaches_interface(param, target, ta, visited))
                || type_reaches_interface(ret, target, ta, visited)
        }
        Type::InterfaceRef { args, .. } => {
            if args
                .iter()
                .any(|arg| type_reaches_interface(arg, target, ta, visited))
            {
                return true;
            }
            // An interface stays visited after its walk: reachability of the
            // one target does not depend on the path, and exploring a shared
            // interface again from each member that references it is
            // exponential in the nesting.
            if !visited.insert(ty.clone()) {
                return false;
            }
            match ta.runtime_type_tests.get(ty) {
                Some(crate::FieldNarrowingTest::Interface(test)) => test
                    .members
                    .values()
                    .any(|member| type_reaches_interface(&member.ty, target, ta, visited)),
                _ => false,
            }
        }
        Type::ClassRef { args, .. } | Type::AliasRef { args, .. } => args
            .iter()
            .any(|arg| type_reaches_interface(arg, target, ta, visited)),
        _ => false,
    }
}

/// Per-root traversal state. Exact instantiations and active declaration
/// identities are distinct: regular recursion reuses a validator, while a
/// type-growing polymorphic edge gets a rejecting helper.
struct Traversal<'a, 'b> {
    bodies: &'a ValidatorBodies,
    validators: &'b mut BTreeSet<Type>,
    rejected: &'b mut BTreeSet<Type>,
    shapes: &'b mut BTreeSet<Shape>,
    /// Object types already walked, across roots, that reference no alias,
    /// interface or class: recording their shapes is the walk's only effect, so
    /// they need no second walk. Walking a deep object again for every runtime
    /// test that contains it is quadratic in its depth.
    plain_objects: &'b mut BTreeSet<Type>,
    active_declarations: BTreeSet<MangledName>,
    seen_instantiations: BTreeSet<Type>,
}

impl<'a, 'b> Traversal<'a, 'b> {
    fn new(
        bodies: &'a ValidatorBodies,
        validators: &'b mut BTreeSet<Type>,
        rejected: &'b mut BTreeSet<Type>,
        shapes: &'b mut BTreeSet<Shape>,
        plain_objects: &'b mut BTreeSet<Type>,
    ) -> Self {
        Self {
            bodies,
            validators,
            rejected,
            shapes,
            plain_objects,
            active_declarations: BTreeSet::new(),
            seen_instantiations: BTreeSet::new(),
        }
    }

    /// Walks `ty`, reporting whether it reached an alias, interface or class
    /// reference, whose handling depends on this root's traversal state.
    fn walk(&mut self, ty: &Type) -> Result<bool, crate::compiler_error::CompilerFailure> {
        let peeled = ty.peel();
        Ok(match peeled {
            Type::Object { fields, index } => {
                if self.plain_objects.contains(peeled) {
                    return Ok(false);
                }
                let mut references = false;
                if let Some(index) = index {
                    references |= self.walk(&index.value)?;
                }
                self.shapes.insert(Shape::Object {
                    index: index.clone(),
                    fields: fields.clone(),
                });
                for field in fields.values() {
                    references |= self.walk(&field.ty)?;
                }
                if !references {
                    self.plain_objects.insert(peeled.clone());
                }
                references
            }
            Type::Array(element) => self.walk(element)?,
            Type::Tuple(crate::types::TupleType { elements, .. })
            | Type::Union(elements)
            | Type::Intersection(elements) => {
                let mut references = false;
                for element in elements {
                    references |= self.walk(element)?;
                }
                references
            }
            Type::Function { params, ret, .. } => {
                let mut references = false;
                for param in params {
                    references |= self.walk(param)?;
                }
                references | self.walk(ret)?
            }
            Type::AliasRef { mangled, .. } => {
                if expanded(self.bodies, peeled)? != *peeled {
                    self.walk_alias(peeled, mangled)?;
                }
                true
            }
            Type::ClassRef { mangled, args, .. } => {
                if expanded(self.bodies, peeled)? != *peeled {
                    for arg in args {
                        self.walk(arg)?;
                    }
                    self.walk_interface(peeled, mangled)?;
                }
                true
            }
            Type::InterfaceRef { mangled, .. } => {
                if expanded(self.bodies, peeled)? != *peeled {
                    self.walk_interface(peeled, mangled)?;
                }
                true
            }
            _ => false,
        })
    }

    fn walk_alias(
        &mut self,
        ty: &Type,
        identity: &MangledName,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if !self.seen_instantiations.insert(ty.clone()) {
            return Ok(());
        }
        self.walk_expanded_reference(ty, identity)?;
        Ok(())
    }

    fn walk_interface(
        &mut self,
        ty: &Type,
        identity: &MangledName,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if !self.seen_instantiations.insert(ty.clone()) {
            if self.active_declarations.contains(identity) {
                self.validators.insert(ty.clone());
            }
            return Ok(());
        }
        self.walk_expanded_reference(ty, identity)?;
        Ok(())
    }

    fn walk_expanded_reference(
        &mut self,
        ty: &Type,
        identity: &MangledName,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if !self.active_declarations.insert(identity.clone()) {
            if let Some(key) = self.bodies.generic_key(ty) {
                if self.validators.insert(key.clone()) {
                    let body = expanded(self.bodies, &key)?;
                    self.walk(&body)?;
                }
            } else {
                self.rejected.insert(ty.clone());
            }
            return Ok(());
        }
        self.validators.insert(ty.clone());
        let body = expanded(self.bodies, ty)?;
        self.walk(&body)?;
        self.active_declarations.remove(identity);
        Ok(())
    }
}

/// [`ValidatorBodies::expand_one`], with a type limit as a codegen failure.
fn expanded(
    bodies: &ValidatorBodies,
    ty: &Type,
) -> Result<Type, crate::compiler_error::CompilerFailure> {
    bodies.expand_one(ty).map_err(|exceeded| {
        exceeded.into_failure(crate::compiler_error::CompilerStage::Codegen, None)
    })
}

fn collect_descriptor_arguments(ty: &Type, out: &mut BTreeSet<Type>) {
    match ty.peel() {
        Type::AliasRef { args, .. }
        | Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. } => {
            for arg in args {
                out.insert(arg.clone());
                collect_descriptor_arguments(arg, out);
            }
        }
        Type::Array(ty) => collect_descriptor_arguments(ty, out),
        Type::Union(types)
        | Type::Intersection(types)
        | Type::Tuple(crate::types::TupleType {
            elements: types, ..
        }) => {
            for ty in types {
                collect_descriptor_arguments(ty, out);
            }
        }
        Type::Object { fields, index } => {
            if let Some(i) = index {
                collect_descriptor_arguments(&i.value, out);
            }
            for field in fields.values() {
                collect_descriptor_arguments(&field.ty, out);
            }
        }
        _ => {}
    }
}

fn is_generic_interface_key(ty: &Type) -> bool {
    let Type::InterfaceRef { args, .. } = ty else {
        return false;
    };
    if args.is_empty() {
        return false;
    }
    let mut seen = BTreeSet::new();
    args.iter()
        .all(|arg| matches!(arg, Type::TypeVar(name) if seen.insert(name.as_str())))
}

#[cfg(test)]
mod inheritance_tests {
    use super::*;

    fn reference(name: &str, args: Vec<Type>) -> Type {
        Type::interface_ref(
            crate::Package("main".into()),
            name,
            crate::mangle::package_symbol("main", name),
            args,
        )
    }

    fn declaration(name: &str, extends: Vec<Type>, ty: Type) -> crate::TypedInterfaceDecl {
        crate::TypedInterfaceDecl {
            name: crate::Ident {
                name: name.into(),
                span: crate::Span::at(crate::FileId(0)),
            },
            generics: crate::GenericParameters::unconstrained(vec!["T".into()]),
            extends,
            members: vec![TypedInterfaceMember::Property {
                name: crate::Ident {
                    name: "value".into(),
                    span: crate::Span::at(crate::FileId(0)),
                },
                ty,
                optional: false,
                readonly: false,
                doc: None,
            }],
            property_names: BTreeSet::from(["value".into()]),
            index: None,
            doc: None,
        }
    }

    #[test]
    fn inherited_validator_fields_substitute_generics_and_keep_own_overrides() {
        let mut base = declaration("Base", vec![], Type::TypeVar("T".into()));
        if let TypedInterfaceMember::Property {
            name,
            optional,
            readonly,
            ..
        } = &mut base.members[0]
        {
            name.name = "inherited".into();
            *optional = true;
            *readonly = true;
        }
        let intermediate = declaration(
            "Intermediate",
            vec![reference("Base", vec![Type::TypeVar("T".into())])],
            Type::String,
        );
        let derived = declaration(
            "Derived",
            vec![reference("Intermediate", vec![Type::TypeVar("T".into())])],
            Type::Number,
        );
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types = vec![
            TypedTypeDecl::Interface(derived),
            TypedTypeDecl::Interface(base),
            TypedTypeDecl::Interface(intermediate),
        ];
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::Boolean]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["value"].ty, Type::Number);
        assert_eq!(fields["inherited"].ty, Type::Boolean);
        assert!(fields["inherited"].optional && fields["inherited"].readonly);
    }

    #[test]
    fn inherited_validator_fields_preserve_recursive_property_edges() {
        let base = declaration(
            "Base",
            vec![],
            Type::union(vec![
                Type::Null,
                reference("Derived", vec![Type::TypeVar("T".into())]),
            ]),
        );
        let mut derived = declaration(
            "Derived",
            vec![reference("Base", vec![Type::TypeVar("T".into())])],
            Type::Number,
        );
        derived.members.clear();
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types = vec![
            TypedTypeDecl::Interface(base),
            TypedTypeDecl::Interface(derived),
        ];
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::String]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(
            fields["value"].ty,
            Type::union(vec![Type::Null, reference("Derived", vec![Type::String])])
        );
    }

    #[test]
    fn diamond_heritage_reuses_flattened_contracts() {
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        for i in 0usize..30 {
            let bases = (i.saturating_sub(2)..i)
                .map(|base| reference(&format!("I{base}"), vec![Type::Number]))
                .collect();
            let mut decl = declaration(&format!("I{i}"), bases, Type::Number);
            if i > 0 {
                decl.members.clear();
            }
            ast.types.push(TypedTypeDecl::Interface(decl));
        }
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("I29", vec![Type::Number]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["value"].ty, Type::Number);
    }

    #[test]
    fn property_override_removes_inherited_method_marker() {
        let mut base = declaration("Base", vec![], Type::Number);
        base.members = vec![TypedInterfaceMember::Method {
            name: crate::Ident {
                name: "value".into(),
                span: crate::Span::at(crate::FileId(0)),
            },
            generics: Default::default(),
            params: vec![],
            return_type: Type::Number,
            doc: None,
        }];
        let function = Type::Function {
            params: vec![],
            ret: Box::new(Type::Number),
            has_rest: false,
            optional: 0,
            predicate: None,
        };
        let derived = declaration(
            "Derived",
            vec![reference("Base", vec![Type::Number])],
            function.clone(),
        );
        let mut method_child = declaration(
            "Methods",
            vec![reference("Base", vec![Type::Number])],
            Type::Number,
        );
        method_child.members.clear();
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types = vec![
            TypedTypeDecl::Interface(base),
            TypedTypeDecl::Interface(derived),
            TypedTypeDecl::Interface(method_child),
        ];
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::Number]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["value"].ty, function);
        assert!(matches!(
            bodies
                .expand_one(&reference("Methods", vec![Type::Number]))
                .unwrap(),
            Type::InterfaceRef { .. }
        ));
    }

    #[test]
    fn heritage_only_dependency_is_selected_for_local_validator() {
        let mut dependency = crate::PackageDeclaration::with_package("@test/base");
        let mangled = crate::mangle::package_symbol("@test/base", "Base");
        dependency.types.insert(
            "Base".into(),
            crate::TypeSymbol {
                name: "Base".into(),
                mangled_name: mangled.clone(),
                declaration_span: crate::Span::at(crate::FileId(0)),
                kind: TypeKind::Interface {
                    extends: vec![],
                    generics: Default::default(),
                    methods: BTreeMap::new(),
                    properties: BTreeMap::from([(
                        "inherited".into(),
                        crate::PropertySig {
                            ty: Type::String,
                            optional: false,
                            readonly: false,
                            intrinsic: false,
                            doc: None,
                        },
                    )]),
                    index: None,
                    dispatch: crate::Dispatch::VTable,
                    doc: None,
                },
            },
        );
        let imported =
            Type::interface_ref(crate::Package("@test/base".into()), "Base", mangled, vec![]);
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types.push(TypedTypeDecl::Interface(declaration(
            "Derived",
            vec![imported],
            Type::Number,
        )));
        let analysis =
            crate::codegen::analysis::CodegenAnalysis::collect(&ast, &[&dependency]).unwrap();
        let usage = analysis.dependency_usage.finish(&[&dependency]);
        let dependencies = [&dependency];
        let selected = usage.dependency_types(&dependencies);
        let bodies = ValidatorBodies::collect(&ast, &selected).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::Number]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["inherited"].ty, Type::String);
    }

    #[test]
    fn inherited_generic_arguments_with_crossing_names_are_simultaneous() {
        let mut base = declaration("Base", vec![], Type::TypeVar("T".into()));
        base.generics = crate::GenericParameters::unconstrained(vec!["T".into(), "U".into()]);
        let mut derived = declaration(
            "Derived",
            vec![reference(
                "Base",
                vec![Type::TypeVar("U".into()), Type::Number],
            )],
            Type::Boolean,
        );
        derived.generics = crate::GenericParameters::unconstrained(vec!["U".into()]);
        derived.members.clear();
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types = vec![
            TypedTypeDecl::Interface(base),
            TypedTypeDecl::Interface(derived),
        ];
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::String]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["value"].ty, Type::String);
    }

    #[test]
    fn raw_intersection_heritage_meets_overlapping_property_types() {
        let mut derived = declaration(
            "Derived",
            vec![Type::intersection(
                [Type::Number, Type::union(vec![Type::Number, Type::String])]
                    .into_iter()
                    .map(|ty| Type::Object {
                        fields: BTreeMap::from([("x".into(), ObjectField::required(ty))]),
                        index: None,
                    })
                    .collect(),
            )],
            Type::Boolean,
        );
        derived.members.clear();
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types.push(TypedTypeDecl::Interface(derived));
        let bodies = ValidatorBodies::collect(&ast, []).unwrap();
        let Type::Object { fields, .. } = bodies
            .expand_one(&reference("Derived", vec![Type::Number]))
            .unwrap()
        else {
            panic!("data object");
        };
        assert_eq!(fields["x"].ty, Type::Number);
    }

    #[test]
    fn cyclic_heritage_metadata_returns_a_limit_error() {
        let mut ast = TypedAst::new();
        ast.package_name = "main".into();
        ast.types.push(TypedTypeDecl::Interface(declaration(
            "Cycle",
            vec![reference("Cycle", vec![Type::TypeVar("T".into())])],
            Type::Number,
        )));
        assert!(matches!(
            ValidatorBodies::collect(&ast, []),
            Err(TypeTooLarge::Depth)
        ));
    }
}
