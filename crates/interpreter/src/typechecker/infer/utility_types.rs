//! Evaluation of TypeScript's built-in utility types.
//!
//! The utilities are prelude aliases (`runtime::prelude::utility_types`) whose
//! bodies are applications of themselves. An application is evaluated here,
//! in Rust, once its arguments no longer mention a type parameter; until then
//! it stays the application (`Type::AliasRef` to the prelude alias). The
//! results follow tsc 6.0.3 (`instantiateMappedType` for the homomorphic
//! mapped types, distributive conditional types for `Exclude`, `Extract`,
//! `Parameters` and `ReturnType`, and the intrinsic string mappings).

use std::collections::BTreeMap;

use crate::mangle::MangledName;
use crate::{ObjectField, Package, Type, TypeKind};

use super::assignable::{TypeResolver, assignable, is_host_backed_interface};
use super::narrowing::strip_undefined;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Utility {
    Partial,
    Required,
    Readonly,
    Pick,
    Omit,
    Exclude,
    Extract,
    NonNullable,
    Parameters,
    ReturnType,
    NoInfer,
    Uppercase,
    Lowercase,
    Capitalize,
    Uncapitalize,
}

const ALL: [(Utility, &str); 15] = [
    (Utility::Partial, "Partial"),
    (Utility::Required, "Required"),
    (Utility::Readonly, "Readonly"),
    (Utility::Pick, "Pick"),
    (Utility::Omit, "Omit"),
    (Utility::Exclude, "Exclude"),
    (Utility::Extract, "Extract"),
    (Utility::NonNullable, "NonNullable"),
    (Utility::Parameters, "Parameters"),
    (Utility::ReturnType, "ReturnType"),
    (Utility::NoInfer, "NoInfer"),
    (Utility::Uppercase, "Uppercase"),
    (Utility::Lowercase, "Lowercase"),
    (Utility::Capitalize, "Capitalize"),
    (Utility::Uncapitalize, "Uncapitalize"),
];

/// The utility a prelude alias names, keyed on its mangled identity so a user
/// type of the same name never matches.
pub(crate) fn utility(mangled: &MangledName) -> Option<Utility> {
    ALL.iter()
        .find(|(_, name)| &crate::mangle::prelude(name) == mangled)
        .map(|(utility, _)| *utility)
}

impl Utility {
    pub(crate) fn name(self) -> &'static str {
        ALL.iter()
            .find(|(utility, _)| *utility == self)
            .map_or("", |(_, name)| name)
    }

    fn is_string_mapping(self) -> bool {
        matches!(
            self,
            Utility::Uppercase | Utility::Lowercase | Utility::Capitalize | Utility::Uncapitalize
        )
    }
}

/// The outcome of evaluating one application.
#[derive(Debug)]
pub(crate) enum Evaluated {
    Resolved(Type),
    /// An argument still mentions a type parameter.
    Deferred,
    /// The application has no representation here. `None` when a constraint
    /// check of the same application already reports it.
    Invalid(Option<String>),
}

/// Whether a type parameter occurs anywhere in `ty`, either a signature
/// `TypeVar` or a body `GenericParam`.
pub(crate) fn holds_type_parameter(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if matches!(ty, Type::TypeVar(_) | Type::GenericParam { .. }) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}

/// The application of `utility` to `args`, or the reason it has none.
pub(crate) fn evaluate(utility: Utility, args: &[Type], types: TypeResolver<'_>) -> Evaluated {
    if args.iter().any(holds_type_parameter) {
        return Evaluated::Deferred;
    }
    if args.iter().any(|arg| matches!(arg.peel(), Type::Error)) {
        return Evaluated::Resolved(Type::Error);
    }
    let evaluator = Evaluator { types };
    let result = match (utility, args) {
        (Utility::Partial | Utility::Required | Utility::Readonly, [operand]) => {
            evaluator.homomorphic(utility, operand, 0)
        }
        (Utility::Pick, [object, keys]) => evaluator.pick(object, keys),
        (Utility::Omit, [object, keys]) => evaluator.omit(object, keys),
        (Utility::Exclude, [operand, filter]) => Ok(evaluator.filter(operand, filter, false)),
        (Utility::Extract, [operand, filter]) => Ok(evaluator.filter(operand, filter, true)),
        (Utility::NonNullable, [operand]) => Ok(non_nullable(operand)),
        (Utility::Parameters | Utility::ReturnType, [operand]) => {
            evaluator.signature_part(utility, operand)
        }
        (Utility::NoInfer, [operand]) => Ok(operand.clone()),
        (mapping, [operand]) if mapping.is_string_mapping() => {
            evaluator.string_mapping(mapping, operand)
        }
        _ => Err(None),
    };
    match result {
        Ok(ty) => Evaluated::Resolved(ty),
        Err(reason) => Evaluated::Invalid(reason),
    }
}

/// The type an application stands for, labeled as tsc prints it: an object
/// result of a mapped utility keeps the application's name (`Partial<User>`),
/// while conditional and intrinsic results, and mapped arrays, tuples and
/// primitives, print bare (`Exclude<"a" | "b", "a">` is `"b"`).
pub(crate) fn label(utility: Utility, args: Vec<Type>, evaluated: Type) -> Type {
    let labeled = match utility {
        Utility::Partial
        | Utility::Required
        | Utility::Readonly
        | Utility::Pick
        | Utility::Omit => matches!(
            evaluated.without_aliases(),
            Type::Object { .. } | Type::Union(_)
        ),
        _ => false,
    };
    if labeled {
        application(utility, args, Some(evaluated))
    } else {
        evaluated
    }
}

/// `utility<args>`: the deferred application when `body` is `None`, else the
/// labeled alias over `body`.
pub(crate) fn application(utility: Utility, args: Vec<Type>, body: Option<Type>) -> Type {
    let name = utility.name();
    let mangled = crate::mangle::prelude(name);
    match body {
        Some(body) => Type::alias_ty(Package::prelude(), name, mangled, args, Box::new(body)),
        None => Type::alias_ref(Package::prelude(), name, mangled, args),
    }
}

/// `NonNullable<T>`, tsc's `T & {}`: `null`, `undefined` and `void` drop out,
/// and `unknown` becomes `{}`.
fn non_nullable(operand: &Type) -> Type {
    let members: Vec<Type> = members(operand)
        .into_iter()
        .filter_map(|member| match member.without_aliases() {
            Type::Null | Type::Undefined | Type::Void => None,
            Type::Unknown => Some(Type::empty_object()),
            _ => Some(member),
        })
        .collect();
    Type::union(members)
}

/// The members a distributive utility maps over: a union's members, or the
/// type itself.
fn members(ty: &Type) -> Vec<Type> {
    match ty.without_aliases() {
        Type::Union(members) => members.clone(),
        Type::Never => Vec::new(),
        _ => vec![ty.clone()],
    }
}

/// Bound on alias expansions while looking for an operand's structure.
const MAX_EXPANSIONS: u32 = 64;

struct Evaluator<'a> {
    types: TypeResolver<'a>,
}

type Outcome = Result<Type, Option<String>>;

/// An object type's fields and index signature, as a mapped type reads them.
type Structure = (BTreeMap<String, ObjectField>, Option<crate::IndexSignature>);

impl Evaluator<'_> {
    /// `Partial`, `Required` and `Readonly`: tsc's homomorphic mapped types,
    /// which distribute over unions and map arrays and tuples elementwise.
    fn homomorphic(&self, utility: Utility, operand: &Type, depth: u32) -> Outcome {
        if depth > MAX_EXPANSIONS {
            return Err(Some("the type argument expands too deeply".into()));
        }
        let next = depth + 1;
        match operand {
            Type::Alias { ty, .. } => self.homomorphic(utility, ty, next),
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                let body = super::type_aliases::alias_ref_body(mangled, name, args, self.types);
                self.homomorphic(utility, &body, next)
            }
            Type::Union(members) => {
                let mapped = members
                    .iter()
                    .map(|member| self.homomorphic(utility, member, next))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Type::union(mapped))
            }
            Type::Never | Type::Error => Ok(operand.clone()),
            Type::Unknown | Type::NonPrimitive | Type::Function { .. } => Ok(Type::empty_object()),
            Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::TemplateLiteral { .. }
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. } => Ok(operand.clone()),
            Type::Array(element) => Ok(match utility {
                Utility::Readonly => Type::Readonly(Box::new(operand.clone())),
                _ => Type::Array(Box::new(map_element(utility, element))),
            }),
            Type::Tuple(tuple) => Ok(match utility {
                Utility::Readonly => Type::Readonly(Box::new(operand.clone())),
                _ => Type::Tuple(map_tuple(utility, tuple)),
            }),
            Type::Readonly(inner) => match inner.as_ref() {
                Type::Array(element) => Ok(Type::Readonly(Box::new(match utility {
                    Utility::Readonly => inner.as_ref().clone(),
                    _ => Type::Array(Box::new(map_element(utility, element))),
                }))),
                Type::Tuple(tuple) => Ok(Type::Readonly(Box::new(match utility {
                    Utility::Readonly => inner.as_ref().clone(),
                    _ => Type::Tuple(map_tuple(utility, tuple)),
                }))),
                other => self.homomorphic(utility, other, next),
            },
            Type::Object { fields, index } => Ok(map_object(utility, fields, index.as_ref())),
            Type::InterfaceRef { .. } | Type::ClassRef { .. } => {
                let (fields, index) = self.structure(operand)?;
                Ok(map_object(utility, &fields, index.as_ref()))
            }
            Type::Refined { ty, .. } => self.homomorphic(utility, ty, next),
            Type::Uint8Array => Err(Some(format!(
                "`{}` does not apply to `Uint8Array`",
                utility.name()
            ))),
            Type::TypeVar(_) | Type::GenericParam { .. } => Err(None),
        }
    }

    /// The fields and index signature of an interface or class instance, as
    /// a plain object type would spell them.
    fn structure(&self, ty: &Type) -> Result<Structure, Option<String>> {
        match ty {
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                if is_host_backed_interface(ty) {
                    return Err(Some(format!(
                        "`{ty}` is built by the host and has no structural form"
                    )));
                }
                let Some(symbol) = self.types.lookup(mangled, name) else {
                    return Err(Some(format!("`{ty}` has no declaration")));
                };
                let TypeKind::Interface {
                    methods,
                    index,
                    generics,
                    ..
                } = &symbol.kind
                else {
                    return Err(Some(format!("`{ty}` is not an interface")));
                };
                if methods.values().any(|method| !method.generics.is_empty()) {
                    return Err(Some(format!(
                        "`{ty}` has a generic method, which a mapped type cannot carry"
                    )));
                }
                let fields = self
                    .types
                    .interface_full_form(mangled, name, args)
                    .unwrap_or_default();
                let index = index.as_ref().map(|index| {
                    let bindings: BTreeMap<String, Type> =
                        generics.iter().cloned().zip(args.iter().cloned()).collect();
                    index.map_types(|ty| {
                        super::generic::substitute_or_record(ty, &bindings, self.types.limits)
                    })
                });
                Ok((fields, index))
            }
            Type::ClassRef { mangled, args, .. } => {
                let fields = self
                    .types
                    .class_full_form(mangled, args)
                    .ok_or_else(|| Some(format!("`{ty}` has no declaration")))?;
                Ok((fields, None))
            }
            Type::Object { fields, index } => Ok((fields.clone(), index.clone())),
            _ => Err(Some(format!("`{ty}` is not an object type"))),
        }
    }

    /// The fields, by name, of each member of `object`, with alias labels
    /// removed. `None` for a member with no object structure.
    fn object_members(&self, object: &Type) -> Result<Vec<Structure>, Option<String>> {
        let mut out = Vec::new();
        let mut pending = vec![(object.clone(), 0)];
        while let Some((ty, depth)) = pending.pop() {
            if depth > MAX_EXPANSIONS {
                return Err(Some("the type argument expands too deeply".into()));
            }
            match ty {
                Type::Alias { ty, .. } | Type::Refined { ty, .. } => {
                    pending.push((*ty, depth + 1));
                }
                Type::AliasRef {
                    mangled,
                    name,
                    args,
                    ..
                } => pending.push((
                    super::type_aliases::alias_ref_body(&mangled, &name, &args, self.types),
                    depth + 1,
                )),
                Type::Union(members) => {
                    pending.extend(members.into_iter().rev().map(|member| (member, depth + 1)));
                }
                other => out.push(self.structure(&other)?),
            }
        }
        Ok(out)
    }

    /// `Pick<T, K>`: for each key in `K`, `T`'s property with its modifiers.
    /// Not distributive: a union `T` contributes each member's property.
    fn pick(&self, object: &Type, keys: &Type) -> Outcome {
        let members = self.object_members(object)?;
        let available = common_keys(&members);
        let mut fields = BTreeMap::new();
        for key in members_of_keys(keys) {
            let Some(name) = key_name(&key) else {
                return Err(Some(format!(
                    "Type `{keys}` does not satisfy the constraint `keyof {object}`"
                )));
            };
            if !available.iter().any(|known| known == &name)
                && !members.iter().all(|(_, index)| index.is_some())
            {
                return Err(Some(format!(
                    "Type `{keys}` does not satisfy the constraint `keyof {object}`"
                )));
            }
            fields.insert(name.clone(), merged_field(&members, &name));
        }
        Ok(Type::Object {
            fields,
            index: None,
        })
    }

    /// `Omit<T, K>`, tsc's `Pick<T, Exclude<keyof T, K>>`. An index signature
    /// on `T` makes `keyof T` all strings, so only the signature survives.
    fn omit(&self, object: &Type, keys: &Type) -> Outcome {
        let members = self.object_members(object)?;
        if let Some(index) = shared_index(&members) {
            return Ok(Type::Object {
                fields: BTreeMap::new(),
                index: Some(index),
            });
        }
        let omitted: Vec<Type> = members_of_keys(keys);
        let omits_every_string = omitted
            .iter()
            .any(|key| matches!(key.without_aliases(), Type::String));
        let mut fields = BTreeMap::new();
        for name in common_keys(&members) {
            let dropped = omits_every_string
                || omitted
                    .iter()
                    .any(|key| key_name(key).as_deref() == Some(name.as_str()));
            if !dropped {
                let field = merged_field(&members, &name);
                fields.insert(name, field);
            }
        }
        Ok(Type::Object {
            fields,
            index: None,
        })
    }

    /// `Exclude<T, U>` (`keep_assignable: false`) and `Extract<T, U>`, which
    /// distribute over `T` and keep each member by whether it is assignable
    /// to `U`. `boolean` and enums distribute over their members, as in tsc.
    fn filter(&self, operand: &Type, filter: &Type, keep_assignable: bool) -> Type {
        let mut kept = Vec::new();
        for member in members(operand) {
            let expanded = match member.without_aliases() {
                Type::Boolean => vec![Type::BooleanLiteral(false), Type::BooleanLiteral(true)],
                enumeration @ (Type::NumberEnum { member: None, .. }
                | Type::StringEnum { member: None, .. }) => {
                    super::comparable::enum_member_types(enumeration, self.types)
                        .unwrap_or_else(|| vec![member.clone()])
                }
                _ => vec![member.clone()],
            };
            kept.extend(
                expanded
                    .into_iter()
                    .filter(|member| assignable(member, filter, self.types) == keep_assignable),
            );
        }
        Type::union(kept)
    }

    /// `Parameters<F>` and `ReturnType<F>`, distributed over a union of
    /// function types.
    fn signature_part(&self, utility: Utility, operand: &Type) -> Outcome {
        let parts = members(operand)
            .into_iter()
            .map(|member| match member.peel() {
                Type::Function {
                    params,
                    optional,
                    ret,
                    has_rest,
                    ..
                } => match utility {
                    Utility::ReturnType => Ok(ret.as_ref().clone()),
                    _ if !*has_rest => Ok(Type::Tuple(crate::TupleType {
                        elements: params.clone(),
                        optional: *optional,
                    })),
                    _ => match params.as_slice() {
                        [rest] => Ok(rest.clone()),
                        _ => Err(Some(format!(
                            "`Parameters` of `{member}` would need a tuple with a rest element, \
                             which Submilli does not support"
                        ))),
                    },
                },
                Type::Error => Ok(Type::Error),
                _ => Err(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Type::union(parts))
    }

    /// `Uppercase`, `Lowercase`, `Capitalize` and `Uncapitalize`, which map
    /// literals with JavaScript's default case mapping and keep `string`
    /// symbolic (`Uppercase<string>`).
    fn string_mapping(&self, mapping: Utility, operand: &Type) -> Outcome {
        let mapped = members(operand)
            .into_iter()
            .map(|member| match member.without_aliases() {
                Type::StringLiteral(text) => Ok(Type::StringLiteral(map_text(mapping, text))),
                Type::StringEnum {
                    member: Some(member),
                    ..
                } => Ok(Type::StringLiteral(map_text(mapping, &member.value))),
                Type::String => Ok(symbolic_string_mapping(mapping)),
                Type::TemplateLiteral {
                    parts,
                    substitutions,
                } => Ok(map_template(mapping, parts, substitutions)),
                Type::Alias { mangled, args, .. }
                    if utility(mangled).is_some_and(Utility::is_string_mapping)
                        && matches!(args.as_slice(), [Type::String]) =>
                {
                    // `Uppercase<Lowercase<string>>` keeps the outer mapping.
                    Ok(symbolic_string_mapping(mapping))
                }
                Type::Error => Ok(Type::Error),
                _ => Err(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Type::union(mapped))
    }
}

/// `Uppercase<string>` and its siblings: a `string` that only the mapping's
/// fixed points inhabit.
pub(crate) fn symbolic_string_mapping(mapping: Utility) -> Type {
    application(mapping, vec![Type::String], Some(Type::String))
}

/// Whether a string literal is unchanged by the symbolic mapping `ty`
/// (`Uppercase<string>` holds `"ABC"` but not `"abc"`). `None` when `ty` is
/// not a symbolic string mapping.
pub(crate) fn symbolic_mapping_of(ty: &Type) -> Option<Utility> {
    let Type::Alias { mangled, args, .. } = ty else {
        return None;
    };
    let mapping = utility(mangled).filter(|utility| utility.is_string_mapping())?;
    matches!(args.as_slice(), [arg] if matches!(arg.without_aliases(), Type::String))
        .then_some(mapping)
}

/// Whether `text` is a fixed point of `mapping`.
pub(crate) fn mapping_preserves(mapping: Utility, text: &str) -> bool {
    map_text(mapping, text) == text
}

fn map_text(mapping: Utility, text: &str) -> String {
    match mapping {
        Utility::Uppercase => text.to_uppercase(),
        Utility::Lowercase => text.to_lowercase(),
        Utility::Capitalize | Utility::Uncapitalize => {
            let mut chars = text.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            // tsc maps `charAt(0)`, one UTF-16 unit: a character outside the
            // BMP is half a surrogate pair there, which no mapping changes.
            if first.len_utf16() != 1 {
                return text.to_string();
            }
            let head: String = if mapping == Utility::Capitalize {
                first.to_uppercase().collect()
            } else {
                first.to_lowercase().collect()
            };
            head + chars.as_str()
        }
        _ => text.to_string(),
    }
}

/// A template literal's mapping: its literal text is mapped, and each hole
/// is wrapped in the mapping. `Capitalize` only touches the first position.
fn map_template(mapping: Utility, parts: &[String], substitutions: &[Type]) -> Type {
    let whole = matches!(mapping, Utility::Uppercase | Utility::Lowercase);
    let first_is_text = parts.first().is_some_and(|part| !part.is_empty());
    let parts = parts
        .iter()
        .enumerate()
        .map(|(position, part)| {
            if whole || position == 0 {
                map_text(mapping, part)
            } else {
                part.clone()
            }
        })
        .collect();
    let substitutions = substitutions
        .iter()
        .enumerate()
        .map(|(position, hole)| {
            let touched = whole || (position == 0 && !first_is_text);
            if touched && matches!(hole.without_aliases(), Type::String) {
                symbolic_string_mapping(mapping)
            } else {
                hole.clone()
            }
        })
        .collect();
    Type::TemplateLiteral {
        parts,
        substitutions,
    }
}

/// An array element under `Partial` (`T | undefined`) or `Required`
/// (without `undefined`).
fn map_element(utility: Utility, element: &Type) -> Type {
    match utility {
        Utility::Partial => Type::union(vec![element.clone(), Type::Undefined]),
        Utility::Required => strip_undefined(element),
        _ => element.clone(),
    }
}

fn map_tuple(utility: Utility, tuple: &crate::TupleType) -> crate::TupleType {
    match utility {
        Utility::Partial => crate::TupleType {
            elements: tuple
                .elements
                .iter()
                .map(|element| map_element(utility, element))
                .collect(),
            optional: tuple.elements.len(),
        },
        Utility::Required => crate::TupleType {
            elements: tuple
                .elements
                .iter()
                .enumerate()
                .map(|(position, element)| {
                    if position >= tuple.required_len() {
                        strip_undefined(element)
                    } else {
                        element.clone()
                    }
                })
                .collect(),
            optional: 0,
        },
        _ => tuple.clone(),
    }
}

fn map_object(
    utility: Utility,
    fields: &BTreeMap<String, ObjectField>,
    index: Option<&crate::IndexSignature>,
) -> Type {
    let fields = fields
        .iter()
        .map(|(name, field)| {
            let mut field = field.clone();
            match utility {
                Utility::Partial => field.optional = true,
                Utility::Required => {
                    // `-?` removes the `undefined` an optional property admits,
                    // even when spelled; a required `T | undefined` keeps it.
                    if field.optional {
                        field.ty = strip_undefined(&field.ty);
                    }
                    field.optional = false;
                }
                _ => field.readonly = true,
            }
            (name.clone(), field)
        })
        .collect();
    let index = index.map(|index| {
        let mut index = index.clone();
        match utility {
            Utility::Partial => {
                index.value = Box::new(Type::union(vec![*index.value, Type::Undefined]));
            }
            Utility::Readonly => index.readonly = true,
            _ => {}
        }
        index
    });
    Type::Object { fields, index }
}

/// The keys a key-type argument names: a union's members, or the type.
fn members_of_keys(keys: &Type) -> Vec<Type> {
    members(keys)
}

/// The property a key type names: a string or number literal, or a string
/// enum member.
fn key_name(key: &Type) -> Option<String> {
    match key.without_aliases() {
        Type::StringLiteral(name) => Some(name.clone()),
        Type::NumberLiteral(value) => Some(crate::runtime::number::format_number_js(value.0)),
        Type::StringEnum {
            member: Some(member),
            ..
        } => Some(member.value.clone()),
        Type::NumberEnum {
            member: Some(member),
            ..
        } => Some(crate::runtime::number::format_number_js(member.value.0)),
        _ => None,
    }
}

/// The property names every member has: `keyof (A | B)`.
fn common_keys(members: &[Structure]) -> Vec<String> {
    let Some(((first, _), rest)) = members.split_first() else {
        return Vec::new();
    };
    first
        .keys()
        .filter(|name| rest.iter().all(|(fields, _)| fields.contains_key(*name)))
        .cloned()
        .collect()
}

/// The index signature every member has, joined, if any.
fn shared_index(members: &[Structure]) -> Option<crate::IndexSignature> {
    let mut indexes = members.iter().map(|(_, index)| index.as_ref());
    let mut joined = indexes.next()??.clone();
    for index in indexes {
        let index = index?;
        joined.value = Box::new(Type::union(vec![
            *joined.value,
            index.value.as_ref().clone(),
        ]));
        joined.readonly |= index.readonly;
    }
    Some(joined)
}

/// Property `name` across `members` (`T[name]` for a union `T`): the union of
/// their types, optional or readonly when any member's is. A member without
/// the property supplies its index signature's value.
fn merged_field(members: &[Structure], name: &str) -> ObjectField {
    let mut types = Vec::new();
    let mut optional = false;
    let mut readonly = false;
    let mut method = true;
    for (fields, index) in members {
        if let Some(field) = fields.get(name) {
            types.push(field.ty.clone());
            optional |= field.optional;
            readonly |= field.readonly;
            method &= field.method;
        } else if let Some(index) = index {
            types.push(index.value.as_ref().clone());
            readonly |= index.readonly;
            method = false;
        }
    }
    ObjectField {
        ty: Type::union(types),
        optional,
        readonly,
        method,
    }
}
