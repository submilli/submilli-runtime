//! Agent-facing declarations. Structural records remain serializable guest objects.
use super::MODULE_NAME;
use crate::{
    DefaultValue, FileId, ObjectField, PackageDeclaration, Param, Span, Type, ValueKind,
    ValueSymbol,
};

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    let line = object(&[("line", Type::Number), ("text", Type::String)], false);
    let diagnostic = object(
        &[
            ("hunk", Type::Number),
            ("line", Type::Number),
            ("message", Type::String),
        ],
        false,
    );
    let mutation = object(
        &[
            ("success", Type::Boolean),
            ("changed", Type::Boolean),
            ("diff", Type::String),
            ("diagnostics", array(diagnostic)),
        ],
        false,
    );
    insert(
        &mut defs,
        "read",
        vec![
            string("path"),
            number("offset", 1.0),
            number("limit", 200.0),
        ],
        object(
            &[
                ("path", Type::String),
                ("lines", array(line.clone())),
                ("truncated", Type::Boolean),
            ],
            false,
        ),
        "/** Numbered UTF-8 lines; one-based offset, default 200 lines. Requires fs.read. */",
    );
    let options = object(
        &[
            ("path", Type::String),
            ("include", array(Type::String)),
            ("exclude", array(Type::String)),
            ("caseSensitive", Type::Boolean),
            ("context", Type::Number),
            ("mode", Type::String),
            ("limit", Type::Number),
        ],
        true,
    );
    let hit = object(
        &[
            ("path", Type::String),
            ("line", Type::Number),
            ("text", Type::String),
            ("before", array(line.clone())),
            ("after", array(line)),
        ],
        false,
    );
    let count = object(&[("path", Type::String), ("count", Type::Number)], false);
    insert(
        &mut defs,
        "search",
        vec![
            string("pattern"),
            Param::with_default(
                "options",
                Type::union(vec![options, Type::Null]),
                DefaultValue::Null,
            ),
        ],
        object(
            &[
                ("matches", array(hit)),
                ("files", array(Type::String)),
                ("counts", array(count)),
                ("truncated", Type::Boolean),
            ],
            false,
        ),
        "/** Regex search, ignoring hidden and ignored paths. Modes: matches (default), files, counts. Root defaults to /; context 0; caseSensitive true; limit 1000. Requires fs.list, fs.stat and fs.read, including ignore files. */",
    );
    let entry = object(
        &[
            ("path", Type::String),
            ("kind", Type::String),
            ("depth", Type::Number),
            ("modifiedAt", Type::Number),
        ],
        false,
    );
    let listing = object(
        &[("entries", array(entry)), ("truncated", Type::Boolean)],
        false,
    );
    insert(
        &mut defs,
        "glob",
        vec![string("pattern")],
        listing.clone(),
        "/** Match file paths relative to /; newest modification first, path breaks ties. Maximum 1000 results. Requires fs.list, fs.stat and fs.read for ignore files. */",
    );
    insert(
        &mut defs,
        "tree",
        vec![string("path"), number("depth", 3.0)],
        listing,
        "/** Ignored/hidden paths omitted; symlinks listed but never traversed. Depth defaults to 3; maximum 1000 entries. Requires fs.list, fs.stat and fs.read for ignore files. */",
    );
    insert(
        &mut defs,
        "edit",
        vec![
            string("path"),
            string("oldString"),
            string("newString"),
            Param::with_default("replaceAll", Type::Boolean, DefaultValue::Boolean(false)),
            number("nearLine", 0.0),
        ],
        mutation.clone(),
        "/** Replace a unique exact anchor; replaceAll selects every occurrence. nearLine chooses the uniquely nearest anchor (0 means absent). Whitespace-only whole-line near matches may preserve indentation. Requires fs.read and fs.write. */",
    );
    insert(
        &mut defs,
        "insertAt",
        vec![
            string("path"),
            Param::new("line", Type::Number),
            string("text"),
        ],
        mutation.clone(),
        "/** Insert text literally before a one-based line; one past the last line appends. Requires fs.read and fs.write. */",
    );
    insert(
        &mut defs,
        "diffText",
        vec![string("a"), string("b")],
        Type::String,
        "/** Pure UTF-16 text comparison; unified diff with three context lines. No capability needed. */",
    );
    insert(
        &mut defs,
        "diffFiles",
        vec![string("a"), string("b")],
        Type::String,
        "/** Unified diff of two strict UTF-8 workspace files. Requires fs.read on both. */",
    );
    insert(
        &mut defs,
        "applyPatch",
        vec![string("path"), string("patch")],
        mutation,
        "/** Apply a single-file unified patch by unique context, ignoring header positions. All hunks must succeed; otherwise nothing is written. Requires fs.read and fs.write. */",
    );
    defs
}
fn object(fields: &[(&str, Type)], optional: bool) -> Type {
    Type::Object {
        index: None,
        fields: fields
            .iter()
            .map(|(name, ty)| {
                (
                    (*name).into(),
                    if optional {
                        ObjectField::optional(ty.clone())
                    } else {
                        ObjectField::required(ty.clone())
                    },
                )
            })
            .collect(),
    }
}
fn array(ty: Type) -> Type {
    Type::Array(Box::new(ty))
}
fn string(name: &str) -> Param {
    Param::new(name, Type::String)
}
fn number(name: &str, value: f64) -> Param {
    Param::with_default(name, Type::Number, DefaultValue::Number(value))
}
fn insert(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.into(),
        ValueSymbol {
            name: name.into(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(FileId::CODE),
            kind: ValueKind::Function {
                generics: vec![],
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(FileId::CODE, doc),
            },
        },
    );
}
