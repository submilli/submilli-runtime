//! Named type declarations, resolved by mangled name.
//!
//! A type in the typed AST refers to an enum, interface or alias by name. The
//! rules that need what the name declares read it here: from the program being
//! checked and from the packages it depends on.

use std::collections::BTreeMap;

use crate::type_size::TypeLimits;
use crate::typechecker::infer::narrowing::LiteralValue;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::types::LiteralF64;
use crate::{
    IndexSignature, MangledName, ObjectField, PackageDeclaration, Type, TypeKind, TypeSymbol,
    TypedAst, TypedInterfaceMember, TypedTypeDecl,
};

/// A recursive alias reaches itself again, so expanding one has to be bounded.
const MAX_ALIAS_EXPANSIONS: u32 = 32;

pub(super) struct TypeDeclarations<'a> {
    /// A script's own declarations. A package's own are the first of `packages`.
    script: Option<&'a TypedAst>,
    packages: Vec<&'a PackageDeclaration>,
}

/// What reading a member off a value of some type yields.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Member {
    Found(Type),
    Missing,
    /// Whether the type has the member cannot be told from the declarations
    /// known here.
    Unresolved,
}

impl<'a> TypeDeclarations<'a> {
    pub(super) fn for_script(ta: &'a TypedAst, dependencies: &[&'a PackageDeclaration]) -> Self {
        Self {
            script: Some(ta),
            packages: dependencies.to_vec(),
        }
    }

    /// A package's typed AST holds the declarations of all its modules without
    /// their mangled names, so two modules' same-named types are told apart
    /// only in `package`.
    pub(super) fn for_package(
        package: &'a PackageDeclaration,
        dependencies: &[&'a PackageDeclaration],
    ) -> Self {
        let mut packages = Vec::with_capacity(dependencies.len().saturating_add(1));
        packages.push(package);
        packages.extend_from_slice(dependencies);
        Self {
            script: None,
            packages,
        }
    }

    /// The value of every variant of an enum, or `None` when its declaration
    /// is not known.
    pub(super) fn enum_values(
        &self,
        mangled: &MangledName,
        name: &str,
    ) -> Option<Vec<LiteralValue>> {
        self.find(mangled, name)?.enum_values()
    }

    /// The declared type of `member` on a value of type `ty`. A nullable value
    /// is read as its non-null type, and a union has the member only when
    /// every alternative does.
    pub(super) fn member(&self, ty: &Type, member: &str) -> Member {
        self.member_within(ty, member, MAX_ALIAS_EXPANSIONS)
    }

    /// All statically declared payload properties, including optional properties.
    /// A union can report a property from any alternative at runtime.
    pub(super) fn property_names(&self, ty: &Type) -> Option<Vec<String>> {
        self.property_names_within(ty, MAX_ALIAS_EXPANSIONS)
    }

    fn property_names_within(&self, ty: &Type, remaining: u32) -> Option<Vec<String>> {
        let remaining = remaining.checked_sub(1)?;
        match ty.peel() {
            Type::Object { fields, .. } => Some(fields.keys().cloned().collect()),
            Type::Union(alternatives) => {
                let mut names = std::collections::BTreeSet::new();
                for alternative in alternatives {
                    if matches!(alternative.peel(), Type::Null) {
                        continue;
                    }
                    names.extend(self.property_names_within(alternative, remaining)?);
                }
                Some(names.into_iter().collect())
            }
            Type::InterfaceRef { mangled, name, .. } => match self.find(mangled, name)? {
                Declared::Typed(TypedTypeDecl::Interface(declaration)) => {
                    Some(declaration.property_names.iter().cloned().collect())
                }
                Declared::Symbol(TypeSymbol {
                    kind: TypeKind::Interface { properties, .. },
                    ..
                }) => Some(properties.keys().cloned().collect()),
                _ => None,
            },
            Type::ClassRef { mangled, .. } => self.class_property_names(mangled, remaining),
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                let (generics, body) = self.find(mangled, name)?.alias()?;
                let body = TypeParamSubstitution::from_pairs(generics, args)
                    .apply(body, &TypeLimits::default())
                    .ok()?;
                self.property_names_within(&body, remaining)
            }
            _ => None,
        }
    }

    fn class_property_names(&self, mangled: &MangledName, remaining: u32) -> Option<Vec<String>> {
        let remaining = remaining.checked_sub(1)?;
        let declared = self
            .script
            .and_then(|ta| {
                ta.types.iter().find(|declared| {
            matches!(declared, TypedTypeDecl::Class(class) if &class.mangled_name == mangled)
        })
            })
            .map(Declared::Typed)
            .or_else(|| {
                self.packages
                    .iter()
                    .find_map(|package| package.type_symbol(mangled))
                    .map(Declared::Symbol)
            })?;
        let (parent, fields): (_, Vec<_>) = match declared {
            Declared::Typed(TypedTypeDecl::Class(class)) => {
                // An authored serializer can report unrelated keys; its result has no
                // statically declared object shape to compare with the bindings.
                if class
                    .methods
                    .iter()
                    .any(|method| method.name.name == "toJson")
                {
                    return None;
                }
                (
                    class.extends.as_ref(),
                    class
                        .fields
                        .iter()
                        .map(|field| (&field.name.name, field.visibility))
                        .chain(
                            class
                                .accessors
                                .iter()
                                .map(|accessor| (&accessor.name().name, accessor.visibility())),
                        )
                        .collect(),
                )
            }
            Declared::Symbol(TypeSymbol {
                kind:
                    TypeKind::Class {
                        fields,
                        methods,
                        extends,
                        ..
                    },
                ..
            }) => {
                if methods.contains_key("toJson") {
                    return None;
                }
                (
                    extends.as_ref().map(|base| &base.parent),
                    fields
                        .iter()
                        .map(|(name, field)| (name, field.visibility))
                        .collect(),
                )
            }
            _ => return None,
        };
        let mut names: std::collections::BTreeSet<_> = match parent {
            Some(parent) => self
                .class_property_names(parent, remaining)?
                .into_iter()
                .collect(),
            None => std::collections::BTreeSet::new(),
        };
        for (name, visibility) in fields {
            if visibility == crate::Visibility::Private {
                names.remove(name);
            } else {
                names.insert(name.clone());
            }
        }
        Some(names.into_iter().collect())
    }

    fn member_within(&self, ty: &Type, member: &str, expansions_left: u32) -> Member {
        match ty.peel() {
            Type::Object { fields, index } => object_member(fields, index.as_ref(), member),
            Type::Union(alternatives) => self.union_member(alternatives, member, expansions_left),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => self.interface_member(mangled, name, args, member),
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => self.alias_member(mangled, name, args, member, expansions_left),
            // A diagnostic already reports the broken type, and a class's
            // fields are not read here: neither is judged.
            Type::Error | Type::ClassRef { .. } => Member::Unresolved,
            Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Void
            | Type::Unknown
            | Type::Function { .. }
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Never
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            // `peel` has removed these.
            | Type::Readonly(_)
            | Type::Refined { .. }
            | Type::Alias { .. } => Member::Missing,
        }
    }

    fn union_member(&self, alternatives: &[Type], member: &str, expansions_left: u32) -> Member {
        let mut found = Vec::new();
        let mut unresolved = false;
        for alternative in alternatives {
            if matches!(alternative.peel(), Type::Null) {
                continue;
            }
            match self.member_within(alternative, member, expansions_left) {
                Member::Found(ty) => found.push(ty),
                Member::Missing => return Member::Missing,
                Member::Unresolved => unresolved = true,
            }
        }
        if unresolved {
            return Member::Unresolved;
        }
        if found.is_empty() {
            return Member::Missing;
        }
        Member::Found(Type::union(found))
    }

    fn interface_member(
        &self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
        member: &str,
    ) -> Member {
        let Some((generics, declared)) = self
            .find(mangled, name)
            .and_then(|declared| declared.interface_member(member))
        else {
            return Member::Unresolved;
        };
        // A substitution too large to build cannot be followed, like a type
        // that does not resolve.
        declared.map_or(Member::Missing, |ty| {
            TypeParamSubstitution::from_pairs(generics, args)
                .apply(ty, &TypeLimits::default())
                .map_or(Member::Unresolved, Member::Found)
        })
    }

    fn alias_member(
        &self,
        mangled: &MangledName,
        name: &str,
        args: &[Type],
        member: &str,
        expansions_left: u32,
    ) -> Member {
        let Some(expansions_left) = expansions_left.checked_sub(1) else {
            return Member::Unresolved;
        };
        let Some((generics, body)) = self.find(mangled, name).and_then(Declared::alias) else {
            return Member::Unresolved;
        };
        let Ok(body) =
            TypeParamSubstitution::from_pairs(generics, args).apply(body, &TypeLimits::default())
        else {
            return Member::Unresolved;
        };
        self.member_within(&body, member, expansions_left)
    }

    fn find(&self, mangled: &MangledName, name: &str) -> Option<Declared<'a>> {
        self.script
            .and_then(|ta| script_declaration(ta, mangled, name))
            .map(Declared::Typed)
            .or_else(|| {
                self.packages
                    .iter()
                    .find_map(|package| package.type_symbol(mangled))
                    .map(Declared::Symbol)
            })
    }
}

/// A declaration in the form its owner keeps it.
#[derive(Clone, Copy)]
enum Declared<'a> {
    Typed(&'a TypedTypeDecl),
    Symbol(&'a TypeSymbol),
}

impl<'a> Declared<'a> {
    fn enum_values(self) -> Option<Vec<LiteralValue>> {
        match self {
            Self::Typed(TypedTypeDecl::NumberEnum(declaration)) => Some(
                declaration
                    .members
                    .iter()
                    .map(|member| number_value(member.value))
                    .collect(),
            ),
            Self::Typed(TypedTypeDecl::StringEnum(declaration)) => Some(
                declaration
                    .members
                    .iter()
                    .map(|member| LiteralValue::String(member.value.clone()))
                    .collect(),
            ),
            Self::Typed(
                TypedTypeDecl::Interface(_) | TypedTypeDecl::Class(_) | TypedTypeDecl::Alias(_),
            ) => None,
            Self::Symbol(symbol) => match &symbol.kind {
                TypeKind::NumberEnum { variants, .. } => Some(
                    variants
                        .iter()
                        .map(|(_, value)| number_value(*value))
                        .collect(),
                ),
                TypeKind::StringEnum { variants, .. } => Some(
                    variants
                        .iter()
                        .map(|(_, value)| LiteralValue::String(value.clone()))
                        .collect(),
                ),
                TypeKind::Interface { .. } | TypeKind::Class { .. } | TypeKind::Alias { .. } => {
                    None
                }
            },
        }
    }

    /// An interface's type parameters with the declared type of `member`, the
    /// latter `None` when the interface has no such property. `None` overall
    /// when that cannot be told.
    fn interface_member(self, member: &str) -> Option<(&'a [String], Option<&'a Type>)> {
        match self {
            // The typed AST lists the members an interface declares itself.
            // One it does not list may be inherited, so its absence is not
            // known.
            Self::Typed(TypedTypeDecl::Interface(declaration)) => {
                listed_property(&declaration.members, member)
                    .or_else(|| index_value(declaration.index.as_ref()))
                    .map(|ty| (declaration.generics.as_slice(), Some(ty)))
            }
            Self::Typed(
                TypedTypeDecl::Class(_)
                | TypedTypeDecl::NumberEnum(_)
                | TypedTypeDecl::StringEnum(_)
                | TypedTypeDecl::Alias(_),
            ) => None,
            Self::Symbol(symbol) => match &symbol.kind {
                TypeKind::Interface {
                    generics,
                    properties,
                    index,
                    ..
                } => Some((
                    generics.as_slice(),
                    properties
                        .get(member)
                        .map(|property| &property.ty)
                        .or_else(|| index_value(index.as_ref())),
                )),
                TypeKind::Class { .. }
                | TypeKind::NumberEnum { .. }
                | TypeKind::StringEnum { .. }
                | TypeKind::Alias { .. } => None,
            },
        }
    }

    /// An alias's type parameters and body.
    fn alias(self) -> Option<(&'a [String], &'a Type)> {
        match self {
            Self::Typed(TypedTypeDecl::Alias(declaration)) => {
                Some((declaration.generics.as_slice(), &declaration.ty))
            }
            Self::Typed(
                TypedTypeDecl::Interface(_)
                | TypedTypeDecl::Class(_)
                | TypedTypeDecl::NumberEnum(_)
                | TypedTypeDecl::StringEnum(_),
            ) => None,
            Self::Symbol(symbol) => match &symbol.kind {
                TypeKind::Alias { generics, ty, .. } => Some((generics.as_slice(), ty)),
                TypeKind::Interface { .. }
                | TypeKind::Class { .. }
                | TypeKind::NumberEnum { .. }
                | TypeKind::StringEnum { .. } => None,
            },
        }
    }
}

/// A script is a single module, so the name alone mangles its declarations.
fn script_declaration<'a>(
    ta: &'a TypedAst,
    mangled: &MangledName,
    name: &str,
) -> Option<&'a TypedTypeDecl> {
    if *mangled != crate::mangle::package_symbol(&ta.package_name, name) {
        return None;
    }
    ta.types
        .iter()
        .find(|declaration| declared_name(declaration) == name)
}

fn declared_name(declaration: &TypedTypeDecl) -> &str {
    match declaration {
        TypedTypeDecl::Interface(declaration) => &declaration.name.name,
        TypedTypeDecl::Class(declaration) => &declaration.name.name,
        TypedTypeDecl::NumberEnum(declaration) => &declaration.name.name,
        TypedTypeDecl::StringEnum(declaration) => &declaration.name.name,
        TypedTypeDecl::Alias(declaration) => &declaration.name.name,
    }
}

/// A field a type declares, or what its index signature holds under any name.
fn object_member(
    fields: &BTreeMap<String, ObjectField>,
    index: Option<&IndexSignature>,
    member: &str,
) -> Member {
    fields
        .get(member)
        .map(|field| &field.ty)
        .or_else(|| index_value(index))
        .map_or(Member::Missing, |ty| Member::Found(ty.clone()))
}

fn listed_property<'a>(members: &'a [TypedInterfaceMember], member: &str) -> Option<&'a Type> {
    members.iter().find_map(|candidate| match candidate {
        TypedInterfaceMember::Property { name, ty, .. } if name.name == member => Some(ty),
        TypedInterfaceMember::Property { .. } | TypedInterfaceMember::Method { .. } => None,
    })
}

fn index_value(index: Option<&IndexSignature>) -> Option<&Type> {
    index.map(|index| &*index.value)
}

fn number_value(value: f64) -> LiteralValue {
    LiteralValue::Number(LiteralF64(value))
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{infer_script, run_package};
    use super::{Member, TypeDeclarations};
    use crate::typechecker::infer::narrowing::LiteralValue;
    use crate::types::LiteralF64;
    use crate::{PackageDeclaration, Type, TypedAst};

    fn typed(source: &str) -> TypedAst {
        let (ta, diags) = infer_script(source);
        assert!(diags.is_empty(), "{diags:?}");
        ta
    }

    fn package(name: &str, modules: &[(&str, &str)]) -> (TypedAst, PackageDeclaration) {
        let (ta, declaration, diags) = run_package(name, modules, &[]);
        assert!(diags.is_empty(), "{diags:?}");
        (ta, declaration)
    }

    fn param_type(ta: &TypedAst, function: &str) -> Type {
        ta.functions
            .iter()
            .find(|f| f.name.name == function)
            .and_then(|f| f.params.first())
            .map(|param| param.ty.clone())
            .expect("function with a parameter")
    }

    fn numbers(values: &[f64]) -> Option<Vec<LiteralValue>> {
        Some(
            values
                .iter()
                .map(|value| LiteralValue::Number(LiteralF64(*value)))
                .collect(),
        )
    }

    fn enum_values(declarations: &TypeDeclarations<'_>, ty: &Type) -> Option<Vec<LiteralValue>> {
        let (Type::NumberEnum { mangled, name, .. } | Type::StringEnum { mangled, name, .. }) =
            ty.peel()
        else {
            panic!("not an enum: {ty:?}");
        };
        declarations.enum_values(mangled, name)
    }

    #[test]
    fn a_script_enum_resolves_by_its_mangled_name() {
        let ta = typed(
            "enum Color { Red, Green = 5 }\n\
             enum Shade { Dark = \"dark\" }\n\
             function f(color: Color): void { }\n\
             function g(shade: Shade): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "f")),
            numbers(&[0.0, 5.0])
        );
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "g")),
            Some(vec![LiteralValue::String("dark".to_string())])
        );
    }

    #[test]
    fn a_same_named_enum_of_another_package_is_not_the_script_enum() {
        let ta = typed("enum Color { Red }\nfunction main(): void { }\n");
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        assert_eq!(
            declarations.enum_values(&crate::mangle::package_symbol("@acme/ui", "Color"), "Color"),
            None
        );
    }

    #[test]
    fn same_named_enums_of_two_modules_are_told_apart() {
        let (ta, declaration) = package(
            "@test/package",
            &[
                (
                    "lib",
                    "import { Level as Other } from \"./other\";\n\
                     enum Level { Low, High }\n\
                     function own(level: Level): void { }\n\
                     function imported(level: Other): void { }\n",
                ),
                ("other", "export enum Level { Only = 7 }\n"),
            ],
        );
        let declarations = TypeDeclarations::for_package(&declaration, &[]);
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "own")),
            numbers(&[0.0, 1.0])
        );
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "imported")),
            numbers(&[7.0])
        );
    }

    #[test]
    fn an_enum_of_a_dependency_resolves_whichever_module_declares_it() {
        let (_, dependency) = package(
            "@test/levels",
            &[
                (
                    "lib",
                    "export { Inner } from \"./inner\";\n\
                     /** Declared at the root. */\n\
                     export enum Outer { A = \"a\" }\n",
                ),
                (
                    "inner",
                    "/** Declared inside. */\nexport enum Inner { One = 1, Two }\n",
                ),
            ],
        );
        let (ta, declaration, diags) = run_package(
            "@test/package",
            &[(
                "lib",
                "import { Inner, Outer } from \"@test/levels\";\n\
                 function inner(level: Inner): void { }\n\
                 function outer(level: Outer): void { }\n",
            )],
            std::slice::from_ref(&dependency),
        );
        assert!(diags.is_empty(), "{diags:?}");
        let declarations = TypeDeclarations::for_package(&declaration, &[&dependency]);
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "inner")),
            numbers(&[1.0, 2.0])
        );
        assert_eq!(
            enum_values(&declarations, &param_type(&ta, "outer")),
            Some(vec![LiteralValue::String("a".to_string())])
        );
        let without = TypeDeclarations::for_package(&declaration, &[]);
        assert_eq!(enum_values(&without, &param_type(&ta, "inner")), None);
    }

    #[test]
    fn a_member_resolves_through_an_interface_an_alias_and_null() {
        let ta = typed(
            "interface Input { teamId: string; nested: Inner | null; label?: string }\n\
             interface Inner { id: number }\n\
             type Aliased = Input;\n\
             function f(input: Aliased | null): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let input = param_type(&ta, "f");
        assert_eq!(
            declarations.member(&input, "teamId"),
            Member::Found(Type::String)
        );
        assert_eq!(
            declarations.member(&input, "label"),
            Member::Found(Type::String)
        );
        let Member::Found(nested) = declarations.member(&input, "nested") else {
            panic!("nested resolves");
        };
        assert_eq!(
            declarations.member(&nested, "id"),
            Member::Found(Type::Number)
        );
    }

    #[test]
    fn a_script_interface_is_not_judged_on_a_member_it_does_not_list() {
        let ta = typed(
            "interface Base { teamId: string }\n\
             interface Input extends Base { title: string }\n\
             function f(input: Input): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let input = param_type(&ta, "f");
        assert_eq!(
            declarations.member(&input, "title"),
            Member::Found(Type::String)
        );
        assert_eq!(declarations.member(&input, "teamId"), Member::Unresolved);
        assert_eq!(declarations.member(&input, "absent"), Member::Unresolved);
    }

    #[test]
    fn a_package_interface_lists_what_it_inherits() {
        let (ta, declaration) = package(
            "@test/package",
            &[(
                "lib",
                "interface Base { teamId: string }\n\
                 interface Input extends Base { title: string; send(): void }\n\
                 function f(input: Input): void { }\n",
            )],
        );
        let declarations = TypeDeclarations::for_package(&declaration, &[]);
        let input = param_type(&ta, "f");
        assert_eq!(
            declarations.member(&input, "teamId"),
            Member::Found(Type::String)
        );
        assert_eq!(
            declarations.member(&input, "title"),
            Member::Found(Type::String)
        );
        assert_eq!(declarations.member(&input, "absent"), Member::Missing);
        assert_eq!(declarations.member(&input, "send"), Member::Missing);
    }

    #[test]
    fn a_generic_interface_member_takes_the_type_argument() {
        let ta = typed(
            "interface Wrapper<T> { data: T }\n\
             interface Payload { id: string }\n\
             function f(input: Wrapper<Payload>): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let Member::Found(data) = declarations.member(&param_type(&ta, "f"), "data") else {
            panic!("data resolves");
        };
        assert_eq!(
            declarations.member(&data, "id"),
            Member::Found(Type::String)
        );
    }

    #[test]
    fn a_recursive_alias_resolves_one_step_at_a_time() {
        let ta = typed(
            "type Node = { value: number; next: Node | null };\n\
             function f(input: Node): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let Member::Found(next) = declarations.member(&param_type(&ta, "f"), "next") else {
            panic!("next resolves");
        };
        assert_eq!(
            declarations.member(&next, "value"),
            Member::Found(Type::Number)
        );
        assert_eq!(declarations.member(&next, "absent"), Member::Missing);
    }

    #[test]
    fn a_union_has_a_member_only_when_every_alternative_does() {
        let ta = typed(
            "function f(input: { id: string; a: number } | { id: number }): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let input = param_type(&ta, "f");
        assert_eq!(
            declarations.member(&input, "id"),
            Member::Found(Type::union(vec![Type::String, Type::Number]))
        );
        assert_eq!(declarations.member(&input, "a"), Member::Missing);
    }

    #[test]
    fn an_index_signature_answers_for_any_name() {
        let ta = typed(
            "function f(input: { [key: string]: number }): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        assert_eq!(
            declarations.member(&param_type(&ta, "f"), "anything"),
            Member::Found(Type::Number)
        );
    }

    #[test]
    fn an_unknown_declaration_is_not_judged() {
        let ta = typed(
            "class Holder { id: string = \"\"; }\n\
             function f(holder: Holder): void { }\n\
             function main(): void { }\n",
        );
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        let imported = Type::interface_ref(
            crate::Package("@acme/sdk".to_string()),
            "Input",
            crate::mangle::package_symbol("@acme/sdk", "Input"),
            Vec::new(),
        );
        assert_eq!(declarations.member(&imported, "teamId"), Member::Unresolved);
        assert_eq!(
            declarations.member(&Type::Error, "teamId"),
            Member::Unresolved
        );
        assert_eq!(
            declarations.member(&param_type(&ta, "f"), "id"),
            Member::Unresolved
        );
    }

    #[test]
    fn a_type_without_fields_has_no_member() {
        let ta = typed("function main(): void { }\n");
        let declarations = TypeDeclarations::for_script(&ta, &[]);
        for ty in [
            Type::Unknown,
            Type::String,
            Type::Number,
            Type::Null,
            Type::Array(Box::new(Type::String)),
            Type::TypeVar("T".to_string()),
        ] {
            assert_eq!(declarations.member(&ty, "host"), Member::Missing, "{ty:?}");
        }
    }

    #[test]
    fn a_dependency_declares_what_the_script_does_not() {
        let ta = typed("function main(): void { }\n");
        let stdlib = crate::stdlib::stdlib_package_declarations();
        let (prelude, host, _) = crate::runtime::prelude::cached_runtime_package_declarations();
        let dependencies: Vec<_> = prelude.iter().chain(host).chain(&stdlib).collect();
        let declarations = TypeDeclarations::for_script(&ta, &dependencies);
        let map = Type::prelude_interface("Map", vec![Type::String, Type::Number]);
        assert_eq!(
            declarations.member(&map, "size"),
            Member::Found(Type::Number)
        );
        assert_eq!(declarations.member(&map, "absent"), Member::Missing);
    }

    #[test]
    fn an_alias_that_never_bottoms_out_is_not_judged() {
        let ta = typed("function main(): void { }\n");
        let mut declaration = PackageDeclaration::with_package("@test/loop");
        let mangled = crate::mangle::package_symbol("@test/loop", "Loop");
        let itself = Type::alias_ref(
            crate::Package("@test/loop".to_string()),
            "Loop",
            mangled.clone(),
            Vec::new(),
        );
        declaration.types.insert(
            "Loop".to_string(),
            crate::TypeSymbol {
                name: "Loop".to_string(),
                mangled_name: mangled,
                declaration_span: crate::Span::at(crate::FileId(0)),
                kind: crate::TypeKind::Alias {
                    generics: Vec::new(),
                    ty: itself.clone(),
                    doc: None,
                },
            },
        );
        let declarations = TypeDeclarations::for_script(&ta, &[&declaration]);
        assert_eq!(declarations.member(&itself, "id"), Member::Unresolved);
    }
}
