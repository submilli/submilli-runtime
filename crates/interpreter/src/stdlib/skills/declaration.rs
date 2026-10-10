//! Type surface of `submilli:skills`.

use std::collections::BTreeMap;

use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

use super::MODULE_NAME;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_interface(
        &mut defs,
        "SkillInfo",
        skill_properties(false),
        "/** One skill this caller may load. Constructed only by `list()`. */",
    );
    insert_interface(
        &mut defs,
        "Skill",
        skill_properties(true),
        "/** A loaded skill: its instructions, and the name to read its files under. Constructed only by `load()`. */",
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
        "/**\n * Load the skill `name`: its description and its instructions.\n *\n * \
         Throws a catchable error when the harness has no skill of that name, or when \
         this runtime has no skill provider.\n * @param name Skill name. Call `list()` \
         for the skills you may load.\n * @capability skill.load { name: $name }\n */",
    );
    insert_fn(
        &mut defs,
        "readFile",
        vec![
            Param::new("name", Type::String),
            Param::new("path", Type::String),
        ],
        Type::String,
        "/**\n * Read a file bundled with the skill `name`, as text.\n *\n * `path` is \
         relative to the skill, with `/` separators, as the skill's instructions name \
         it — `\"templates/report.md\"`. An absolute path, or one with an empty, `.` \
         or `..` segment, throws a `RangeError` without asking the harness; a file the \
         skill does not have throws a catchable error.\n * @param name Skill name.\n \
         * @param path The file's path inside the skill.\n * @capability skill.load { \
         name: $name }\n */",
    );
    defs
}

fn interface_type(name: &str) -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, name),
        package: crate::Package(MODULE_NAME.to_string()),
        name: name.to_string(),
        args: Vec::new(),
    }
}

/// `name` and `description`, plus `content` for a loaded skill.
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
                methods: BTreeMap::new(),
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
