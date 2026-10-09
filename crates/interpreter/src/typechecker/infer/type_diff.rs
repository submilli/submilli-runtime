use crate::rendering::{RenderError, RenderLimits, Writer};
use crate::type_rendering::write_type;
use crate::{ObjectField, Type};
use std::collections::BTreeMap;
#[cfg(test)]
thread_local! {
    static FAIL_RENDER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn type_mismatch_help(expected: &Type, got: &Type) -> Result<Vec<String>, RenderError> {
    let mut help = Vec::new();
    help.try_reserve(2).map_err(|_| RenderError::Allocation)?;
    if let Some(diff) = format_type_diff(expected, got)? {
        help.push(diff);
    }
    if let Some(note) = guard_loss_note(got)? {
        help.push(note);
    }
    if let Some(note) = weak_type_note(expected, got)? {
        help.push(note);
    }
    Ok(help)
}

/// Explains a rejection by the weak-type rule: an object type whose fields are
/// all optional takes only a value that has at least one of them.
fn weak_type_note(expected: &Type, got: &Type) -> Result<Option<String>, RenderError> {
    let (
        Type::Object {
            fields: expected_fields,
            index: None,
        },
        Type::Object {
            fields: got_fields, ..
        },
    ) = (expected.peel(), got.peel())
    else {
        return Ok(None);
    };
    if !super::assignable::weak_type_rejects(got_fields, expected_fields) {
        return Ok(None);
    }
    for ty in [expected, got] {
        match crate::type_rendering::check_for_copy(ty) {
            Ok(()) => {}
            Err(RenderError::Truncated) => return Ok(Some(crate::rendering::TRUNCATED.into())),
            Err(error) => return Err(error),
        }
    }
    Writer::render(RenderLimits::default(), |out| {
        out.push("every field of `")?;
        write_type(out, expected)?;
        out.push("` is optional, and `")?;
        write_type(out, got)?;
        out.push("` has none of them; as in TypeScript, a value must share at least one field with such a type")
    })
    .map(|rendered| Some(rendered.text))
}

pub(super) fn format_type_diff(expected: &Type, got: &Type) -> Result<Option<String>, RenderError> {
    #[cfg(test)]
    if FAIL_RENDER.with(|fail| fail.replace(false)) {
        return Err(RenderError::Allocation);
    }

    // Comparison helpers operate on compiler-bounded trees. Invalid direct inputs
    // are abbreviated before invoking recursive equality/readonly comparison.
    for ty in [expected, got] {
        match crate::type_rendering::check_for_copy(ty) {
            Ok(()) => {}
            Err(RenderError::Truncated) => return Ok(Some(crate::rendering::TRUNCATED.into())),
            Err(error) => return Err(error),
        }
    }
    let has_diff = matches!(
        (expected, got),
        (Type::Object { .. }, Type::Object { .. }) | (Type::Function { .. }, Type::Function { .. })
    );
    let readonly = super::assignable::drops_readonly(got, expected);
    if !has_diff && !readonly {
        return Ok(None);
    }
    Writer::render(RenderLimits::default(), |out| {
        if readonly {
            out.push("`")?;
            write_type(out, got)?;
            out.push("` is `readonly` and cannot be assigned to the mutable type `")?;
            write_type(out, expected)?;
            return out
                .push("`; copy it with `[...value]`, or make the target type `readonly` too");
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
                if ai == bi {
                    write_object_diff(out, a, b)
                } else {
                    write_expected(out, expected, got)
                }
            }
            // Differing optional counts make the parameter lists differ
            // where no row shows it, so the plain mismatch says more.
            (
                Type::Function {
                    optional: a_optional,
                    ..
                },
                Type::Function {
                    optional: b_optional,
                    ..
                },
            ) if a_optional != b_optional => write_expected(out, expected, got),
            (
                Type::Function {
                    params: a,
                    ret: ar,
                    has_rest: false,
                    ..
                },
                Type::Function {
                    params: b,
                    ret: br,
                    has_rest: true,
                    ..
                },
            ) => write_rest_function_diff(out, a, ar, b, br),
            (
                Type::Function {
                    params: a, ret: ar, ..
                },
                Type::Function {
                    params: b, ret: br, ..
                },
            ) => write_function_diff(out, a, ar, b, br),
            _ => Err(RenderError::InvalidMetadata(
                "unsupported structural difference",
            )),
        }
    })
    .map(|rendered| Some(rendered.text))
}

fn write_expected(out: &mut Writer, expected: &Type, got: &Type) -> Result<(), RenderError> {
    out.push("expected `")?;
    write_type(out, expected)?;
    out.push("`, got `")?;
    write_type(out, got)?;
    out.push("`")
}

pub(super) fn guard_loss_note(got: &Type) -> Result<Option<String>, RenderError> {
    match crate::type_rendering::check_for_copy(got) {
        Ok(()) => {}
        Err(RenderError::Truncated) => return Ok(Some(crate::rendering::TRUNCATED.into())),
        Err(error) => return Err(error),
    }
    let Type::Function {
        predicate: Some(predicate),
        ..
    } = got.peel()
    else {
        return Ok(None);
    };
    Writer::render(RenderLimits::default(), |out| {
        out.format(format_args!("this value is a type guard (`arg{} is ", predicate.parameter_index))?;
        write_type(out, &predicate.asserted_type)?;
        out.push("`), and the rendering above is lossy: no function-type annotation can carry a predicate, so a value declared at that type compiles and then narrows nothing. Only a direct call of the guard narrows.")
    }).map(|rendered| Some(rendered.text))
}

fn newline(out: &mut Writer, has_rows: &mut bool) -> Result<(), RenderError> {
    if *has_rows {
        out.push("\n")?;
    }
    *has_rows = true;
    Ok(())
}

fn write_object_diff(
    out: &mut Writer,
    expected: &BTreeMap<String, ObjectField>,
    got: &BTreeMap<String, ObjectField>,
) -> Result<(), RenderError> {
    let mut has_rows = false;
    for (left, right, label) in [(expected, got, "missing"), (got, expected, "extra")] {
        let mut first = true;
        for name in left.keys() {
            out.step()?;
            if right.contains_key(name) {
                continue;
            }
            if first {
                newline(out, &mut has_rows)?;
                out.format(format_args!("{label} field(s): "))?;
                first = false;
            } else {
                out.push(", ")?;
            }
            out.format(format_args!("`{name}`"))?;
        }
    }
    let mut wrong = false;
    for (name, e) in expected {
        out.step()?;
        let Some(g) = got.get(name) else {
            continue;
        };
        if e == g {
            continue;
        }
        if !wrong {
            newline(out, &mut has_rows)?;
            out.push("field type mismatches:")?;
            wrong = true;
        }
        out.format(format_args!("\n  `{name}`: "))?;
        if e.optional == g.optional {
            write_expected(out, &e.ty, &g.ty)?;
        } else {
            out.format(format_args!(
                "expected `{}: ",
                if e.optional { "?" } else { "" }
            ))?;
            write_type(out, &e.ty)?;
            out.format(format_args!(
                "` (optional={}), got `{}: ",
                e.optional,
                if g.optional { "?" } else { "" }
            ))?;
            write_type(out, &g.ty)?;
            out.format(format_args!("` (optional={})", g.optional))?;
        }
    }
    if !has_rows {
        out.push("(no structural difference detected)")?;
    }
    Ok(())
}

/// A function with a rest parameter, `b`, where the fixed-arity `a` is
/// expected: each expected parameter past its fixed ones is compared with the
/// rest parameter's element type. When they all fit, the only difference left
/// is the parameter count Submilli needs to differ.
fn write_rest_function_diff(
    out: &mut Writer,
    a: &[Type],
    ar: &Type,
    b: &[Type],
    br: &Type,
) -> Result<(), RenderError> {
    let Some((Type::Array(element), fixed)) =
        b.split_last().map(|(rest, fixed)| (rest.peel(), fixed))
    else {
        return write_function_diff(out, a, ar, b, br);
    };
    let spread_len = fixed.len().max(a.len());
    let mut spread = Vec::new();
    spread
        .try_reserve(spread_len)
        .map_err(|_| RenderError::Allocation)?;
    spread.extend(fixed.iter().cloned());
    spread.extend(std::iter::repeat_n(
        (**element).clone(),
        spread_len.saturating_sub(fixed.len()),
    ));
    let fits = spread.len() == a.len() && a == spread.as_slice() && ar == br;
    if a.len() == b.len() && fits {
        return out.format(format_args!(
            "a function with a rest parameter can't stand for one with as many parameters ({})",
            a.len()
        ));
    }
    write_function_diff(out, a, ar, &spread, br)
}

fn write_function_diff(
    out: &mut Writer,
    a: &[Type],
    ar: &Type,
    b: &[Type],
    br: &Type,
) -> Result<(), RenderError> {
    let mut has_rows = false;
    if b.len() > a.len() {
        newline(out, &mut has_rows)?;
        out.format(format_args!(
            "arity: expected at most {} param(s), got {}",
            a.len(),
            b.len()
        ))?;
    }
    for (i, (e, g)) in a.iter().zip(b).enumerate() {
        out.step()?;
        if e != g {
            newline(out, &mut has_rows)?;
            out.format(format_args!("param {}: ", i.saturating_add(1)))?;
            write_expected(out, e, g)?;
        }
    }
    if ar != br {
        newline(out, &mut has_rows)?;
        out.push("return: ")?;
        write_expected(out, ar, br)?;
    }
    if !has_rows {
        out.push("(no structural difference detected)")?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn format_type_diff(a: &Type, b: &Type) -> Option<String> {
        super::format_type_diff(a, b).unwrap()
    }
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
    fn rendering_failure_retains_triggering_type_mismatch() {
        crate::type_size::tests::on_compiler_stack(|| {
            FAIL_RENDER.with(|fail| fail.set(true));
            let error = crate::compile::compile_script_checked(
                "function main(): number { return \"wrong\"; }",
                "broken.ts",
                crate::FileId(0),
                &[],
                &[],
            )
            .unwrap_err();
            assert!(matches!(
                error.fatal,
                Some(crate::compiler_error::CompilerFailure::Internal { .. })
            ));
            assert!(
                error
                    .diagnostics
                    .iter()
                    .any(|diag| diag.message.contains("expected `number`")),
                "{error:?}"
            );
            assert!(
                crate::compile::compile_script_checked(
                    "function main(): number { return 42; }",
                    "good.ts",
                    crate::FileId(0),
                    &[],
                    &[],
                )
                .is_ok()
            );
        });
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
                optional: 0,
            },
            &Type::Function {
                params: vec![Type::Number, Type::Number],
                ret: Box::new(Type::Void),
                predicate: None,
                has_rest: false,
                optional: 0,
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
                optional: 0,
            },
            &Type::Function {
                params: vec![Type::Number, Type::Number],
                ret: Box::new(Type::Boolean),
                predicate: None,
                has_rest: false,
                optional: 0,
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
                optional: 0,
            },
            &Type::Function {
                params: vec![],
                ret: Box::new(Type::String),
                predicate: None,
                has_rest: false,
                optional: 0,
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
                optional: 0,
            },
            &Type::Function {
                params: vec![Type::String, Type::String, Type::Number],
                ret: Box::new(Type::Void),
                predicate: None,
                has_rest: false,
                optional: 0,
            },
        );
        insta::assert_snapshot!(out.unwrap());
    }
}
