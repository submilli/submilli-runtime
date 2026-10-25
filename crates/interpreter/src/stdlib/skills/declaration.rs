//! Type surface of `submilli:skills`.

use std::collections::BTreeMap;

use crate::{
    Dispatch, MethodSig, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol,
    ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_interface(
        &mut defs,
        "SkillInfo",
        skill_properties(false),
        BTreeMap::new(),
        "/** One skill this caller may load. Constructed only by `list()`. */",
    );
    insert_interface(
        &mut defs,
        "Skill",
        skill_properties(true),
        skill_methods(),
        "/** A loaded skill. Read bundled files with `skill.readFile(path)`. Skills are never mounted or accessible through `submilli:fs`. Constructed only by `load()`. */",
    );
    insert_fn(
        &mut defs,
        "list",
        Vec::new(),
        Type::Array(Box::new(interface_type("SkillInfo"))),
        "/**\n * The skills this caller may load.\n *\n * Each skill the harness offers is \
         filtered under `skill.load` by the same `name` filter that gates `load`, so \
         the list never offers a skill the caller would be denied. Nothing in it \
         reveals how many were filtered out.\n * @capability skill.load\n */",
    );
    insert_fn(
        &mut defs,
        "load",
        vec![Param::new("name", Type::String)],
        interface_type("Skill"),
        "/**\n * Load the skill `name`: its description, instructions and bundled file names.\n *\n * \
         A name that is not one skill name — empty, `.` or `..`, containing `/`, \
         `\\` or NUL, or over 4096 bytes — throws a `RangeError` without asking the \
         harness. Throws a catchable error when the harness has no skill of that name, \
         or when this runtime has no skill provider.\n * @param name Skill name, as \
         `list()` returns it.\n * @capability skill.load { name: $name }\n */",
    );
    defs
}

fn skill_methods() -> BTreeMap<String, MethodSig> {
    BTreeMap::from([(
        "readFile".to_string(),
        MethodSig {
            optional: false,
            generics: Vec::new(),
            params: vec![Param::new("path", Type::String)],
            ret: Type::String,
            predicate: None,
            doc: crate::doc(
                crate::FileId::SKILLS,
                "/** Read a bundled file as text. Pass a path from `files`, relative to this skill with `/` separators. Absolute paths, empty, `.` or `..` segments, `\\`, `:` or NUL, and paths over 4096 bytes throw a `RangeError` before asking the provider. A missing file throws a catchable error. Files are never accessible through `submilli:fs`. The `skill.load` policy is checked again using this skill's name.\n * @param path The file's path inside this skill.\n * @capability skill.load\n */",
            ),
        },
    )])
}

fn interface_type(name: &str) -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, name),
        package: crate::Package(MODULE_NAME.to_string()),
        name: name.to_string(),
        args: Vec::new(),
    }
}

/// `name` and `description`, plus instructions and file names for a loaded skill.
fn skill_properties(loaded: bool) -> BTreeMap<String, PropertySig> {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "name",
        Type::String,
        "/** The skill name, exactly as `load` and `readFile` expect it. */",
    );
    insert_property(
        &mut properties,
        "description",
        Type::union(vec![Type::String, Type::Undefined]),
        "/** What the skill is for, or `undefined` when the harness gives none. */",
    );
    if loaded {
        insert_property(
            &mut properties,
            "files",
            Type::Array(Box::new(Type::String)),
            "/** Bundled file names relative to this skill, without reading their contents. Pass each to `skill.readFile(path)`. These are not filesystem paths and cannot be read through `submilli:fs`. Invalid paths are omitted. */",
        );
        insert_property(
            &mut properties,
            "content",
            Type::String,
            "/** The skill's instructions. */",
        );
    }
    properties
}

fn insert_interface(
    defs: &mut PackageDeclaration,
    name: &str,
    properties: BTreeMap<String, PropertySig>,
    methods: BTreeMap<String, MethodSig>,
    doc: &str,
) {
    defs.types.insert(
        name.to_string(),
        TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::SKILLS),
            kind: TypeKind::Interface {
                index: None,
                generics: Vec::new(),
                methods,
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::SKILLS, doc),
            },
        },
    );
}

fn insert_property(
    properties: &mut BTreeMap<String, PropertySig>,
    name: &str,
    ty: Type,
    doc: &str,
) {
    properties.insert(
        name.to_string(),
        PropertySig {
            ty,
            readonly: true,
            optional: false,
            intrinsic: false,
            doc: crate::doc(crate::FileId::SKILLS, doc),
        },
    );
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::SKILLS),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::SKILLS, doc),
            },
        },
    );
}
