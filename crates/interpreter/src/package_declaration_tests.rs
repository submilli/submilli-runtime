//! `PackageDeclaration`s built from parsed and inferred source.

use crate::PackageDeclaration;
use crate::{Asi, Token, TokenKind, Type, capture, check, desugar, infer, parse};

fn typed_ast(source: &str) -> crate::TypedAst {
    let mut asi = Asi::new(source, crate::FileId(0));
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        let tok = asi.next_token();
        let is_eof = matches!(tok.kind, TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let _ = asi.into_diagnostics();
    let (ast, _) = parse(source, tokens, crate::FileId(0));
    let (prelude_defs, host_defs, _) =
        crate::runtime::prelude::cached_runtime_package_declarations();
    let mut packages = Vec::with_capacity(prelude_defs.len() + host_defs.len());
    packages.extend(prelude_defs.iter());
    packages.extend(host_defs.iter());
    let (mut ta, _) = infer(source, "main", &ast, &packages);
    let _ = check(&ta);
    ta = capture(ta).unwrap();
    ta = desugar(ta, crate::FileId(0)).unwrap();
    ta
}

#[test]
fn collects_object_shape_from_let_annotation() {
    let ta = typed_ast("let p: { x: number; y: number } = { x: 1, y: 2 };");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    let object = defs
        .shapes
        .iter()
        .find_map(|s| match s {
            crate::Shape::Object { fields, .. } => Some(fields),
            _ => None,
        })
        .expect("object shape in defs.shapes");
    assert_eq!(
        object.get("x"),
        Some(&crate::ObjectField::required(Type::Number)),
    );
    assert_eq!(
        object.get("y"),
        Some(&crate::ObjectField::required(Type::Number)),
    );
}

#[test]
fn generic_constraints_survive_serialization_including_unused_parameters() {
    let (ta, diagnostics) = crate::typecheck_to_typed_ast(
        "type Empty<T extends number> = {};\nfunction identity<T extends number, U extends T>(value: U): U { return value; }\nfunction main(): void {}",
        crate::FileId(0),
    );
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != crate::Severity::Error),
        "{diagnostics:?}"
    );
    let declaration = PackageDeclaration::from_typed_ast(&ta);
    let serialized = serde_json::to_string(&declaration).unwrap();
    let restored: PackageDeclaration = serde_json::from_str(&serialized).unwrap();
    let crate::ValueKind::Function { generics, .. } = &restored.values["identity"].kind else {
        panic!("function declaration");
    };
    assert_eq!(generics.names, ["T", "U"]);
    assert_eq!(generics.constraints.get("T"), Some(&Type::Number));
    assert_eq!(
        generics.constraints.get("U"),
        Some(&Type::TypeVar("T".into()))
    );
    let crate::TypeKind::Alias { generics, .. } = &restored.types["Empty"].kind else {
        panic!("alias declaration");
    };
    assert_eq!(generics.constraints.get("T"), Some(&Type::Number));
    restored.check_type_limits().unwrap();
}

#[test]
fn preserves_readonly_and_writable_object_shapes() {
    let ta =
        typed_ast("let a: { readonly x: number } = { x: 1 }; let b: { x: number } = { x: 2 };");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    let readonly_flags: std::collections::BTreeSet<bool> = defs
        .shapes
        .iter()
        .filter_map(|shape| {
            let crate::Shape::Object { fields, .. } = shape else {
                return None;
            };
            fields.get("x").map(|field| field.readonly)
        })
        .collect();
    assert_eq!(
        readonly_flags,
        std::collections::BTreeSet::from([false, true])
    );
}

#[test]
fn collects_array_shape_from_let_annotation() {
    let ta = typed_ast("let xs: number[] = [1, 2, 3];");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    assert!(
        defs.shapes
            .iter()
            .any(|s| matches!(s, crate::Shape::Array(_))),
        "expected an Array shape in defs.shapes: {:?}",
        defs.shapes,
    );
}

#[test]
fn deduplicates_identical_object_shapes() {
    let ta = typed_ast("let a = { x: 1 }; let b = { x: 2 };");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    let object_count = defs
        .shapes
        .iter()
        .filter(|s| matches!(s, crate::Shape::Object { .. }))
        .count();
    assert_eq!(
        object_count, 1,
        "expected one Object entry, got {object_count}"
    );
}

#[test]
fn distinguishes_object_shapes_with_different_fields() {
    let ta = typed_ast("let a = { x: 1 }; let b = { y: 2 };");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    let object_count = defs
        .shapes
        .iter()
        .filter(|s| matches!(s, crate::Shape::Object { .. }))
        .count();
    assert_eq!(object_count, 2);
}

#[test]
fn canonical_shape_is_stable_across_modules() {
    let ta_a = typed_ast("let p = { x: 1, y: 2 };");
    let ta_b = typed_ast("let q = { y: 2, x: 1 };");
    let defs_a = PackageDeclaration::from_typed_ast(&ta_a);
    let defs_b = PackageDeclaration::from_typed_ast(&ta_b);
    let key_a = defs_a
        .shapes
        .iter()
        .find_map(|s| match s {
            crate::Shape::Object { .. } => Some(s.canonical_display()),
            _ => None,
        })
        .expect("object shape in defs_a");
    let key_b = defs_b
        .shapes
        .iter()
        .find_map(|s| match s {
            crate::Shape::Object { .. } => Some(s.canonical_display()),
            _ => None,
        })
        .expect("object shape in defs_b");
    assert_eq!(key_a, key_b, "canonical key should match across modules");
}

#[test]
fn nested_array_of_object_collects_both() {
    let ta = typed_ast("let xs: { x: number }[] = [];");
    let defs = PackageDeclaration::from_typed_ast(&ta);
    let has_array = defs
        .shapes
        .iter()
        .any(|s| matches!(s, crate::Shape::Array(_)));
    let has_object = defs
        .shapes
        .iter()
        .any(|s| matches!(s, crate::Shape::Object { .. }));
    assert!(has_array, "expected Array shape");
    assert!(has_object, "expected Object shape");
}
