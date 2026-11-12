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

/// The utility a prelude alias names, keyed on its mangled identity so a user
/// type of the same name never matches.
pub(crate) fn utility(mangled: &MangledName) -> Option<Utility> {
    let name = mangled
        .as_str()
        .strip_prefix(crate::mangle::PRELUDE_PACKAGE)?
        .strip_prefix(crate::mangle::SEP)?;
    Utility::ALL
        .into_iter()
        .find(|utility| utility.name() == name)
}

impl Utility {
    pub(crate) const ALL: [Utility; 15] = [
        Utility::Partial,
        Utility::Required,
        Utility::Readonly,
        Utility::Pick,
        Utility::Omit,
        Utility::Exclude,
        Utility::Extract,
        Utility::NonNullable,
        Utility::Parameters,
        Utility::ReturnType,
        Utility::NoInfer,
        Utility::Uppercase,
        Utility::Lowercase,
        Utility::Capitalize,
        Utility::Uncapitalize,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Utility::Partial => "Partial",
            Utility::Required => "Required",
            Utility::Readonly => "Readonly",
            Utility::Pick => "Pick",
            Utility::Omit => "Omit",
            Utility::Exclude => "Exclude",
            Utility::Extract => "Extract",
            Utility::NonNullable => "NonNullable",
            Utility::Parameters => "Parameters",
            Utility::ReturnType => "ReturnType",
            Utility::NoInfer => "NoInfer",
            Utility::Uppercase => "Uppercase",
            Utility::Lowercase => "Lowercase",
            Utility::Capitalize => "Capitalize",
            Utility::Uncapitalize => "Uncapitalize",
        }
    }

    pub(crate) fn is_string_mapping(self) -> bool {
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
    if !structure_known(utility, args) {
        return Evaluated::Deferred;
    }
    // An argument that is itself a lazy form (`Uppercase<B["s"]>` over a later
    // declaration) is evaluated first.
    let mut failures = Vec::new();
    let args: Vec<Type> = args
        .iter()
        .map(|arg| evaluate_guarded(arg.clone(), types, &mut failures))
        .collect();
    if let Some(failure) = failures.pop() {
        return Evaluated::Invalid(Some(failure));
    }
    let args = args.as_slice();
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
    let keeps_name = match utility {
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
    if keeps_name {
        labeled(utility, args, evaluated)
    } else {
        evaluated
    }
}

/// `utility<args>` as an application still to evaluate (a `Type::AliasRef`
/// to the prelude alias).
pub(crate) fn deferred(utility: Utility, args: Vec<Type>) -> Type {
    let name = utility.name();
    Type::alias_ref(Package::prelude(), name, crate::mangle::prelude(name), args)
}

/// `utility<args>` labeling the type `body` it evaluated to.
pub(crate) fn labeled(utility: Utility, args: Vec<Type>, body: Type) -> Type {
    let name = utility.name();
    Type::alias_ty(
        Package::prelude(),
        name,
        crate::mangle::prelude(name),
        args,
        Box::new(body),
    )
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
pub(crate) fn members(ty: &Type) -> Vec<Type> {
    match ty.without_aliases() {
        Type::Union(members) => members.clone(),
        Type::Never => Vec::new(),
        _ => vec![ty.clone()],
    }
}

/// Bound on recursion back-edge (`AliasRef`) expansions while looking for an
/// operand's structure. Peeling an inline alias label is finite and free.
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
            Type::Alias { ty, .. } => self.homomorphic(utility, ty, depth),
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
            Type::Array(_) | Type::Tuple(_) => Ok(match utility {
                Utility::Readonly => Type::Readonly(Box::new(operand.clone())),
                _ => map_container(utility, operand),
            }),
            Type::Readonly(inner) if matches!(inner.as_ref(), Type::Array(_) | Type::Tuple(_)) => {
                Ok(Type::Readonly(Box::new(map_container(utility, inner))))
            }
            Type::Readonly(inner) => self.homomorphic(utility, inner, next),
            Type::Object { fields, index } => Ok(map_object(utility, fields, index.as_ref())),
            Type::InterfaceRef { .. } | Type::ClassRef { .. } => {
                let (fields, index) = self.structure(operand)?;
                Ok(map_object(utility, &fields, index.as_ref()))
            }
            Type::Refined { ty, .. } => self.homomorphic(utility, ty, depth),
            Type::Uint8Array => Err(Some(format!(
                "`{}` does not apply to `Uint8Array`",
                utility.name()
            ))),
            Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::KeyOf(_)
            | Type::IndexedAccess { .. }
            | Type::Mapped { .. }
            | Type::Conditional { .. }
            | Type::Intersection(_) => Err(None),
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
                    pending.push((*ty, depth));
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
        let mut index = None;
        // A key outside `keyof T` fails `Pick`'s constraint, which reports it.
        for key in self.key_members(keys) {
            if matches!(key.without_aliases(), Type::String | Type::Number) {
                // `Pick<Record<string, V>, string>` keeps the index signature.
                index = Some(shared_index(&members).ok_or(None)?);
                continue;
            }
            let Some(name) = key_name(&key) else {
                return Err(None);
            };
            if !available.iter().any(|known| known == &name)
                && !members.iter().all(|(_, index)| index.is_some())
            {
                return Err(None);
            }
            fields.insert(name.clone(), merged_field(&members, &name));
        }
        Ok(Type::Object { fields, index })
    }

    /// The keys a key-type argument names: a union's members, with an enum
    /// spread into its members, as tsc spreads a string enum in `Pick`.
    fn key_members(&self, keys: &Type) -> Vec<Type> {
        members(keys)
            .into_iter()
            .flat_map(|key| match key.without_aliases() {
                enumeration @ (Type::NumberEnum { member: None, .. }
                | Type::StringEnum { member: None, .. }) => {
                    super::comparable::enum_member_types(enumeration, self.types)
                        .unwrap_or_else(|| vec![key.clone()])
                }
                _ => vec![key],
            })
            .collect()
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
        let omitted: Vec<Type> = self.key_members(keys);
        let omits_every = |base: Type| omitted.iter().any(|key| key.without_aliases() == &base);
        let (omits_every_string, omits_every_number) =
            (omits_every(Type::String), omits_every(Type::Number));
        let mut fields = BTreeMap::new();
        for name in common_keys(&members) {
            let numeric = name
                .parse::<f64>()
                .is_ok_and(|value| crate::runtime::number::format_number_js(value) == name);
            let dropped = omits_every_string
                || (omits_every_number && numeric)
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
        let mapped = self
            .key_members(operand)
            .into_iter()
            .map(|member| match member.without_aliases() {
                Type::StringLiteral(text) => Ok(Type::StringLiteral(map_text(mapping, text))),
                Type::StringEnum {
                    member: Some(member),
                    ..
                } => Ok(Type::StringLiteral(map_text(mapping, &member.value))),
                Type::TemplateLiteral {
                    parts,
                    substitutions,
                } => Ok(map_template(mapping, parts, substitutions)),
                Type::Error => Ok(Type::Error),
                _ => map_symbolic(mapping, &member).ok_or(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Type::union(mapped))
    }
}

/// `mapping` applied to `string` or to a symbolic mapping of it, which stays
/// symbolic: `Uppercase<string>`, and `Uppercase<Lowercase<string>>`, a
/// different set (`"ß"` lowercases to itself and uppercases to `"SS"`). A
/// mapping applied to itself is unchanged. `None` for any other operand.
fn map_symbolic(mapping: Utility, operand: &Type) -> Option<Type> {
    if matches!(operand.without_aliases(), Type::String) && symbolic_chain(operand).is_none() {
        return Some(labeled(mapping, vec![Type::String], Type::String));
    }
    let chain = symbolic_chain(operand)?;
    if chain.last() == Some(&mapping) {
        return Some(operand.clone());
    }
    Some(labeled(mapping, vec![operand.clone()], Type::String))
}

/// The mappings a symbolic string mapping applies to `string`, innermost
/// first: `[Lowercase, Uppercase]` for `Uppercase<Lowercase<string>>`. `None`
/// when `ty` is not one.
pub(crate) fn symbolic_chain(ty: &Type) -> Option<Vec<Utility>> {
    let Type::Alias { mangled, args, .. } = ty else {
        return None;
    };
    let mapping = utility(mangled).filter(|utility| utility.is_string_mapping())?;
    let [arg] = args.as_slice() else {
        return None;
    };
    let mut chain = match arg {
        Type::String => Vec::new(),
        inner => symbolic_chain(inner)?,
    };
    chain.push(mapping);
    Some(chain)
}

/// Whether `text` is in the set a chain of mappings over `string` describes:
/// applying the chain leaves it unchanged.
pub(crate) fn chain_preserves(chain: &[Utility], text: &str) -> bool {
    chain
        .iter()
        .fold(text.to_string(), |text, mapping| map_text(*mapping, &text))
        == text
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
            match map_symbolic(mapping, hole) {
                Some(mapped) if touched => mapped,
                _ => hole.clone(),
            }
        })
        .collect();
    Type::TemplateLiteral {
        parts,
        substitutions,
    }
}

/// An array or tuple under `Partial` or `Required`, element by element, or
/// unchanged under `Readonly` (whose wrapper the caller adds).
fn map_container(utility: Utility, container: &Type) -> Type {
    match container {
        Type::Array(element) => Type::Array(Box::new(map_element(utility, element))),
        Type::Tuple(tuple) => Type::Tuple(map_tuple(utility, tuple)),
        other => other.clone(),
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

/// `object[index]` for concrete operands, tsc's indexed access type. It
/// distributes over a union index, and over a union object for a property
/// every member has. A literal key reads the property (with `undefined` when
/// it is optional), `number` reads an array or tuple element, and `string` or
/// `number` reads an index signature's value. `Err` holds the diagnostic.
pub(crate) fn index_access_type(
    object: &Type,
    index: &Type,
    types: TypeResolver<'_>,
) -> Result<Type, String> {
    if matches!(object.peel(), Type::Error) || matches!(index.peel(), Type::Error) {
        return Ok(Type::Error);
    }
    let evaluator = Evaluator { types };
    let mut results = Vec::new();
    for key in members(index) {
        results.push(evaluator.index_one(object, &key, 0)?);
    }
    Ok(Type::union(results))
}

impl Evaluator<'_> {
    fn index_one(&self, object: &Type, key: &Type, depth: u32) -> Result<Type, String> {
        if depth > MAX_EXPANSIONS {
            return Err("the indexed type expands too deeply".into());
        }
        let next = depth + 1;
        let cannot = || format!("Type `{key}` cannot be used to index type `{object}`");
        match object {
            Type::Alias { ty, .. } | Type::Refined { ty, .. } => {
                return self.index_one(ty, key, depth);
            }
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                let body = super::type_aliases::alias_ref_body(mangled, name, args, self.types);
                return self.index_one(&body, key, next);
            }
            Type::Union(members) => {
                let parts = members
                    .iter()
                    .map(|member| self.index_one(member, key, next))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| match key_name(key) {
                        Some(name) => {
                            format!("Property `{name}` does not exist on type `{object}`")
                        }
                        None => cannot(),
                    })?;
                return Ok(Type::union(parts));
            }
            Type::Readonly(inner) => return self.index_one(inner, key, depth),
            _ => {}
        }
        // `"0"` names element `0`, as the number does.
        let element_key = match key.without_aliases() {
            Type::StringLiteral(name) if builtin_view(object).is_some() => name
                .parse::<f64>()
                .ok()
                .filter(|value| crate::runtime::number::format_number_js(*value) == *name)
                .map(|value| Type::NumberLiteral(crate::types::LiteralF64(value))),
            _ => None,
        };
        let key_ty = element_key.as_ref().unwrap_or(key.without_aliases());
        if let (Type::Tuple(tuple), Type::NumberLiteral(value)) = (object, key_ty) {
            let position = value.0 as usize;
            return match tuple.elements.get(position) {
                Some(element) if value.0.fract() == 0.0 && value.0 >= 0.0 => Ok(
                    ObjectField::widen_optional(position >= tuple.required_len(), element.clone()),
                ),
                _ => Err(format!(
                    "Tuple type `{object}` of length {} has no element at index {}",
                    tuple.elements.len(),
                    crate::runtime::number::format_number_js(value.0)
                )),
            };
        }
        // A tuple's length is each count its optional elements allow
        // (`[A, B?]["length"]` is `1 | 2`), as `t.length` reads it.
        if let (Type::Tuple(tuple), Type::StringLiteral(name)) = (object, key_ty)
            && name == "length"
        {
            let lengths = (tuple.required_len()..=tuple.elements.len())
                .map(|length| Type::NumberLiteral(crate::types::LiteralF64(length as f64)))
                .collect();
            return Ok(Type::union(lengths));
        }
        // A string, array or primitive reads an element by number and any
        // other property from its prelude interface, as tsc reads `T[K]`
        // through the apparent type.
        if let Some((element, view)) = builtin_view(object) {
            return match element {
                Some(element) if is_numeric_key(key_ty) => Ok(element),
                _ => self.index_one(&view, key, next),
            };
        }
        let (fields, index) = match object {
            // Interface members, generic methods aside, whose types an index
            // reads where a mapped type would have to carry them.
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } if !is_host_backed_interface(object) => {
                // A generic method has no function type here; it reads as
                // `unknown` rather than as a missing property.
                if let Some(member) = key_name(key)
                    && let Some(symbol) = self.types.lookup(mangled, name)
                    && let TypeKind::Interface { methods, .. } = &symbol.kind
                    && methods
                        .get(&member)
                        .is_some_and(|method| !method.generics.is_empty())
                {
                    return Ok(Type::Unknown);
                }
                (
                    self.types
                        .interface_full_form(mangled, name, args)
                        .ok_or_else(cannot)?,
                    self.types.index_signature(object),
                )
            }
            _ => self.structure(object).map_err(|_| cannot())?,
        };
        if let Some(name) = key_name(key) {
            if let Some(field) = fields.get(&name) {
                return Ok(field.read_ty());
            }
            if let Some(index) = &index
                && crate::property_keys::matches_name(&index.key, &name, self.types.limits)
            {
                return Ok(index.value.as_ref().clone());
            }
            return Err(format!(
                "Property `{name}` does not exist on type `{object}`"
            ));
        }
        if let Some(index) = &index
            && (matches!(key_ty, Type::String) && matches!(index.key.peel(), Type::String)
                || matches!(key_ty, Type::Number))
        {
            return Ok(index.value.as_ref().clone());
        }
        Err(cannot())
    }
}

/// The element type a built-in value reads by number (`None` for a value
/// with no elements) and the prelude interface it reads other properties
/// from: `Array<T>` for an array or tuple, `String` for a string.
fn builtin_view(ty: &Type) -> Option<(Option<Type>, Type)> {
    let interface = apparent_interface(ty)?;
    let element = match ty {
        Type::Array(element) => Some(element.as_ref().clone()),
        Type::Tuple(tuple) => Some(Type::union(tuple.elements.clone())),
        Type::String | Type::StringLiteral(_) | Type::TemplateLiteral { .. } => Some(Type::String),
        Type::StringEnum { .. } => Some(Type::String),
        Type::Uint8Array => Some(Type::Number),
        _ => None,
    };
    let args = match &element {
        Some(element) if interface == "Array" => vec![element.clone()],
        _ => Vec::new(),
    };
    Some((element, Type::prelude_interface(interface, args)))
}

fn is_numeric_key(key: &Type) -> bool {
    matches!(key, Type::Number | Type::NumberLiteral(_)) || matches!(key, Type::NumberEnum { .. })
}

/// Whether `ty` holds a utility application whose arguments no longer
/// mention a type parameter, which [`normalize`] would evaluate.
pub(crate) fn has_evaluable_application(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if let Type::AliasRef { mangled, args, .. } = ty
            && utility(mangled).is_some()
            && !args.iter().any(holds_type_parameter)
        {
            return true;
        }
        if is_concrete_keyof_or_index(ty) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}

/// Whether `ty` is a `keyof` or `T[K]` whose operands are concrete.
fn is_concrete_keyof_or_index(ty: &Type) -> bool {
    matches!(ty, Type::KeyOf(_) | Type::IndexedAccess { .. }) && !holds_type_parameter(ty)
}

/// The message for a form whose evaluation only names itself.
pub(crate) fn refers_to_itself(form: &Type) -> String {
    format!("`{form}` refers to itself")
}

/// `ty` itself evaluated when it is a `keyof` or `T[K]` whose operands are
/// concrete, or a template literal whose holes are (`on${Capitalize<"x">}`),
/// as relations meet them inside generic members instantiated at concrete
/// types. `None` for any other type.
pub(crate) fn evaluate_concrete_operator(ty: &Type, types: TypeResolver<'_>) -> Option<Type> {
    let operator = ty.without_aliases();
    let template =
        matches!(operator, Type::TemplateLiteral { .. }) && has_evaluable_application(operator);
    if !is_concrete_keyof_or_index(operator) && !template {
        return None;
    }
    Some(normalize(operator, types).unwrap_or(Type::Error))
}

/// `ty` with every utility application, `keyof` and `T[K]` whose operands
/// became concrete (after instantiation substituted its type parameters)
/// replaced by the type it stands for, labeled as a written application is.
/// One that has no evaluation becomes `Type::Error`; [`normalize_reporting`]
/// says why.
pub(crate) fn normalize(
    ty: &Type,
    types: TypeResolver<'_>,
) -> Result<Type, crate::type_size::TypeTooLarge> {
    normalize_reporting(ty, types, &mut Vec::new())
}

/// [`normalize`], adding to `failures` why each form that has no evaluation
/// has none, for the caller to report at its span.
pub(crate) fn normalize_reporting(
    ty: &Type,
    types: TypeResolver<'_>,
    failures: &mut Vec<String>,
) -> Result<Type, crate::type_size::TypeTooLarge> {
    if !has_evaluable_application(ty) {
        return Ok(ty.clone());
    }
    normalize_node(ty, types, failures)
}

fn normalize_node(
    ty: &Type,
    types: TypeResolver<'_>,
    failures: &mut Vec<String>,
) -> Result<Type, crate::type_size::TypeTooLarge> {
    let mapped =
        crate::type_size::map_children(ty, |child| normalize_node(child, types, failures))?;
    // A template whose holes evaluated (`on${Capitalize<"click">}`) folds to
    // the string it spells.
    let mapped = match mapped {
        template @ Type::TemplateLiteral { .. } => {
            crate::property_keys::normalize_template(template, &mut types.limits.budget())?
        }
        other => other,
    };
    Ok(evaluate_guarded(mapped, types, failures))
}

thread_local! {
    /// The concrete forms being evaluated on this thread. A lazy form over a
    /// later declaration can name another (`A["b"]` names `B["c"]`); one met
    /// again while it evaluates only names itself, which is a cycle.
    static EVALUATING: std::cell::RefCell<Vec<Type>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Whether `ty` is a `keyof`, `T[K]` or utility application whose operands
/// are concrete, which evaluates to another type.
fn is_concrete_operator(ty: &Type) -> bool {
    is_concrete_keyof_or_index(ty) || (ty.is_utility_application() && !holds_type_parameter(ty))
}

thread_local! {
    /// Evaluations of concrete forms, kept while every declaration is bound
    /// and results cannot change ([`SettledForms`]). A chain of forms over
    /// later declarations (`I0["a"]` is `I1["a"]`, ...) then evaluates each
    /// link once rather than once per form that reaches it.
    static SETTLED: std::cell::RefCell<Option<std::collections::BTreeMap<Type, Result<Type, String>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Keeps the evaluations of concrete forms while it lives.
pub(crate) struct SettledForms;

impl SettledForms {
    pub(crate) fn enter() -> Self {
        SETTLED.with(|settled| *settled.borrow_mut() = Some(std::collections::BTreeMap::new()));
        SettledForms
    }
}

impl Drop for SettledForms {
    fn drop(&mut self) {
        SETTLED.with(|settled| *settled.borrow_mut() = None);
    }
}

/// Keeps `form` on [`EVALUATING`] while it lives, so the entry goes even
/// when evaluation unwinds.
struct Evaluating;

impl Evaluating {
    fn enter(form: &Type) -> Self {
        EVALUATING.with(|stack| stack.borrow_mut().push(form.clone()));
        Evaluating
    }
}

impl Drop for Evaluating {
    fn drop(&mut self) {
        EVALUATING.with(|stack| stack.borrow_mut().pop());
    }
}

/// `ty` evaluated while it is a concrete form, through as many forms as it
/// names, guarded against cycles. Only the outer type is evaluated: the
/// fields of a result keep their lazy forms, so mutually referring
/// declarations (`A` holding `Partial<B>`, `B` holding `Partial<A>`) stay finite.
fn evaluate_guarded(ty: Type, types: TypeResolver<'_>, failures: &mut Vec<String>) -> Type {
    if !is_concrete_operator(&ty) {
        return ty;
    }
    if EVALUATING.with(|stack| stack.borrow().contains(&ty)) {
        failures.push(refers_to_itself(&ty));
        return Type::Error;
    }
    let settled = SETTLED.with(|settled| {
        settled
            .borrow()
            .as_ref()
            .and_then(|settled| settled.get(&ty).cloned())
    });
    let evaluated = settled.unwrap_or_else(|| {
        let evaluated = evaluate_unsettled(&ty, types);
        SETTLED.with(|settled| {
            if let Some(settled) = settled.borrow_mut().as_mut() {
                settled.insert(ty.clone(), evaluated.clone());
            }
        });
        evaluated
    });
    evaluated.unwrap_or_else(|reason| {
        failures.push(reason);
        Type::Error
    })
}

/// [`evaluate_guarded`] for a form not evaluated before: its type, or why it
/// has none.
fn evaluate_unsettled(ty: &Type, types: TypeResolver<'_>) -> Result<Type, String> {
    let _evaluating = Evaluating::enter(ty);
    match evaluate_form(ty, types)? {
        result if result == *ty => Err(refers_to_itself(ty)),
        result => {
            let mut failures = Vec::new();
            let evaluated = evaluate_guarded(result, types, &mut failures);
            match failures.into_iter().next() {
                Some(reason) => Err(reason),
                None => Ok(evaluated),
            }
        }
    }
}

/// One evaluation step of `ty` when it is a lazy `keyof` or `T[K]` with
/// concrete operands; `None` for any other type.
pub(crate) fn evaluate_step(ty: &Type, types: TypeResolver<'_>) -> Option<Result<Type, String>> {
    let form = ty.without_aliases();
    if holds_type_parameter(form) {
        return None;
    }
    match form {
        Type::KeyOf(operand) => Some(keyof_step(operand, types)),
        Type::IndexedAccess { object, index } => Some(index_access_type(object, index, types)),
        _ => None,
    }
}

fn keyof_step(operand: &Type, types: TypeResolver<'_>) -> Result<Type, String> {
    keyof_type(operand, types)
        .ok_or_else(|| format!("`keyof` needs an object type or interface, got `{operand}`"))
}

/// One evaluation step of a concrete form.
fn evaluate_form(form: &Type, types: TypeResolver<'_>) -> Result<Type, String> {
    match form {
        Type::KeyOf(operand) => keyof_step(operand, types),
        Type::IndexedAccess { object, index } => {
            let mut failures = Vec::new();
            let object = evaluate_guarded(object.as_ref().clone(), types, &mut failures);
            let index = evaluate_guarded(index.as_ref().clone(), types, &mut failures);
            if let Some(failure) = failures.pop() {
                return Err(failure);
            }
            index_access_type(&object, &index, types)
        }
        Type::AliasRef { mangled, args, .. } => match utility(mangled) {
            Some(utility) => match evaluate(utility, args, types) {
                Evaluated::Resolved(result) => Ok(label(utility, args.clone(), result)),
                Evaluated::Deferred => Ok(form.clone()),
                Evaluated::Invalid(reason) => Err(reason
                    .unwrap_or_else(|| format!("`{form}` has no evaluation for these arguments"))),
            },
            None => Ok(form.clone()),
        },
        _ => Ok(form.clone()),
    }
}

/// `keyof ty` for a concrete `ty`, tsc's index type: the union of its
/// property names as string literals, with an index signature's key type.
/// `None` for a type with no member list (a primitive, an array).
pub(crate) fn keyof_type(ty: &Type, types: TypeResolver<'_>) -> Option<Type> {
    match ty.peel() {
        // No value of these has a key, so `k: keyof T` takes none.
        Type::Unknown | Type::Null | Type::Undefined | Type::Void => return Some(Type::Never),
        // Every key, as tsc's `string | number | symbol` without `symbol`.
        Type::Never => return Some(Type::union(vec![Type::String, Type::Number])),
        Type::Error => return Some(Type::Error),
        _ => {}
    }
    if let Some(index) = types.index_signature(ty) {
        if matches!(index.key.peel(), Type::String) {
            return Some(if index.string_keyof_includes_number {
                Type::union(vec![Type::String, Type::Number])
            } else {
                Type::String
            });
        }
        let mut keys: Vec<Type> = member_names(ty, types, 0)
            .unwrap_or_default()
            .into_iter()
            .flat_map(key_literals)
            .collect();
        keys.push(*index.key);
        return Some(Type::union(keys));
    }
    // An array, tuple, string or byte array is indexed by number besides its
    // members' names, and so is a union of them (`keyof (string |
    // number[])`), whose keys are those every member has.
    let has_elements = |ty: &Type| builtin_view(ty).is_some_and(|(element, _)| element.is_some());
    let numeric = match ty.peel() {
        Type::Union(members) => members.iter().all(|member| has_elements(member.peel())),
        other => has_elements(other),
    };
    let mut keys: Vec<Type> = member_names(ty, types, 0)?
        .into_iter()
        .flat_map(key_literals)
        .collect();
    // A tuple's positions are keys too: `keyof [A, B]` holds `"0"` and `"1"`.
    if let Type::Tuple(tuple) = ty.peel() {
        keys.extend(
            (0..tuple.elements.len()).flat_map(|position| key_literals(position.to_string())),
        );
    }
    if numeric {
        keys.push(Type::Number);
    }
    Some(Type::union(keys))
}

/// The literal types a property name stands for in `keyof`. The field map
/// keeps names only (SUB-2111), so a numeric name (`0`, or `"0"` quoted) is
/// both the number literal tsc gives for `{ 0: … }` and the string literal it
/// gives for `{ "0": … }`.
fn key_literals(name: String) -> Vec<Type> {
    match name.parse::<f64>() {
        Ok(value) if crate::runtime::number::format_number_js(value) == name => vec![
            Type::NumberLiteral(crate::types::LiteralF64(value)),
            Type::StringLiteral(name),
        ],
        _ => vec![Type::StringLiteral(name)],
    }
}

/// The prelude interface a primitive, array or byte array reads its members
/// from, as tsc reads `keyof string` from `String`.
pub(crate) fn apparent_interface(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::String
        | Type::StringLiteral(_)
        | Type::TemplateLiteral { .. }
        | Type::StringEnum { .. } => "String",
        Type::Number | Type::NumberLiteral(_) | Type::NumberEnum { .. } => "Number",
        Type::Boolean | Type::BooleanLiteral(_) => "Boolean",
        Type::BigInt | Type::BigIntLiteral(_) => "BigInt",
        Type::Array(_) | Type::Tuple(_) => "Array",
        Type::Uint8Array => "Uint8Array",
        _ => return None,
    })
}

/// The property names `keyof` reports: an object type's fields, an
/// interface's properties and methods, a class instance's public members,
/// and the names every member of a union has.
fn member_names(ty: &Type, types: TypeResolver<'_>, depth: u32) -> Option<Vec<String>> {
    if depth > MAX_EXPANSIONS {
        return None;
    }
    let next = depth + 1;
    if let Some(interface) = apparent_interface(ty.peel()) {
        let mangled = crate::mangle::prelude(interface);
        let TypeKind::Interface {
            methods,
            properties,
            ..
        } = &types.lookup(&mangled, interface)?.kind
        else {
            return None;
        };
        return Some(properties.keys().chain(methods.keys()).cloned().collect());
    }
    match ty.peel() {
        Type::Function { .. } => Some(Vec::new()),
        Type::Object { fields, .. } => Some(fields.keys().cloned().collect()),
        Type::Union(members) => {
            let mut names: Option<Vec<String>> = None;
            for member in members {
                let member_names = member_names(member, types, next)?;
                names = Some(match names {
                    None => member_names,
                    Some(names) => names
                        .into_iter()
                        .filter(|name| member_names.contains(name))
                        .collect(),
                });
            }
            names
        }
        Type::InterfaceRef { mangled, name, .. } => {
            let TypeKind::Interface {
                methods,
                properties,
                ..
            } = &types.lookup(mangled, name)?.kind
            else {
                return None;
            };
            Some(properties.keys().chain(methods.keys()).cloned().collect())
        }
        Type::ClassRef { mangled, args, .. } => {
            Some(types.class_full_form(mangled, args)?.into_keys().collect())
        }
        Type::AliasRef {
            mangled,
            name,
            args,
            ..
        } => member_names(
            &super::type_aliases::alias_ref_body(mangled, name, args, types),
            types,
            next,
        ),
        _ => None,
    }
}

/// Whether `utility` can be evaluated over `args` now: a mapped utility or
/// `ReturnType`/`Parameters` needs only its operand's outer shape (tsc
/// evaluates `ReturnType<() => T>` to `T`), `Pick` and `Omit` also concrete
/// keys, and the conditional and intrinsic utilities concrete arguments.
fn structure_known(utility: Utility, args: &[Type]) -> bool {
    match (utility, args) {
        (
            Utility::Partial
            | Utility::Required
            | Utility::Readonly
            | Utility::ReturnType
            | Utility::Parameters,
            [operand],
        ) => known_shape(operand),
        (Utility::Pick | Utility::Omit, [object, keys]) => {
            known_shape(object) && !holds_type_parameter(keys)
        }
        _ => !args.iter().any(holds_type_parameter),
    }
}

/// Whether `ty`'s outer shape is known: it is no type parameter, form over
/// one or narrowing of one, and no union with such a member.
fn known_shape(ty: &Type) -> bool {
    match ty.without_aliases() {
        Type::Union(members) => members.iter().all(known_shape),
        Type::Refined { .. } => false,
        other => !other.lowers_to_erased_slot(),
    }
}

/// The utility and arguments of an application that still mentions a type
/// parameter.
pub(crate) fn deferred_application(ty: &Type) -> Option<(Utility, &[Type])> {
    let Type::AliasRef { mangled, args, .. } = ty else {
        return None;
    };
    let utility = utility(mangled)?;
    args.iter()
        .any(holds_type_parameter)
        .then_some((utility, args.as_slice()))
}
