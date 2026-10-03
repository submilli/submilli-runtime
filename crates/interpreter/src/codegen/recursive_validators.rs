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
    ) -> Self {
        let mut imported_aliases = BTreeMap::new();
        let mut imported_interfaces = BTreeMap::new();
        for dependency_type in dependency_types {
            let sym = dependency_type.symbol;
            match &sym.kind {
                TypeKind::Alias { generics, ty, .. } => {
                    imported_aliases
                        .entry(sym.mangled_name.clone())
                        .or_insert_with(|| (generics.clone(), ty.clone()));
                }
                TypeKind::Interface {
                    generics,
                    methods,
                    properties,
                    ..
                } if methods.is_empty() => {
                    imported_interfaces
                        .entry(sym.mangled_name.clone())
                        .or_insert_with(|| {
                            let fields = properties
                                .iter()
                                .map(|(field, sig)| {
                                    (
                                        field.clone(),
                                        ObjectField {
                                            ty: sig.ty.clone(),
                                            optional: sig.optional,
                                            readonly: sig.readonly,
                                        },
                                    )
                                })
                                .collect();
                            (generics.clone(), fields)
                        });
                }
                _ => {}
            }
        }
        let mut local_aliases = BTreeMap::new();
        let mut local_interfaces = BTreeMap::new();
        for decl in &ta.types {
            match decl {
                TypedTypeDecl::Alias(alias) => {
                    local_aliases.insert(
                        alias.name.name.clone(),
                        (alias.generics.clone(), alias.ty.clone()),
                    );
                }
                TypedTypeDecl::Interface(iface) => {
                    if let Some(fields) = data_only_fields(&iface.members) {
                        local_interfaces
                            .insert(iface.name.name.clone(), (iface.generics.clone(), fields));
                    }
                }
                _ => {}
            }
        }
        Self {
            class_fields: ta.runtime_class_fields.clone(),
            imported_aliases,
            local_aliases,
            imported_interfaces,
            local_interfaces,
            limits: TypeLimits::default(),
        }
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
                    TypeParamSubstitution::from_pairs(generics, args).apply(body, &self.limits)?
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
                    .then(|| TypeParamSubstitution::from_pairs(generics, args));
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

/// Property field map for a method-free interface, or `None` if any member is a
/// method (a method-bearing interface has no plain-object representation).
fn data_only_fields(members: &[TypedInterfaceMember]) -> Option<BTreeMap<String, ObjectField>> {
    let mut fields = BTreeMap::new();
    for member in members {
        match member {
            TypedInterfaceMember::Method { .. } => return None,
            TypedInterfaceMember::Property {
                name,
                ty,
                optional,
                readonly,
                ..
            } => {
                fields.insert(
                    name.name.clone(),
                    ObjectField {
                        ty: ty.clone(),
                        optional: *optional,
                        readonly: *readonly,
                    },
                );
            }
        }
    }
    Some(fields)
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
                crate::InterfaceCarrier::Array(ty) | crate::InterfaceCarrier::Set(ty) => {
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
        Type::Tuple(elements) | Type::Union(elements) => elements
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
            Type::Tuple(elements) | Type::Union(elements) => {
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
        Type::Union(types) | Type::Tuple(types) => {
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
    matches!(ty, Type::InterfaceRef { args, .. } if !args.is_empty() && args.iter().all(|arg| matches!(arg, Type::TypeVar(_))))
}
