use crate::rendering::{RenderError, RenderLimits, Writer};
use crate::type_rendering::write_type;

use crate::type_rendering::CopyBudget;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{DefaultValue, Intrinsic, MethodSig, Param, Type};

#[derive(Clone, Copy)]
pub(super) enum SignatureKind<'a> {
    Constructor {
        name: &'a str,
        params: &'a [Param],
    },
    Function {
        name: &'a str,
        generics: &'a [String],
        params: &'a [Param],
        ret: &'a Type,
        doc: Option<&'a crate::DocComment>,
        predicate: Option<&'a crate::TypePredicate>,
    },
    Method {
        receiver_ty: &'a Type,
        name: &'a str,
        sig: &'a MethodSig,
    },
    Anon {
        params: &'a [Type],
        ret: &'a Type,
        /// The last `params` entry is the rest array.
        has_rest: bool,
    },
    Intrinsic {
        kind: Intrinsic,
    },
}

pub(super) fn format_signature(
    kind: SignatureKind<'_>,
    substitution: &TypeParamSubstitution,
) -> Result<String, RenderError> {
    let limits = CopyBudget::default();
    Writer::render(RenderLimits::default(), |out| match kind {
        SignatureKind::Constructor { name, params } => {
            out.format(format_args!("new {name}("))?;
            write_params(out, params, &TypeParamSubstitution::new(), &limits)?;
            out.push(")")
        }
        SignatureKind::Function {
            name,
            generics,
            params,
            ret,
            doc,
            predicate,
        } => {
            validate_predicate(params, predicate)?;
            if let Some(doc) = doc {
                super::format_definition::write_doc_block(out, "", doc)?;
            }
            out.format(format_args!("function {name}"))?;
            write_generic_list(out, generics)?;
            out.push("(")?;
            write_params(out, params, &TypeParamSubstitution::new(), &limits)?;
            out.push("): ")?;
            write_return(
                out,
                params,
                ret,
                predicate,
                &TypeParamSubstitution::new(),
                &limits,
            )
        }
        SignatureKind::Method {
            receiver_ty,
            name,
            sig,
        } => {
            validate_predicate(&sig.params, sig.predicate.as_ref())?;
            if let Some(doc) = &sig.doc {
                super::format_definition::write_doc_block(out, "", doc)?;
            }
            write_type(out, receiver_ty)?;
            out.format(format_args!(".{name}"))?;
            write_generic_list(out, &sig.generics)?;
            out.push("(")?;
            write_params(out, &sig.params, substitution, &limits)?;
            out.push("): ")?;
            write_return(
                out,
                &sig.params,
                &sig.ret,
                sig.predicate.as_ref(),
                substitution,
                &limits,
            )
        }
        SignatureKind::Anon {
            params,
            ret,
            has_rest,
        } => {
            write_synthetic_params(out, params, has_rest)?;
            out.push(" => ")?;
            write_type(out, ret)
        }
        SignatureKind::Intrinsic { kind } => write_intrinsic(out, kind),
    })
    .map(|rendered| rendered.text)
}

pub(super) fn validate_predicate(
    params: &[Param],
    predicate: Option<&crate::TypePredicate>,
) -> Result<(), RenderError> {
    if predicate.is_some_and(|predicate| {
        usize::try_from(predicate.parameter_index)
            .ok()
            .and_then(|index| params.get(index))
            .is_none()
    }) {
        return Err(RenderError::InvalidMetadata(
            "predicate parameter is absent",
        ));
    }
    Ok(())
}

pub(super) fn substituted(
    ty: &Type,
    sub: &TypeParamSubstitution,
    limits: &CopyBudget,
) -> Result<Type, RenderError> {
    // Validate before substitution can clone a malformed, over-deep input.
    limits.check_substitution(ty, sub)?;
    sub.apply(ty, &limits.types)
        .map_err(|_| RenderError::Truncated)
}

pub(super) fn write_params(
    out: &mut Writer,
    params: &[Param],
    sub: &TypeParamSubstitution,
    limits: &CopyBudget,
) -> Result<(), RenderError> {
    for (i, param) in params.iter().enumerate() {
        out.step()?;
        if i != 0 {
            out.push(", ")?;
        }
        let ty = substituted(&param.ty, sub, limits)?;
        write_named_param(out, &param.name, &ty, param.default.as_ref(), param.rest)?;
    }
    Ok(())
}

pub(super) fn write_named_param(
    out: &mut Writer,
    name: &str,
    ty: &Type,
    default: Option<&DefaultValue>,
    rest: bool,
) -> Result<(), RenderError> {
    if rest {
        out.push("...")?;
    }
    if !name.is_empty() {
        out.format(format_args!("{name}: "))?;
    }
    write_type(out, ty)?;
    if let Some(default) = default {
        out.push(" = ")?;
        write_default_value(out, default)?;
    }
    Ok(())
}

pub(super) fn write_default_value(
    out: &mut Writer,
    default: &DefaultValue,
) -> Result<(), RenderError> {
    match default {
        DefaultValue::Number(n) => out.push(&crate::runtime::number::format_number_js(*n)),
        DefaultValue::String(s) => out.format(format_args!("{s:?}")),
        DefaultValue::Boolean(b) => out.format(format_args!("{b}")),
        DefaultValue::Null => out.push("null"),
        DefaultValue::EmptyArray => out.push("[]"),
        DefaultValue::EmptyObject => out.push("{}"),
        DefaultValue::GlobalConst(m) => out.push(short_symbol(m.as_str())),
        DefaultValue::EnumVariant {
            enum_mangled,
            variant,
            ..
        } => out.format(format_args!(
            "{}.{variant}",
            short_symbol(enum_mangled.as_str())
        )),
    }
}

fn short_symbol(mangled: &str) -> &str {
    mangled.rsplit('#').next().unwrap_or(mangled)
}

fn write_return(
    out: &mut Writer,
    params: &[Param],
    ret: &Type,
    predicate: Option<&crate::TypePredicate>,
    sub: &TypeParamSubstitution,
    limits: &CopyBudget,
) -> Result<(), RenderError> {
    if let Some(predicate) = predicate {
        let param =
            params
                .get(predicate.parameter_index as usize)
                .ok_or(RenderError::InvalidMetadata(
                    "predicate parameter is absent",
                ))?;
        out.format(format_args!("{} is ", param.name))?;
        return write_type(out, &substituted(&predicate.asserted_type, sub, limits)?);
    }
    write_type(out, &substituted(ret, sub, limits)?)
}

pub(super) fn write_synthetic_params(
    out: &mut Writer,
    params: &[Type],
    rest: bool,
) -> Result<(), RenderError> {
    if rest && params.is_empty() {
        return Err(RenderError::InvalidMetadata(
            "rest signature has no parameter",
        ));
    }
    out.push("(")?;
    for (i, param) in params.iter().enumerate() {
        if i != 0 {
            out.push(", ")?;
        }
        if rest && i == params.len().saturating_sub(1) {
            out.push("...")?;
        }
        out.format(format_args!("arg{i}: "))?;
        write_type(out, param)?;
    }
    out.push(")")
}

fn write_intrinsic(out: &mut Writer, kind: Intrinsic) -> Result<(), RenderError> {
    if matches!(kind, Intrinsic::JsonStringify) {
        return out.push(
            "JSON.stringify<T>(value: T, replacer?: null, space?: number | string | null): string",
        );
    }
    if matches!(kind, Intrinsic::JsonParse) {
        return out.push("JSON.parse(text: string): unknown");
    }
    out.push(kind.name())?;
    out.push("(")?;
    write_params(
        out,
        &kind.params(),
        &TypeParamSubstitution::new(),
        &CopyBudget::default(),
    )?;
    out.push("): ")?;
    write_type(out, &kind.ret())
}

pub(super) fn write_generic_list(out: &mut Writer, generics: &[String]) -> Result<(), RenderError> {
    if generics.is_empty() {
        return Ok(());
    }
    out.push("<")?;
    for (i, generic) in generics.iter().enumerate() {
        if i != 0 {
            out.push(", ")?;
        }
        out.push(generic)?;
    }
    out.push(">")
}
#[cfg(test)]
mod tests {
    use super::*;

    fn format_signature(kind: SignatureKind<'_>, substitution: &TypeParamSubstitution) -> String {
        super::format_signature(kind, substitution).unwrap()
    }

    #[test]
    fn oversized_signature_does_not_hide_invalid_predicate() {
        let predicate = crate::TypePredicate {
            parameter_index: 9,
            asserted_type: Type::String,
        };
        let name = "f".repeat(100_000);
        let result = super::format_signature(
            SignatureKind::Function {
                name: &name,
                generics: &[],
                params: &[],
                ret: &Type::Boolean,
                doc: None,
                predicate: Some(&predicate),
            },
            &TypeParamSubstitution::new(),
        );
        assert!(matches!(result, Err(RenderError::InvalidMetadata(_))));
    }

    #[test]
    fn function_no_generics_no_params() {
        let out = format_signature(
            SignatureKind::Function {
                name: "main",
                generics: &[],
                params: &[],
                ret: &Type::Void,
                doc: None,
                predicate: None,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "function main(): void");
    }

    #[test]
    fn function_with_named_params() {
        let out = format_signature(
            SignatureKind::Function {
                name: "id",
                generics: &["T".to_string()],
                params: &[Param::new("x", Type::TypeVar("T".to_string()))],
                ret: &Type::TypeVar("T".to_string()),
                doc: None,
                predicate: None,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "function id<T>(x: T): T");
    }

    #[test]
    fn function_multi_param_and_generic_named() {
        let out = format_signature(
            SignatureKind::Function {
                name: "pair",
                generics: &["T".to_string(), "U".to_string()],
                params: &[
                    Param::new("a", Type::TypeVar("T".to_string())),
                    Param::new("b", Type::TypeVar("U".to_string())),
                ],
                ret: &Type::TypeVar("T".to_string()),
                doc: None,
                predicate: None,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "function pair<T, U>(a: T, b: U): T");
    }

    #[test]
    fn function_falls_back_to_positional_when_names_empty() {
        let out = format_signature(
            SignatureKind::Function {
                name: "string_concat",
                generics: &[],
                params: &[Param::anon(Type::String), Param::anon(Type::String)],
                ret: &Type::String,
                doc: None,
                predicate: None,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "function string_concat(string, string): string");
    }

    #[test]
    fn anon_function() {
        let out = format_signature(
            SignatureKind::Anon {
                params: &[Type::Number, Type::String],
                ret: &Type::Boolean,
                has_rest: false,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "(arg0: number, arg1: string) => boolean");
    }

    /// A dropped `has_rest` renders a *different type* than the one the message
    /// is about — the arity message says `1+` while the lift shows a fixed arity.
    #[test]
    fn anon_function_with_rest() {
        let out = format_signature(
            SignatureKind::Anon {
                params: &[Type::Number, Type::Array(Box::new(Type::Number))],
                ret: &Type::Number,
                has_rest: true,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(out, "(arg0: number, ...arg1: number[]) => number");
    }

    #[test]
    fn method_no_generics() {
        let sig = MethodSig {
            generics: vec![],
            params: vec![],
            ret: Type::String,
            predicate: None,
            doc: None,
        };
        let sub = TypeParamSubstitution::new();
        let out = format_signature(
            SignatureKind::Method {
                receiver_ty: &Type::Number,
                name: "toString",
                sig: &sig,
            },
            &sub,
        );
        assert_eq!(out, "number.toString(): string");
    }

    #[test]
    fn method_substitutes_interface_generic_with_named_param() {
        let sig = MethodSig {
            generics: vec!["U".to_string()],
            params: vec![Param::new(
                "fn",
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    ret: Box::new(Type::TypeVar("U".to_string())),
                    predicate: None,
                    has_rest: false,
                },
            )],
            ret: Type::Array(Box::new(Type::TypeVar("U".to_string()))),
            predicate: None,
            doc: None,
        };
        let sub = TypeParamSubstitution::from_pairs(&["T".to_string()], &[Type::Number]);
        let out = format_signature(
            SignatureKind::Method {
                receiver_ty: &Type::Array(Box::new(Type::Number)),
                name: "map",
                sig: &sig,
            },
            &sub,
        );
        assert_eq!(out, "number[].map<U>(fn: (arg0: number) => U): U[]");
    }

    #[test]
    fn method_on_interface_ref_multi_arg_with_named_param() {
        let sig = MethodSig {
            generics: vec![],
            params: vec![Param::new("key", Type::TypeVar("K".to_string()))],
            ret: Type::TypeVar("V".to_string()),
            predicate: None,
            doc: None,
        };
        let receiver = Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![Type::String, Type::Number],
        };
        let sub = TypeParamSubstitution::from_pairs(
            &["K".to_string(), "V".to_string()],
            &[Type::String, Type::Number],
        );
        let out = format_signature(
            SignatureKind::Method {
                receiver_ty: &receiver,
                name: "get",
                sig: &sig,
            },
            &sub,
        );
        assert_eq!(out, "Map<string, number>.get(key: string): number");
    }

    #[test]
    fn method_falls_back_to_positional_when_names_empty() {
        let sig = MethodSig {
            generics: vec![],
            params: vec![Param::anon(Type::TypeVar("K".to_string()))],
            ret: Type::TypeVar("V".to_string()),
            predicate: None,
            doc: None,
        };
        let receiver = Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![Type::String, Type::Number],
        };
        let sub = TypeParamSubstitution::from_pairs(
            &["K".to_string(), "V".to_string()],
            &[Type::String, Type::Number],
        );
        let out = format_signature(
            SignatureKind::Method {
                receiver_ty: &receiver,
                name: "get",
                sig: &sig,
            },
            &sub,
        );
        assert_eq!(out, "Map<string, number>.get(string): number");
    }

    #[test]
    fn intrinsic_assert() {
        let out = format_signature(
            SignatureKind::Intrinsic {
                kind: Intrinsic::Assert,
            },
            &TypeParamSubstitution::new(),
        );
        assert_eq!(
            out,
            "assert(condition: boolean, message: string = \"assertion failed\"): void"
        );
    }
}
