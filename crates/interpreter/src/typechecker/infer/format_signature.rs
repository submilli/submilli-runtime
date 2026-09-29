use std::fmt::Write;

use crate::type_size::TypeLimits;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;
use crate::{DefaultValue, EnumVariantValue, Intrinsic, MethodSig, Param, Type};

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

/// `substitution` is the method lift's type-parameter table, empty for every
/// other kind. It arrives from the caller because only member resolution knows
/// which declaration a signature is written in. A substitution that passes a
/// type limit renders as `Type::Error` and is recorded in `limits`.
pub(super) fn format_signature(
    kind: SignatureKind<'_>,
    substitution: &TypeParamSubstitution,
    limits: &TypeLimits,
) -> String {
    match kind {
        SignatureKind::Constructor { name, params } => {
            let mut out = format!("new {name}(");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_named_param(&mut out, &p.name, &p.ty, p.default.as_ref(), p.rest);
            }
            out.push(')');
            out
        }
        SignatureKind::Function {
            name,
            generics,
            params,
            ret,
            doc,
            predicate,
        } => format_function(name, generics, params, ret, doc, predicate, limits),
        SignatureKind::Method {
            receiver_ty,
            name,
            sig,
        } => format_method(receiver_ty, name, sig, substitution, limits),
        SignatureKind::Anon {
            params,
            ret,
            has_rest,
        } => format_anon(params, ret, has_rest),
        SignatureKind::Intrinsic { kind } => format_intrinsic(kind),
    }
}

fn format_function(
    name: &str,
    generics: &[String],
    params: &[Param],
    ret: &Type,
    doc: Option<&crate::DocComment>,
    predicate: Option<&crate::TypePredicate>,
    limits: &TypeLimits,
) -> String {
    let mut out = String::new();
    if let Some(d) = doc {
        super::format_definition::write_doc_block(&mut out, "", d);
    }
    out.push_str("function ");
    out.push_str(name);
    write_generic_list(&mut out, generics);
    out.push('(');
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_named_param(&mut out, &p.name, &p.ty, p.default.as_ref(), p.rest);
    }
    out.push_str("): ");
    write_return(
        &mut out,
        params,
        ret,
        predicate,
        &TypeParamSubstitution::new(),
        limits,
    );
    out
}

pub(super) fn write_named_param(
    out: &mut String,
    name: &str,
    ty: &Type,
    default: Option<&DefaultValue>,
    rest: bool,
) {
    if rest {
        out.push_str("...");
    }
    if name.is_empty() {
        write!(out, "{ty}").unwrap();
    } else {
        write!(out, "{name}: {ty}").unwrap();
    }
    if let Some(d) = default {
        out.push_str(" = ");
        write_default_value(out, d);
    }
}

pub(super) fn write_default_value(out: &mut String, default: &DefaultValue) {
    match default {
        DefaultValue::Number(n) => out.push_str(&crate::runtime::number::format_number_js(*n)),
        DefaultValue::String(s) => write!(out, "{s:?}").unwrap(),
        DefaultValue::Boolean(b) => write!(out, "{b}").unwrap(),
        DefaultValue::Null => out.push_str("null"),
        DefaultValue::EmptyArray => out.push_str("[]"),
        DefaultValue::EmptyObject => out.push_str("{}"),
        DefaultValue::GlobalConst(m) => out.push_str(short_symbol(m.as_str())),
        DefaultValue::EnumVariant {
            enum_mangled,
            variant,
            ..
        } => write!(out, "{}.{}", short_symbol(enum_mangled.as_str()), variant).unwrap(),
    }
    // EnumVariantValue is consulted only when codegen needs the
    // raw value; rendering uses the source-level name (`Color.Red`).
    let _ = EnumVariantValue::Number(0.0);
}

fn short_symbol(mangled: &str) -> &str {
    mangled.rsplit('#').next().unwrap_or(mangled)
}

fn format_method(
    receiver_ty: &Type,
    name: &str,
    sig: &MethodSig,
    substitution: &TypeParamSubstitution,
    limits: &TypeLimits,
) -> String {
    let mut out = String::new();
    if let Some(d) = &sig.doc {
        super::format_definition::write_doc_block(&mut out, "", d);
    }
    write!(out, "{receiver_ty}.{name}").unwrap();
    write_generic_list(&mut out, &sig.generics);
    out.push('(');
    for (i, p) in sig.params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_named_param(
            &mut out,
            &p.name,
            &substitution.apply_or_record(&p.ty, limits),
            p.default.as_ref(),
            p.rest,
        );
    }
    out.push_str("): ");
    write_return(
        &mut out,
        &sig.params,
        &sig.ret,
        sig.predicate.as_ref(),
        substitution,
        limits,
    );
    out
}

fn write_return(
    out: &mut String,
    params: &[Param],
    ret: &Type,
    predicate: Option<&crate::TypePredicate>,
    substitution: &TypeParamSubstitution,
    limits: &TypeLimits,
) {
    if let Some(predicate) = predicate
        && let Some(param) = params.get(predicate.parameter_index as usize)
    {
        let asserted = substitution.apply_or_record(&predicate.asserted_type, limits);
        out.push_str(&format!("{} is {asserted}", param.name));
        return;
    }
    let ret = substitution.apply_or_record(ret, limits);
    out.push_str(&ret.to_string());
}

/// An anonymous callee has only parameter *types* to lift, so it renders through
/// the same synthesized-name path [`Type`]'s own `Display` uses — which is what
/// keeps the lift and the type it describes spelled identically.
fn format_anon(params: &[Type], ret: &Type, has_rest: bool) -> String {
    let mut out = String::new();
    crate::types::write_synthetic_params(&mut out, params, has_rest).unwrap();
    write!(out, " => {ret}").unwrap();
    out
}

fn format_intrinsic(kind: Intrinsic) -> String {
    // Type::Error stub param renders badly; hardcode the readable form.
    if matches!(kind, Intrinsic::JsonStringify) {
        return "JSON.stringify<T>(value: T, replacer?: null, space?: number | string | null): string"
            .to_string();
    }
    // Type::Error sentinel for per-call return; hardcode the readable form.
    if matches!(kind, Intrinsic::JsonParse) {
        return "JSON.parse(text: string): unknown".to_string();
    }
    let mut out = String::from(kind.name());
    let params = kind.params();
    out.push('(');
    for (i, p) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_named_param(&mut out, &p.name, &p.ty, p.default.as_ref(), p.rest);
    }
    write!(out, "): {}", kind.ret()).unwrap();
    out
}

fn write_generic_list(out: &mut String, generics: &[String]) {
    if generics.is_empty() {
        return;
    }
    out.push('<');
    for (i, g) in generics.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(g);
    }
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders without a type limit being reached; shadows the 3-argument form.
    fn format_signature(kind: SignatureKind<'_>, substitution: &TypeParamSubstitution) -> String {
        let limits = TypeLimits::default();
        let out = super::format_signature(kind, substitution, &limits);
        assert_eq!(limits.take(), Ok(()));
        out
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
