//! The bodies a package declares, and whether a caller can reach each one.

use std::collections::BTreeSet;

use super::super::check_calls::SearchRoot;
use crate::compiler_error::CompilerFailure;
use crate::{
    ClosureBody, ExportKind, ExprId, GlobalKind, MangledName, Span, TypedAst, TypedClassAccessor,
    TypedClassDecl, TypedClassField, TypedExpr, TypedExprKind, TypedParam, TypedStmtKind,
    TypedTypeDecl, Visibility,
};

pub(super) struct Body<'a> {
    /// The body as a message names it: `send`, `Client.send`, `new Client`.
    pub(super) label: String,
    pub(super) label_span: Span,
    pub(super) params: &'a [TypedParam],
    /// A block, or the expression an arrow function returns.
    pub(super) root: SearchRoot,
    /// The function value the body belongs to, for a body a `const` holds.
    pub(super) function_value: Option<ExprId>,
    /// The symbol a call to the body names: a function's, or the `const`
    /// that holds it. `None` for a member of a class.
    pub(super) mangled: Option<MangledName>,
    /// Whether the body runs on a `this` the caller holds.
    pub(super) has_receiver: bool,
    /// Parameter properties: the fields a constructor stores its parameters
    /// in without a statement that says so.
    pub(super) parameter_properties: Vec<&'a TypedClassField>,
    pub(super) exposure: Exposure,
}

pub(super) enum Exposure {
    /// Part of the package's public API.
    Public,
    /// `symbol` is not exported from the package root.
    Unexported {
        symbol: String,
    },
    PrivateMember,
}

/// Every function, then every constructor, method and accessor, then every
/// function value a top-level `const` holds.
pub(super) fn of<'a>(
    ta: &'a TypedAst,
    exported_globals: &BTreeSet<&MangledName>,
) -> Result<Vec<Body<'a>>, CompilerFailure> {
    let public_functions = exported(ta, ExportKind::Function);
    let public_classes = public_classes(ta);
    let functions = ta.functions.iter().map(|function| Body {
        label: function.name.name.clone(),
        label_span: function.name.span,
        params: &function.params,
        root: SearchRoot::Stmt(function.body),
        function_value: None,
        mangled: Some(function.mangled_name.clone()),
        has_receiver: false,
        parameter_properties: Vec::new(),
        exposure: if public_functions.contains(&function.mangled_name) {
            Exposure::Public
        } else {
            Exposure::Unexported {
                symbol: function.name.name.clone(),
            }
        },
    });
    let members = classes(ta)
        .flat_map(|class| member_bodies(class, public_classes.contains(&class.mangled_name)));
    let mut bodies: Vec<_> = functions.chain(members).collect();
    for statement in &ta.top_level_statements {
        bodies.extend(const_function_body(ta, exported_globals, *statement)?);
    }
    Ok(bodies)
}

/// The body of the function value that `statement` binds to a top-level
/// `const`, which callers reach by the name of the `const`.
fn const_function_body<'a>(
    ta: &'a TypedAst,
    exported_globals: &BTreeSet<&MangledName>,
    statement: crate::StmtId,
) -> Result<Option<Body<'a>>, CompilerFailure> {
    let statement = ta
        .try_stmt(statement)
        .map_err(crate::typechecker::arena_failure)?;
    let TypedStmtKind::AssignGlobal {
        ident,
        mangled,
        value,
        ..
    } = &statement.kind
    else {
        return Ok(None);
    };
    if !is_module_const(ta, mangled) {
        return Ok(None);
    }
    let (function_value, function) = held_value(ta, *value)?;
    let TypedExprKind::Closure { params, body, .. } = &function.kind else {
        return Ok(None);
    };
    Ok(Some(Body {
        label: ident.name.clone(),
        label_span: ident.span,
        params,
        root: match body {
            ClosureBody::Block(block) => SearchRoot::Stmt(*block),
            ClosureBody::Expr(result) => SearchRoot::Expr(*result),
        },
        function_value: Some(function_value),
        mangled: Some(mangled.clone()),
        has_receiver: false,
        parameter_properties: Vec::new(),
        exposure: if exported_globals.contains(mangled) {
            Exposure::Public
        } else {
            Exposure::Unexported {
                symbol: ident.name.clone(),
            }
        },
    }))
}

/// The expression under the wrappers that yield the value they wrap: a cast,
/// a non-null assertion and a narrowing check the value and pass it on, so
/// the `const` holds the function under them. A conditional is not one: it
/// holds one of two values.
fn held_value(ta: &TypedAst, value: ExprId) -> Result<(ExprId, &TypedExpr), CompilerFailure> {
    let mut id = value;
    let mut wrapper_span = None;
    // A wrapper holds an expression other than itself, so a walk of more
    // steps than the arena has expressions has met one of them twice: the
    // wrappers form a cycle and hold no value.
    for _ in 0..ta.exprs_len() {
        let expr = ta.try_expr(id).map_err(crate::typechecker::arena_failure)?;
        let (TypedExprKind::Cast { value: inner, .. }
        | TypedExprKind::NonNullAssert { value: inner }
        | TypedExprKind::Narrowed { inner, .. }) = &expr.kind
        else {
            return Ok((id, expr));
        };
        wrapper_span = wrapper_span.or(Some(expr.span));
        id = *inner;
    }
    let failure = crate::typechecker::invariant_failure(
        "the wrappers around the value of a top-level `const` form a cycle",
    );
    Err(match wrapper_span {
        Some(span) => failure.with_span(span),
        None => failure,
    })
}

/// Whether `mangled` names a module `const`, which cannot be rebound. A
/// static field is a `const` global as well, under the name of its class,
/// but a writable one.
pub(super) fn is_module_const(ta: &TypedAst, mangled: &MangledName) -> bool {
    let is_const = ta
        .globals
        .iter()
        .any(|global| global.mangled_name == *mangled && global.kind == GlobalKind::Const);
    let is_static_field = classes(ta).any(|class| {
        mangled
            .as_str()
            .strip_prefix(class.mangled_name.as_str())
            .is_some_and(|member| member.starts_with(crate::mangle::SEP))
    });
    is_const && !is_static_field
}

fn member_bodies(class: &TypedClassDecl, class_is_public: bool) -> Vec<Body<'_>> {
    let class_name = &class.name.name;
    let exposure = |visibility: Visibility| match (class_is_public, visibility) {
        (false, Visibility::Public | Visibility::Private) => Exposure::Unexported {
            symbol: class_name.clone(),
        },
        (true, Visibility::Private) => Exposure::PrivateMember,
        (true, Visibility::Public) => Exposure::Public,
    };
    let constructor = class.constructor.iter().map(|constructor| Body {
        label: format!("new {class_name}"),
        label_span: class.name.span,
        params: &constructor.params,
        root: SearchRoot::Stmt(constructor.body),
        function_value: None,
        mangled: None,
        has_receiver: true,
        parameter_properties: class
            .fields
            .iter()
            .filter(|field| field.auto_assigned)
            .collect(),
        exposure: exposure(Visibility::Public),
    });
    let methods = class.methods.iter().map(|method| Body {
        label: format!("{class_name}.{}", method.name.name),
        label_span: method.name.span,
        params: &method.params,
        root: SearchRoot::Stmt(method.body),
        function_value: None,
        mangled: None,
        has_receiver: true,
        parameter_properties: Vec::new(),
        exposure: exposure(method.visibility),
    });
    let accessors = class.accessors.iter().map(|accessor| Body {
        label: format!("{class_name}.{}", accessor.name().name),
        label_span: accessor.name().span,
        params: accessor_params(accessor),
        root: SearchRoot::Stmt(accessor.body()),
        function_value: None,
        mangled: None,
        has_receiver: true,
        parameter_properties: Vec::new(),
        exposure: exposure(accessor.visibility()),
    });
    constructor.chain(methods).chain(accessors).collect()
}

fn accessor_params(accessor: &TypedClassAccessor) -> &[TypedParam] {
    match accessor {
        TypedClassAccessor::Getter { .. } => &[],
        TypedClassAccessor::Setter { param, .. } => std::slice::from_ref(param),
    }
}

/// The classes the root module exports, with the package's own classes they
/// extend: a caller reaches an inherited member through the exported class.
fn public_classes(ta: &TypedAst) -> BTreeSet<&MangledName> {
    let exported = exported(ta, ExportKind::Type);
    let mut public = BTreeSet::new();
    for class in classes(ta).filter(|class| exported.contains(&class.mangled_name)) {
        let mut current = Some(class);
        while let Some(class) = current {
            // A class already seen has had its ancestors added, which also
            // ends the walk on an `extends` cycle.
            if !public.insert(&class.mangled_name) {
                break;
            }
            current = class
                .extends
                .as_ref()
                .and_then(|parent| classes(ta).find(|class| class.mangled_name == *parent));
        }
    }
    public
}

pub(super) fn exported(ta: &TypedAst, kind: ExportKind) -> BTreeSet<&MangledName> {
    ta.exports
        .iter()
        .filter(|entry| entry.kind == kind)
        .map(|entry| &entry.target)
        .collect()
}

fn classes(ta: &TypedAst) -> impl Iterator<Item = &TypedClassDecl> {
    ta.types.iter().filter_map(|declaration| match declaration {
        TypedTypeDecl::Class(class) => Some(class),
        TypedTypeDecl::Interface(_)
        | TypedTypeDecl::NumberEnum(_)
        | TypedTypeDecl::StringEnum(_)
        | TypedTypeDecl::Alias(_) => None,
    })
}
