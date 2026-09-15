use crate::{Package, Type, TypeAnnotation, TypeAnnotationKind, TypeKind, TypeSymbol};

use super::Inferer;

/// An already-resolved class identity in type position: how the source spelled
/// it (`Box`, `ns.Box`) alongside the symbol it resolved to.
struct ClassName {
    display: String,
    package: Package,
    name: String,
    mangled: crate::MangledName,
}
use super::format_definition;
use super::reserved::{is_reserved_object_field, override_field_signature};

/// Which no-value types a [`valueless_within`] scan refuses, and how deep it
/// looks. The two axes are the whole difference between its two wrappers.
#[derive(Clone, Copy)]
struct ValuelessScan {
    include_void: bool,
    /// Refuse `never` as well as `void`. `never` has no values either, but it
    /// is *absorbed* rather than represented — `string | never` collapses to
    /// `string`, and an erased slot typed `never` is simply never written — so
    /// only the type-argument rule, where erasure is physical, refuses it.
    include_never: bool,
    /// Follow into an `InterfaceRef`/`ClassRef`'s type arguments. Whether
    /// `T = void` needs a value slot depends on where the *declaration* puts
    /// `T`: `Sink<void>` is legitimate and supported (its `emit` returns `T`,
    /// and a `void` return has no result slot at all), while `Map<string, void>`
    /// is not. Only the class rule, which erases every argument to a boxed
    /// slot regardless, can refuse them all without a position analysis.
    follow_type_args: bool,
}

/// A position that holds a value, named for the diagnostic. `void` reaches all
/// of these and has no runtime representation in any of them.
#[derive(Clone, Copy)]
pub(super) enum ValuePosition {
    Parameter,
    UnionMember,
    ArrayElement,
    TupleElement,
    FieldType,
    FieldValue,
}

impl std::fmt::Display for ValuePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ValuePosition::Parameter => "a parameter type",
            ValuePosition::UnionMember => "a union member",
            ValuePosition::ArrayElement => "an array element",
            ValuePosition::TupleElement => "a tuple element",
            ValuePosition::FieldType => "a field type",
            ValuePosition::FieldValue => "a field value",
        })
    }
}

/// The first no-value type inside `ty` that would need a value slot, per `rule`.
fn valueless_within<'t>(ty: &'t Type, rule: ValuelessScan) -> Option<&'t Type> {
    let recur = |t: &'t Type| valueless_within(t, rule);
    match ty.peel() {
        t @ Type::Void if rule.include_void => Some(t),
        t @ Type::Never if rule.include_never => Some(t),
        Type::Union(members) => members.iter().find_map(recur),
        Type::Array(elem) => recur(elem),
        Type::Tuple(elems) => elems.iter().find_map(recur),
        // A function's own `void` return is legitimate; only its parameters
        // occupy value slots.
        Type::Function { params, .. } => params.iter().find_map(recur),
        Type::InterfaceRef { args, .. } | Type::ClassRef { args, .. } if rule.follow_type_args => {
            args.iter().find_map(recur)
        }
        _ => None,
    }
}

/// The `void` inside `ty` that would need a value slot — the rule for ordinary
/// value positions (a parameter, a union member, an element, a field).
pub(super) fn void_within_value_position(ty: &Type) -> Option<&Type> {
    valueless_within(
        ty,
        ValuelessScan {
            include_void: true,
            include_never: false,
            follow_type_args: false,
        },
    )
}

/// The first `void`/`never` anywhere inside `ty`, which cannot occupy a value
/// slot and so cannot be erased into one as a type argument.
pub(super) fn valueless_within_type_argument(ty: &Type, include_void: bool) -> Option<&Type> {
    valueless_within(
        ty,
        ValuelessScan {
            include_void,
            include_never: true,
            follow_type_args: true,
        },
    )
}

impl<'a> Inferer<'a> {
    fn resolve_imported_alias_reference(
        &mut self,
        display_name: &str,
        type_name: &str,
        package: Package,
        sym: TypeSymbol,
        arg_annots: &[TypeAnnotation],
        span: crate::Span,
    ) -> Type {
        let TypeKind::Alias { generics, ty, .. } = sym.kind else {
            return Type::Error;
        };
        if arg_annots.len() != generics.len() {
            let plural = if generics.len() == 1 {
                "argument"
            } else {
                "arguments"
            };
            self.error(
                span,
                format!(
                    "type alias `{}` expects {} type {}, got {}",
                    display_name,
                    generics.len(),
                    plural,
                    arg_annots.len(),
                ),
            );
            return Type::Error;
        }

        let resolved_args: Vec<Type> = arg_annots.iter().map(|a| self.resolve_type(a)).collect();
        let body = if generics.is_empty() {
            ty
        } else {
            let sub =
                crate::typechecker::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &generics,
                    &resolved_args,
                );
            sub.apply(&ty)
        };
        Type::alias_ty(
            package,
            type_name.to_string(),
            sym.mangled_name,
            resolved_args,
            Box::new(body),
        )
    }

    fn resolve_imported_type_symbol(
        &mut self,
        display_name: &str,
        package: Package,
        sym: TypeSymbol,
        args: &[TypeAnnotation],
        span: crate::Span,
    ) -> Type {
        let type_name = sym.name.clone();
        let mangled = sym.mangled_name.clone();
        match &sym.kind {
            TypeKind::NumberEnum { .. } => {
                if !args.is_empty() {
                    self.error(span, format!("enum `{display_name}` is not a generic type"));
                    return Type::Error;
                }
                Type::number_enum(package, type_name, mangled)
            }
            TypeKind::StringEnum { .. } => {
                if !args.is_empty() {
                    self.error(span, format!("enum `{display_name}` is not a generic type"));
                    return Type::Error;
                }
                Type::string_enum(package, type_name, mangled)
            }
            TypeKind::Interface { generics, .. } => {
                if args.len() != generics.len() {
                    let header = format_definition::format_interface_header(display_name, generics);
                    let plural = if generics.len() == 1 {
                        "argument"
                    } else {
                        "arguments"
                    };
                    self.error_with_help(
                        span,
                        format!(
                            "interface `{}` expects {} type {}, got {}",
                            display_name,
                            generics.len(),
                            plural,
                            args.len(),
                        ),
                        vec![header],
                    );
                    return Type::Error;
                }
                let resolved_args: Vec<Type> = args.iter().map(|a| self.resolve_type(a)).collect();
                Type::interface_ref(package, type_name, mangled, resolved_args)
            }
            TypeKind::Class { generics, .. } => {
                let generics = generics.clone();
                self.resolve_class_reference(
                    ClassName {
                        display: display_name.to_string(),
                        package,
                        name: type_name,
                        mangled,
                    },
                    &generics,
                    args,
                    span,
                )
            }
            TypeKind::Alias { .. } => self.resolve_imported_alias_reference(
                display_name,
                &type_name,
                package,
                sym,
                args,
                span,
            ),
        }
    }

    /// A class name in type position: arity-check the type arguments against the
    /// class's generics and build the `ClassRef`. `void`/`never` are rejected as
    /// arguments — generics are erased to boxed value slots, and a void
    /// instantiation would give the same method two incompatible physical
    /// shapes (a `(): V` slot is value-returning; `(): void` is not).
    fn resolve_class_reference(
        &mut self,
        class: ClassName,
        generics: &[String],
        args: &[TypeAnnotation],
        span: crate::Span,
    ) -> Type {
        let ClassName {
            display,
            package,
            name,
            mangled,
        } = class;
        let display_name = display.as_str();
        if args.len() != generics.len() {
            if generics.is_empty() {
                self.error(
                    span,
                    format!("class `{display_name}` is not a generic type"),
                );
                return Type::Error;
            }
            let header = format_definition::format_class_header(display_name, generics);
            let plural = if generics.len() == 1 {
                "argument"
            } else {
                "arguments"
            };
            self.error_with_help(
                span,
                format!(
                    "class `{}` expects {} type {}, got {}",
                    display_name,
                    generics.len(),
                    plural,
                    args.len(),
                ),
                vec![header],
            );
            return Type::Error;
        }
        let resolved_args: Vec<Type> = args.iter().map(|a| self.resolve_type(a)).collect();
        for (annot, resolved) in args.iter().zip(&resolved_args) {
            // The argument itself needs a value slot, but an interface or
            // callback inside it may legitimately have void returns.
            if let Some(offender) =
                super::void_type_arguments::invalid_argument(resolved, false, self.resolver())
            {
                self.error(
                    annot.span,
                    format!(
                        "`{offender}` cannot be used as a type argument to class \
                         `{display_name}` — use a value type"
                    ),
                );
                return Type::Error;
            }
        }
        Type::class_ref(package, name, mangled, resolved_args)
    }

    /// [`resolve_type`](Self::resolve_type) for a position that holds a
    /// *value*. `void` (and `never`) have no runtime representation, so a
    /// composite built over one has no lowering — codegen would reach
    /// `value_type called on Void`.
    pub(super) fn resolve_value_type(
        &mut self,
        annot: &TypeAnnotation,
        position: ValuePosition,
    ) -> Type {
        let ty = self.resolve_type(annot);
        if self.reject_void_value(&ty, annot.span, position) {
            return Type::Error;
        }
        ty
    }

    /// Resolve an annotation that has already been resolved once, dropping the
    /// diagnostics that are replays of ones already recorded at the same span.
    ///
    /// The body pass needs the type in the body's generic scope (`T` as a
    /// `GenericParam` rather than a signature `TypeVar`), so it cannot reuse
    /// the stored one — and re-resolving replays every diagnostic the
    /// signature pass raised, including the nested `void` screens inside
    /// [`resolve_type`]'s composite arms. Drop only the replays: `bind_class`
    /// skips an accessor's annotation on a duplicate-member or
    /// duplicate-accessor error, and the body pass is then the *only* pass to
    /// resolve it, so discarding wholesale would lose a real diagnostic.
    pub(super) fn re_resolve_type(&mut self, annot: &TypeAnnotation) -> Type {
        let before = self.diagnostics.len();
        let ty = self.resolve_type(annot);
        for replayed in self.diagnostics.split_off(before) {
            let already_reported = self
                .diagnostics
                .iter()
                .any(|d| d.span == replayed.span && d.message == replayed.message);
            if !already_reported {
                self.diagnostics.push(replayed);
            }
        }
        ty
    }

    /// How many *errors* have been reported so far.
    ///
    /// Callers that suppress a follow-on diagnostic must count errors, not all
    /// diagnostics: an error aborts before codegen, so a warning is the only
    /// thing that can both raise the count and let compilation continue —
    /// which would silence the follow-on and let its subject reach codegen.
    pub(super) fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .count()
    }

    /// Report the "`void` is not a value" diagnostic if `ty` carries one, and
    /// say whether it did — the `bool` is what lets each caller keep only its
    /// own recovery. Type *arguments* word their own message elsewhere; every
    /// other value position, annotated or inferred, lands here.
    pub(super) fn reject_void_value(
        &mut self,
        ty: &Type,
        span: crate::Span,
        position: ValuePosition,
    ) -> bool {
        let Some(offender) = void_within_value_position(ty) else {
            return false;
        };
        self.error_with_help(
            span,
            format!("`{offender}` cannot be {position} — it has no values"),
            vec![format!(
                "`{offender}` is only meaningful as a return type; use a type that has values"
            )],
        );
        true
    }

    pub(super) fn resolve_type(&mut self, annot: &TypeAnnotation) -> Type {
        let resolved = self.resolve_type_inner(annot);
        if matches!(
            resolved.peel(),
            Type::InterfaceRef { .. } | Type::AliasRef { .. }
        ) && let Some(position) =
            super::void_type_arguments::invalid_position(&resolved, false, self.resolver())
        {
            self.error(
                annot.span,
                format!("type argument containing `void` requires {position} — use a value type"),
            );
            return Type::Error;
        }
        resolved
    }

    fn resolve_type_inner(&mut self, annot: &TypeAnnotation) -> Type {
        match &annot.kind {
            TypeAnnotationKind::Name { name_span, args } => {
                let text = &self.source[name_span.start as usize..name_span.end as usize];
                // Body context checked first: `T` in a function body resolves to GenericParam, not the signature TypeVar.
                if let Some(gp) = self.lookup_body_gp(text) {
                    if !args.is_empty() {
                        self.error(
                            annot.span,
                            format!("type parameter `{text}` is not generic"),
                        );
                        return Type::Error;
                    }
                    return gp.clone();
                }
                if self.is_generic_in_scope(text) {
                    if !args.is_empty() {
                        self.error(
                            annot.span,
                            format!("type parameter `{text}` is not generic"),
                        );
                        return Type::Error;
                    }
                    return Type::TypeVar(text.to_string());
                }
                let primitive = match text {
                    "number" => Some(Type::Number),
                    "bigint" => Some(Type::BigInt),
                    "string" => Some(Type::String),
                    "boolean" => Some(Type::Boolean),
                    "void" => Some(Type::Void),
                    "null" => Some(Type::Null),
                    // Resolves to Type::Uint8Array (not InterfaceRef) — the prelude interface exists for method dispatch only.
                    "Uint8Array" => Some(Type::Uint8Array),
                    "unknown" => Some(Type::Unknown),
                    // `any` is rejected in the parser (spec §2.11); resolve to the
                    // poison type so a program that still reaches here doesn't
                    // cascade an "unknown type name `any`" on top of that error.
                    "any" => Some(Type::Error),
                    "never" => Some(Type::Never),
                    _ => None,
                };
                if let Some(prim) = primitive {
                    if !args.is_empty() {
                        self.error(annot.span, format!("`{text}` is not a generic type"));
                        return Type::Error;
                    }
                    return prim;
                }
                // `Array<T>` is the standard-library spelling of `T[]`; desugar to
                // the same internal `Type::Array` so the two are fully
                // interchangeable (indexing, assignability, method dispatch). The
                // prelude `Array` interface exists for method dispatch only, like
                // `Uint8Array`.
                if text == "Array" {
                    if args.len() != 1 {
                        self.error_with_help(
                            annot.span,
                            format!("`Array<T>` expects 1 type argument, got {}", args.len()),
                            vec!["write `Array<T>` or equivalently `T[]`".to_string()],
                        );
                        return Type::Error;
                    }
                    let elem_ty = self.resolve_value_type(&args[0], ValuePosition::ArrayElement);
                    return Type::Array(Box::new(elem_ty));
                }
                if let Some(sym) = self.lookup_named_type(text) {
                    let package = self.type_package(text);
                    // Identity = the declaring symbol's mangled name; no change
                    // needed here, `mangled_name` carries the public/internal form.
                    let mangled = sym.mangled_name.clone();
                    return match &sym.kind {
                        TypeKind::NumberEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Type::Error;
                            }
                            Type::number_enum(package, text.to_string(), mangled)
                        }
                        TypeKind::StringEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Type::Error;
                            }
                            Type::string_enum(package, text.to_string(), mangled)
                        }
                        TypeKind::Interface { generics, .. } => {
                            if args.len() != generics.len() {
                                let header =
                                    format_definition::format_interface_header(text, generics);
                                let plural = if generics.len() == 1 {
                                    "argument"
                                } else {
                                    "arguments"
                                };
                                self.error_with_help(
                                    annot.span,
                                    format!(
                                        "interface `{}` expects {} type {}, got {}",
                                        text,
                                        generics.len(),
                                        plural,
                                        args.len(),
                                    ),
                                    vec![header],
                                );
                                return Type::Error;
                            }
                            let resolved_args: Vec<Type> =
                                args.iter().map(|a| self.resolve_type(a)).collect();
                            Type::interface_ref(package, text.to_string(), mangled, resolved_args)
                        }
                        TypeKind::Class { generics, .. } => {
                            let generics = generics.clone();
                            let name = text.to_string();
                            return self.resolve_class_reference(
                                ClassName {
                                    display: name.clone(),
                                    package,
                                    name,
                                    mangled,
                                },
                                &generics,
                                args,
                                annot.span,
                            );
                        }
                        // Type-alias use site. Resolution is on-demand (the
                        // body may not be resolved yet) and cycle-aware (a
                        // self / mutual reference mid-resolution yields a lazy
                        // `AliasRef`). That lives in `resolve_alias_reference`,
                        // which needs `&mut self`, so drop the `sym` borrow and
                        // the source-borrowed `text` first.
                        TypeKind::Alias { .. } => {
                            let name = text.to_string();
                            let arg_annots: Vec<TypeAnnotation> = args.clone();
                            let _ = sym;
                            return self.resolve_alias_reference(&name, &arg_annots, annot.span);
                        }
                    };
                }
                if text == "WeakMap" {
                    self.error_with_help(
                        annot.span,
                        "`WeakMap` is not supported".to_string(),
                        vec![
                            "use `Map<K, V>` instead — Submilli has no weak references, so `WeakMap` would behave identically to `Map`".to_string(),
                        ],
                    );
                    return Type::Error;
                }
                if text == "WeakSet" {
                    self.error_with_help(
                        annot.span,
                        "`WeakSet` is not supported".to_string(),
                        vec![
                            "use `Set<T>` instead — Submilli has no weak references, so `WeakSet` would behave identically to `Set`".to_string(),
                        ],
                    );
                    return Type::Error;
                }
                if text == "Date" {
                    self.error_with_help(
                        annot.span,
                        "`Date` is not supported".to_string(),
                        vec![
                            "use `Temporal.Now.instant()` for wall-clock time, or `Temporal.ZonedDateTime` / `Temporal.Instant` for time values. `Date` is intentionally out of scope — see Temporal for a correct, immutable, timezone-aware time API.".to_string(),
                        ],
                    );
                    return Type::Error;
                }
                if text == "Record" {
                    self.error_with_help(
                        annot.span,
                        "`Record<K, V>` is not supported".to_string(),
                        vec![
                            "use `Map<K, V>` instead — Submilli has no index signatures. e.g. `const m = new Map<string, number>()`".to_string(),
                        ],
                    );
                    return Type::Error;
                }
                let help: Vec<String> = self
                    .closest_type_name(text)
                    .map(|s| vec![format!("did you mean `{}`?", s)])
                    .unwrap_or_default();
                self.error_with_help(annot.span, format!("unknown type `{text}`"), help);
                Type::Error
            }
            TypeAnnotationKind::Qualified { path, args } => {
                debug_assert!(path.len() >= 2, "Qualified path has ≥2 segments");
                let text: String = path
                    .iter()
                    .map(|s| &self.source[s.start as usize..s.end as usize])
                    .collect::<Vec<&str>>()
                    .join(".");
                if let Some(sym) = self.lookup_named_type(&text) {
                    let package = self.type_package(&text);
                    let mangled = sym.mangled_name.clone();
                    return match &sym.kind {
                        TypeKind::NumberEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Type::Error;
                            }
                            Type::number_enum(package, text, mangled)
                        }
                        TypeKind::StringEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Type::Error;
                            }
                            Type::string_enum(package, text, mangled)
                        }
                        TypeKind::Interface { generics, .. } => {
                            if args.len() != generics.len() {
                                let header =
                                    format_definition::format_interface_header(&text, generics);
                                let plural = if generics.len() == 1 {
                                    "argument"
                                } else {
                                    "arguments"
                                };
                                self.error_with_help(
                                    annot.span,
                                    format!(
                                        "interface `{}` expects {} type {}, got {}",
                                        text,
                                        generics.len(),
                                        plural,
                                        args.len(),
                                    ),
                                    vec![header],
                                );
                                return Type::Error;
                            }
                            let resolved_args: Vec<Type> =
                                args.iter().map(|a| self.resolve_type(a)).collect();
                            Type::interface_ref(package, text, mangled, resolved_args)
                        }
                        TypeKind::Class { generics, .. } => {
                            let generics = generics.clone();
                            return self.resolve_class_reference(
                                ClassName {
                                    display: text.clone(),
                                    package,
                                    name: text,
                                    mangled,
                                },
                                &generics,
                                args,
                                annot.span,
                            );
                        }
                        TypeKind::Alias { .. } => {
                            let arg_annots: Vec<TypeAnnotation> = args.clone();
                            let _ = sym;
                            return self.resolve_alias_reference(&text, &arg_annots, annot.span);
                        }
                    };
                }
                let root = &self.source[path[0].start as usize..path[0].end as usize];
                if let Some(ns) = self.namespace_bindings.get(root) {
                    let package_name = ns.members.package_name().to_string();
                    let member_name = path[1..]
                        .iter()
                        .map(|s| &self.source[s.start as usize..s.end as usize])
                        .collect::<Vec<&str>>()
                        .join(".");
                    if let Some(sym) = ns.members.type_symbol(&member_name).cloned() {
                        return self.resolve_imported_type_symbol(
                            &text,
                            Package(package_name),
                            sym,
                            args,
                            annot.span,
                        );
                    }
                    let help = ns.members.exports_help();
                    self.error_with_help(
                        annot.span,
                        format!("package `{package_name}` does not export type `{member_name}`"),
                        help,
                    );
                    return Type::Error;
                }
                self.error(annot.span, format!("unknown type `{text}`"));
                Type::Error
            }
            TypeAnnotationKind::Array(elem) => {
                let elem_ty = self.resolve_value_type(elem, ValuePosition::ArrayElement);
                Type::Array(Box::new(elem_ty))
            }
            TypeAnnotationKind::Tuple(elements) => {
                let resolved: Vec<Type> = elements
                    .iter()
                    .map(|e| self.resolve_value_type(e, ValuePosition::TupleElement))
                    .collect();
                Type::Tuple(resolved)
            }
            TypeAnnotationKind::Object { fields } => {
                let mut resolved: std::collections::BTreeMap<String, crate::ObjectField> =
                    std::collections::BTreeMap::new();
                for field in fields {
                    if is_reserved_object_field(&field.name.name) {
                        self.error(
                            field.name.span,
                            super::reserved::reserved_field_message(&field.name.name),
                        );
                        return Type::Error;
                    }
                    let ty = self.resolve_value_type(&field.ty, ValuePosition::FieldType);
                    // Optional override fields rejected — would require null-check on every dispatch.
                    if let Some(expected) = override_field_signature(&field.name.name) {
                        if field.optional {
                            self.error(
                                field.name.span,
                                format!("`{}` cannot be optional", field.name.name),
                            );
                            return Type::Error;
                        }
                        if !super::assignable(&ty, &expected, self.resolver()) {
                            self.error(
                                field.name.span,
                                format!(
                                    "field `{}` must have type `{}` (got `{}`)",
                                    field.name.name, expected, ty,
                                ),
                            );
                            return Type::Error;
                        }
                    }
                    resolved
                        .entry(field.name.name.clone())
                        .or_insert(crate::ObjectField {
                            ty,
                            optional: field.optional,
                            readonly: field.readonly,
                        });
                }
                Type::Object { fields: resolved }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                self.report_duplicate_params(params.iter().map(|f| &f.name));
                let resolved_params: Vec<Type> = params
                    .iter()
                    .map(|f| self.resolve_value_type(&f.ty, ValuePosition::Parameter))
                    .collect();
                // The return position is the one place `void` belongs.
                let resolved_ret = self.resolve_type(return_type);
                let has_rest = params.last().is_some_and(|f| f.rest);
                Type::Function {
                    params: resolved_params,
                    ret: Box::new(resolved_ret),
                    // Predicate types not supported in function-type annotations (only in function/arrow bodies).
                    predicate: None,
                    has_rest,
                }
            }
            TypeAnnotationKind::Union(members) => {
                let resolved: Vec<Type> = members
                    .iter()
                    .map(|m| self.resolve_value_type(m, ValuePosition::UnionMember))
                    .collect();
                Type::union(resolved)
            }
            TypeAnnotationKind::StringLiteral(s) => Type::StringLiteral(s.clone()),
            TypeAnnotationKind::NumberLiteral(v) => Type::NumberLiteral(*v),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::test_support::{run, run_with_packages};
    use crate::{
        Dispatch, PackageDeclaration, Param, Span, Type, TypeKind, TypeSymbol, ValueKind,
        ValueSymbol,
    };

    fn namespace_type_test_package() -> PackageDeclaration {
        let mut package = PackageDeclaration::with_package("test:ns");
        let search_result = TypeSymbol {
            name: "SearchResult".to_string(),
            mangled_name: crate::mangle::package_symbol("test:ns", "SearchResult"),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties: BTreeMap::new(),
                dispatch: Dispatch::VTable,
                doc: None,
            },
        };
        let box_type = TypeSymbol {
            name: "Box".to_string(),
            mangled_name: crate::mangle::package_symbol("test:ns", "Box"),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Interface {
                generics: vec!["T".to_string()],
                methods: BTreeMap::new(),
                properties: BTreeMap::new(),
                dispatch: Dispatch::VTable,
                doc: None,
            },
        };
        let result_list = TypeSymbol {
            name: "ResultList".to_string(),
            mangled_name: crate::mangle::package_symbol("test:ns", "ResultList"),
            declaration_span: Span::at(crate::FileId(0)),
            kind: TypeKind::Alias {
                generics: Vec::new(),
                ty: Type::Array(Box::new(Type::interface_ref(
                    crate::Package("test:ns".to_string()),
                    "SearchResult",
                    crate::mangle::package_symbol("test:ns", "SearchResult"),
                    Vec::new(),
                ))),
                doc: None,
            },
        };
        package
            .types
            .insert("SearchResult".to_string(), search_result.clone());
        package.types.insert("Box".to_string(), box_type);
        package.types.insert("ResultList".to_string(), result_list);
        package.values.insert(
            "searchJson".to_string(),
            ValueSymbol {
                name: "searchJson".to_string(),
                mangled_name: crate::mangle::package_symbol("test:ns", "searchJson"),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("query", Type::String)],
                    ret: Type::Array(Box::new(Type::interface_ref(
                        crate::Package("test:ns".to_string()),
                        "SearchResult",
                        search_result.mangled_name,
                        Vec::new(),
                    ))),
                    type_predicate: None,
                    doc: None,
                },
            },
        );
        package
    }

    #[test]
    fn qualified_unknown_type_diagnoses() {
        let (_ta, diags) = run("function main(x: NoSuchNs.Foo): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unknown type `NoSuchNs.Foo`")),
            "expected unknown-type diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn qualified_three_level_unknown_diagnoses() {
        let (_ta, diags) = run("function main(x: Foo.Bar.Baz): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unknown type `Foo.Bar.Baz`")),
            "expected three-level unknown-type diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn namespace_imported_interface_type_resolves() {
        let package = namespace_type_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(): void {
                 const r: ns.SearchResult[] = ns.searchJson("hello");
               }"#,
            &[&package],
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn namespace_imported_generic_interface_arity_diagnoses() {
        let package = namespace_type_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(x: ns.Box): void { }"#,
            &[&package],
        );
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("interface `ns.Box` expects 1 type argument, got 0")),
            "expected arity diagnostic, got: {diags:?}",
        );
        assert!(
            diags
                .iter()
                .flat_map(|d| d.help.iter())
                .any(|h| h.contains("interface ns.Box<T>")),
            "expected qualified interface header, got: {diags:?}",
        );
    }

    #[test]
    fn namespace_imported_alias_type_resolves() {
        let package = namespace_type_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(xs: ns.ResultList): void { }"#,
            &[&package],
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn namespace_imported_missing_type_diagnoses_exports() {
        let package = namespace_type_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(x: ns.Missing): void { }"#,
            &[&package],
        );
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("package `test:ns` does not export type `Missing`")),
            "expected namespace type diagnostic, got: {diags:?}",
        );
        assert!(
            diags
                .iter()
                .flat_map(|d| d.help.iter())
                .any(|h| h.contains("`SearchResult`") && h.contains("`searchJson`")),
            "expected mixed namespace export help, got: {diags:?}",
        );
    }

    #[test]
    fn named_imported_type_still_resolves() {
        let package = namespace_type_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import { SearchResult, searchJson } from "test:ns";
               function main(): void {
                 const r: SearchResult[] = searchJson("hello");
               }"#,
            &[&package],
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn generic_interface_annotation_resolves_with_args() {
        let (ta, diags) = run("interface Foo<T> { x: T; } \
             function main(f: Foo<number>): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("main").unwrap().kind {
            ValueKind::Function { params, .. } => match &params[0].ty {
                Type::InterfaceRef { name, args, .. } => {
                    assert_eq!(name, "Foo");
                    assert_eq!(args, &[Type::Number]);
                }
                other => panic!("expected InterfaceRef, got {other:?}"),
            },
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn generic_interface_arity_too_few_diagnoses() {
        let (_ta, diags) = run("interface Foo<T, U> { x: T; y: U; } \
             function main(f: Foo<number>): void { }");
        let msg = &diags[0].message;
        assert!(
            msg.contains("interface `Foo` expects 2 type arguments, got 1"),
            "unexpected message: {msg}"
        );
        assert!(
            diags[0]
                .help
                .iter()
                .any(|h| h.contains("interface Foo<T, U>")),
            "expected help with interface header, got {:?}",
            diags[0].help
        );
    }

    #[test]
    fn generic_interface_arity_too_many_diagnoses() {
        let (_ta, diags) = run("interface Foo<T> { x: T; } \
             function main(f: Foo<number, string>): void { }");
        let msg = &diags[0].message;
        assert!(
            msg.contains("interface `Foo` expects 1 type argument, got 2"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn generic_interface_missing_args_diagnoses() {
        let (_ta, diags) = run("interface Foo<T> { x: T; } \
             function main(f: Foo): void { }");
        let msg = &diags[0].message;
        assert!(
            msg.contains("interface `Foo` expects 1 type argument, got 0"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn args_on_non_generic_interface_diagnoses() {
        let (_ta, diags) = run("interface Foo { } \
             function main(f: Foo<number>): void { }");
        let msg = &diags[0].message;
        assert!(
            msg.contains("interface `Foo` expects 0 type arguments, got 1"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn args_on_primitive_diagnoses() {
        let (_ta, diags) = run("function main(x: number<string>): void { }");
        let msg = &diags[0].message;
        assert!(
            msg.contains("`number` is not a generic type"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn generic_interface_propagates_inner_unknown() {
        let (_ta, diags) = run("interface Foo<T> { x: T; } \
             function main(f: Foo<unknown_type>): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unknown type `unknown_type`")),
            "expected unknown-type diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn generic_interface_nested_args_resolve() {
        let (ta, diags) = run("interface Foo<T> { x: T; } \
             function main(f: Foo<number[]>): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        if let ValueKind::Function { params, .. } = &reg.values.get("main").unwrap().kind {
            match &params[0].ty {
                Type::InterfaceRef { args, .. } => {
                    assert_eq!(args, &[Type::Array(Box::new(Type::Number))]);
                }
                other => panic!("expected InterfaceRef, got {other:?}"),
            }
        }
    }

    #[test]
    fn array_generic_spelling_desugars_to_array() {
        // `Array<number>` resolves to the identical internal type as `number[]`.
        let (_ta, diags) = run("function main(): void { const a: Array<number> = [1]; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn array_generic_spelling_param_resolves_to_array() {
        let (ta, diags) = run("function f(a: Array<number>): void { } function main(): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { params, .. } => {
                assert_eq!(params[0].ty, Type::Array(Box::new(Type::Number)));
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn array_generic_spelling_wrong_arity_diagnoses() {
        let (_ta, diags) = run("function main(): void { const a: Array<number, string> = []; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("`Array<T>` expects 1 type argument, got 2")),
            "expected arity diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn generic_interface_type_var_arg_resolves() {
        let (ta, diags) = run("interface Foo<T> { x: T; } \
             function f<T>(a: Foo<T>): void { } \
             function main(): void { }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { params, .. } => match &params[0].ty {
                Type::InterfaceRef { name, args, .. } => {
                    assert_eq!(name, "Foo");
                    assert_eq!(args.len(), 1);
                    assert!(
                        matches!(&args[0], Type::TypeVar(n) if n == "T"),
                        "expected TypeVar(T), got {:?}",
                        args[0],
                    );
                }
                other => panic!("expected InterfaceRef, got {other:?}"),
            },
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn unknown_type_in_param_diagnoses() {
        let (ta, diags) = run("function f(a: foo): void { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unknown type `foo`");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { params, .. } => {
                assert_eq!(params, &[Param::new("a", Type::Error)]);
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn unknown_type_in_return_diagnoses() {
        let (ta, diags) = run("function f(): foo { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unknown type `foo`");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { ret, .. } => assert_eq!(*ret, Type::Error),
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn unknown_type_in_let_diagnoses() {
        let (ta, diags) = run("let x: foo = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unknown type `foo`");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("x").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Error),
            _ => panic!("expected Let"),
        }
    }

    // self-referential / forward / mutually-recursive types
    // resolve without an `unknown type` diagnostic, and the cycle
    // detector terminates (the test completing at all proves no hang).

    #[test]
    fn self_referential_interface_resolves() {
        let (_ta, diags) = run("interface Node { value: number; next: Node | null; } \
             function main(): void { const n: Node | null = null; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn self_referential_type_alias_resolves() {
        let (_ta, diags) = run("type Node = { value: number; next: Node | null }; \
             function main(): void { const n: Node | null = null; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn forward_referenced_alias_resolves_regardless_of_order() {
        // `A` references `B` declared after it — order-independent now.
        let (_ta, diags) = run("type A = B[]; type B = number; \
             function main(): void { const a: A = [1, 2]; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn mutually_recursive_interfaces_resolve() {
        let (_ta, diags) = run("interface Branch { leaf: Leaf | null; } \
             interface Leaf { value: number; parent: Branch | null; } \
             function main(): void { const b: Branch | null = null; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn recursive_union_alias_resolves() {
        let (_ta, diags) = run("type Json = number | string | Json[]; \
             function main(): void { const j: Json[] = [1, \"x\"]; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn degenerate_self_cycle_terminates() {
        // `type A = A` is degenerate but must not hang the resolver.
        let (_ta, _diags) = run("type A = A; function main(): void { }");
    }

    /// A package exporting a generic class — the shape the built-in Map/Set
    /// flip produces, exercised here independently of user-class syntax.
    fn generic_class_test_package() -> PackageDeclaration {
        use crate::MethodSig;
        let mut package = PackageDeclaration::with_package("test:ns");
        let mangled = crate::mangle::package_symbol("test:ns", "Crate");
        let t = || Type::TypeVar("T".to_string());
        let mut methods = BTreeMap::new();
        methods.insert(
            "get".to_string(),
            MethodSig {
                generics: Vec::new(),
                params: Vec::new(),
                ret: t(),
                predicate: None,
                doc: None,
            },
        );
        methods.insert(
            "put".to_string(),
            MethodSig {
                generics: Vec::new(),
                params: vec![Param::new("v", t())],
                ret: Type::Void,
                predicate: None,
                doc: None,
            },
        );
        package.types.insert(
            "Crate".to_string(),
            TypeSymbol {
                name: "Crate".to_string(),
                mangled_name: mangled,
                declaration_span: Span::at(crate::FileId(0)),
                kind: TypeKind::Class {
                    generics: vec!["T".to_string()],
                    fields: BTreeMap::new(),
                    methods,
                    method_visibility: BTreeMap::new(),
                    accessors: Vec::new(),
                    constructor: vec![Param::new("v", t())],
                    statics: BTreeMap::new(),
                    static_visibility: BTreeMap::new(),
                    static_fields: BTreeMap::new(),
                    extends: None,
                    implements: Vec::new(),
                    doc: None,
                },
            },
        );
        package
    }

    #[test]
    fn package_generic_class_resolves_and_substitutes() {
        let package = generic_class_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import { Crate } from "test:ns";
               function main(): void {
                 const c = new Crate("x");
                 const s: string = c.get();
                 c.put(s + "!");
                 const explicit: Crate<number> = new Crate<number>(1);
                 const n: number = explicit.get() + 1;
               }"#,
            &[&package],
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn package_generic_class_arity_diagnoses_with_header() {
        let package = generic_class_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(x: ns.Crate): void { }"#,
            &[&package],
        );
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("class `ns.Crate` expects 1 type argument, got 0")),
            "expected arity diagnostic, got: {diags:?}",
        );
        assert!(
            diags
                .iter()
                .flat_map(|d| d.help.iter())
                .any(|h| h.contains("class ns.Crate<T>")),
            "expected class header help, got: {diags:?}",
        );
    }

    #[test]
    fn package_generic_class_instanceof_bare_name() {
        let package = generic_class_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import { Crate } from "test:ns";
               function main(u: unknown): void {
                 if (u instanceof Crate) {
                   const inner: unknown = u.get();
                 }
               }"#,
            &[&package],
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn package_generic_class_void_type_arg_diagnoses() {
        let package = generic_class_test_package();
        let (_ta, diags) = run_with_packages(
            r#"import { Crate } from "test:ns";
               function main(x: Crate<void> | null): void { }"#,
            &[&package],
        );
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("`void` cannot be used as a type argument to class `Crate`")),
            "expected void-arg diagnostic, got: {diags:?}",
        );
    }
}
