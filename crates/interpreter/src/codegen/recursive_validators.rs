//! Discovery + planning for per-alias recursive runtime validators.
//!
//! `x as T` lowers to a structural walk of `T`. When `T` is
//! recursive the walk would loop forever at codegen time, so each recursion
//! back-edge (`Type::AliasRef`) is compiled to a *call* into a generated helper
//! function whose body validates the alias's expanded shape — the runtime
//! recursion terminates on the (finite) input value instead.
//!
//! This module finds every back-edge reachable from an `as`-cast check shape,
//! records the expanded body each helper must emit, and collects the object
//! shapes those bodies need so the subtype/vtable pre-pass emits them even when
//! no literal of that shape appears in the program.

use std::collections::{BTreeMap, BTreeSet};

use crate::codegen::dependency_usage::DependencyType;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{
    ObjectField, Shape, Type, TypeKind, TypedAst, TypedExprKind, TypedInterfaceMember,
    TypedTypeDecl,
};

/// Resolves a recursion back-edge to its body, for both `Type::AliasRef`
/// (→ alias body) and `Type::InterfaceRef` (→ structural object of a
/// **data-only** interface's properties). Built from the user module's
/// declarations plus imported dependencies; user declarations win a name clash.
pub struct AliasBodies {
    map: BTreeMap<String, (Vec<String>, Type)>,
    /// Interface name → (generics, property field map). Method-bearing
    /// interfaces are omitted — they can't be constructed as plain objects, so
    /// the typechecker rejects them as JSON/cast targets before codegen.
    interfaces: BTreeMap<String, (Vec<String>, BTreeMap<String, ObjectField>)>,
}

impl AliasBodies {
    pub fn collect<'a>(
        ta: &TypedAst,
        dependency_types: impl IntoIterator<Item = &'a DependencyType<'a>>,
    ) -> Self {
        let mut map: BTreeMap<String, (Vec<String>, Type)> = BTreeMap::new();
        let mut interfaces: BTreeMap<String, (Vec<String>, BTreeMap<String, ObjectField>)> =
            BTreeMap::new();
        for dependency_type in dependency_types {
            let name = dependency_type.name;
            let sym = dependency_type.symbol;
            match &sym.kind {
                TypeKind::Alias { generics, ty, .. } => {
                    map.entry(name.to_string())
                        .or_insert_with(|| (generics.clone(), ty.clone()));
                }
                TypeKind::Interface {
                    generics,
                    methods,
                    properties,
                    ..
                } if methods.is_empty() => {
                    interfaces.entry(name.to_string()).or_insert_with(|| {
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
        for decl in &ta.types {
            match decl {
                TypedTypeDecl::Alias(alias) => {
                    map.insert(
                        alias.name.name.clone(),
                        (alias.generics.clone(), alias.ty.clone()),
                    );
                }
                TypedTypeDecl::Interface(iface) => {
                    if let Some(fields) = data_only_fields(&iface.members) {
                        interfaces
                            .insert(iface.name.name.clone(), (iface.generics.clone(), fields));
                    }
                }
                _ => {}
            }
        }
        Self { map, interfaces }
    }

    /// Expand one level: a `Type::AliasRef` becomes its alias body, a
    /// `Type::InterfaceRef` becomes its data-only property shape (both peeled,
    /// with generic args substituted); any other type returns `ty.peel()` cloned.
    pub fn expand_one(&self, ty: &Type) -> Type {
        match ty.peel() {
            Type::AliasRef { name, args, .. } => {
                let Some((generics, body)) = self.map.get(name) else {
                    return ty.peel().clone();
                };
                let expanded = if generics.is_empty() {
                    body.clone()
                } else {
                    TypeParamSubstitution::from_pairs(generics, args).apply(body)
                };
                expanded.peel().clone()
            }
            Type::InterfaceRef { name, args, .. } => {
                let Some((generics, fields)) = self.interfaces.get(name) else {
                    return ty.peel().clone();
                };
                let sub = (!generics.is_empty())
                    .then(|| TypeParamSubstitution::from_pairs(generics, args));
                let fields = fields
                    .iter()
                    .map(|(field, f)| {
                        let ty = match &sub {
                            Some(sub) => sub.apply(&f.ty),
                            None => f.ty.clone(),
                        };
                        (
                            field.clone(),
                            ObjectField {
                                ty,
                                optional: f.optional,
                                readonly: f.readonly,
                            },
                        )
                    })
                    .collect();
                Type::Object { fields }
            }
            other => other.clone(),
        }
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
}

/// The full plan: cast helpers and the extra object shapes their bodies need.
pub struct RecursiveValidators {
    pub cast: Vec<ValidatorPlan>,
    pub extra_shapes: Vec<Shape>,
}

pub fn discover(ta: &TypedAst, aliases: &AliasBodies) -> RecursiveValidators {
    let mut shapes: BTreeSet<Shape> = BTreeSet::new();

    let mut cast_keys: BTreeSet<Type> = BTreeSet::new();

    for i in 0..ta.exprs_len() {
        let expr = ta.expr(crate::ExprId(i as u32));
        if let TypedExprKind::Cast {
            check: Some(shape), ..
        } = &expr.kind
        {
            walk(shape, aliases, &mut cast_keys, &mut shapes);
        }
    }

    let plan = |keys: BTreeSet<Type>| -> Vec<ValidatorPlan> {
        keys.into_iter()
            .map(|key| {
                let body = aliases.expand_one(&key);
                ValidatorPlan { key, body }
            })
            .collect()
    };

    RecursiveValidators {
        cast: plan(cast_keys),
        extra_shapes: shapes.into_iter().collect(),
    }
}

/// Walk a validation target, registering reachable back-edges in `refs` and the
/// object shapes that helper bodies will construct/test in `shapes`. The `refs`
/// set both deduplicates and breaks recursion: a re-encountered back-edge is
/// skipped before re-expanding.
fn walk(ty: &Type, aliases: &AliasBodies, refs: &mut BTreeSet<Type>, shapes: &mut BTreeSet<Shape>) {
    let peeled = ty.peel();
    match peeled {
        Type::Object { fields } => {
            shapes.insert(Shape::Object {
                fields: fields.clone(),
            });
            for f in fields.values() {
                walk(&f.ty, aliases, refs, shapes);
            }
        }
        Type::Array(elem) => walk(elem, aliases, refs, shapes),
        Type::Tuple(elems) => {
            for e in elems {
                walk(e, aliases, refs, shapes);
            }
        }
        Type::Union(members) => {
            for m in members {
                walk(m, aliases, refs, shapes);
            }
        }
        // Both lower to a generated validator: an `AliasRef` re-enters its alias
        // body, an `InterfaceRef` its data-only property shape. The `refs` set
        // breaks recursion so a self-referential interface terminates.
        Type::AliasRef { .. } | Type::InterfaceRef { .. } if refs.insert(peeled.clone()) => {
            let body = aliases.expand_one(peeled);
            walk(&body, aliases, refs, shapes);
        }
        _ => {}
    }
}
