use std::collections::BTreeMap;
use std::fmt::Write;

use crate::{ObjectField, Type};

/// Named in the help whenever a `void` expression reaches a value slot: the
/// message alone ("expected `unknown`, got `void`") says what is wrong but not
/// what to write instead.
const VOID_IS_NOT_A_VALUE: &str = "`void` is not a value: a function declared `: void` produces nothing. \
Call it as its own statement, then produce the value separately.";

/// The `help:` lines the *diff* earns: the structural difference, plus a note
/// when one side's rendering is lossy. Call sites may add their own on top. Separate entries, because they are
/// separate advice — folding them into one string prints the second without a
/// gutter, as stray prose in the diagnostic body.
pub(super) fn type_mismatch_help(expected: &Type, got: &Type) -> Vec<String> {
    format_type_diff(expected, got)
        .into_iter()
        .chain(guard_loss_note(got))
        .collect()
}

pub(super) fn format_type_diff(expected: &Type, got: &Type) -> Option<String> {
    if matches!(got.peel(), Type::Void) && !matches!(expected.peel(), Type::Void) {
        return Some(VOID_IS_NOT_A_VALUE.to_string());
    }
    if super::assignable::drops_readonly(got, expected) {
        return Some(format!(
            "`{got}` is `readonly` and cannot be assigned to the mutable type `{expected}`; \
             copy it with `[...value]`, or make the target type `readonly` too"
        ));
    }
    match (expected, got) {
        (
            Type::Object {
                fields: a,
                index: ai,
            },
            Type::Object {
                fields: b,
                index: bi,
            },
        ) => {
            if ai != bi {
                return Some(format!("expected `{expected}`, got `{got}`"));
            }
            Some(format_object_diff(a, b))
        }
        (
            Type::Function {
                params: pa,
                ret: ra,
                ..
            },
            Type::Function {
                params: pb,
                ret: rb,
                ..
            },
        ) => Some(format_function_diff(pa, ra, pb, rb)),
        _ => None,
    }
}

/// A type guard prints as its `boolean` return: [`Type::Function`]'s `Display`
/// has no spelling for the predicate, and the function-type grammar has no
/// annotation form for one either.
///
/// The rendering *parses*, which is what makes it worth a note — pasted into an
/// annotation it silently drops the narrowing, and the loss surfaces much later,
/// as a failed read inside the `if` the guard was supposed to open.
pub(super) fn guard_loss_note(got: &Type) -> Option<String> {
    let Type::Function {
        predicate: Some(predicate),
        ..
    } = got.peel()
    else {
        return None;
    };
    let param = format!("arg{}", predicate.parameter_index);
    Some(format!(
        "this value is a type guard (`{param} is {}`), and the rendering above is lossy: \
         no function-type annotation can carry a predicate, so a value declared at that \
         type compiles and then narrows nothing. Only a direct call of the guard narrows.",
        predicate.asserted_type,
    ))
}

fn format_object_diff(
    expected: &BTreeMap<String, ObjectField>,
    got: &BTreeMap<String, ObjectField>,
) -> String {
    let missing: Vec<&String> = expected
        .keys()
        .filter(|n| !got.contains_key(n.as_str()))
        .collect();
    let extra: Vec<&String> = got
        .keys()
        .filter(|n| !expected.contains_key(n.as_str()))
        .collect();
    let wrong: Vec<(&String, &ObjectField, &ObjectField)> = expected
        .iter()
        .filter_map(|(name, want)| {
            got.get(name).and_then(|g| {
                if g == want {
                    None
                } else {
                    Some((name, want, g))
                }
            })
        })
        .collect();

    let mut rows: Vec<String> = Vec::new();
    if !missing.is_empty() {
        rows.push(format!(
            "missing field(s): {}",
            missing
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !extra.is_empty() {
        rows.push(format!(
            "extra field(s): {}",
            extra
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !wrong.is_empty() {
        let mut s = String::from("field type mismatches:");
        for (n, e, g) in &wrong {
            if e.optional == g.optional {
                write!(s, "\n  `{}`: expected `{}`, got `{}`", n, e.ty, g.ty).unwrap();
            } else {
                let e_marker = if e.optional { "?" } else { "" };
                let g_marker = if g.optional { "?" } else { "" };
                write!(
                    s,
                    "\n  `{}`: expected `{}: {}` (optional={}), got `{}: {}` (optional={})",
                    n, e_marker, e.ty, e.optional, g_marker, g.ty, g.optional,
                )
                .unwrap();
            }
        }
        rows.push(s);
    }

    if rows.is_empty() {
        return "(no structural difference detected)".to_string();
    }
    rows.join("\n")
}

fn format_function_diff(pa: &[Type], ra: &Type, pb: &[Type], rb: &Type) -> String {
    let mut rows: Vec<String> = Vec::new();
    if pb.len() > pa.len() {
        rows.push(format!(
            "arity: expected at most {} param(s), got {}",
            pa.len(),
            pb.len(),
        ));
    }
    for (i, (e, g)) in pa.iter().zip(pb.iter()).enumerate() {
        if e != g {
            rows.push(format!("param {}: expected `{}`, got `{}`", i + 1, e, g));
        }
    }
    if ra != rb {
        rows.push(format!("return: expected `{ra}`, got `{rb}`"));
    }
    if rows.is_empty() {
        return "(no structural difference detected)".to_string();
    }
    rows.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn obj(pairs: &[(&str, Type)]) -> Type {
        let mut fields = BTreeMap::new();
        for (k, v) in pairs {
            fields.insert((*k).to_string(), ObjectField::required(v.clone()));
        }
        Type::Object {
            index: None,
            fields,
        }
    }

    #[test]
    fn primitive_vs_primitive_returns_none() {
        assert_eq!(format_type_diff(&Type::Number, &Type::String), None);
    }

    #[test]
    fn object_vs_primitive_returns_none() {
        assert_eq!(format_type_diff(&obj(&[]), &Type::String), None);
    }

    #[test]
    fn object_missing_field() {
        let out = format_type_diff(
            &obj(&[("x", Type::Number), ("y", Type::Number)]),
            &obj(&[("x", Type::Number)]),
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn object_extra_field() {
        let out = format_type_diff(
            &obj(&[("x", Type::Number)]),
            &obj(&[("x", Type::Number), ("y", Type::Number)]),
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn object_field_type_mismatch() {
        let out = format_type_diff(
            &obj(&[("name", Type::String)]),
            &obj(&[("name", Type::Number)]),
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn object_all_three_dimensions() {
        let out = format_type_diff(
            &obj(&[("x", Type::Number), ("y", Type::String)]),
            &obj(&[("x", Type::Boolean), ("z", Type::Number)]),
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn function_arity_diff() {
        let out = format_type_diff(
            &Type::Function {
                params: vec![Type::Number],
                ret: Box::new(Type::Void),
                predicate: None,
                has_rest: false,
            },
            &Type::Function {
                params: vec![Type::Number, Type::Number],
                ret: Box::new(Type::Void),
                predicate: None,
                has_rest: false,
            },
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn function_param_type_diff() {
        let out = format_type_diff(
            &Type::Function {
                params: vec![Type::Number, Type::String],
                ret: Box::new(Type::Boolean),
                predicate: None,
                has_rest: false,
            },
            &Type::Function {
                params: vec![Type::Number, Type::Number],
                ret: Box::new(Type::Boolean),
                predicate: None,
                has_rest: false,
            },
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn function_return_type_diff() {
        let out = format_type_diff(
            &Type::Function {
                params: vec![],
                ret: Box::new(Type::Number),
                predicate: None,
                has_rest: false,
            },
            &Type::Function {
                params: vec![],
                ret: Box::new(Type::String),
                predicate: None,
                has_rest: false,
            },
        );
        insta::assert_snapshot!(out.unwrap());
    }

    #[test]
    fn function_combined_arity_param_return() {
        let out = format_type_diff(
            &Type::Function {
                params: vec![Type::Number, Type::String],
                ret: Box::new(Type::Boolean),
                predicate: None,
                has_rest: false,
            },
            &Type::Function {
                params: vec![Type::String, Type::String, Type::Number],
                ret: Box::new(Type::Void),
                predicate: None,
                has_rest: false,
            },
        );
        insta::assert_snapshot!(out.unwrap());
    }
}
