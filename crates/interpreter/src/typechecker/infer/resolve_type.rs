use crate::compiler_error::CompilerFailure;

use crate::{Package, Type, TypeAnnotation, TypeAnnotationKind, TypeKind, TypeSymbol};

use super::Inferer;
use super::format_definition;
use super::reserved::{is_reserved_object_field, override_field_signature};
use super::void_value::ValuePosition;

/// An already-resolved class identity in type position: how the source spelled
/// it (`Box`, `ns.Box`) alongside the symbol it resolved to.
struct ClassName {
    display: String,
    package: Package,
    name: String,
    mangled: crate::MangledName,
}

/// `readonly` around a resolved array or tuple. A poisoned operand stays poisoned
/// rather than becoming a readonly wrapper around nothing.
fn readonly_of(operand: Type) -> Type {
    match operand {
        Type::Array(_) | Type::Tuple(_) => Type::Readonly(Box::new(operand)),
        other => other,
    }
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
    ) -> Result<Type, CompilerFailure> {
        let TypeKind::Alias { generics, ty, .. } = sym.kind else {
            return Ok(Type::Error);
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
            return Ok(Type::Error);
        }

        let resolved_args: Vec<Type> = arg_annots
            .iter()
            .map(|a| self.resolve_type(a))
            .collect::<Result<_, _>>()?;
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
        Ok(Type::alias_ty(
            package,
            type_name.to_string(),
            sym.mangled_name,
            resolved_args,
            Box::new(body),
        ))
    }

    fn resolve_imported_type_symbol(
        &mut self,
        display_name: &str,
        package: Package,
        sym: TypeSymbol,
        args: &[TypeAnnotation],
        span: crate::Span,
    ) -> Result<Type, CompilerFailure> {
        let type_name = sym.name.clone();
        let mangled = sym.mangled_name.clone();
        Ok(match &sym.kind {
            TypeKind::NumberEnum { .. } => {
                if !args.is_empty() {
                    self.error(span, format!("enum `{display_name}` is not a generic type"));
                    return Ok(Type::Error);
                }
                Type::number_enum(package, type_name, mangled)
            }
            TypeKind::StringEnum { .. } => {
                if !args.is_empty() {
                    self.error(span, format!("enum `{display_name}` is not a generic type"));
                    return Ok(Type::Error);
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
                    return Ok(Type::Error);
                }
                let resolved_args: Vec<Type> = args
                    .iter()
                    .map(|a| self.resolve_type(a))
                    .collect::<Result<_, _>>()?;
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
                )?
            }
            TypeKind::Alias { .. } => self.resolve_imported_alias_reference(
                display_name,
                &type_name,
                package,
                sym,
                args,
                span,
            )?,
        })
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
    ) -> Result<Type, CompilerFailure> {
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
                return Ok(Type::Error);
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
            return Ok(Type::Error);
        }
        let resolved_args: Vec<Type> = args
            .iter()
            .map(|a| self.resolve_type(a))
            .collect::<Result<_, _>>()?;
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
                return Ok(Type::Error);
            }
        }
        Ok(Type::class_ref(package, name, mangled, resolved_args))
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
    pub(super) fn re_resolve_type(
        &mut self,
        annot: &TypeAnnotation,
    ) -> Result<Type, CompilerFailure> {
        let before = self.diagnostics.len();
        let ty = self.resolve_type(annot)?;
        for replayed in self.diagnostics.split_off(before) {
            let already_reported = self
                .diagnostics
                .iter()
                .any(|d| d.span == replayed.span && d.message == replayed.message);
            if !already_reported {
                self.diagnostics.push(replayed);
            }
        }
        Ok(ty)
    }

    pub(super) fn resolve_type(&mut self, annot: &TypeAnnotation) -> Result<Type, CompilerFailure> {
        let resolved = self.resolve_type_inner(annot)?;
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
            return Ok(Type::Error);
        }
        if let Some(index) = self.resolver().index_signature(&resolved)
            && let Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } = resolved.peel()
            && let Some(fields) = self.resolver().interface_full_form(mangled, name, args)
        {
            self.check_index_fields(&fields, Some(&index), annot.span);
        }
        Ok(resolved)
    }

    pub(super) fn resolve_type_inner(
        &mut self,
        annot: &TypeAnnotation,
    ) -> Result<Type, CompilerFailure> {
        Ok(match &annot.kind {
            TypeAnnotationKind::Name { name, args } => {
                let text = name.name.as_str();
                // Body context checked first: `T` in a function body resolves to GenericParam, not the signature TypeVar.
                if let Some(gp) = self.lookup_body_gp(text) {
                    if !args.is_empty() {
                        self.error(
                            annot.span,
                            format!("type parameter `{text}` is not generic"),
                        );
                        return Ok(Type::Error);
                    }
                    return Ok(gp.clone());
                }
                if self.is_generic_in_scope(text) {
                    if !args.is_empty() {
                        self.error(
                            annot.span,
                            format!("type parameter `{text}` is not generic"),
                        );
                        return Ok(Type::Error);
                    }
                    return Ok(Type::TypeVar(text.to_string()));
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
                        return Ok(Type::Error);
                    }
                    return Ok(prim);
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
                        return Ok(Type::Error);
                    }
                    let elem_ty = self.resolve_value_type(&args[0], ValuePosition::ArrayElement)?;
                    return Ok(Type::Array(Box::new(elem_ty)));
                }
                // `ReadonlyArray<T>` is likewise the library spelling of `readonly T[]`.
                if text == "ReadonlyArray" {
                    if args.len() != 1 {
                        self.error_with_help(
                            annot.span,
                            format!(
                                "`ReadonlyArray<T>` expects 1 type argument, got {}",
                                args.len()
                            ),
                            vec![
                                "write `ReadonlyArray<T>` or equivalently `readonly T[]`"
                                    .to_string(),
                            ],
                        );
                        return Ok(Type::Error);
                    }
                    let elem_ty = self.resolve_value_type(&args[0], ValuePosition::ArrayElement)?;
                    return Ok(readonly_of(Type::Array(Box::new(elem_ty))));
                }
                if let Some(sym) = self.lookup_named_type(text) {
                    let package = self.type_package(text);
                    // Identity = the declaring symbol's mangled name; no change
                    // needed here, `mangled_name` carries the public/internal form.
                    let mangled = sym.mangled_name.clone();
                    return Ok(match &sym.kind {
                        TypeKind::NumberEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Ok(Type::Error);
                            }
                            Type::number_enum(package, text.to_string(), mangled)
                        }
                        TypeKind::StringEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Ok(Type::Error);
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
                                return Ok(Type::Error);
                            }
                            let resolved_args: Vec<Type> = args
                                .iter()
                                .map(|a| self.resolve_type(a))
                                .collect::<Result<_, _>>()?;
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
                    });
                }
                if text == "WeakMap" {
                    self.error_with_help(
                        annot.span,
                        "`WeakMap` is not supported".to_string(),
                        vec![
                            "use `Map<K, V>` instead — Submilli has no weak references, so `WeakMap` would behave identically to `Map`".to_string(),
                        ],
                    );
                    return Ok(Type::Error);
                }
                if text == "WeakSet" {
                    self.error_with_help(
                        annot.span,
                        "`WeakSet` is not supported".to_string(),
                        vec![
                            "use `Set<T>` instead — Submilli has no weak references, so `WeakSet` would behave identically to `Set`".to_string(),
                        ],
                    );
                    return Ok(Type::Error);
                }
                if text == "Date" {
                    self.error_with_help(
                        annot.span,
                        "`Date` is not supported".to_string(),
                        vec![
                            "use `Temporal.Now.instant()` for wall-clock time, or `Temporal.ZonedDateTime` / `Temporal.Instant` for time values. `Date` is intentionally out of scope — see Temporal for a correct, immutable, timezone-aware time API.".to_string(),
                        ],
                    );
                    return Ok(Type::Error);
                }
                if text == "Record" {
                    return self.resolve_record(args, annot.span);
                }
                let help: Vec<String> = self
                    .closest_type_name(text)
                    .map(|s| vec![format!("did you mean `{}`?", s)])
                    .unwrap_or_default();
                self.error_with_help(annot.span, format!("unknown type `{text}`"), help);
                Type::Error
            }
            TypeAnnotationKind::Qualified { path, args } => {
                let text: String = path
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(".");
                if let Some(sym) = self.lookup_named_type(&text) {
                    let package = self.type_package(&text);
                    let mangled = sym.mangled_name.clone();
                    return Ok(match &sym.kind {
                        TypeKind::NumberEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Ok(Type::Error);
                            }
                            Type::number_enum(package, text, mangled)
                        }
                        TypeKind::StringEnum { .. } => {
                            if !args.is_empty() {
                                self.error(
                                    annot.span,
                                    format!("enum `{text}` is not a generic type"),
                                );
                                return Ok(Type::Error);
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
                                return Ok(Type::Error);
                            }
                            let resolved_args: Vec<Type> = args
                                .iter()
                                .map(|a| self.resolve_type(a))
                                .collect::<Result<_, _>>()?;
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
                    });
                }
                let Some((root_name, rest)) = path.split_first() else {
                    return Ok(Type::Error);
                };
                let root = root_name.name.as_str();
                if let Some(ns) = self.namespace_bindings.get(root) {
                    let package_name = ns.members.package_name().to_string();
                    let member_name = rest
                        .iter()
                        .map(|s| s.name.as_str())
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
                    return Ok(Type::Error);
                }
                self.error(annot.span, format!("unknown type `{text}`"));
                Type::Error
            }
            TypeAnnotationKind::Array(elem) => {
                let elem_ty = self.resolve_value_type(elem, ValuePosition::ArrayElement)?;
                Type::Array(Box::new(elem_ty))
            }
            TypeAnnotationKind::Tuple(elements) => {
                let resolved: Vec<Type> = elements
                    .iter()
                    .map(|e| self.resolve_value_type(e, ValuePosition::TupleElement))
                    .collect::<Result<_, _>>()?;
                Type::Tuple(resolved)
            }
            TypeAnnotationKind::Readonly(operand) => readonly_of(self.resolve_type(operand)?),
            TypeAnnotationKind::Object { fields, index } => {
                let mut resolved: std::collections::BTreeMap<String, crate::ObjectField> =
                    std::collections::BTreeMap::new();
                for field in fields {
                    if is_reserved_object_field(&field.name.name) {
                        self.error(
                            field.name.span,
                            super::reserved::reserved_field_message(&field.name.name),
                        );
                        return Ok(Type::Error);
                    }
                    let ty = self.resolve_value_type(&field.ty, ValuePosition::FieldType)?;
                    // Optional override fields rejected — would require null-check on every dispatch.
                    if let Some(expected) = override_field_signature(&field.name.name) {
                        if field.optional {
                            self.error(
                                field.name.span,
                                format!("`{}` cannot be optional", field.name.name),
                            );
                            return Ok(Type::Error);
                        }
                        if !super::assignable(&ty, &expected, self.resolver()) {
                            self.error(
                                field.name.span,
                                format!(
                                    "field `{}` must have type `{}` (got `{}`)",
                                    field.name.name, expected, ty,
                                ),
                            );
                            return Ok(Type::Error);
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
                let index = index
                    .as_ref()
                    .map(|annotation| self.resolve_index_signature(annotation))
                    .transpose()?;
                self.check_index_fields(&resolved, index.as_ref(), annot.span);
                Type::Object {
                    index,
                    fields: resolved,
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                self.report_duplicate_params(params.iter().map(|f| &f.name));
                let resolved_params: Vec<Type> = params
                    .iter()
                    .map(|f| self.resolve_value_type(&f.ty, ValuePosition::Parameter))
                    .collect::<Result<_, _>>()?;
                // The return position is the one place `void` belongs.
                let resolved_ret = self.resolve_type(return_type)?;
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
                    .collect::<Result<_, _>>()?;
                Type::union(resolved)
            }
            TypeAnnotationKind::StringLiteral(s) => Type::StringLiteral(s.clone()),
            TypeAnnotationKind::NumberLiteral(v) => Type::NumberLiteral(*v),
            TypeAnnotationKind::BooleanLiteral(b) => Type::BooleanLiteral(*b),
            TypeAnnotationKind::KeyOf(operand) => self.resolve_keyof(operand)?,
            TypeAnnotationKind::TypeOf { path } => self.resolve_typeof(path),
        })
    }

    /// `keyof T` as the union of `T`'s member names, resolved eagerly to string literal
    /// types. Matches TypeScript: methods count as members alongside properties, and
    /// `keyof` of a type with no members is `never` — which `Type::union` already
    /// produces from an empty vector.
    ///
    /// Only concrete operands are supported. `keyof T` for a type parameter has no
    /// eager answer and would need a deferred type node, so it is rejected by name
    /// rather than resolved to something narrower than it should be.
    /// `typeof x` — the type of the value `x`, read from the value namespace.
    ///
    /// Locals shadow globals, as everywhere else. A dotted path walks object fields
    /// from the root value, which is what makes `typeof o.k` work.
    fn resolve_typeof(&mut self, path: &[crate::Ident]) -> Type {
        let Some((root_span, rest)) = path.split_first() else {
            return Type::Error;
        };
        let name = root_span.name.clone();

        let Some(mut ty) = self.lookup_value_type(&name) else {
            self.error(root_span.span, format!("unresolved identifier `{name}`"));
            return Type::Error;
        };

        for seg in rest {
            let field = seg.name.clone();
            let Some(next) = self.field_type_of(&ty, &field) else {
                self.error(seg.span, format!("`{field}` is not a field of `{ty}`"));
                return Type::Error;
            };
            ty = next;
        }
        ty
    }

    /// The declared type of a value, locals shadowing globals.
    fn lookup_value_type(&self, name: &str) -> Option<Type> {
        if let Some(local) = self.scopes.get(name) {
            return Some(local.ty.clone());
        }
        Some(match &self.top_symbols.get(name)?.kind {
            crate::ValueKind::Let { ty, .. } | crate::ValueKind::Const { ty, .. } => ty.clone(),
            crate::ValueKind::Function {
                params,
                ret,
                type_predicate,
                ..
            } => Type::Function {
                params: params.iter().map(|p| p.ty.clone()).collect(),
                ret: Box::new(ret.clone()),
                predicate: type_predicate.clone().map(Box::new),
                has_rest: params.last().is_some_and(|p| p.rest),
            },
        })
    }

    /// One field read for a `typeof a.b` path. Object types carry their fields
    /// directly; an interface reference resolves through its declaring symbol.
    fn field_type_of(&self, ty: &Type, field: &str) -> Option<Type> {
        match ty.peel() {
            Type::Object { fields, index } => fields
                .get(field)
                .map(crate::types::ObjectField::read_ty)
                .or_else(|| index.as_ref().map(crate::IndexSignature::read_ty)),
            Type::InterfaceRef { .. } => {
                let (mangled, _package, name, _args) = ty.interface_routing()?;
                let sym = self.lookup_structural_type(&mangled, name)?;
                let TypeKind::Interface { properties, .. } = &sym.kind else {
                    return None;
                };
                properties
                    .get(field)
                    .map(|p| crate::ObjectField::widen_optional(p.optional, p.ty.clone()))
            }
            _ => None,
        }
    }

    fn resolve_keyof(&mut self, operand: &TypeAnnotation) -> Result<Type, CompilerFailure> {
        let resolved = self.resolve_value_type(operand, ValuePosition::UnionMember)?;
        if self.resolver().index_signature(&resolved).is_some() {
            return Ok(Type::String);
        }
        let Some(names) = self.member_names_of(&resolved) else {
            if !matches!(resolved.peel(), Type::Error) {
                self.error(
                    operand.span,
                    format!("`keyof` needs an object type or interface, got `{resolved}`"),
                );
            }
            return Ok(Type::Error);
        };
        Ok(Type::union(
            names.into_iter().map(Type::StringLiteral).collect(),
        ))
    }

    /// The member names `keyof` reports, or `None` if the type has no member list to
    /// read. Interfaces flatten inherited members at declaration time, so the two maps
    /// on the symbol are the whole surface.
    fn member_names_of(&self, ty: &Type) -> Option<Vec<String>> {
        match ty.peel() {
            Type::Object { fields, .. } => Some(fields.keys().cloned().collect()),
            Type::InterfaceRef { .. } => {
                let (mangled, _package, name, _args) = ty.interface_routing()?;
                let sym = self.lookup_structural_type(&mangled, name)?;
                let TypeKind::Interface {
                    methods,
                    properties,
                    ..
                } = &sym.kind
                else {
                    return None;
                };
                Some(properties.keys().chain(methods.keys()).cloned().collect())
            }
            _ => None,
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
                index: None,
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
                index: None,
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
    fn keyof_interface_accepts_a_member_name() {
        let (_ta, diags) = run("interface P { a: number; b: number; } type K = keyof P; \
             function main(): void { const k: K = \"a\"; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// The point of `keyof`: a name that is not a member is rejected, so the type is
    /// genuinely narrowed rather than widened to `string`.
    #[test]
    fn keyof_interface_rejects_a_name_that_is_not_a_member() {
        let (_ta, diags) = run("interface P { a: number; b: number; } type K = keyof P; \
             function main(): void { const k: K = \"zzz\"; }");
        assert!(!diags.is_empty(), "`zzz` is not a key of `P`");
    }

    #[test]
    fn keyof_object_type_accepts_a_member_name() {
        let (_ta, diags) = run("type O = { x: number; y: string }; \
             function main(): void { const k: keyof O = \"y\"; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// TypeScript counts methods as members, and the two maps are kept separately.
    #[test]
    fn keyof_includes_method_names() {
        let (_ta, diags) = run("interface I { a: number; m(): void; } \
             function main(): void { const k: keyof I = \"m\"; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// `keyof` of a type with no members is `never`, as in TypeScript.
    #[test]
    fn keyof_of_an_empty_interface_is_never() {
        let (_ta, diags) = run("interface Empty {} \
             function main(): void { const k: keyof Empty = \"x\"; }");
        assert!(
            diags.iter().any(|d| d.message.contains("never")),
            "expected a `never` mismatch: {diags:?}"
        );
    }

    #[test]
    fn keyof_binds_tighter_than_array_suffix() {
        let (_ta, diags) = run("interface P { a: number; } type KS = (keyof P)[]; \
             function main(): void { const ks: KS = [\"a\"]; }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn keyof_rejects_an_operand_with_no_members() {
        let (_ta, diags) = run("function main(): void { const k: keyof number = \"x\"; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("needs an object type or interface")),
            "expected a `keyof` operand diagnostic: {diags:?}"
        );
    }

    /// `keyof` is contextual, not reserved — TypeScript allows it as an identifier.
    #[test]
    fn keyof_is_still_usable_as_an_identifier() {
        let (_ta, diags) = run("function main(): void { const keyof: number = 1; \
             const o = { keyof: 2 }; }");
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
                    narrowing_checks: BTreeMap::new(),
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
