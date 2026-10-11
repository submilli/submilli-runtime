//! The `submilli:git` package declaration: repository constructors and methods.

use std::collections::BTreeMap;

use crate::{
    DefaultValue, FieldSig, FileId, MethodSig, ObjectField, PackageDeclaration, Param, Span, Type,
    TypeKind, TypeSymbol, Visibility,
};

use super::MODULE_NAME;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    let mut statics = BTreeMap::new();
    insert_static(
        &mut statics,
        "open",
        vec![string("path")],
        repository_type(),
        "/** Open an ordinary repository under the VFS root. Gated operations check the current caller's grants. */",
    );
    insert_static(
        &mut statics,
        "init",
        vec![string("path"), options(&[("branch", Type::String)])],
        repository_type(),
        "/** Initialize a repository, default branch main. Requires blueprint Git identity.\n * @capability git.init { path }\n */",
    );
    insert_static(
        &mut statics,
        "clone",
        vec![
            string("url"),
            string("path"),
            options(&[("branch", Type::String)]),
        ],
        repository_type(),
        "/** Clone HTTPS into an empty VFS directory, following remote HEAD unless branch is specified. Requires only git.clone; authentication uses host-only GIT_TOKEN when needed.\n * @capability git.clone { path, remote: $url, remoteName: \"origin\", branch: string }\n */",
    );
    insert_repository_class(&mut defs, statics);
    defs
}

fn insert_static(
    statics: &mut BTreeMap<String, MethodSig>,
    name: &str,
    params: Vec<Param>,
    ret: Type,
    doc: &str,
) {
    statics.insert(
        name.into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params,
            ret,
            predicate: None,
            doc: crate::doc(FileId::GIT, doc),
        },
    );
}

fn insert_repository_class(defs: &mut PackageDeclaration, statics: BTreeMap<String, MethodSig>) {
    let mut methods = BTreeMap::new();
    let entry = object(
        &[
            ("path", Type::String),
            ("staged", Type::String),
            ("unstaged", Type::String),
            ("untracked", Type::Boolean),
        ],
        false,
    );
    let commit = object(
        &[
            ("id", Type::String),
            ("message", Type::String),
            ("authorName", Type::String),
            ("authorEmail", Type::String),
        ],
        false,
    );
    methods.insert(
        "status".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![],
            ret: with_optional(
                object(
                    &[("entries", array(entry)), ("clean", Type::Boolean)],
                    false,
                ),
                "branch",
                Type::String,
            ),
            predicate: None,
            doc: crate::doc(
                FileId::GIT,
                "/** Inspect staged, unstaged and untracked paths. */",
            ),
        },
    );
    methods.insert(
        "log".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![options(&[
                ("limit", Type::Number),
                ("offset", Type::Number),
            ])],
            ret: with_optional(
                object(&[("commits", array(commit))], false),
                "nextOffset",
                Type::Number,
            ),
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Read history, default 50 commits; limit 1..1000, nonnegative integer offset. Traversal is resource-bounded. */"),
        },
    );
    methods.insert(
        "diff".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![options(&[
                ("mode", Type::String),
                ("from", Type::String),
                ("to", Type::String),
            ])],
            ret: object(
                &[
                    ("patch", Type::String),
                    ("binaryPaths", array(Type::String)),
                ],
                false,
            ),
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Compare working (default), staged, or refs (requires from and to). Bounded patches and binary path markers. */"),
        },
    );
    methods.insert(
        "show".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("ref"), string("path")],
            ret: Type::Uint8Array,
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Read file bytes from a commit. */"),
        },
    );
    methods.insert(
        "branches".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![],
            ret: array(object(
                &[
                    ("name", Type::String),
                    ("id", Type::String),
                    ("current", Type::Boolean),
                ],
                false,
            )),
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** List local branches. */"),
        },
    );
    methods.insert(
        "remotes".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![],
            ret: array(object(
                &[("name", Type::String), ("url", Type::String)],
                false,
            )),
            predicate: None,
            doc: crate::doc(
                FileId::GIT,
                "/** List named remotes without credentials. */",
            ),
        },
    );
    methods.insert(
        "add".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![Param::new("paths", array(Type::String))],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Stage explicit files or directories, including deletions. Use . for the working tree. */"),
        },
    );
    methods.insert(
        "commit".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("message")],
            ret: Type::String,
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Commit staged changes with Blueprint identity and return the commit ID. Empty commits are refused.\n * @capability git.commit { path: string, branch: string }\n */"),
        },
    );
    methods.insert(
        "createBranch".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("name"), default_string("start", "HEAD")],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(
                FileId::GIT,
                "/** Create a branch without overwriting an existing branch. */",
            ),
        },
    );
    methods.insert(
        "switchBranch".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("name")],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(
                FileId::GIT,
                "/** Switch local branches; requires a completely clean working tree. */",
            ),
        },
    );
    methods.insert(
        "addRemote".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("name"), string("url")],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(
                FileId::GIT,
                "/** Add a named HTTPS remote, for example origin or upstream. */",
            ),
        },
    );
    methods.insert(
        "setRemoteUrl".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![string("name"), string("url")],
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Update an existing remote's HTTPS URL. */"),
        },
    );
    methods.insert(
        "fetch".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![
                default_string("remote", "origin"),
                default_string("branch", ""),
            ],
            ret: object(&[("branches", array(Type::String))], false),
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Fetch remote-tracking branches. Empty branch requests every remote branch; every selected branch must be authorized. GIT_TOKEN is optional for public repositories.\n * @capability git.fetch { path: string, remoteName: $remote, remote: string, branch: string }\n */"),
        },
    );
    methods.insert(
        "pull".into(),
        MethodSig {
            optional: false,
            generics: vec![],
            params: vec![
                default_string("remote", "origin"),
                default_string("branch", ""),
            ],
            ret: object(
                &[("previous", Type::String), ("current", Type::String)],
                false,
            ),
            predicate: None,
            doc: crate::doc(FileId::GIT, "/** Fetch and fast-forward the current branch under git.fetch. Divergence and dirty worktrees are refused.\n * @capability git.fetch { path: string, remoteName: $remote, remote: string, branch: string }\n */"),
        },
    );
    defs.types.insert(
        "Repository".into(),
        TypeSymbol {
            name: "Repository".into(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Repository"),
            declaration_span: Span::at(FileId::GIT),
            kind: TypeKind::Class {
                generics: vec![],
                methods,
                fields: BTreeMap::from([("path".into(), FieldSig {
                    ty: Type::String, visibility: Visibility::Private, readonly: true,
                    optional: false, doc: None,
                })]),
                narrowing_checks: BTreeMap::new(),
                method_visibility: BTreeMap::new(),
                accessors: vec![],
                constructor: vec![string("path")],
                constructor_visibility: crate::Visibility::Public,
                statics,
                static_visibility: BTreeMap::new(),
                static_fields: BTreeMap::new(),
                extends: None,
                implements: vec![],
                doc: crate::doc(
                    FileId::GIT,
                    "/** A repository under the VFS root. Construct to open an existing repository, or use Repository.open, Repository.init or Repository.clone. */",
                ),
            },
        },
    );
}

fn object(fields: &[(&str, Type)], optional: bool) -> Type {
    Type::Object {
        index: None,
        fields: fields
            .iter()
            .map(|(name, ty)| {
                (
                    (*name).to_owned(),
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

/// `ty` with an optional member the host leaves out when it has no value.
fn with_optional(mut ty: Type, name: &str, field: Type) -> Type {
    if let Type::Object { fields, .. } = &mut ty {
        fields.insert(name.to_owned(), ObjectField::optional(field));
    }
    ty
}

fn array(ty: Type) -> Type {
    Type::Array(Box::new(ty))
}

fn string(name: &str) -> Param {
    Param::new(name, Type::String)
}

fn default_string(name: &str, value: &str) -> Param {
    Param::with_default(name, Type::String, DefaultValue::String(value.into()))
}

fn options(fields: &[(&str, Type)]) -> Param {
    Param::with_default(
        "options",
        Type::union(vec![object(fields, true), Type::Undefined]),
        DefaultValue::Undefined,
    )
}

fn repository_type() -> Type {
    Type::ClassRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Repository"),
        package: crate::Package(MODULE_NAME.into()),
        name: "Repository".into(),
        args: vec![],
    }
}
