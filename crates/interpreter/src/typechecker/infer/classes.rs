//! Class declarations: signature binding, inheritance validation, and the
//! method/constructor body pass. Mirrors the interface binder
//! ([`super::Inferer::bind_interface`]) for signatures and the function-body
//! pass ([`super::generic::Inferer::infer_functions`]) for bodies. Codegen is a
//! later slice (SUB-480+); this pass only typechecks.

use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use crate::{
    AccessorKind, AccessorSig, ClassMember, FieldSig, Ident, MangledName, MethodSig, Param, Span,
    StmtKind, Type, TypeAnnotation, TypeKind, TypeSymbol, TypedClassAccessor,
    TypedClassConstructor, TypedClassDecl, TypedClassField, TypedClassMethod, TypedParam,
    Visibility,
};

use super::void_value::ValuePosition;
use super::{Inferer, type_limit_at, type_limit_unlocated};
use crate::type_size::{TypeLimits, TypeTooLarge};

use super::assignable::ImplementsFailure;
use super::generic::{
    erase_generic_params, erase_generic_params_in_expr, erase_generic_params_in_stmt,
    substitute_typevars,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeTestMode {
    General,
    AllowAliasRefs,
}

/// A static member found by [`Inferer::class_static_in_chain`].
pub(super) enum StaticResolution {
    Method(MethodSig, Visibility),
    Field(FieldSig),
}

/// One class's `implements` clause, held until every class signature is bound.
pub(in crate::typechecker) struct PendingImplements {
    class_name: String,
    mangled: MangledName,
    generic_names: Vec<String>,
    /// Each target's annotation span and resolved type. Non-interface targets
    /// are already diagnosed and ride along as `Type::Error`.
    targets: Vec<(Span, Type)>,
}

/// What a class's `extends` clause resolved to.
enum ExtendsTarget {
    Absent,
    /// A usable parent. Its member surface is known even when a diagnosed
    /// type-argument error left the args erased.
    Class(crate::ClassExtends),
    /// The clause named something unusable — already diagnosed. Nothing is
    /// known about what the class inherits (see
    /// [`Inferer::unresolved_parents`]).
    Unresolved,
}

/// An *instance* member of a class that has a parent, and so *might* redeclare
/// an inherited name — a method, a field (plain or parameter property), or an
/// accessor. Whether an ancestor actually declares the name, and at which kind,
/// is resolved by the checks themselves.
struct RedeclarationCandidate {
    name: String,
    span: Span,
    child_class: MangledName,
    kind: MemberKind,
    /// For an accessor property, where each half is declared. `get x` and `set
    /// x` are one property and one candidate, but they are checked against the
    /// inherited accessor separately, and each mismatch has to point its caret
    /// at the half that is wrong. Empty for every other kind.
    accessor_spans: BTreeMap<AccessorKind, Span>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MemberKind {
    Method,
    Field,
    Accessor,
}

impl MemberKind {
    /// How the kind is named in a diagnostic.
    fn noun(self) -> &'static str {
        match self {
            MemberKind::Method => "method",
            MemberKind::Field => "field",
            MemberKind::Accessor => "accessor",
        }
    }

    /// The indefinite article [`noun`](Self::noun) takes.
    fn article(self) -> &'static str {
        match self {
            MemberKind::Accessor => "an",
            MemberKind::Method | MemberKind::Field => "a",
        }
    }
}

/// The read and write types of a field-write target. They coincide for data
/// fields and diverge only for an accessor whose getter return type and setter
/// parameter type differ.
pub(super) struct FieldRw {
    pub(super) read: Type,
    pub(super) write: Type,
}

impl FieldRw {
    pub(super) fn uniform(ty: Type) -> Self {
        FieldRw {
            read: ty.clone(),
            write: ty,
        }
    }
}

impl<'a> Inferer<'a> {
    pub(super) fn bind_class(
        &mut self,
        name: Ident,
        generics: Vec<Ident>,
        extends: Option<TypeAnnotation>,
        implements: Vec<TypeAnnotation>,
        members: Vec<ClassMember>,
        doc: Option<crate::DocComment>,
    ) -> Result<(), CompilerFailure> {
        let generic_names: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
        // Push class-level generics so member annotations can name them. Generic
        // classes are not otherwise resolved in v1 (rejected as type args in
        // `resolve_type`), but a `class Box<T> { value: T }` still binds cleanly.
        self.push_signature_generics(generic_names.clone());

        let mangled = self.mangle_top_symbol(&name.name)?;
        let extends_clause = match self.resolve_extends_target(extends.as_ref())? {
            ExtendsTarget::Class(e) => Some(e),
            ExtendsTarget::Absent => None,
            ExtendsTarget::Unresolved => {
                self.unresolved_parents.insert(mangled.clone());
                None
            }
        };
        let implements_types = self.resolve_implements_targets(&implements)?;
        let implements_mangled: Vec<MangledName> = implements_types
            .iter()
            .filter_map(|t| match t.peel() {
                Type::InterfaceRef { mangled, .. } => Some(mangled.clone()),
                _ => None,
            })
            .collect();

        let mut fields: BTreeMap<String, FieldSig> = BTreeMap::new();
        let mut methods: BTreeMap<String, MethodSig> = BTreeMap::new();
        let mut method_visibility: BTreeMap<String, Visibility> = BTreeMap::new();
        let mut statics: BTreeMap<String, MethodSig> = BTreeMap::new();
        let mut static_visibility: BTreeMap<String, Visibility> = BTreeMap::new();
        let mut static_fields: BTreeMap<String, FieldSig> = BTreeMap::new();
        let mut constructor: Vec<Param> = Vec::new();
        let mut seen_constructor = false;
        // Each `get`/`set` is its own declaration. `(property, is_setter)` guards
        // duplicates; the property's `FieldSig` is updated incrementally below.
        let mut accessor_sigs: Vec<AccessorSig> = Vec::new();
        let mut seen_accessor: std::collections::BTreeSet<(String, bool)> =
            std::collections::BTreeSet::new();

        for member in &members {
            match member {
                ClassMember::Field {
                    name: f_name,
                    modifiers,
                    ty,
                    initializer,
                    doc: f_doc,
                    ..
                } if modifiers.static_span.is_some() => {
                    if self.reject_bad_static_name(&statics, &static_fields, f_name, &name) {
                        continue;
                    }
                    if initializer.is_none() {
                        self.error_with_help(
                            f_name.span,
                            "a static field must be initialized".to_string(),
                            vec![format!(
                                "add `= <value>` — there is no constructor moment to assign \
                                 `{}.{}`",
                                name.name, f_name.name
                            )],
                        );
                        continue;
                    }
                    let resolved = self.resolve_value_type(ty, ValuePosition::FieldType)?;
                    if self.reject_class_generic_in_static(
                        f_name,
                        &generic_names,
                        &[],
                        std::iter::once(&resolved),
                    ) {
                        continue;
                    }
                    static_fields.insert(
                        f_name.name.clone(),
                        FieldSig {
                            ty: resolved,
                            visibility: modifiers.visibility,
                            readonly: modifiers.readonly.is_some(),
                            optional: false,
                            doc: f_doc.clone(),
                        },
                    );
                }
                ClassMember::Method {
                    name: m_name,
                    modifiers,
                    generics: m_generics,
                    params,
                    return_type,
                    doc: m_doc,
                    ..
                } if modifiers.static_span.is_some() => {
                    if self.reject_bad_static_name(&statics, &static_fields, m_name, &name) {
                        continue;
                    }
                    let m_generic_names: Vec<String> =
                        m_generics.iter().map(|g| g.name.clone()).collect();
                    self.push_signature_generics(m_generic_names.clone());
                    let resolved_params: Vec<Param> = self.resolve_params(params)?;
                    let resolved_ret = self.resolve_type(return_type)?;
                    self.pop_signature_generics();
                    if self.reject_class_generic_in_static(
                        m_name,
                        &generic_names,
                        &m_generic_names,
                        resolved_params.iter().map(|p| &p.ty).chain([&resolved_ret]),
                    ) {
                        continue;
                    }
                    statics.insert(
                        m_name.name.clone(),
                        MethodSig {
                            generics: m_generic_names,
                            params: resolved_params,
                            ret: resolved_ret,
                            predicate: None,
                            doc: m_doc.clone(),
                        },
                    );
                    static_visibility.insert(m_name.name.clone(), modifiers.visibility);
                }
                ClassMember::Field {
                    name: f_name,
                    modifiers,
                    optional,
                    ty,
                    doc: f_doc,
                    ..
                } => {
                    if self.reject_duplicate_member(&fields, &methods, f_name, &name) {
                        continue;
                    }
                    let resolved = self.resolve_value_type(ty, ValuePosition::FieldType)?;
                    fields.insert(
                        f_name.name.clone(),
                        FieldSig {
                            ty: resolved,
                            visibility: modifiers.visibility,
                            readonly: modifiers.readonly.is_some(),
                            optional: *optional,
                            doc: f_doc.clone(),
                        },
                    );
                }
                ClassMember::Method {
                    name: m_name,
                    modifiers,
                    generics: m_generics,
                    params,
                    return_type,
                    doc: m_doc,
                    ..
                } => {
                    if self.reject_duplicate_member(&fields, &methods, m_name, &name) {
                        continue;
                    }
                    let m_generic_names: Vec<String> =
                        m_generics.iter().map(|g| g.name.clone()).collect();
                    // Method-level generics on class methods have no codegen
                    // (the vtable slot would get a stub body that traps) —
                    // reject at bind time instead of at runtime. Class-level
                    // generics (`class Box<T>`) are fine.
                    if let Some(g) = m_generics.first() {
                        self.error_with_help(
                            g.span,
                            format!(
                                "type parameters on class instance methods are not \
                                 supported yet (`{}<{}>`)",
                                m_name.name, g.name
                            ),
                            vec![format!(
                                "move it to a top-level generic function \
                                 (`function {}<{}>(self: {}, …)`), or make it \
                                 `static {}<{}>(…)` — statics may declare their own \
                                 type parameters",
                                m_name.name,
                                g.name,
                                if generic_names.is_empty() {
                                    name.name.clone()
                                } else {
                                    format!("{}<{}>", name.name, generic_names.join(", "))
                                },
                                m_name.name,
                                g.name
                            )],
                        );
                    }
                    self.push_signature_generics(m_generic_names.clone());
                    let resolved_params: Vec<Param> = self.resolve_params(params)?;
                    let resolved_ret = self.resolve_type(return_type)?;
                    self.pop_signature_generics();
                    // These two names fill the class's universal vtable slots
                    // (used by `String(x)`, interpolation, `JSON.stringify`),
                    // whose funcref shape is fixed at `(): string`.
                    if matches!(m_name.name.as_str(), "toString" | "toJson")
                        && (!resolved_params.is_empty()
                            || !m_generic_names.is_empty()
                            || !matches!(resolved_ret.peel(), crate::Type::String))
                    {
                        self.error_with_help(
                            m_name.span,
                            format!(
                                "class method `{}` must have signature `(): string`",
                                m_name.name
                            ),
                            vec![format!(
                                "`{}` overrides the built-in conversion used by `String(x)`, \
                                 string interpolation, and `JSON.stringify`; declare it as \
                                 `{}(): string` or pick another method name",
                                m_name.name, m_name.name
                            )],
                        );
                    }
                    methods.insert(
                        m_name.name.clone(),
                        MethodSig {
                            generics: m_generic_names,
                            params: resolved_params,
                            ret: resolved_ret,
                            predicate: None,
                            doc: m_doc.clone(),
                        },
                    );
                    method_visibility.insert(m_name.name.clone(), modifiers.visibility);
                }
                ClassMember::Constructor { params, span, .. } => {
                    if seen_constructor {
                        self.error(
                            *span,
                            format!("class `{}` declares more than one constructor", name.name),
                        );
                        continue;
                    }
                    seen_constructor = true;
                    constructor = self.resolve_parameter_types(params)?;
                    // Parameter properties declare a field with the param's type.
                    for (decl, resolved) in params.iter().zip(constructor.iter()) {
                        let Some(modifiers) = &decl.modifiers else {
                            continue;
                        };
                        if self.reject_duplicate_member(&fields, &methods, &decl.name, &name) {
                            continue;
                        }
                        fields.insert(
                            decl.name.name.clone(),
                            FieldSig {
                                ty: resolved.ty.clone(),
                                visibility: modifiers.visibility,
                                readonly: modifiers.readonly.is_some(),
                                optional: false,
                                doc: None,
                            },
                        );
                    }
                }
                ClassMember::Accessor {
                    name: a_name,
                    modifiers,
                    kind,
                    param,
                    return_type,
                    span,
                    ..
                } => {
                    let is_setter = *kind == AccessorKind::Set;
                    // The first accessor for a property guards against a clash with a
                    // field or method of the same name.
                    let first = !seen_accessor.contains(&(a_name.name.clone(), false))
                        && !seen_accessor.contains(&(a_name.name.clone(), true));
                    if !seen_accessor.insert((a_name.name.clone(), is_setter)) {
                        self.error(
                            *span,
                            format!(
                                "duplicate `{}` accessor `{}`",
                                if is_setter { "set" } else { "get" },
                                a_name.name
                            ),
                        );
                        continue;
                    }
                    if first && self.reject_duplicate_member(&fields, &methods, a_name, &name) {
                        continue;
                    }
                    // A getter contributes the read type; a setter makes the property
                    // writable (its independent write type lives on the `AccessorSig`).
                    // The getter/setter read/write types are independent (TS 4.3+).
                    let visibility = modifiers.visibility;
                    match kind {
                        AccessorKind::Get => {
                            let ret_ty = return_type
                                .as_ref()
                                .map(|t| self.resolve_type(t))
                                .transpose()?
                                .unwrap_or(Type::Error);
                            fields
                                .entry(a_name.name.clone())
                                .and_modify(|f| f.ty = ret_ty.clone())
                                .or_insert_with(|| FieldSig {
                                    ty: ret_ty.clone(),
                                    visibility,
                                    readonly: true,
                                    optional: false,
                                    doc: None,
                                });
                            accessor_sigs.push(AccessorSig::Getter {
                                name: a_name.name.clone(),
                                ret_ty,
                            });
                        }
                        AccessorKind::Set => {
                            let write_ty = param
                                .as_ref()
                                .and_then(|p| p.ty.as_ref())
                                .map(|t| self.resolve_value_type(t, ValuePosition::Parameter))
                                .transpose()?
                                .unwrap_or(Type::Error);
                            let param_name = param
                                .as_ref()
                                .map_or_else(|| "value".to_string(), |p| p.name.name.clone());
                            fields
                                .entry(a_name.name.clone())
                                .and_modify(|f| f.readonly = false)
                                .or_insert_with(|| FieldSig {
                                    ty: write_ty.clone(),
                                    visibility,
                                    readonly: false,
                                    optional: false,
                                    doc: None,
                                });
                            accessor_sigs.push(AccessorSig::Setter {
                                name: a_name.name.clone(),
                                param: crate::Param {
                                    name: param_name,
                                    ty: write_ty,
                                    default: None,
                                    rest: false,
                                },
                            });
                        }
                    }
                }
            }
        }

        self.pop_signature_generics();

        self.local_class_mangles.insert(mangled.clone());
        let symbol = TypeSymbol {
            name: name.name.clone(),
            mangled_name: mangled.clone(),
            declaration_span: name.span,
            kind: TypeKind::Class {
                generics: generic_names.clone(),
                fields,
                narrowing_checks: BTreeMap::new(),
                methods,
                method_visibility,
                accessors: accessor_sigs,
                constructor,
                statics,
                static_visibility,
                static_fields,
                extends: extends_clause,
                implements: implements_mangled,
                doc,
            },
        };
        self.types
            .insert(name.name.clone(), self.package_name.to_string(), symbol);

        // `implements` conformance needs every class signature bound — an
        // inherited member may come from a parent declared further down the
        // file — so the check itself is deferred to
        // `check_pending_implements`.
        self.pending_implements.push(PendingImplements {
            class_name: name.name.clone(),
            mangled,
            generic_names,
            targets: implements
                .iter()
                .map(|a| a.span)
                .zip(implements_types)
                .collect(),
        });

        Ok(())
    }

    /// Check that every class satisfies each interface it declares. Runs once
    /// all class signatures are bound, so a member inherited from a
    /// later-declared parent counts.
    pub(super) fn check_pending_implements(&mut self) -> Result<(), CompilerFailure> {
        for pending in std::mem::take(&mut self.pending_implements) {
            // An unresolvable parent could have supplied any of the interface's
            // members, so conformance is unknowable rather than broken.
            if !self.inherits_unresolved_parent(&pending.mangled) {
                self.check_class_implements(&pending)?;
            }
            let first_target = pending.targets.first().map(|(span, _)| *span);
            self.type_size_checkpoint(first_target)?;
        }
        Ok(())
    }

    fn check_class_implements(
        &mut self,
        pending: &PendingImplements,
    ) -> Result<(), CompilerFailure> {
        // A generic class is checked at fresh opaque parameters, applied to
        // both sides, so `Box<T> implements Container<T>` still matches member
        // for member while `Box<T> implements Container<string>` is caught —
        // instantiating with bare `TypeVar`s would make both pass.
        let class_args: Vec<Type> = pending
            .generic_names
            .iter()
            .map(|name| self.fresh_generic_param(name))
            .collect::<Result<_, _>>()?;
        let opaque: BTreeMap<String, Type> = pending
            .generic_names
            .iter()
            .cloned()
            .zip(class_args.iter().cloned())
            .collect();
        for (span, iface_ty) in &pending.targets {
            // Peeled to agree with `resolve_implements_targets`, which admits an
            // aliased interface. An unpeeled match reads `type N = Named` as a
            // non-interface and skips conformance with no diagnostic at all.
            let Type::InterfaceRef {
                mangled: iface_mangled,
                name: iface_name,
                args: iface_args,
                ..
            } = iface_ty.peel()
            else {
                continue;
            };
            let iface_args: Vec<Type> = iface_args
                .iter()
                .map(|a| substitute_typevars(a, &opaque, &self.type_limits))
                .collect::<Result<_, _>>()
                .map_err(type_limit_at(*span))?;
            let failures = self.resolver().implements_failures(
                &pending.mangled,
                &class_args,
                iface_mangled,
                iface_name,
                &iface_args,
            );
            if !failures.is_empty() {
                self.report_implements_failures(*span, &pending.class_name, iface_ty, &failures);
            }
        }
        Ok(())
    }

    /// Render a class→interface conformance failure as an LLM-native diagnostic:
    /// the message names the offending members, and the `help` block lifts the
    /// interface's expected shape plus the class's actual signature for every
    /// member whose type is incompatible.
    fn report_implements_failures(
        &mut self,
        span: Span,
        class_name: &str,
        iface_ty: &Type,
        failures: &[ImplementsFailure],
    ) {
        let mut clauses: Vec<String> = Vec::new();
        let mut help = vec![self.format_definition(iface_ty)];
        for failure in failures {
            match failure {
                ImplementsFailure::Missing(member) => {
                    clauses.push(format!("missing member `{member}`"));
                }
                ImplementsFailure::Incompatible {
                    member,
                    expected,
                    actual,
                } => {
                    clauses.push(format!("member `{member}` has an incompatible signature"));
                    help.push(format!(
                        "`{class_name}.{member}` is `{actual}`, but `{iface_ty}` declares it as `{expected}`"
                    ));
                }
                ImplementsFailure::NotReadable { member, ty } => {
                    clauses.push(format!("member `{member}` is write-only (no getter)"));
                    help.push(format!(
                        "`{iface_ty}` requires a readable `{member}: {ty}`; add a \
                         `get {member}(): {ty}` accessor to `{class_name}`"
                    ));
                }
                ImplementsFailure::NotWritable { member, ty } => {
                    clauses.push(format!("member `{member}` is not writable"));
                    // The interface-side fix leads because it is the one that always
                    // applies: the class member may be a method or a get-only
                    // accessor, neither of which takes a setter the way a field does.
                    help.push(format!(
                        "`{iface_ty}` declares `{member}: {ty}` as settable, but \
                         `{class_name}.{member}` only reads — declare the interface \
                         property `readonly {member}: {ty}`, or make the class member \
                         writable (a non-`readonly` field, or a \
                         `set {member}(value: {ty})` accessor)"
                    ));
                }
                ImplementsFailure::OptionalityMismatch { member, ty } => {
                    clauses.push(format!("member `{member}` is optional"));
                    help.push(format!(
                        "`{iface_ty}` requires `{member}: {ty}`, but \
                         `{class_name}.{member}` is declared `{member}?: {ty}` — \
                         drop the `?` on the class, or mark the interface \
                         property `{member}?: {ty}`"
                    ));
                }
            }
        }
        self.error_with_help(
            span,
            format!(
                "class `{class_name}` does not implement `{iface_ty}`: {}",
                clauses.join(", ")
            ),
            help,
        );
    }

    fn reject_duplicate_member(
        &mut self,
        fields: &BTreeMap<String, FieldSig>,
        methods: &BTreeMap<String, MethodSig>,
        member: &Ident,
        class: &Ident,
    ) -> bool {
        if fields.contains_key(&member.name) || methods.contains_key(&member.name) {
            self.error(
                member.span,
                format!(
                    "duplicate member `{}` on class `{}`",
                    member.name, class.name
                ),
            );
            return true;
        }
        false
    }

    /// Duplicate-or-reserved check for the statics namespace. Statics and instance
    /// members live in disjoint namespaces, so a static may share a name with an
    /// instance member (TS-compatible) — only other statics collide.
    fn reject_bad_static_name(
        &mut self,
        statics: &BTreeMap<String, MethodSig>,
        static_fields: &BTreeMap<String, FieldSig>,
        member: &Ident,
        class: &Ident,
    ) -> bool {
        if statics.contains_key(&member.name) || static_fields.contains_key(&member.name) {
            self.error(
                member.span,
                format!(
                    "duplicate static member `{}` on class `{}`",
                    member.name, class.name
                ),
            );
            return true;
        }
        if matches!(member.name.as_str(), "prototype" | "name" | "length") {
            self.error_with_help(
                member.span,
                format!(
                    "static member `{}` conflicts with a built-in class property",
                    member.name
                ),
                vec![
                    "TypeScript reserves `prototype`, `name`, and `length` on class objects; \
                     pick another name"
                        .to_string(),
                ],
            );
            return true;
        }
        false
    }

    /// A static runs without an instance, so the class's type parameters have no
    /// binding there (TS rule). Method-level generics shadow class generics and
    /// stay allowed.
    fn reject_class_generic_in_static<'t>(
        &mut self,
        member: &Ident,
        class_generics: &[String],
        method_generics: &[String],
        signature_types: impl Iterator<Item = &'t Type> + Clone,
    ) -> bool {
        let mentioned = class_generics
            .iter()
            .filter(|g| !method_generics.contains(g))
            .find(|g| {
                signature_types
                    .clone()
                    .any(|ty| signature_mentions_typevar(ty, g))
            });
        let Some(g) = mentioned else {
            return false;
        };
        self.error_with_help(
            member.span,
            format!(
                "static member `{}` cannot reference class type parameter `{g}`",
                member.name
            ),
            vec![format!(
                "a static runs without an instance, so the class's `{g}` has no binding; \
                 declare the parameter on the method itself: `static {}<{g}>(…)`",
                member.name
            )],
        );
        true
    }

    /// Recover the parent of an `extends` clause whose type arguments failed to
    /// resolve, instantiated at erased args so member lookup still works.
    /// `None` when the clause named no class at all — the caller then treats the
    /// whole inherited surface as unknown.
    fn errored_extends_parent(&self, annot: &TypeAnnotation) -> Option<crate::ClassExtends> {
        let name = match &annot.kind {
            crate::TypeAnnotationKind::Name { name, .. } => name.name.clone(),
            crate::TypeAnnotationKind::Qualified { path, .. } => path
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<&str>>()
                .join("."),
            _ => return None,
        };
        self.lookup_named_type(&name).and_then(|s| match &s.kind {
            TypeKind::Class { generics, .. } => Some(crate::ClassExtends {
                parent: s.mangled_name.clone(),
                // `Error`, not `unknown`: inherited members come back poisoned
                // so operations on them stay silent, instead of drawing a
                // second diagnostic that advises narrowing an `unknown`.
                args: vec![Type::Error; generics.len()],
            }),
            _ => None,
        })
    }

    fn resolve_extends_target(
        &mut self,
        extends: Option<&TypeAnnotation>,
    ) -> Result<ExtendsTarget, CompilerFailure> {
        let Some(annot) = extends else {
            return Ok(ExtendsTarget::Absent);
        };
        // Runs inside the class's signature-generics scope, so a generic
        // parent's args may name the child's own type params
        // (`class Tagged<T> extends Box<T>` → args `[TypeVar(T)]`).
        // Arity/void violations surface in `resolve_class_reference` and come
        // back as `Type::Error`.
        let ty = self.resolve_type(annot)?;
        Ok(match ty.peel() {
            Type::ClassRef { mangled, args, .. } => ExtendsTarget::Class(crate::ClassExtends {
                parent: mangled.clone(),
                args: args.clone(),
            }),
            // The clause named a real class but got its type arguments wrong.
            // That error is already reported; keep the parent link (at erased
            // args) so `super(...)` and inherited members still resolve —
            // dropping it would bury the real diagnostic under a pile of
            // "requires a parent class" / "field does not exist" follow-ons.
            Type::Error => match self.errored_extends_parent(annot) {
                Some(parent) => ExtendsTarget::Class(parent),
                None => ExtendsTarget::Unresolved,
            },
            other => {
                let other = other.clone();
                self.error_with_help(
                    annot.span,
                    format!(
                        "a class can only `extends` another class, but `{other}` is not a class"
                    ),
                    vec![
                        "to share a shape without inheritance, use `implements` with an interface"
                            .to_string(),
                    ],
                );
                ExtendsTarget::Unresolved
            }
        })
    }

    /// Resolve each `implements` target to its `Type`. Non-interface targets are
    /// diagnosed and resolved to `Type::Error` so positions line up with the
    /// annotation list (the caller derives mangled names and conformance from these).
    fn resolve_implements_targets(
        &mut self,
        implements: &[TypeAnnotation],
    ) -> Result<Vec<Type>, CompilerFailure> {
        let mut out = Vec::new();
        for annot in implements {
            let ty = self.resolve_type(annot)?;
            match ty.peel() {
                Type::InterfaceRef { .. } | Type::Error => out.push(ty),
                other => {
                    let other = other.clone();
                    self.error_with_help(
                        annot.span,
                        format!("a class can only `implements` an interface, but `{other}` is not an interface"),
                        vec!["`extends` a class for inheritance; `implements` lists interfaces".to_string()],
                    );
                    out.push(Type::Error);
                }
            }
        }
        Ok(out)
    }

    /// Reject `extends` cycles and signature-incompatible overrides. Runs after
    /// every class signature is bound so parent chains are fully resolvable.
    pub(super) fn check_class_inheritance(
        &mut self,
        top_level: &[crate::StmtId],
    ) -> Result<(), CompilerFailure> {
        // (class name, `extends` annotation span) for every class with a parent.
        let extending: Vec<(String, Span)> = top_level
            .iter()
            .map(|id| {
                Ok::<_, CompilerFailure>(
                    match &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind {
                        StmtKind::ClassDecl {
                            name,
                            extends: Some(ext),
                            ..
                        } => Some((name.name.clone(), ext.span)),
                        _ => None,
                    },
                )
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;

        for (class_name, ext_span) in &extending {
            let Some(start) = self.class_mangled(class_name) else {
                continue;
            };
            let mut chain = vec![start.clone()];
            let mut parent = self.class_parent(&start);
            let mut cyclic = false;
            while let Some(p) = parent {
                if chain.contains(&p) {
                    cyclic = true;
                    break;
                }
                if chain.len() >= crate::compiler_limits::MAX_CLASS_CHAIN_LEN {
                    return Err(CompilerFailure::Limit {
                        stage: crate::compiler_error::CompilerStage::Infer,
                        span: Some(*ext_span),
                        message: format!(
                            "class `{class_name}` has more than {} classes in its inheritance \
                             chain, counting itself and every ancestor",
                            crate::compiler_limits::MAX_CLASS_CHAIN_LEN
                        ),
                        help: vec![
                            "flatten the hierarchy, or compose behavior instead of extending"
                                .into(),
                        ],
                    });
                }
                chain.push(p.clone());
                parent = self.class_parent(&p);
            }
            self.type_size_checkpoint(Some(*ext_span))?;
            if cyclic {
                self.invalid_class_hierarchies.insert(start);
                self.error_with_help(
                    *ext_span,
                    format!(
                        "circular inheritance: class `{class_name}` is part of an `extends` cycle"
                    ),
                    vec![
                        "classes form a single-inheritance tree; break the cycle so no class is its own ancestor"
                            .to_string(),
                    ],
                );
            }
        }

        self.resolve_implicit_constructors(top_level)?;
        self.check_member_redeclarations(top_level)?;

        Ok(())
    }

    /// A subclass with no `constructor` member inherits the nearest ancestor's
    /// constructor signature, so `new Sub(...)` and `super(...)` check against it
    /// (codegen forwards the same params to the parent init). Runs after every
    /// class signature is bound, so the parent chain is fully resolvable.
    fn resolve_implicit_constructors(
        &mut self,
        top_level: &[crate::StmtId],
    ) -> Result<(), CompilerFailure> {
        let has_explicit = |members: &[ClassMember]| {
            members
                .iter()
                .any(|m| matches!(m, ClassMember::Constructor { .. }))
        };
        let explicit_ctor: std::collections::BTreeSet<MangledName> = top_level
            .iter()
            .map(|id| {
                Ok::<_, CompilerFailure>(
                    match &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind {
                        StmtKind::ClassDecl { name, members, .. } if has_explicit(members) => {
                            self.class_mangled(&name.name)
                        }
                        _ => None,
                    },
                )
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;
        let implicit: Vec<(String, MangledName)> = top_level
            .iter()
            .map(|id| {
                Ok::<_, CompilerFailure>(
                    match &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind {
                        StmtKind::ClassDecl {
                            name,
                            members,
                            extends: Some(_),
                            ..
                        } if !has_explicit(members) => self
                            .class_mangled(&name.name)
                            .map(|m| (name.name.clone(), m)),
                        _ => None,
                    },
                )
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;

        for (child_name, child) in implicit {
            // An unresolvable parent takes its constructor signature with it:
            // stand in a variadic one so `new Sub(…)` accepts whatever the
            // program passes rather than reporting an arity the class never had.
            let params = self
                .nearest_explicit_ctor_params(&child, &explicit_ctor)
                .or_else(|| {
                    self.inherits_unresolved_parent(&child)
                        .then(|| vec![erased_ctor_rest_param()])
                });
            if let Some(params) = params
                && let Some(sym) = self.types.lookup_mut(&child_name)
                && let TypeKind::Class { constructor, .. } = &mut sym.kind
            {
                *constructor = params;
            }
        }

        Ok(())
    }

    /// Constructor signature of the nearest ancestor of `child` that declares an
    /// explicit constructor (cycle-guarded), substituted at the bindings the
    /// intervening extends clauses instantiate — an implicit subclass of a
    /// generic parent stores concrete ctor params. `None` when no ancestor does.
    fn nearest_explicit_ctor_params(
        &self,
        child: &MangledName,
        explicit_ctor: &std::collections::BTreeSet<MangledName>,
    ) -> Option<Vec<Param>> {
        use super::generic::substitute_typevars;
        self.walk_ancestors(child, |sym, bindings| {
            let p = &sym.mangled_name;
            // An imported ancestor's `constructor` field already holds its
            // effective (possibly inherited) params, resolved in its own package —
            // so any imported ancestor is a valid stopping point, the same way a
            // local ancestor with an explicit constructor is.
            let imported = !self.local_class_mangles.contains(p);
            if !explicit_ctor.contains(p) && !imported {
                return ControlFlow::Continue(());
            }
            let TypeKind::Class { constructor, .. } = &sym.kind else {
                return ControlFlow::Break(None);
            };
            let params = constructor
                .iter()
                .map(|param| {
                    Ok(Param {
                        ty: substitute_typevars(&param.ty, bindings, &self.type_limits)?,
                        ..param.clone()
                    })
                })
                .collect();
            ControlFlow::Break(self.type_limits.ok_or_record(params))
        })
    }

    fn check_member_redeclarations(
        &mut self,
        top_level: &[crate::StmtId],
    ) -> Result<(), CompilerFailure> {
        for member in self.redeclaration_candidates(top_level)? {
            // A cross-kind redeclaration has no same-kind check to run — the
            // ancestor walk each one does looks for its own kind and would sail
            // past the declaration that actually collides.
            if self.reported_cross_kind_redeclaration(&member)? {
                continue;
            }
            match member.kind {
                MemberKind::Method => self.check_method_override(&member)?,
                MemberKind::Field => self.check_field_redeclaration(&member)?,
                MemberKind::Accessor => self.check_accessor_redeclaration(&member)?,
            }
            self.type_size_checkpoint(Some(member.span))?;
        }

        Ok(())
    }

    /// Every instance member of every class that has a parent, in source order —
    /// except a class's accessor properties, which land last, by property name,
    /// since the get/set pair is collected into one candidate after the walk.
    /// Materialized rather than checked in place because the walk borrows the
    /// AST out of `self`, while the checks need `self` mutably to report.
    fn redeclaration_candidates(
        &self,
        top_level: &[crate::StmtId],
    ) -> Result<Vec<RedeclarationCandidate>, CompilerFailure> {
        let mut found: Vec<RedeclarationCandidate> = Vec::new();
        for id in top_level {
            let StmtKind::ClassDecl {
                name: class_name,
                members,
                ..
            } = &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind
            else {
                continue;
            };
            let Some(child_class) = self.class_mangled(&class_name.name) else {
                continue;
            };
            // Only classes with a parent can redeclare.
            if self.class_parent(&child_class).is_none() {
                continue;
            }
            let redeclared =
                |name: &crate::Ident, span: Span, kind: MemberKind| RedeclarationCandidate {
                    name: name.name.clone(),
                    span,
                    child_class: child_class.clone(),
                    kind,
                    accessor_spans: BTreeMap::new(),
                };
            // `get x` and `set x` are one property under one name, and the
            // checks run per property, so the pair contributes one candidate —
            // built after the member walk, once both halves' spans are known.
            let mut accessor_candidates: BTreeMap<&str, RedeclarationCandidate> = BTreeMap::new();
            for member in members {
                match member {
                    ClassMember::Method {
                        name,
                        modifiers,
                        span,
                        ..
                    } if modifiers.static_span.is_none() => {
                        found.push(redeclared(name, *span, MemberKind::Method));
                    }
                    ClassMember::Field {
                        name,
                        modifiers,
                        span,
                        ..
                    } if modifiers.static_span.is_none() => {
                        found.push(redeclared(name, *span, MemberKind::Field));
                    }
                    ClassMember::Accessor {
                        name,
                        modifiers,
                        kind,
                        span,
                        ..
                    } if modifiers.static_span.is_none() => {
                        accessor_candidates
                            .entry(name.name.as_str())
                            .or_insert_with(|| redeclared(name, *span, MemberKind::Accessor))
                            .accessor_spans
                            .insert(*kind, *span);
                    }
                    // A parameter property declares a field just as a `Field`
                    // member does, and codegen lays it out identically — so it
                    // shadows identically and needs the same check.
                    ClassMember::Constructor { params, .. } => found.extend(
                        params
                            .iter()
                            .filter(|p| p.modifiers.is_some())
                            .map(|p| redeclared(&p.name, p.name.span, MemberKind::Field)),
                    ),
                    // Statics live in their own namespace and share no slot with
                    // anything inherited.
                    ClassMember::Method { .. }
                    | ClassMember::Field { .. }
                    | ClassMember::Accessor { .. } => {}
                }
            }
            found.extend(accessor_candidates.into_values());
        }
        Ok(found)
    }

    /// Reject a member that redeclares an inherited name at a *different* kind.
    /// The two never unify: an accessor over an inherited field leaves the field
    /// winning every access and the accessor bodies dead, a field over an
    /// inherited accessor appends a second, independent property under one name,
    /// and a field over an inherited method leaves `c.v` and `p.v()` naming
    /// different members.
    fn reported_cross_kind_redeclaration(
        &mut self,
        member: &RedeclarationCandidate,
    ) -> Result<bool, CompilerFailure> {
        let opaque = self.opaque_own_generics(&member.child_class)?;
        let Some((inherited_kind, owner)) =
            self.ancestor_member_kind(&member.child_class, &member.name, &opaque)
        else {
            return Ok(false);
        };
        if inherited_kind == member.kind {
            return Ok(false);
        }
        let owner_name = self.class_name_of(&owner);
        let (child, parent) = (member.kind.noun(), inherited_kind.noun());
        self.error_with_help(
            member.span,
            format!(
                "`{}` redeclares an inherited {parent} as {} {child}",
                member.name,
                member.kind.article()
            ),
            vec![
                format!("inherited: {parent} `{}` on `{owner_name}`", member.name),
                format!(
                    "a redeclaration shares the inherited member's storage or vtable slot, so it \
                     has to keep its kind — declare `{}` as {} {parent}, or rename it",
                    member.name,
                    inherited_kind.article()
                ),
            ],
        );
        Ok(true)
    }

    /// An accessor redeclaring an inherited accessor shares its vtable slot, so
    /// the slot's recorded signature is the *base* declarer's: a getter whose
    /// return doesn't fit it (or a setter that won't accept what the inherited
    /// one does) miscompiles rather than dispatching.
    fn check_accessor_redeclaration(
        &mut self,
        member: &RedeclarationCandidate,
    ) -> Result<(), CompilerFailure> {
        let opaque = self.opaque_own_generics(&member.child_class)?;
        for own in self.class_own_accessors(&member.child_class, &member.name) {
            let Some(inherited) = self.ancestor_accessor(&member.child_class, &own, &opaque) else {
                continue;
            };
            let Some(cmp) =
                accessor_comparison(&member.name, &own, &inherited, &opaque, &self.type_limits)
                    .map_err(type_limit_at(member.span))?
            else {
                continue;
            };
            if super::assignable(&cmp.subtype, &cmp.supertype, self.resolver()) {
                continue;
            }
            let span = member
                .accessor_spans
                .get(&cmp.half)
                .copied()
                .unwrap_or(member.span);
            self.error_with_help(
                span,
                format!(
                    "accessor `{}` is not compatible with the inherited accessor",
                    cmp.accessor
                ),
                vec![
                    format!("inherited:  {}", cmp.inherited),
                    format!("redeclared: {}", cmp.redeclared),
                    format!(
                        "the two share one vtable slot — keep the inherited signature, or rename \
                         `{}`",
                        member.name
                    ),
                ],
            );
        }
        Ok(())
    }

    /// The class's own getter and setter for `name` (either may be absent).
    fn class_own_accessors(&self, mangled: &MangledName, name: &str) -> Vec<AccessorSig> {
        let Some(sym) = self.class_by_mangled(mangled) else {
            return Vec::new();
        };
        let TypeKind::Class { accessors, .. } = &sym.kind else {
            return Vec::new();
        };
        accessors
            .iter()
            .filter(|a| a.name() == name)
            .cloned()
            .collect()
    }

    /// Nearest ancestor's accessor matching `own`'s half (getter against getter,
    /// setter against setter), substituted at the bindings the child's `extends`
    /// clause instantiates.
    fn ancestor_accessor(
        &self,
        child: &MangledName,
        own: &AccessorSig,
        own_bindings: &BTreeMap<String, Type>,
    ) -> Option<AccessorSig> {
        let name = own.name().to_string();
        let want_getter = matches!(own, AccessorSig::Getter { .. });
        self.walk_ancestors_at(child, own_bindings, |sym, bindings| {
            let TypeKind::Class { accessors, .. } = &sym.kind else {
                return ControlFlow::Continue(());
            };
            let found = accessors.iter().find(|a| {
                a.name() == name && matches!(a, AccessorSig::Getter { .. }) == want_getter
            });
            match found {
                Some(sig) => {
                    ControlFlow::Break(self.type_limits.ok_or_record(substitute_accessor_sig(
                        sig,
                        bindings,
                        &self.type_limits,
                    )))
                }
                // A class that redeclares the property's other half but not this
                // one still leaves this half inherited from further up.
                None => ControlFlow::Continue(()),
            }
        })
    }

    /// The kind the nearest ancestor declares `name` at, with the class that
    /// declares it.
    fn ancestor_member_kind(
        &self,
        child: &MangledName,
        name: &str,
        own_bindings: &BTreeMap<String, Type>,
    ) -> Option<(MemberKind, MangledName)> {
        self.walk_ancestors_at(child, own_bindings, |sym, _| {
            match declared_member_kind(sym, name) {
                Some(kind) => ControlFlow::Break(Some((kind, sym.mangled_name.clone()))),
                None => ControlFlow::Continue(()),
            }
        })
    }

    fn check_method_override(
        &mut self,
        member: &RedeclarationCandidate,
    ) -> Result<(), CompilerFailure> {
        let Some(child_sig) = self.class_method_sig(&member.child_class, &member.name) else {
            return Ok(());
        };
        // A generic declared on the method itself is not in this map and keeps
        // its existing (lenient) treatment.
        let opaque = self.opaque_own_generics(&member.child_class)?;
        let Some(parent_sig) = self.ancestor_method_sig(&member.child_class, &member.name, &opaque)
        else {
            return Ok(());
        };
        let child_fn = substitute_typevars(&method_fn_type(&child_sig), &opaque, &self.type_limits)
            .map_err(type_limit_at(member.span))?;
        let parent_fn = method_fn_type(&parent_sig);
        // TypeScript lets an override declare fewer parameters, as any function
        // may. Here the override fills the inherited method's vtable slot, whose
        // Wasm signature is fixed, so it must declare them all.
        if child_sig.params.len() < parent_sig.params.len() {
            self.error_with_help(
                member.span,
                format!(
                    "override of method `{}` must declare the inherited method's {} parameter(s)",
                    member.name,
                    parent_sig.params.len()
                ),
                vec![
                    format!("inherited: {parent_fn}"),
                    format!("override:  {child_fn}"),
                    "declare the parameters it ignores too".to_string(),
                ],
            );
            return Ok(());
        }
        if !super::assignable(&child_fn, &parent_fn, self.resolver()) {
            self.error_with_help(
                member.span,
                format!(
                    "override of method `{}` is not compatible with the inherited signature",
                    member.name
                ),
                vec![
                    format!("inherited: {parent_fn}"),
                    format!("override:  {child_fn}"),
                ],
            );
        }
        Ok(())
    }

    /// A redeclared field shadows the inherited one into a single property
    /// sharing one payload slot, so a parent-typed receiver reads the child's
    /// value through the *parent's* declared type, and a parent method writes
    /// the child's storage. Sound only if the child's type still satisfies the
    /// parent's, and only if the redeclaration doesn't reach storage it has no
    /// business naming.
    fn check_field_redeclaration(
        &mut self,
        member: &RedeclarationCandidate,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let Some(child_field) = self.class_own_field_sig(&member.child_class, &member.name) else {
            return Ok(());
        };
        let opaque = self.opaque_own_generics(&member.child_class)?;
        let Some((parent_field, parent_class)) =
            self.ancestor_field_decl(&member.child_class, &member.name, &opaque)
        else {
            return Ok(());
        };

        let child_ty =
            substitute_typevars(&field_read_ty(&child_field), &opaque, &self.type_limits)
                .map_err(type_limit_at(member.span))?;
        let parent_ty = field_read_ty(&parent_field);
        if !super::assignable(&child_ty, &parent_ty, self.resolver()) {
            self.error_with_help(
                member.span,
                format!(
                    "field `{}` is not compatible with the inherited declaration",
                    member.name
                ),
                vec![
                    format!("inherited:  {}: {parent_ty}", member.name),
                    format!("redeclared: {}: {child_ty}", member.name),
                    format!(
                        "give `{}` a type assignable to `{parent_ty}`, or rename it",
                        member.name
                    ),
                ],
            );
            return Ok(());
        }

        // `private` is module-scoped, so a field private to another module is a
        // name this class cannot see — yet the shared slot would hand it write
        // access to that module's encapsulated state.
        if parent_field.visibility != Visibility::Public
            && !self.local_class_mangles.contains(&parent_class)
        {
            let owner = self.class_name_of(&parent_class);
            self.error_with_help(
                member.span,
                format!(
                    "field `{}` redeclares a field that is private to another module",
                    member.name
                ),
                vec![format!(
                    "the two would share one storage slot, so this would alias state \
                     `{owner}` encapsulates — rename the field"
                )],
            );
            return Ok(());
        }

        if child_field.visibility != parent_field.visibility {
            let inherited = visibility_keyword(parent_field.visibility);
            self.error_with_help(
                member.span,
                format!(
                    "field `{}` redeclares an inherited field at a different visibility",
                    member.name
                ),
                vec![
                    format!("inherited:  {inherited} {}", member.name),
                    format!(
                        "redeclared: {} {}",
                        visibility_keyword(child_field.visibility),
                        member.name
                    ),
                    format!("declare `{}` as `{inherited}`, or rename it", member.name),
                ],
            );
        }

        self.record_narrowing_check(member, &child_ty, &opaque)?;
        Ok(())
    }

    /// Records the read guard for a redeclaration that *narrows* the inherited
    /// type — see [`crate::FieldNarrowingCheck`] for what it buys.
    ///
    /// Mutual assignability means no narrowing and so no guard: the parent
    /// cannot write anything the child does not already admit.
    fn record_narrowing_check(
        &mut self,
        member: &RedeclarationCandidate,
        child_ty: &Type,
        opaque: &BTreeMap<String, Type>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        // Against the *widest* ancestor, not the nearest: the slot is shared with
        // every declaration above, so an intermediate class narrowing it first
        // must not shrink what this guard defends against.
        let Some((widest_field, widest_class)) =
            self.widest_ancestor_field_decl(&member.child_class, &member.name, opaque)
        else {
            return Ok(());
        };
        let parent_ty = field_read_ty(&widest_field);
        if super::assignable(&parent_ty, child_ty, self.resolver()) {
            return Ok(());
        }
        let Some(test) = self.narrowing_test(&parent_ty, child_ty) else {
            return Ok(());
        };
        let child_name = self.class_name_of(&member.child_class);
        let parent_name = self.class_name_of(&widest_class);
        let field = &member.name;
        let check = crate::FieldNarrowingCheck {
            declaration: Some(member.child_class.clone()),
            minimal_test_target: (self.array_representation_distinguishes(&parent_ty, child_ty)
                || (matches!(test, crate::FieldNarrowingTest::NonNull)
                    && !type_mentions_erased_parameter(child_ty)))
            .then(|| child_ty.clone()),
            test,
            message: format!(
                "field `{field}` holds a value its declaration does not admit: \
                 `{child_name}.{field}: {child_ty}` narrows the inherited \
                 `{parent_name}.{field}: {parent_ty}` and the two share one storage slot, \
                 so code that only sees the inherited declaration can write it"
            ),
        };
        self.field_narrowing_checks
            .insert((member.child_class.clone(), field.clone()), check.clone());
        let _: () = if let Some(symbol) = self.types.lookup_mut(&child_name)
            && let TypeKind::Class {
                narrowing_checks, ..
            } = &mut symbol.kind
        {
            narrowing_checks.insert(field.clone(), check);
        };
        Ok(())
    }

    /// Only skip the element walk when every array the ancestor admits also
    /// satisfies the child. Unknown, structural, and other erased alternatives
    /// require the full validator because they can conceal incompatible arrays.
    fn array_representation_distinguishes(&self, parent: &Type, child: &Type) -> bool {
        let child_non_null = super::narrowing::strip_null(child);
        if !matches!(child_non_null.peel(), Type::Array(_)) {
            return false;
        }
        match parent.peel() {
            Type::Union(members) => members
                .iter()
                .all(|member| self.array_representation_distinguishes(member, child)),
            Type::Array(_) | Type::Tuple(_) => super::assignable(parent, child, self.resolver()),
            Type::Null
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::BigInt => true,
            _ => false,
        }
    }

    /// What the read has to verify, or `None` when nothing it can lower would.
    /// Prefers [`crate::FieldNarrowingTest::NonNull`] when stripping `null` is the
    /// whole difference — see the variant for why that is worth a case of its own
    /// — and otherwise tests structural conformance to the narrowed type.
    /// Interfaces retain a descriptor for both data properties and methods;
    /// data-only interfaces simply carry an empty method set.
    ///
    /// `None` is not a safe default, only the status quo: for a leaf whose Wasm
    /// lowering the parent's value also satisfies the bare cast *succeeds* and the
    /// wrong-typed value escapes to fail somewhere else. Widening the allowlist
    /// shrinks that hole.
    fn narrowing_test(
        &self,
        parent_ty: &Type,
        child_ty: &Type,
    ) -> Option<crate::FieldNarrowingTest> {
        // A presence check can never reject a value the child's declaration
        // admits, so it is sound whenever that declaration rejects `null`. An
        // erased type parameter cannot promise that here — `Sub<T>`'s `v: T` is
        // `string | null` at `Sub<string | null>` — but it does not have to:
        // `emit_narrowed_field_read` re-asks at the read, where the substitution
        // is known, and skips the test when the read's own type admits `null`.
        let child_rejects_null = !super::expr::type_admits_null(child_ty, self.resolver());
        // Reaching here means the parent is not assignable to the child, so if
        // every *non-null* parent value is, `null` is the whole difference and
        // presence is the complete check.
        let parent_non_null = super::narrowing::strip_null(parent_ty);
        if child_rejects_null && super::assignable(&parent_non_null, child_ty, self.resolver()) {
            return Some(crate::FieldNarrowingTest::NonNull);
        }
        if crate::typed_ast::field_runtime_type_is_testable(child_ty) {
            return Some(crate::FieldNarrowingTest::Shape(child_ty.peel().clone()));
        }
        if let Some(test) = self.interface_narrowing_test(child_ty) {
            return Some(crate::FieldNarrowingTest::Interface(test));
        }
        if type_mentions_erased_parameter(child_ty) {
            return Some(crate::FieldNarrowingTest::Substituted);
        }
        // Nothing in `runtime_type_is_testable`, no interface metadata, and no
        // erased parameter to re-ask at the read. Presence still catches the
        // part of the narrowing that removes `null`.
        (child_rejects_null && super::expr::type_admits_null(parent_ty, self.resolver()))
            .then_some(crate::FieldNarrowingTest::NonNull)
    }

    pub(super) fn interface_narrowing_test(
        &self,
        child_ty: &Type,
    ) -> Option<crate::InterfaceNarrowingTest> {
        let (interface_ty, nullable) = match child_ty.peel() {
            Type::InterfaceRef { .. } => (child_ty.peel(), false),
            Type::Union(members) => {
                let nullable = members
                    .iter()
                    .any(|member| matches!(member.peel(), Type::Null));
                let mut non_null = members
                    .iter()
                    .filter(|member| !matches!(member.peel(), Type::Null));
                let interface = non_null.next()?;
                if non_null.next().is_some() {
                    return None;
                }
                (interface.peel(), nullable)
            }
            _ => return None,
        };
        let Type::InterfaceRef {
            mangled,
            name,
            args,
            package,
            ..
        } = interface_ty
        else {
            return None;
        };
        let members = self.resolver().interface_full_form(mangled, name, args)?;
        let crate::TypeKind::Interface {
            methods, dispatch, ..
        } = &self.resolver().lookup(mangled, name)?.kind
        else {
            return None;
        };
        let target = interface_ty.clone();
        let non_shape_carriers =
            self.non_shape_interface_carriers(&target, package, name, dispatch, &members);
        Some(crate::InterfaceNarrowingTest {
            index: self.resolver().index_signature(interface_ty),
            members,
            methods: methods.keys().cloned().collect(),
            shape_allowed: *dispatch == crate::Dispatch::VTable
                || non_shape_carriers.contains(&crate::InterfaceCarrier::ObjectShape),
            non_shape_carriers,
            nullable,
        })
    }

    fn non_shape_interface_carriers(
        &self,
        target: &Type,
        package: &crate::Package,
        name: &str,
        dispatch: &crate::Dispatch,
        members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let mut carriers = self.primitive_interface_carriers(target);
        carriers.extend(self.collection_interface_carriers(target, name, members));
        carriers.extend(self.direct_interface_carriers(package, name, dispatch));
        carriers
    }

    fn primitive_interface_carriers(
        &self,
        target: &Type,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let mut carriers = std::collections::BTreeSet::new();
        for (carrier, ty) in [
            (crate::InterfaceCarrier::Number, Type::Number),
            (crate::InterfaceCarrier::Boolean, Type::Boolean),
            (crate::InterfaceCarrier::String, Type::String),
            (crate::InterfaceCarrier::BigInt, Type::BigInt),
            (crate::InterfaceCarrier::Uint8Array, Type::Uint8Array),
        ] {
            if super::assignable(&ty, target, self.resolver()) {
                carriers.insert(carrier);
            }
        }
        carriers
    }

    fn collection_interface_carriers(
        &self,
        target: &Type,
        name: &str,
        members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let target_args = match target.peel() {
            Type::InterfaceRef { args, .. } => args.as_slice(),
            _ => &[],
        };
        let candidates = runtime_carrier_candidates(members);
        let mut carriers =
            self.array_and_set_interface_carriers(target, name, target_args, &candidates, members);
        carriers.extend(self.map_interface_carriers(
            target,
            name,
            target_args,
            &candidates,
            members,
        ));
        carriers
    }

    /// How the instantiations of a collection carrier relate to `target`, where
    /// that can be decided without trying each one. A field typed as a union of
    /// hundreds of literals makes hundreds of candidate arguments, and `Map`
    /// tries every pair of them.
    ///
    /// Member names do not depend on type arguments, so a missing required
    /// member, or a weak target sharing no member with the carrier, rules out
    /// every instantiation. When none of the carrier members the target names
    /// mentions the carrier's type parameters, and the target has no index
    /// signature constraining member types, every instantiation compares the
    /// same members, so one answers for all.
    fn carrier_fit(
        &self,
        template: &Type,
        target: &Type,
        target_members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> CarrierFit {
        let Some((mangled, _, name, args)) = template.interface_routing() else {
            return CarrierFit::TryEach;
        };
        let Some(form) = self.resolver().interface_full_form(&mangled, name, &args) else {
            return CarrierFit::TryEach;
        };
        if target_members
            .iter()
            .any(|(member, field)| !field.optional && !form.contains_key(member))
        {
            return CarrierFit::None;
        }
        let weak = !target_members.is_empty() && target_members.values().all(|f| f.optional);
        if weak && !form.is_empty() && !target_members.keys().any(|k| form.contains_key(k)) {
            return CarrierFit::None;
        }
        // An index signature of `unknown` admits every member of every
        // instantiation: only `void` is not assignable to `unknown`, and no
        // candidate is `void`.
        let index_constrains_members = self
            .resolver()
            .index_signature(target)
            .is_some_and(|index| !matches!(index.value.peel(), Type::Unknown));
        if index_constrains_members
            || self.carrier_members_mention_parameters(&mangled, name, target_members)
        {
            return CarrierFit::TryEach;
        }
        if super::assignable(template, target, self.resolver()) {
            CarrierFit::Every
        } else {
            CarrierFit::None
        }
    }

    /// Whether any member of the carrier interface `name` that `target_members`
    /// also names mentions one of the carrier's type parameters. True when the
    /// declaration cannot be read, which keeps the per-instantiation search.
    fn carrier_members_mention_parameters(
        &self,
        mangled: &MangledName,
        name: &str,
        target_members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> bool {
        let Some(symbol) = self.resolver().lookup(mangled, name) else {
            return true;
        };
        let TypeKind::Interface {
            generics,
            methods,
            properties,
            ..
        } = &symbol.kind
        else {
            return true;
        };
        let mentions = |ty: &Type| {
            generics
                .iter()
                .any(|generic| signature_mentions_typevar(ty, generic))
        };
        target_members.keys().any(|member| {
            methods.get(member).is_some_and(|sig| {
                sig.params.iter().any(|param| mentions(&param.ty)) || mentions(&sig.ret)
            }) || properties
                .get(member)
                .is_some_and(|property| mentions(&property.ty))
        })
    }

    fn array_and_set_interface_carriers(
        &self,
        target: &Type,
        name: &str,
        target_args: &[Type],
        candidates: &[Type],
        members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let mut carriers = std::collections::BTreeSet::new();
        let exact_element = matches!(name, "Iterable" | "Set")
            .then(|| target_args.first())
            .flatten();
        if let Some(element) = exact_element {
            let array = Type::Array(Box::new(element.clone()));
            if super::assignable(&array, target, self.resolver()) {
                carriers.insert(crate::InterfaceCarrier::Array(array));
            }
            let candidate = Type::prelude_interface("Set".to_string(), vec![element.clone()]);
            if super::assignable(&candidate, target, self.resolver()) {
                carriers.insert(crate::InterfaceCarrier::Set(element.clone()));
            }
        } else {
            // Candidate instantiations only answer whether the backing's
            // non-generic surface (for example `length`) satisfies the target.
            // When every instantiation matches, the target leaves contents
            // unconstrained and an `Any` carrier is sound. Otherwise retain
            // only the matching instantiations and validate their contents.
            let array_fit =
                self.carrier_fit(&Type::Array(Box::new(Type::Unknown)), target, members);
            let array_matches = array_fit.matching(candidates, |element| {
                let array = Type::Array(Box::new(element.clone()));
                let tuple = Type::Tuple(vec![element.clone()]);
                super::assignable(&array, target, self.resolver())
                    || super::assignable(&tuple, target, self.resolver())
            });
            if array_matches.len() == candidates.len() {
                carriers.insert(crate::InterfaceCarrier::ArrayAny);
            } else {
                carriers.extend(
                    array_matches.into_iter().map(|element| {
                        crate::InterfaceCarrier::Array(Type::Array(Box::new(element)))
                    }),
                );
            }
            let set_fit = self.carrier_fit(
                &Type::prelude_interface("Set".to_string(), vec![Type::Unknown]),
                target,
                members,
            );
            let set_matches = set_fit.matching(candidates, |element| {
                let set = Type::prelude_interface("Set".to_string(), vec![element.clone()]);
                super::assignable(&set, target, self.resolver())
            });
            if set_matches.len() == candidates.len() {
                carriers.insert(crate::InterfaceCarrier::SetAny);
            } else {
                carriers.extend(set_matches.into_iter().map(crate::InterfaceCarrier::Set));
            }
        }
        carriers
    }

    fn map_interface_carriers(
        &self,
        target: &Type,
        name: &str,
        target_args: &[Type],
        candidates: &[Type],
        members: &std::collections::BTreeMap<String, crate::ObjectField>,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let mut carriers = std::collections::BTreeSet::new();
        let exact_map_pair = if name == "Map" {
            target_args.first().zip(target_args.get(1))
        } else if name == "Iterable" {
            target_args
                .first()
                .and_then(|element| match element.peel() {
                    Type::Tuple(items) if items.len() == 2 => items.first().zip(items.get(1)),
                    _ => None,
                })
        } else {
            None
        };
        if let Some((key, value)) = exact_map_pair {
            let candidate =
                Type::prelude_interface("Map".to_string(), vec![key.clone(), value.clone()]);
            if super::assignable(&candidate, target, self.resolver()) {
                carriers.insert(crate::InterfaceCarrier::Map(key.clone(), value.clone()));
            }
        } else {
            let fit = self.carrier_fit(
                &Type::prelude_interface("Map".to_string(), vec![Type::Unknown, Type::Unknown]),
                target,
                members,
            );
            match fit {
                CarrierFit::None => {}
                CarrierFit::Every => {
                    carriers.insert(crate::InterfaceCarrier::MapAny);
                }
                CarrierFit::TryEach => {
                    let map_matches: Vec<(Type, Type)> = candidates
                        .iter()
                        .flat_map(|key| {
                            candidates.iter().filter_map(|value| {
                                let map = Type::prelude_interface(
                                    "Map".to_string(),
                                    vec![key.clone(), value.clone()],
                                );
                                super::assignable(&map, target, self.resolver())
                                    .then(|| (key.clone(), value.clone()))
                            })
                        })
                        .collect();
                    if map_matches.len() == candidates.len() * candidates.len() {
                        carriers.insert(crate::InterfaceCarrier::MapAny);
                    } else {
                        carriers.extend(
                            map_matches
                                .into_iter()
                                .map(|(key, value)| crate::InterfaceCarrier::Map(key, value)),
                        );
                    }
                }
            }
        }
        carriers
    }

    fn direct_interface_carriers(
        &self,
        package: &crate::Package,
        name: &str,
        dispatch: &crate::Dispatch,
    ) -> std::collections::BTreeSet<crate::InterfaceCarrier> {
        let mut carriers = std::collections::BTreeSet::new();
        if *dispatch != crate::Dispatch::VTable {
            match name {
                "RegExp" => {
                    carriers.insert(crate::InterfaceCarrier::RegExp);
                }
                "RegExpMatch" => {
                    carriers.insert(crate::InterfaceCarrier::RegExpMatch);
                }
                "Temporal.Instant" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalInstant);
                }
                "Temporal.Duration" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalDuration);
                }
                "Temporal.ZonedDateTime" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalZonedDateTime);
                }
                "Temporal.PlainDate" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalPlainDate);
                }
                "Temporal.PlainTime" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalPlainTime);
                }
                "Temporal.PlainDateTime" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalPlainDateTime);
                }
                "Temporal.PlainYearMonth" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalPlainYearMonth);
                }
                "Temporal.PlainMonthDay" => {
                    carriers.insert(crate::InterfaceCarrier::TemporalPlainMonthDay);
                }
                "TextEncoder" | "TextDecoder" => {
                    carriers.insert(crate::InterfaceCarrier::ObjectShape);
                }
                _ => {}
            }
            let host_carrier = match (package.as_str(), name) {
                ("submilli:fs", "Stat") => Some(crate::InterfaceCarrier::FsStat),
                ("submilli:fs", "Peek") => Some(crate::InterfaceCarrier::FsPeek),
                ("submilli:fs", "DirEntry") => Some(crate::InterfaceCarrier::FsDirEntry),
                ("submilli:fs", "Info") => Some(crate::InterfaceCarrier::FsInfo),
                ("submilli:fs", "FileWriter") => Some(crate::InterfaceCarrier::FsFileWriter),
                ("submilli:http", "Response") => Some(crate::InterfaceCarrier::HttpResponse),
                ("submilli:http", "DownloadResult") => {
                    Some(crate::InterfaceCarrier::HttpDownloadResult)
                }
                ("submilli:session", "Entry") => Some(crate::InterfaceCarrier::SessionEntry),
                ("submilli:session", "Page") => Some(crate::InterfaceCarrier::SessionPage),
                ("submilli:url", "URL") => Some(crate::InterfaceCarrier::Url),
                _ => None,
            };
            carriers.extend(host_carrier);
        }
        carriers
    }

    pub(super) fn record_runtime_type_test(&mut self, ty: &Type) -> Result<(), CompilerFailure> {
        self.record_runtime_type_test_inner(ty, RuntimeTestMode::General, &mut BTreeSet::new())?;
        Ok(())
    }

    fn record_runtime_type_test_inner(
        &mut self,
        ty: &Type,
        mode: RuntimeTestMode,
        active_interfaces: &mut BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        let key = super::generic::erase_generic_params(ty.peel());
        if let Some(existing) = self.typed_ast.runtime_type_tests.get(&key) {
            if mode == RuntimeTestMode::AllowAliasRefs
                && matches!(existing, crate::FieldNarrowingTest::Representation)
                && crate::typed_ast::field_runtime_type_is_testable(&key)
            {
                self.typed_ast
                    .runtime_type_tests
                    .insert(key.clone(), crate::FieldNarrowingTest::Shape(key));
            }
            return Ok(());
        }
        // Break recursive interface discovery conservatively. A later outer
        // call replaces this placeholder with the resolved descriptor.
        self.typed_ast
            .runtime_type_tests
            .insert(key.clone(), crate::FieldNarrowingTest::Representation);
        self.record_runtime_test_dependencies(&key, active_interfaces)?;
        self.record_runtime_class_fields(&key, active_interfaces)?;
        let testable = if mode == RuntimeTestMode::AllowAliasRefs {
            crate::typed_ast::field_runtime_type_is_testable(&key)
        } else {
            crate::typed_ast::runtime_type_is_testable(&key)
        };
        let test = if let Some(members) = self.enum_runtime_members(&key) {
            crate::FieldNarrowingTest::Shape(members)
        } else if let Some(interface) = self.interface_narrowing_test(&key) {
            let interface_identity = match key.peel() {
                Type::InterfaceRef { mangled, .. } => Some(mangled.clone()),
                Type::Union(members) => members.iter().find_map(|member| match member.peel() {
                    Type::InterfaceRef { mangled, .. } => Some(mangled.clone()),
                    _ => None,
                }),
                _ => None,
            };
            if let Some(identity) = interface_identity.as_ref()
                && !active_interfaces.insert(identity.clone())
            {
                self.record_generic_interface_validators(&interface, identity)?;
                crate::FieldNarrowingTest::Interface(interface)
            } else {
                if let Some(index) = &interface.index {
                    self.record_runtime_type_test_inner(
                        &index.value,
                        RuntimeTestMode::AllowAliasRefs,
                        active_interfaces,
                    )?;
                }
                for member in interface.members.values() {
                    self.record_runtime_type_test_inner(
                        &member.ty,
                        RuntimeTestMode::AllowAliasRefs,
                        active_interfaces,
                    )?;
                }
                if let Some(identity) = interface_identity {
                    active_interfaces.remove(&identity);
                }
                crate::FieldNarrowingTest::Interface(interface)
            }
        } else if testable {
            crate::FieldNarrowingTest::Shape(key.clone())
        } else {
            crate::FieldNarrowingTest::Representation
        };
        self.typed_ast.runtime_type_tests.insert(key, test);
        Ok(())
    }

    fn enum_runtime_members(&self, ty: &Type) -> Option<Type> {
        let (Type::NumberEnum { mangled, .. } | Type::StringEnum { mangled, .. }) = ty else {
            return None;
        };
        let symbol = self
            .type_registry
            .lookup(mangled)
            .or_else(|| self.types.lookup_by_mangled(mangled))?;
        let members = match &symbol.kind {
            TypeKind::NumberEnum { variants, .. } => variants
                .iter()
                .map(|(_, value)| Type::NumberLiteral(crate::types::LiteralF64(*value)))
                .collect(),
            TypeKind::StringEnum { variants, .. } => variants
                .iter()
                .map(|(_, value)| Type::StringLiteral(value.clone()))
                .collect(),
            _ => return None,
        };
        Some(Type::union(members))
    }

    fn record_runtime_class_fields(
        &mut self,
        ty: &Type,
        active: &mut BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        let Type::ClassRef { mangled, args, .. } = ty else {
            return Ok(());
        };
        // Diagnosed source cycles have no runtime descriptor; compilation will
        // return their diagnostics. Unexplained dependency cycles remain fatal.
        if self.invalid_class_hierarchies.contains(mangled) {
            return Ok(());
        }
        if !active.insert(mangled.clone()) {
            self.typed_ast
                .runtime_class_fields
                .insert(ty.clone(), Type::Never);
            return Ok(());
        }
        let mut fields = BTreeMap::new();
        let mut guards = Vec::new();
        let mut contexts = Vec::new();
        let mut chain = Vec::new();
        for_each_class_in_chain(
            |name| self.class_by_mangled(name),
            &self.type_limits,
            mangled,
            args,
            |symbol, bindings| chain.push((symbol.clone(), bindings.clone())),
        )
        .ok_or_else(|| {
            self.pending_limit_or(super::inference_failure(
                "incomplete runtime class metadata",
            ))
        })?;
        for (symbol, bindings) in &chain {
            let TypeKind::Class {
                generics,
                fields: declared,
                narrowing_checks,
                accessors,
                ..
            } = &symbol.kind
            else {
                return Err(super::inference_failure(
                    "runtime class metadata is not a class",
                ));
            };
            if !generics.is_empty() {
                contexts.push(crate::typed_ast::InstanceTypeContext {
                    declaration: symbol.mangled_name.clone(),
                    args: generics
                        .iter()
                        .map(|name| {
                            bindings.get(name).cloned().ok_or_else(|| {
                                super::inference_failure("missing runtime class generic binding")
                            })
                        })
                        .collect::<Result<_, _>>()?,
                });
            }
            for (name, check) in narrowing_checks {
                let field = declared
                    .get(name)
                    .ok_or_else(|| super::inference_failure("missing guarded-field signature"))?;
                guards.push(crate::typed_ast::InstantiatedFieldGuard {
                    field: name.clone(),
                    target: substitute_typevars(&field_read_ty(field), bindings, &self.type_limits)
                        .map_err(type_limit_unlocated)?,
                    check: check.clone(),
                });
            }
            for (name, field) in declared {
                if accessors.iter().any(|accessor| accessor.name() == name) {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    fields.entry(name.clone())
                {
                    entry.insert(crate::ObjectField {
                        ty: substitute_typevars(&field.ty, bindings, &self.type_limits)
                            .map_err(type_limit_unlocated)?,
                        optional: field.optional,
                        readonly: field.readonly,
                    });
                }
            }
        }
        self.typed_ast
            .runtime_class_contexts
            .insert(ty.clone(), contexts);
        self.typed_ast
            .runtime_field_guards
            .insert(ty.clone(), guards.clone());
        for guard in guards {
            self.record_runtime_type_test_inner(
                &guard.target,
                RuntimeTestMode::AllowAliasRefs,
                active,
            )?;
        }
        let shape = Type::Object {
            index: None,
            fields,
        };
        self.typed_ast
            .runtime_class_fields
            .insert(ty.clone(), shape.clone());
        self.record_runtime_type_test_inner(&shape, RuntimeTestMode::AllowAliasRefs, active)?;
        active.remove(mangled);
        Ok(())
    }

    fn record_runtime_test_dependencies(
        &mut self,
        ty: &Type,
        active_interfaces: &mut BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        let mut record = |dependency: &Type| {
            self.record_runtime_type_test_inner(
                dependency,
                RuntimeTestMode::AllowAliasRefs,
                active_interfaces,
            )
        };
        match ty.peel() {
            Type::Array(element) => record(element)?,
            Type::Tuple(elements)
            | Type::Union(elements)
            | Type::ClassRef { args: elements, .. } => {
                for element in elements {
                    record(element)?;
                }
            }
            Type::Object { fields, index } => {
                if let Some(i) = index {
                    record(&i.value)?;
                }
                for field in fields.values() {
                    record(&field.ty)?;
                }
            }
            Type::Function { params, ret, .. } => {
                for param in params {
                    record(param)?;
                }
                record(ret)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn record_generic_interface_validators(
        &mut self,
        interface: &crate::InterfaceNarrowingTest,
        identity: &MangledName,
    ) -> Result<(), CompilerFailure> {
        let mut references = BTreeSet::new();
        for member in interface.members.values() {
            collect_interface_instantiations(&member.ty, identity, &mut references);
        }
        for reference in references {
            let Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } = &reference
            else {
                continue;
            };
            let Some(symbol) = self.resolver().lookup(mangled, name) else {
                continue;
            };
            let TypeKind::Interface { generics, .. } = &symbol.kind else {
                continue;
            };
            if args.is_empty() {
                continue;
            }
            let mut key = reference.clone();
            if let Type::InterfaceRef { args, .. } = &mut key {
                *args = generics.iter().cloned().map(Type::TypeVar).collect();
            }
            self.record_runtime_type_test_inner(
                &key,
                RuntimeTestMode::AllowAliasRefs,
                &mut BTreeSet::new(),
            )?;
        }
        Ok(())
    }

    fn narrowing_check_for(
        &self,
        class: &MangledName,
        field: &str,
    ) -> Option<Box<crate::FieldNarrowingCheck>> {
        self.field_narrowing_checks
            .get(&(class.clone(), field.to_string()))
            .cloned()
            .map(Box::new)
    }

    fn class_name_of(&mut self, mangled: &MangledName) -> String {
        self.class_by_mangled(mangled)
            .map_or_else(|| mangled.to_string(), |sym| sym.name.clone())
    }

    /// The class's own type parameters as fresh opaque [`Type::GenericParam`]s,
    /// keyed by name for [`substitute_typevars`]. Same guard
    /// `check_class_implements` uses: comparing at bare `TypeVar`s would make
    /// every parameterized member match everything.
    fn opaque_own_generics(
        &mut self,
        mangled: &MangledName,
    ) -> Result<BTreeMap<String, Type>, CompilerFailure> {
        let Some(sym) = self.class_by_mangled(mangled) else {
            return Ok(BTreeMap::new());
        };
        let TypeKind::Class { generics, .. } = &sym.kind else {
            return Ok(BTreeMap::new());
        };
        generics
            .clone()
            .into_iter()
            .map(|g| {
                let opaque = self.fresh_generic_param(&g)?;
                Ok((g, opaque))
            })
            .collect()
    }

    /// The mangled name of a class declared with `name`, if it resolves to a class.
    fn class_mangled(&self, name: &str) -> Option<MangledName> {
        let sym = self.types.lookup(name)?;
        matches!(sym.kind, TypeKind::Class { .. }).then(|| sym.mangled_name.clone())
    }

    /// Resolve a class symbol by its mangled name — current-module classes live
    /// in the type namespace, imported parents in the FQN registry.
    pub(super) fn class_by_mangled(&self, mangled: &MangledName) -> Option<TypeSymbol> {
        if let Some(sym) = self.type_registry.lookup(mangled)
            && matches!(sym.kind, TypeKind::Class { .. })
        {
            return Some(sym.clone());
        }
        self.types
            .iter_names()
            .filter_map(|n| self.types.lookup(n))
            .find(|s| &s.mangled_name == mangled && matches!(s.kind, TypeKind::Class { .. }))
            .cloned()
    }

    /// Walk the `extends` chain from `start` (instantiated at `start_args`),
    /// yielding each class symbol with the type-parameter bindings active AT
    /// that class. Cycle-guarded.
    ///
    /// `visit` returns [`ControlFlow::Continue`] to keep climbing, or
    /// `Break(answer)` to stop — `Break(None)` being "this class settles the
    /// question, with nothing to report" (a hidden member shadowing an
    /// inherited one, say).
    fn walk_class_chain<T>(
        &self,
        start: &MangledName,
        start_args: &[Type],
        visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>) -> ControlFlow<Option<T>>,
    ) -> Option<T> {
        walk_class_chain_with(
            |m| self.class_by_mangled(m),
            &self.type_limits,
            start,
            start_args,
            visit,
        )
    }

    /// [`walk_class_chain`](Self::walk_class_chain) starting at `child`'s
    /// parent — for questions about what a class *inherits*, where the class
    /// itself must not answer (override compatibility, an implicit
    /// constructor's inherited signature).
    fn walk_ancestors<T>(
        &self,
        child: &MangledName,
        visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>) -> ControlFlow<Option<T>>,
    ) -> Option<T> {
        let sym = self.class_by_mangled(child)?;
        self.walk_ancestors_at(child, &identity_bindings(&sym), visit)
    }

    /// [`walk_ancestors`](Self::walk_ancestors) with the child's own type
    /// parameters bound to something other than themselves. Comparing an
    /// inherited member against a redeclaration needs both sides instantiated
    /// at the *same* opaque parameters: left as bare `TypeVar`s, either side
    /// makes [`super::assignable`] answer `true` unconditionally.
    fn walk_ancestors_at<T>(
        &self,
        child: &MangledName,
        own_bindings: &BTreeMap<String, Type>,
        visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>) -> ControlFlow<Option<T>>,
    ) -> Option<T> {
        let sym = self.class_by_mangled(child)?;
        // The child stands at its own parameters, so its extends-args resolve
        // through them before the walk proper begins.
        let (parent, parent_args) = self
            .type_limits
            .ok_or_record(parent_hop(&sym, own_bindings, &self.type_limits))
            .flatten()?;
        self.walk_class_chain(&parent, &parent_args, visit)
    }

    /// True when this class or an ancestor failed to resolve its `extends`
    /// clause. That parent could have declared any member, so a member that
    /// looks missing here is unknowable rather than wrong — callers report
    /// nothing and resolve to [`Type::Error`] instead
    /// (see [`Inferer::unresolved_parents`]).
    pub(super) fn inherits_unresolved_parent(&self, class_mangled: &MangledName) -> bool {
        if self.unresolved_parents.is_empty() {
            return false;
        }
        self.walk_class_chain(class_mangled, &[], |sym, _| {
            if self.unresolved_parents.contains(&sym.mangled_name) {
                ControlFlow::Break(Some(()))
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_some()
    }

    /// [`inherits_unresolved_parent`](Self::inherits_unresolved_parent) for a
    /// receiver type: true only for a class instance with an unknowable
    /// inherited surface.
    pub(super) fn receiver_inherits_unresolved_parent(&self, receiver_ty: &Type) -> bool {
        match receiver_ty.peel() {
            Type::ClassRef { mangled, .. } => self.inherits_unresolved_parent(mangled),
            _ => false,
        }
    }

    /// [`inherits_unresolved_parent`](Self::inherits_unresolved_parent) for the
    /// class whose body is being checked — the question `super(...)` and
    /// `super.method(...)` ask before reporting a missing parent.
    fn current_class_inherits_unresolved_parent(&self) -> bool {
        self.current_class
            .as_ref()
            .is_some_and(|ty| self.receiver_inherits_unresolved_parent(ty))
    }

    /// A class field visible at the current access site: it exists on the class
    /// or an ancestor, and is either public or declared in the current module
    /// (module-scoped privacy). Returns `None` for a missing or hidden field.
    /// The field type comes back with the declaring class's generics
    /// substituted from `class_args`.
    pub(super) fn class_field_visible(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        field: &str,
    ) -> Option<(FieldSig, MangledName)> {
        self.walk_class_chain(class_mangled, class_args, |sym, bindings| {
            let TypeKind::Class { fields, .. } = &sym.kind else {
                return ControlFlow::Break(None);
            };
            let Some(f) = fields.get(field) else {
                return ControlFlow::Continue(());
            };
            let m = &sym.mangled_name;
            // A private field still shadows an inherited one of the same name:
            // the walk stops here either way.
            if f.visibility != Visibility::Public && !self.local_class_mangles.contains(m) {
                return ControlFlow::Break(None);
            }
            let Some(ty) = self
                .type_limits
                .ok_or_record(super::generic::substitute_typevars(
                    &f.ty,
                    bindings,
                    &self.type_limits,
                ))
            else {
                return ControlFlow::Break(None);
            };
            ControlFlow::Break(Some((FieldSig { ty, ..f.clone() }, m.clone())))
        })
    }

    /// Type of reading field `name` off a class receiver: the visible field's
    /// type, widened to `T | null` when optional. Reports the write-only case
    /// (setter, no getter) — a property with no readable value. Shared by
    /// `obj.f` and `obj?.f` so the two never disagree about what a class field
    /// reads as. `None` means no such visible field.
    pub(super) fn class_field_read_ty(
        &mut self,
        class_mangled: &MangledName,
        class_args: &[Type],
        name: &crate::Ident,
    ) -> Option<Type> {
        let (field, _decl) = self.class_field_visible(class_mangled, class_args, &name.name)?;
        // The getter wins over the synthesized field entry: a class declaring
        // only `set name` records the *setter's* parameter type there, while the
        // read runs an inherited getter whose return type is what comes back.
        if let Some(read) = self.class_getter(class_mangled, class_args, &name.name) {
            return Some(read);
        }
        if self
            .class_setter(class_mangled, class_args, &name.name)
            .is_some()
        {
            self.error_with_help(
                name.span,
                format!("property `{}` is write-only (no getter)", name.name),
                vec![format!("add a `get {}()` accessor to read it", name.name)],
            );
        }
        Some(field_read_ty(&field))
    }

    /// Reports reading a static member off an *instance* receiver, if a static
    /// of that name exists on the class or an ancestor. Shared by `obj.f` and
    /// `obj?.f` so both name the same fix instead of one falling back to a
    /// bare "no such field".
    pub(super) fn report_static_on_instance(
        &mut self,
        receiver_ty: &Type,
        class_mangled: &MangledName,
        name: &crate::Ident,
    ) -> bool {
        if self
            .class_static_in_chain(class_mangled, &name.name)
            .is_none()
        {
            return false;
        }
        let class_display = receiver_ty.peel().clone();
        // The help must be code that parses: a static is reached through the bare
        // class name, so `Box<number>.kind` — what the receiver's `Display` gives —
        // would hand the reader a parse error as its fix.
        let class_name = match receiver_ty.peel() {
            Type::ClassRef { name, .. } => name.clone(),
            other => other.to_string(),
        };
        self.error_with_help(
            name.span,
            format!(
                "`{}` is a static member of `{class_display}` — access it on the \
                 class, not an instance",
                name.name,
            ),
            vec![format!("write `{class_name}.{}`", name.name)],
        );
        true
    }

    /// A `readonly` data field is writable only through `this` in the
    /// constructor of the class that declares it.
    pub(super) fn readonly_write_allowed(
        &self,
        receiver: crate::ExprId,
        decl_mangled: &MangledName,
    ) -> Result<bool, CompilerFailure> {
        let current_class_mangled = match &self.current_class {
            Some(Type::ClassRef { mangled, .. }) => Some(mangled.clone()),
            _ => None,
        };
        Ok(self.in_constructor
            && matches!(
                self.ast
                    .try_expr(receiver)
                    .map_err(super::arena_failure)?
                    .kind,
                crate::ExprKind::This
            )
            && current_class_mangled.as_ref() == Some(decl_mangled))
    }

    /// The read and write types of class field or accessor `name`, for the
    /// read-modify-write forms (`x.f += v`, `x.f++`) which need both. Reports
    /// and returns `None` when the target is missing, one-sided (getter-only /
    /// setter-only), or optional. A `readonly` violation is reported but still
    /// yields the field's types, so the RHS keeps typechecking.
    pub(super) fn class_read_write_target(
        &mut self,
        mangled: &MangledName,
        class_args: &[Type],
        receiver: crate::ExprId,
        receiver_ty: &Type,
        name: &Ident,
        op: super::diagnostics::RwOp,
    ) -> Result<Option<FieldRw>, CompilerFailure> {
        let Some((field, decl_mangled)) = self.class_field_visible(mangled, class_args, &name.name)
        else {
            if !self.try_report_method_assignment(name.span, receiver_ty, &name.name) {
                let help = self.interface_member_miss_help(receiver_ty, &name.name);
                self.error_with_help(
                    name.span,
                    format!("field `{}` does not exist on `{receiver_ty}`", name.name),
                    help,
                );
            }
            return Ok(None);
        };
        // An accessor is readable iff it has a getter and writable iff it has a
        // setter; `(None, None)` is a data field, where the `readonly` ctor-only
        // rule applies instead.
        match (
            self.class_getter(mangled, class_args, &name.name),
            self.class_setter(mangled, class_args, &name.name),
        ) {
            (Some(read), Some(write)) => return Ok(Some(FieldRw { read, write })),
            (None, Some(_)) => {
                self.error_with_help(
                    name.span,
                    format!(
                        "property `{}` on `{receiver_ty}` is write-only (no getter)",
                        name.name
                    ),
                    vec![format!("add a `get {}()` accessor to read it", name.name)],
                );
                return Ok(None);
            }
            (Some(_), None) => {
                self.error_with_help(
                    name.span,
                    format!(
                        "cannot assign to read-only accessor `{}` on `{receiver_ty}`",
                        name.name,
                    ),
                    vec![format!(
                        "add a `set {}(v: T)` accessor to write it",
                        name.name
                    )],
                );
                return Ok(None);
            }
            (None, None) => {}
        }
        if field.readonly && !self.readonly_write_allowed(receiver, &decl_mangled)? {
            self.error_with_help(
                name.span,
                format!(
                    "cannot assign to readonly field `{}` on `{receiver_ty}`",
                    name.name,
                ),
                vec![
                    "a `readonly` field is writable only in the constructor of its declaring class"
                        .to_string(),
                ],
            );
        }
        // Reported here rather than left to the operator check, which would
        // blame `+=` without naming the null.
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        if self.try_report_nullable_rw_target(
            recv_span,
            receiver_ty,
            name,
            &field.ty,
            field.optional,
            op,
        ) {
            return Ok(None);
        }
        Ok(Some(FieldRw::uniform(field.ty.clone())))
    }

    /// The read (getter return) type of accessor property `prop`, searching up the
    /// `extends` chain. `None` if `prop` has no getter — a plain data field or a
    /// write-only accessor.
    pub(super) fn class_getter(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        prop: &str,
    ) -> Option<Type> {
        self.find_accessor(class_mangled, class_args, prop, |a| match a {
            AccessorSig::Getter { ret_ty, .. } => Some(ret_ty.clone()),
            AccessorSig::Setter { .. } => None,
        })
    }

    /// The write (setter parameter) type of accessor property `prop`, searching up
    /// the `extends` chain. `None` if `prop` has no setter — a plain data field or a
    /// read-only accessor.
    pub(super) fn class_setter(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        prop: &str,
    ) -> Option<Type> {
        self.find_accessor(class_mangled, class_args, prop, |a| match a {
            AccessorSig::Setter { param, .. } => Some(param.ty.clone()),
            AccessorSig::Getter { .. } => None,
        })
    }

    /// Walk the `extends` chain to the class that declares the requested half of
    /// accessor `prop` and `extract` its type, substituted at the declaring
    /// class's bindings.
    ///
    /// The two halves inherit independently, matching the vtable layout: a class
    /// that declares only `set prop` leaves `get prop` in the slot its ancestor
    /// filled, and a parent-typed receiver reads it. So a level declaring only
    /// the other half is not the end of the search. A plain data field of the
    /// same name *is* — it shadows any inherited accessor outright.
    fn find_accessor(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        prop: &str,
        extract: impl Fn(&AccessorSig) -> Option<Type>,
    ) -> Option<Type> {
        self.walk_class_chain(class_mangled, class_args, |sym, bindings| {
            let TypeKind::Class {
                fields, accessors, ..
            } = &sym.kind
            else {
                return ControlFlow::Break(None);
            };
            if let Some(ty) = accessors
                .iter()
                .filter(|a| a.name() == prop)
                .find_map(&extract)
            {
                return ControlFlow::Break(self.type_limits.ok_or_record(
                    super::generic::substitute_typevars(&ty, bindings, &self.type_limits),
                ));
            }
            // An accessor synthesizes a `fields` entry for typing, so the data-field
            // shadow only applies where this level declares neither half.
            let declares_other_half = accessors.iter().any(|a| a.name() == prop);
            if fields.contains_key(prop) && !declares_other_half {
                return ControlFlow::Break(None);
            }
            ControlFlow::Continue(())
        })
    }

    /// A class method resolved up the inheritance chain: its declared signature
    /// (unsubstituted — callers deciding boxing must see the raw `TypeVar`s),
    /// the type-parameter bindings active at the declaring class, the mangled
    /// name of that class, and the method's visibility.
    pub(super) fn class_method_in_chain(
        &self,
        class_mangled: &MangledName,
        class_args: &[Type],
        method: &str,
    ) -> Option<ResolvedMethod> {
        self.walk_class_chain(class_mangled, class_args, |sym, bindings| {
            let TypeKind::Class {
                methods,
                method_visibility,
                ..
            } = &sym.kind
            else {
                return ControlFlow::Break(None);
            };
            let Some(sig) = methods.get(method) else {
                return ControlFlow::Continue(());
            };
            ControlFlow::Break(Some(ResolvedMethod {
                sig: sig.clone(),
                bindings: bindings.clone(),
                declared_by: sym.mangled_name.clone(),
                visibility: method_visibility
                    .get(method)
                    .copied()
                    .unwrap_or(Visibility::Public),
            }))
        })
    }

    /// A static member resolved up the inheritance chain (TS hands statics down
    /// to subclasses): the resolution plus the mangled name of the *defining*
    /// class — the dispatch key is minted from the definer, so `B.f()` calls
    /// `A#static#f`.
    pub(super) fn class_static_in_chain(
        &self,
        class_mangled: &MangledName,
        member: &str,
    ) -> Option<(StaticResolution, MangledName)> {
        let mut cur = Some(class_mangled.clone());
        let mut seen: Vec<MangledName> = Vec::new();
        while let Some(m) = cur {
            if seen.contains(&m) {
                break;
            }
            seen.push(m.clone());
            let TypeKind::Class {
                statics,
                static_visibility,
                static_fields,
                extends,
                ..
            } = self.class_by_mangled(&m)?.kind
            else {
                break;
            };
            if let Some(sig) = statics.get(member) {
                let vis = static_visibility
                    .get(member)
                    .copied()
                    .unwrap_or(Visibility::Public);
                return Some((StaticResolution::Method(sig.clone(), vis), m));
            }
            if let Some(field) = static_fields.get(member) {
                return Some((StaticResolution::Field(field.clone()), m));
            }
            cur = extends.map(|e| e.parent);
        }
        None
    }

    /// Whether the chain declares an *instance* member (field, method, or
    /// accessor-backed property) of this name — used to steer the
    /// instance-member-on-class-object diagnostic.
    pub(super) fn class_instance_member_in_chain(
        &self,
        class_mangled: &MangledName,
        member: &str,
    ) -> bool {
        let mut cur = Some(class_mangled.clone());
        let mut seen: Vec<MangledName> = Vec::new();
        while let Some(m) = cur {
            if seen.contains(&m) {
                break;
            }
            seen.push(m.clone());
            let Some(sym) = self.class_by_mangled(&m) else {
                break;
            };
            let TypeKind::Class {
                fields,
                methods,
                extends,
                ..
            } = sym.kind
            else {
                break;
            };
            if fields.contains_key(member) || methods.contains_key(member) {
                return true;
            }
            cur = extends.map(|e| e.parent);
        }
        false
    }

    /// All static member names visible on the class (own + inherited), for
    /// did-you-mean and the bare-class-name help.
    pub(super) fn class_static_names(&self, class_mangled: &MangledName) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut cur = Some(class_mangled.clone());
        let mut seen: Vec<MangledName> = Vec::new();
        while let Some(m) = cur {
            if seen.contains(&m) {
                break;
            }
            seen.push(m.clone());
            let Some(sym) = self.class_by_mangled(&m) else {
                break;
            };
            let TypeKind::Class {
                statics,
                static_fields,
                extends,
                ..
            } = sym.kind
            else {
                break;
            };
            names.extend(statics.keys().cloned());
            names.extend(static_fields.keys().cloned());
            cur = extends.map(|e| e.parent);
        }
        names.sort();
        names.dedup();
        names
    }

    /// Typecheck `super(...)`: delegate to the parent constructor. Valid only in
    /// a constructor body of a class with a parent.
    pub(super) fn infer_super_call(
        &mut self,
        args: Vec<crate::ExprId>,
        span: Span,
    ) -> Result<(crate::TypedExprKind, Type), CompilerFailure> {
        // Taken before the arguments are inferred, so a `super(...)` among
        // them isn't mistaken for the statement.
        let is_statement = std::mem::take(&mut self.super_call_is_statement);
        self.check_super_call_position(span, is_statement);

        let parent = self.current_super.clone();
        // The parent's ctor params, substituted at the extends clause's type
        // args (`class Tagged<T> extends Box<T>` checks `super(v)` against
        // `Box`'s ctor with its generics bound to the clause's args).
        let ctor_params = match parent
            .as_ref()
            .and_then(|e| self.class_by_mangled(&e.parent).map(|sym| (e, sym)))
        {
            Some((e, sym)) => match sym.kind {
                TypeKind::Class {
                    constructor,
                    generics,
                    ..
                } => {
                    if generics.len() != e.args.len() {
                        return Err(super::inference_failure(
                            "parent class generic argument mismatch",
                        ));
                    }
                    let bindings: BTreeMap<String, Type> = generics
                        .iter()
                        .cloned()
                        .zip(e.args.iter().cloned())
                        .collect();
                    Some(
                        constructor
                            .iter()
                            .map(|p| {
                                // Extends args may name the child's own type
                                // params — map those to the body's live GPs.
                                let ty = super::generic::substitute_typevars(
                                    &p.ty,
                                    &bindings,
                                    &self.type_limits,
                                )
                                .and_then(|ty| self.apply_body_instantiations(&ty))
                                .map_err(type_limit_at(span))?;
                                Ok(Param { ty, ..p.clone() })
                            })
                            .collect::<Result<Vec<_>, CompilerFailure>>()?,
                    )
                }
                _ => None,
            },
            None => None,
        };
        let (Some(params), Some(parent)) = (ctor_params, parent) else {
            if self.in_constructor && !self.current_class_inherits_unresolved_parent() {
                self.error_with_help(
                    span,
                    "`super(...)` requires a parent class".to_string(),
                    vec!["only a class with an `extends` clause can call `super(...)`".to_string()],
                );
            }
            let outer = std::mem::replace(&mut self.in_super_arguments, true);
            for arg in &args {
                let _ = self.infer_expr(*arg, None)?;
            }
            self.in_super_arguments = outer;
            return Ok((crate::TypedExprKind::Null, Type::Void));
        };
        let parent_ty = self.super_receiver_type(&parent)?;
        let outer = std::mem::replace(&mut self.in_super_arguments, true);
        let typed_args = self.bind_param_call_args(
            &params,
            &Type::Void,
            super::expr::CallLift::Constructor {
                class_ty: &parent_ty,
            },
            &args,
            span,
        )?;
        self.in_super_arguments = outer;
        Ok((
            crate::TypedExprKind::SuperCtorCall {
                parent: parent.parent,
                args: typed_args,
            },
            Type::Void,
        ))
    }

    /// Report a `super(...)` outside a subclass constructor's own body, or
    /// not a statement of its own. Record any call in that body for the
    /// once-only and read-before-`super` checks.
    fn check_super_call_position(&mut self, span: Span, is_statement: bool) {
        if !self.in_constructor {
            self.error_with_help(
                span,
                "`super(...)` is only valid inside a constructor".to_string(),
                vec!["call the parent constructor from this class's `constructor`".to_string()],
            );
            return;
        }
        // A class with no `extends` clause is told so by `infer_super_call`
        // instead. One whose parent failed to resolve is still checked.
        if self.current_super.is_none() && !self.current_class_inherits_unresolved_parent() {
            return;
        }
        if self.in_nested_function {
            // Not the constructor's own call: it can run late or never, so it
            // neither counts toward `super_seen` nor ends the window before it.
            self.error_with_help(
                span,
                "`super(...)` can't be called from a function nested in a constructor".to_string(),
                vec!["call the parent constructor directly in the `constructor` body".to_string()],
            );
            return;
        }
        // Still recorded below: the call is made, so the end-of-body check
        // mustn't also report it missing.
        if !is_statement {
            self.error_with_help(
                span,
                "`super(...)` must be a statement of its own".to_string(),
                vec!["write `super(...);` on its own line, so it runs on every path".to_string()],
            );
        }
        self.note_constructor_super_call(span);
    }

    /// A read of `this`, or of a `super` member, in a subclass constructor.
    /// Until `super(...)` returns the instance isn't built: a read in the
    /// call's own arguments, or in a `catch` or `finally` around it, is
    /// reported here, and one before the call is flagged for the call to
    /// report. A read inside an arrow or nested function counts too, since
    /// nothing stops the parent's constructor, or the handler, calling it
    /// early. A function expression is skipped: it clears `current_class`
    /// because its `this` is its own, and `super` in it is a parse error.
    pub(super) fn note_read_before_super(&mut self, span: Span) {
        if !self.in_constructor || self.current_super.is_none() || self.current_class.is_none() {
            return;
        }
        if self.in_super_arguments {
            self.error_with_help(
                span,
                "the arguments of `super(...)` can't read `this` or a `super` member".to_string(),
                vec!["compute the argument from the constructor's parameters".to_string()],
            );
            return;
        }
        if self.in_super_handler {
            self.error_with_help(
                span,
                "a `catch` or `finally` around `super(...)` can't read `this` or a `super` member"
                    .to_string(),
                vec![
                    "it also runs when `super(...)` throws, before the instance is built"
                        .to_string(),
                ],
            );
            return;
        }
        if !self.super_seen {
            self.read_before_super = true;
        }
    }

    /// Whether `expr` is a `super(...)` call, parenthesized or not, as opposed
    /// to one inside it.
    pub(super) fn is_super_call(&self, expr: crate::ExprId) -> Result<bool, CompilerFailure> {
        match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
            crate::ExprKind::Paren(inner) => self.is_super_call(*inner),
            crate::ExprKind::Call { callee, .. } => Ok(matches!(
                self.ast
                    .try_expr(*callee)
                    .map_err(super::arena_failure)?
                    .kind,
                crate::ExprKind::Super
            )),
            _ => Ok(false),
        }
    }

    /// A subclass constructor may call `super(...)` exactly once, before any
    /// read of `this` or a `super` member. Record that we've seen it for the end-of-body check.
    fn note_constructor_super_call(&mut self, span: Span) {
        if self.super_seen {
            self.error(span, "`super(...)` may only be called once".to_string());
        }
        if self.read_before_super {
            self.error_with_help(
                span,
                "`super(...)` must be called before accessing `this` or a `super` member"
                    .to_string(),
                vec!["move the `super(...)` call to the top of the constructor".to_string()],
            );
        }
        self.super_seen = true;
    }

    /// Typecheck `super.method(...)`: resolve the method on the parent chain and
    /// emit a direct call of the declaring class's body. Valid only inside a
    /// subclass body.
    pub(super) fn infer_super_method_call(
        &mut self,
        name: crate::Ident,
        args: Vec<crate::ExprId>,
        span: Span,
    ) -> Result<(crate::TypedExprKind, Type), CompilerFailure> {
        self.note_read_before_super(span);
        let Some(parent) = self.current_super.clone() else {
            if !self.current_class_inherits_unresolved_parent() {
                self.error_with_help(
                    span,
                    "`super.method(...)` is only valid inside a subclass".to_string(),
                    vec![
                        "only a class with an `extends` clause can call `super.method(...)`"
                            .to_string(),
                    ],
                );
            }
            for arg in &args {
                let _ = self.infer_expr(*arg, None)?;
            }
            return Ok((crate::TypedExprKind::Null, Type::Error));
        };
        let Some(resolved) = self.class_method_in_chain(&parent.parent, &parent.args, &name.name)
        else {
            if !self.inherits_unresolved_parent(&parent.parent) {
                self.error(
                    span,
                    format!("no method `{}` on the parent class", name.name),
                );
            }
            for arg in &args {
                let _ = self.infer_expr(*arg, None)?;
            }
            return Ok((crate::TypedExprKind::Null, Type::Error));
        };
        let ResolvedMethod {
            sig,
            bindings,
            declared_by: owner,
            ..
        } = resolved;
        if !sig.generics.is_empty() {
            self.error(
                span,
                format!(
                    "`super.{}(...)` on a generic method is not supported yet",
                    name.name
                ),
            );
        }
        let sig = substitute_method_sig(&sig, &bindings, &self.type_limits)
            .map_err(type_limit_at(span))?;
        let sig = MethodSig {
            params: sig
                .params
                .iter()
                .map(|p| {
                    Ok(Param {
                        ty: self.apply_body_instantiations(&p.ty)?,
                        ..p.clone()
                    })
                })
                .collect::<Result<_, TypeTooLarge>>()
                .map_err(type_limit_at(span))?,
            ret: self
                .apply_body_instantiations(&sig.ret)
                .map_err(type_limit_at(span))?,
            ..sig
        };
        let parent_ty = self.super_receiver_type(&parent)?;
        let typed_args = self.bind_param_call_args(
            &sig.params,
            &sig.ret,
            super::expr::CallLift::Method {
                receiver_ty: &parent_ty,
                name: &name.name,
                sig: &sig,
            },
            &args,
            span,
        )?;
        Ok((
            crate::TypedExprKind::SuperMethodCall {
                owner,
                name,
                args: typed_args,
            },
            sig.ret,
        ))
    }

    fn super_receiver_type(&self, parent: &crate::ClassExtends) -> Result<Type, CompilerFailure> {
        let sym = self
            .class_by_mangled(&parent.parent)
            .ok_or_else(|| super::inference_failure("missing resolved parent class"))?;
        Ok(Type::class_ref(
            self.type_package(&sym.name),
            sym.name.clone(),
            parent.parent.clone(),
            parent
                .args
                .iter()
                .map(|ty| self.apply_body_instantiations(ty))
                .collect::<Result<_, _>>()
                .map_err(type_limit_unlocated)?,
        ))
    }

    fn class_parent(&self, mangled: &MangledName) -> Option<MangledName> {
        match self.class_by_mangled(mangled)?.kind {
            TypeKind::Class { extends, .. } => extends.map(|e| e.parent),
            _ => None,
        }
    }

    fn class_method_sig(&self, mangled: &MangledName, method: &str) -> Option<MethodSig> {
        match self.class_by_mangled(mangled)?.kind {
            TypeKind::Class { methods, .. } => methods.get(method).cloned(),
            _ => None,
        }
    }

    /// Nearest ancestor's signature for `method`, walking the `extends` chain
    /// (cycle-guarded), substituted at the bindings the child's extends clause
    /// instantiates — so an override check compares against the parent sig as
    /// the child actually sees it. `None` when no ancestor declares it.
    fn ancestor_method_sig(
        &self,
        child: &MangledName,
        method: &str,
        own_bindings: &BTreeMap<String, Type>,
    ) -> Option<MethodSig> {
        self.walk_ancestors_at(child, own_bindings, |sym, bindings| {
            let TypeKind::Class { methods, .. } = &sym.kind else {
                return ControlFlow::Continue(());
            };
            match methods.get(method) {
                Some(sig) => {
                    ControlFlow::Break(self.type_limits.ok_or_record(substitute_method_sig(
                        sig,
                        bindings,
                        &self.type_limits,
                    )))
                }
                None => ControlFlow::Continue(()),
            }
        })
    }

    /// The class's *own* [`data_field`], ignoring anything inherited. Statics
    /// live in a separate map and never resolve here.
    fn class_own_field_sig(&self, mangled: &MangledName, field: &str) -> Option<FieldSig> {
        data_field(&self.class_by_mangled(mangled)?, field).cloned()
    }

    /// The *widest* ancestor declaring `field` — the farthest one up the chain.
    /// Declarations narrow monotonically downward, so this is the type through
    /// which the largest set of values can be written into the shared slot, and
    /// therefore the one a read guard has to defend against.
    /// [`ancestor_field_decl`](Self::ancestor_field_decl) answers the
    /// redeclaration *rules*, which are about the adjacent pair.
    fn widest_ancestor_field_decl(
        &self,
        child: &MangledName,
        field: &str,
        own_bindings: &BTreeMap<String, Type>,
    ) -> Option<(FieldSig, MangledName)> {
        let mut widest = None;
        self.walk_ancestors_at::<()>(child, own_bindings, |sym, bindings| {
            if let Some(sig) = data_field(sym, field) {
                let Some(ty) = self.type_limits.ok_or_record(substitute_typevars(
                    &sig.ty,
                    bindings,
                    &self.type_limits,
                )) else {
                    widest = None;
                    return ControlFlow::Break(None);
                };
                widest = Some((FieldSig { ty, ..sig.clone() }, sym.mangled_name.clone()));
            }
            ControlFlow::Continue(())
        });
        widest
    }

    /// Nearest ancestor declaring `field` as a data field, with the class that
    /// declares it, substituted at the bindings the child's `extends` clause
    /// instantiates. The counterpart of
    /// [`ancestor_method_sig`](Self::ancestor_method_sig) for data fields.
    fn ancestor_field_decl(
        &self,
        child: &MangledName,
        field: &str,
        own_bindings: &BTreeMap<String, Type>,
    ) -> Option<(FieldSig, MangledName)> {
        self.walk_ancestors_at(child, own_bindings, |sym, bindings| {
            match data_field(sym, field) {
                Some(sig) => {
                    let ty = self.type_limits.ok_or_record(substitute_typevars(
                        &sig.ty,
                        bindings,
                        &self.type_limits,
                    ));
                    ControlFlow::Break(
                        ty.map(|ty| (FieldSig { ty, ..sig.clone() }, sym.mangled_name.clone())),
                    )
                }
                None => ControlFlow::Continue(()),
            }
        })
    }

    fn declaration_was_rejected(&self, span: Span) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == crate::Severity::Error && diagnostic.span == span
        })
    }

    /// Typecheck every class's field initializers, method bodies, and
    /// constructor body, then mirror the result into the typed AST. Runs after
    /// `infer_functions` so bodies can construct sibling classes and call
    /// top-level functions.
    pub(super) fn infer_classes(&mut self) -> Result<(), CompilerFailure> {
        let top_level: Vec<_> = self.ast.top_level.clone();
        for stmt_id in top_level {
            let stmt = self
                .ast
                .try_stmt(stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            let StmtKind::ClassDecl {
                name,
                extends: _,
                implements: _,
                members,
                doc,
                ..
            } = stmt.kind
            else {
                continue;
            };
            if self.rejected_class_names.contains(&name.name) {
                continue;
            }
            let Some(sym) = self.types.lookup(&name.name) else {
                if self.declaration_was_rejected(name.span) {
                    continue;
                }
                return Err(
                    super::inference_failure("missing class signature").with_span(name.span)
                );
            };
            let TypeKind::Class {
                generics: class_generics,
                fields: field_sigs,
                methods: method_sigs,
                method_visibility,
                statics: static_sigs,
                constructor: ctor_params,
                extends: parent,
                implements,
                ..
            } = sym.kind.clone()
            else {
                if self.declaration_was_rejected(name.span) {
                    continue;
                }
                return Err(
                    super::inference_failure("class signature has the wrong symbol kind")
                        .with_span(name.span),
                );
            };
            let mangled = sym.mangled_name.clone();
            self.typed_ast
                .runtime_class_parameters
                .insert(mangled.clone(), class_generics.clone());

            // Static bodies run without an instance: checked before `current_class`
            // is set so `this`/`super` get the static-specific rejections — and
            // before the class generics enter body scope (a static can't
            // reference them).
            self.check_static_methods(&members, &static_sigs, &mangled, &name.name)?;

            // Class-level generics live for the whole set of instance bodies:
            // `this` is the class instantiated at its own params
            // (`Box<GP(T)>`), so `this.value: T` reads as the body's `T` and
            // `T` / `Box<T>` annotations inside bodies resolve. Member
            // signature types entering body scopes go through `class_inst`
            // (TypeVar → GenericParam); typed-AST decl types keep the raw
            // signature TypeVar forms codegen expects.
            let class_inst = if class_generics.is_empty() {
                BTreeMap::new()
            } else {
                self.push_body_generics(class_generics.clone())?
            };
            let class_ty = Type::class_ref(
                crate::Package(self.package_name.to_string()),
                name.name.clone(),
                mangled.clone(),
                class_generics
                    .iter()
                    .map(|g| {
                        class_inst.get(g).cloned().ok_or_else(|| {
                            super::inference_failure("missing class body generic binding")
                        })
                    })
                    .collect::<Result<_, _>>()?,
            );

            let prev_class = self.current_class.replace(class_ty.clone());
            let prev_super = std::mem::replace(&mut self.current_super, parent.clone());
            let exprs_before = self.typed_ast.exprs_len();
            let stmts_before = self.typed_ast.stmts_len();

            let typed_fields =
                self.check_class_fields(&members, &mangled, &field_sigs, &class_inst)?;
            let typed_ctor = self.check_constructor(&members, &ctor_params, &class_inst)?;
            let typed_methods =
                self.check_class_methods(&members, &method_sigs, &method_visibility, &class_inst)?;
            let accessors = self.check_class_accessors(&members, &field_sigs)?;

            self.current_class = prev_class;
            self.current_super = prev_super;
            if !class_generics.is_empty() {
                self.pop_body_generics();
                // Erase the class-level GPs from every body typed here —
                // ctor, field initializers, and accessors don't have their own
                // erasure loops (methods do; a second pass over their ranges
                // is a no-op).
                for id in self
                    .typed_ast
                    .expr_ids()
                    .map_err(crate::typechecker::arena_failure)?
                    .skip(exprs_before)
                {
                    erase_generic_params_in_expr(
                        self.typed_ast
                            .try_expr_mut(id)
                            .map_err(crate::typechecker::arena_failure)?,
                    );
                }
                for id in self
                    .typed_ast
                    .stmt_ids()
                    .map_err(crate::typechecker::arena_failure)?
                    .skip(stmts_before)
                {
                    erase_generic_params_in_stmt(
                        self.typed_ast
                            .try_stmt_mut(id)
                            .map_err(crate::typechecker::arena_failure)?,
                    );
                }
            }

            let decl = TypedClassDecl {
                name: name.clone(),
                fields: typed_fields,
                inherited_ctor_params: if typed_ctor.is_some() {
                    Vec::new()
                } else {
                    ctor_params
                        .iter()
                        .map(|p| TypedParam {
                            name: crate::Ident {
                                name: p.name.clone(),
                                span: name.span,
                            },
                            ty: p.ty.clone(),
                            boxed: false,
                            rest: p.rest,
                            default: p.default.clone(),
                        })
                        .collect()
                },
                constructor: typed_ctor,
                methods: typed_methods,
                accessors,
                // Codegen is generics-erased: the typed AST carries only the
                // parent's name, never the extends args.
                extends: parent.map(|e| e.parent),
                implements,
                mangled_name: mangled,
                doc,
            };
            self.add_typed_type_decl(
                crate::TypedTypeDecl::Class(decl),
                sym_for_export(self, &name)?,
            )?;
            self.type_size_checkpoint(Some(name.span))?;
        }

        Ok(())
    }

    fn check_class_fields(
        &mut self,
        members: &[ClassMember],
        mangled: &MangledName,
        field_sigs: &BTreeMap<String, FieldSig>,
        class_inst: &BTreeMap<String, Type>,
    ) -> Result<Vec<TypedClassField>, CompilerFailure> {
        let mut out = Vec::new();
        for member in members {
            let ClassMember::Field {
                name,
                modifiers,
                initializer,
                doc,
                ..
            } = member
            else {
                continue;
            };
            // Statics live in a separate namespace and may shadow an instance
            // field's name; their initializers are checked as module globals.
            if modifiers.static_span.is_some() {
                continue;
            }
            let Some(sig) = field_sigs.get(&name.name) else {
                if self.declaration_was_rejected(name.span) {
                    continue;
                }
                return Err(
                    super::inference_failure("missing class member signature").with_span(name.span)
                );
            };
            // Field initializers may read `this`; `current_class` is already set.
            let body_ty = substitute_typevars(&sig.ty, class_inst, &self.type_limits)
                .map_err(type_limit_at(name.span))?;
            let typed_init = initializer
                .map(|expr| {
                    let (typed, value_ty) = self.infer_expr(expr, Some(&body_ty))?;
                    if !super::assignable(&value_ty, &body_ty, self.resolver())
                        && !matches!(value_ty, Type::Error)
                    {
                        let span = self.ast.try_expr(expr).map_err(super::arena_failure)?.span;
                        self.error(
                            span,
                            format!(
                                "field `{}` initializer is `{value_ty}`, expected `{}`",
                                name.name, sig.ty
                            ),
                        );
                    }
                    Ok::<_, CompilerFailure>(typed)
                })
                .transpose()?;
            out.push(TypedClassField {
                name: name.clone(),
                ty: sig.ty.clone(),
                visibility: sig.visibility,
                readonly: sig.readonly,
                optional: sig.optional,
                initializer: typed_init,
                auto_assigned: false,
                narrowing_check: self.narrowing_check_for(mangled, &name.name),
                doc: doc.clone(),
            });
        }
        // Parameter properties: declared on the constructor, assigned from the
        // param. Materialized as auto-assigned fields (no initializer) so they
        // get struct slots and satisfy interface conformance like a plain field.
        if let Some(ClassMember::Constructor { params, .. }) = members
            .iter()
            .find(|m| matches!(m, ClassMember::Constructor { .. }))
        {
            for param in params {
                if param.modifiers.is_none() {
                    continue;
                }
                let Some(sig) = field_sigs.get(&param.name.name) else {
                    if self.declaration_was_rejected(param.name.span) {
                        continue;
                    }
                    return Err(
                        super::inference_failure("missing parameter-property signature")
                            .with_span(param.name.span),
                    );
                };
                out.push(TypedClassField {
                    name: param.name.clone(),
                    ty: sig.ty.clone(),
                    visibility: sig.visibility,
                    readonly: sig.readonly,
                    optional: sig.optional,
                    initializer: None,
                    auto_assigned: true,
                    narrowing_check: self.narrowing_check_for(mangled, &param.name.name),
                    doc: None,
                });
            }
        }
        Ok(out)
    }

    fn check_constructor(
        &mut self,
        members: &[ClassMember],
        ctor_params: &[Param],
        class_inst: &BTreeMap<String, Type>,
    ) -> Result<Option<TypedClassConstructor>, CompilerFailure> {
        let Some((params, body)) = members.iter().find_map(|m| match m {
            ClassMember::Constructor { params, body, .. } => Some((params, *body)),
            _ => None,
        }) else {
            return Ok(None);
        };

        let typed_params = bind_params_for_body(self, params, ctor_params, class_inst)?;
        let prev_in_ctor = std::mem::replace(&mut self.in_constructor, true);
        let prev_super_seen = std::mem::replace(&mut self.super_seen, false);
        let prev_read_before = std::mem::replace(&mut self.read_before_super, false);
        let prev_nested = std::mem::replace(&mut self.in_nested_function, false);
        // A constructor returns no value; a bare `return;` is fine.
        let prev_return = self.current_return.replace(Type::Void);
        let prev_reachable = std::mem::replace(&mut self.reachable, true);
        let body_id = self
            .infer_body_with_narrowing_boundary(body)?
            .ok_or_else(|| super::inference_failure("constructor body is a Block"))?;
        // A subclass constructor must initialize the parent via `super(...)`.
        if self.current_super.is_some() && !self.super_seen {
            let ctor_span = members
                .iter()
                .find_map(|m| match m {
                    ClassMember::Constructor { span, .. } => Some(*span),
                    _ => None,
                })
                .map_or_else(
                    || {
                        Ok::<_, CompilerFailure>(
                            self.ast.try_stmt(body).map_err(super::arena_failure)?.span,
                        )
                    },
                    Ok,
                )?;
            self.error_with_help(
                ctor_span,
                "a subclass constructor must call `super(...)`".to_string(),
                vec![
                    "call the parent constructor with `super(...)` before using `this`".to_string(),
                ],
            );
        }
        self.current_return = prev_return;
        self.reachable = prev_reachable;
        self.in_constructor = prev_in_ctor;
        self.super_seen = prev_super_seen;
        self.read_before_super = prev_read_before;
        self.in_nested_function = prev_nested;
        self.scopes.pop();

        Ok(Some(TypedClassConstructor {
            params: typed_params,
            body: body_id,
        }))
    }

    fn check_class_methods(
        &mut self,
        members: &[ClassMember],
        method_sigs: &BTreeMap<String, MethodSig>,
        method_visibility: &BTreeMap<String, Visibility>,
        class_inst: &BTreeMap<String, Type>,
    ) -> Result<Vec<TypedClassMethod>, CompilerFailure> {
        let mut out = Vec::new();
        for member in members {
            let ClassMember::Method {
                name,
                modifiers,
                params,
                body,
                doc,
                ..
            } = member
            else {
                continue;
            };
            // A static may share an instance method's name — don't let the name
            // lookup bind its body against the instance signature.
            if modifiers.static_span.is_some() {
                continue;
            }
            let Some(sig) = method_sigs.get(&name.name) else {
                if self.declaration_was_rejected(name.span) {
                    continue;
                }
                return Err(
                    super::inference_failure("missing class member signature").with_span(name.span)
                );
            };

            if params.len() != sig.params.len() && !self.declaration_was_rejected(name.span) {
                return Err(
                    super::inference_failure("method parameter/signature length mismatch")
                        .with_span(name.span),
                );
            }
            let body_instantiation = self.push_body_generics(sig.generics.clone())?;
            // Method-level generics shadow class-level ones of the same name.
            let mut merged = class_inst.clone();
            merged.extend(body_instantiation);
            let body_param_types: Vec<Type> = sig
                .params
                .iter()
                .map(|p| substitute_typevars(&p.ty, &merged, &self.type_limits))
                .collect::<Result<_, _>>()
                .map_err(type_limit_at(name.span))?;
            let body_ret = substitute_typevars(&sig.ret, &merged, &self.type_limits)
                .map_err(type_limit_at(name.span))?;

            let typed_params: Vec<TypedParam> = params
                .iter()
                .zip(sig.params.iter())
                .map(|(p, rp)| TypedParam {
                    name: p.name.clone(),
                    ty: rp.ty.clone(),
                    boxed: false,
                    rest: rp.rest,
                    default: rp.default.clone(),
                })
                .collect();

            self.scopes.push();
            for (p, body_ty) in params.iter().zip(body_param_types.iter()) {
                self.scopes
                    .insert(p.name.name.clone(), body_ty.clone(), false, p.name.span);
            }
            let prev_return = self.current_return.replace(body_ret);
            let prev_reachable = std::mem::replace(&mut self.reachable, true);
            let exprs_before = self.typed_ast.exprs_len();
            let stmts_before = self.typed_ast.stmts_len();
            let body_id = self
                .infer_body_with_narrowing_boundary(*body)?
                .ok_or_else(|| super::inference_failure("method body is a Block"))?;
            self.current_return = prev_return;
            self.reachable = prev_reachable;
            self.scopes.pop();
            self.pop_body_generics();
            for id in self
                .typed_ast
                .expr_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(exprs_before)
            {
                erase_generic_params_in_expr(
                    self.typed_ast
                        .try_expr_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }
            for id in self
                .typed_ast
                .stmt_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(stmts_before)
            {
                erase_generic_params_in_stmt(
                    self.typed_ast
                        .try_stmt_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }

            out.push(TypedClassMethod {
                name: name.clone(),
                generics: sig.generics.clone(),
                params: typed_params,
                return_type: sig.ret.clone(),
                body: body_id,
                visibility: method_visibility
                    .get(&name.name)
                    .copied()
                    .unwrap_or(Visibility::Public),
                doc: doc.clone(),
            });
        }
        Ok(out)
    }

    /// Typecheck each static method body into an ordinary [`crate::TypedFunction`]
    /// keyed `Class#static#name` — codegen, DWARF, and the export surface treat a
    /// static as a plain top-level function. Runs with `current_class` unset (no
    /// receiver); `current_static` routes `this` to its tailored diagnostic.
    fn check_static_methods(
        &mut self,
        members: &[ClassMember],
        static_sigs: &BTreeMap<String, MethodSig>,
        class_mangled: &MangledName,
        class_name: &str,
    ) -> Result<(), CompilerFailure> {
        for member in members {
            let ClassMember::Method {
                name,
                modifiers,
                params,
                body,
                doc,
                span,
                ..
            } = member
            else {
                continue;
            };
            if modifiers.static_span.is_none() {
                continue;
            }
            let Some(sig) = static_sigs.get(&name.name) else {
                if self.declaration_was_rejected(name.span) {
                    continue;
                }
                return Err(
                    super::inference_failure("missing class member signature").with_span(name.span)
                );
            };

            if params.len() != sig.params.len() && !self.declaration_was_rejected(name.span) {
                return Err(
                    super::inference_failure("method parameter/signature length mismatch")
                        .with_span(name.span),
                );
            }
            let body_instantiation = self.push_body_generics(sig.generics.clone())?;
            let body_param_types: Vec<Type> = sig
                .params
                .iter()
                .map(|p| substitute_typevars(&p.ty, &body_instantiation, &self.type_limits))
                .collect::<Result<_, _>>()
                .map_err(type_limit_at(name.span))?;
            let body_ret = substitute_typevars(&sig.ret, &body_instantiation, &self.type_limits)
                .map_err(type_limit_at(name.span))?;
            let typed_params: Vec<TypedParam> = params
                .iter()
                .zip(sig.params.iter())
                .map(|(p, rp)| TypedParam {
                    name: p.name.clone(),
                    ty: rp.ty.clone(),
                    boxed: false,
                    rest: rp.rest,
                    default: rp.default.clone(),
                })
                .collect();

            self.scopes.push();
            for (p, body_ty) in params.iter().zip(body_param_types.iter()) {
                self.scopes
                    .insert(p.name.name.clone(), body_ty.clone(), false, p.name.span);
            }
            let prev_return = self.current_return.replace(body_ret);
            let prev_reachable = std::mem::replace(&mut self.reachable, true);
            let prev_static = self
                .current_static
                .replace((class_name.to_string(), name.name.clone()));
            let exprs_before = self.typed_ast.exprs_len();
            let stmts_before = self.typed_ast.stmts_len();
            let body_id = self
                .infer_body_with_narrowing_boundary(*body)?
                .ok_or_else(|| super::inference_failure("static method body is a Block"))?;
            self.current_return = prev_return;
            self.reachable = prev_reachable;
            self.current_static = prev_static;
            self.scopes.pop();
            self.pop_body_generics();
            for id in self
                .typed_ast
                .expr_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(exprs_before)
            {
                erase_generic_params_in_expr(
                    self.typed_ast
                        .try_expr_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }
            for id in self
                .typed_ast
                .stmt_ids()
                .map_err(crate::typechecker::arena_failure)?
                .skip(stmts_before)
            {
                erase_generic_params_in_stmt(
                    self.typed_ast
                        .try_stmt_mut(id)
                        .map_err(crate::typechecker::arena_failure)?,
                );
            }

            self.add_typed_function(crate::TypedFunction {
                name: Ident {
                    name: format!("{class_name}.{}", name.name),
                    span: name.span,
                },
                mangled_name: crate::mangle::static_member(class_mangled, &name.name),
                generics: sig.generics.clone(),
                params: typed_params,
                return_type: sig.ret.clone(),
                type_predicate: None,
                body: body_id,
                doc: doc.clone(),
                span: *span,
            })?;
        }

        Ok(())
    }

    /// Typecheck each accessor body into a [`TypedClassAccessor`] (one per
    /// `get`/`set`). A getter returns its own read type; a setter takes its own
    /// write type — the two are independent. Codegen lowers each to a vtable
    /// method — they never become `TypedClassMethod`s.
    fn check_class_accessors(
        &mut self,
        members: &[ClassMember],
        field_sigs: &BTreeMap<String, FieldSig>,
    ) -> Result<Vec<TypedClassAccessor>, CompilerFailure> {
        let mut accessors: Vec<TypedClassAccessor> = Vec::new();
        for member in members {
            let ClassMember::Accessor {
                name,
                kind,
                modifiers,
                param,
                return_type,
                body,
                ..
            } = member
            else {
                continue;
            };
            // Duplicate members may have no property signature. Their annotations
            // and bodies still need checking to report independent errors.
            let visibility = field_sigs
                .get(&name.name)
                .map_or(modifiers.visibility, |sig| sig.visibility);
            match kind {
                AccessorKind::Get => {
                    // Annotations resolve inside the class's body-generics
                    // scope (`T` → GenericParam) so the body checks against
                    // the live form; the stored decl type is erased back to
                    // the signature TypeVar form codegen expects.
                    let ret_ty = return_type
                        .as_ref()
                        .map(|t| self.re_resolve_type(t))
                        .transpose()?
                        .unwrap_or(Type::Error);
                    let body_id = self.check_accessor_body(*body, &[], &ret_ty)?;
                    accessors.push(TypedClassAccessor::Getter {
                        name: name.clone(),
                        ret_ty: erase_generic_params(&ret_ty),
                        visibility,
                        body: body_id,
                    });
                }
                AccessorKind::Set => {
                    let write_ty = param
                        .as_ref()
                        .and_then(|p| p.ty.as_ref())
                        .map(|t| self.re_resolve_type(t))
                        .transpose()?
                        .unwrap_or(Type::Error);
                    let pname = param.as_ref().map_or_else(
                        || Ident {
                            name: "value".to_string(),
                            span: name.span,
                        },
                        |p| p.name.clone(),
                    );
                    let typed_param = TypedParam {
                        name: pname,
                        ty: write_ty,
                        boxed: false,
                        rest: false,
                        default: None,
                    };
                    let body_id = self.check_accessor_body(
                        *body,
                        std::slice::from_ref(&typed_param),
                        &Type::Void,
                    )?;
                    accessors.push(TypedClassAccessor::Setter {
                        name: name.clone(),
                        param: TypedParam {
                            ty: erase_generic_params(&typed_param.ty),
                            ..typed_param
                        },
                        visibility,
                        body: body_id,
                    });
                }
            }
        }
        Ok(accessors)
    }

    /// Typecheck an accessor body with its params in scope and the given return type.
    fn check_accessor_body(
        &mut self,
        body: crate::StmtId,
        params: &[TypedParam],
        ret: &Type,
    ) -> Result<crate::StmtId, CompilerFailure> {
        self.scopes.push();
        for p in params {
            self.scopes
                .insert(p.name.name.clone(), p.ty.clone(), false, p.name.span);
        }
        let prev_return = self.current_return.replace(ret.clone());
        let prev_reachable = std::mem::replace(&mut self.reachable, true);
        let body_id = self
            .infer_body_with_narrowing_boundary(body)?
            .ok_or_else(|| super::inference_failure("accessor body is a Block"))?;
        self.current_return = prev_return;
        self.reachable = prev_reachable;
        self.scopes.pop();
        Ok(body_id)
    }
}

/// What [`Inferer::carrier_fit`] decided about a collection carrier's
/// instantiations.
enum CarrierFit {
    /// No instantiation satisfies the target.
    None,
    /// Every instantiation does.
    Every,
    /// It depends on the arguments, so each candidate is tried.
    TryEach,
}

impl CarrierFit {
    /// The candidates whose instantiation satisfies the target, trying each
    /// with `satisfies` only when the fit is not already decided.
    fn matching(self, candidates: &[Type], mut satisfies: impl FnMut(&Type) -> bool) -> Vec<Type> {
        match self {
            Self::None => Vec::new(),
            Self::Every => candidates.to_vec(),
            Self::TryEach => candidates
                .iter()
                .filter(|candidate| satisfies(candidate))
                .cloned()
                .collect(),
        }
    }
}

fn runtime_carrier_candidates(
    members: &std::collections::BTreeMap<String, crate::ObjectField>,
) -> Vec<Type> {
    let mut candidates = vec![
        Type::Unknown,
        Type::Number,
        Type::Boolean,
        Type::String,
        Type::BigInt,
        Type::Uint8Array,
    ];
    for member in members.values() {
        collect_runtime_carrier_candidates(&member.ty, &mut candidates);
    }
    candidates
}

fn collect_runtime_carrier_candidates(ty: &Type, out: &mut Vec<Type>) {
    let ty = ty.peel();
    if !out.contains(ty) && !matches!(ty, Type::Void | Type::Error) {
        out.push(ty.clone());
    }
    match ty {
        Type::Array(element) => collect_runtime_carrier_candidates(element, out),
        Type::Tuple(elements) | Type::Union(elements) => {
            for element in elements {
                collect_runtime_carrier_candidates(element, out);
            }
        }
        Type::Object { fields, index } => {
            if let Some(index) = index {
                collect_runtime_carrier_candidates(&index.value, out);
            }
            for field in fields.values() {
                collect_runtime_carrier_candidates(&field.ty, out);
            }
        }
        Type::Function { params, ret, .. } => {
            for param in params {
                collect_runtime_carrier_candidates(param, out);
            }
            collect_runtime_carrier_candidates(ret, out);
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => {
            for arg in args {
                collect_runtime_carrier_candidates(arg, out);
            }
        }
        _ => {}
    }
}

fn collect_interface_instantiations(ty: &Type, identity: &MangledName, out: &mut BTreeSet<Type>) {
    match ty.peel() {
        Type::InterfaceRef { mangled, .. } if mangled == identity => {
            out.insert(ty.peel().clone());
        }
        Type::Array(element) => collect_interface_instantiations(element, identity, out),
        Type::Tuple(elements) | Type::Union(elements) => {
            for element in elements {
                collect_interface_instantiations(element, identity, out);
            }
        }
        Type::Object { fields, index } => {
            if let Some(index) = index {
                collect_interface_instantiations(&index.value, identity, out);
            }
            for field in fields.values() {
                collect_interface_instantiations(&field.ty, identity, out);
            }
        }
        Type::Function { params, ret, .. } => {
            for param in params {
                collect_interface_instantiations(param, identity, out);
            }
            collect_interface_instantiations(ret, identity, out);
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => {
            for arg in args {
                collect_interface_instantiations(arg, identity, out);
            }
        }
        _ => {}
    }
}

/// A method resolved somewhere up a class's `extends` chain.
pub(super) struct ResolvedMethod {
    /// The declared signature — unsubstituted, so callers deciding physical
    /// boxing still see the raw type variables.
    pub sig: MethodSig,
    /// Type-parameter bindings active at the declaring class.
    pub bindings: BTreeMap<String, Type>,
    pub declared_by: MangledName,
    pub visibility: Visibility,
}

/// [`Inferer::walk_class_chain`](Inferer::walk_class_chain) over any symbol
/// source, so the assignability resolver — which has its own lookup tables —
/// climbs chains by the same rules rather than by a second hand-rolled loop.
pub(super) fn walk_class_chain_with<T>(
    lookup: impl Fn(&MangledName) -> Option<TypeSymbol>,
    limits: &TypeLimits,
    start: &MangledName,
    start_args: &[Type],
    visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>) -> ControlFlow<Option<T>>,
) -> Option<T> {
    walk_chain(lookup, limits, start, start_args, visit).0
}

/// The walk, plus whether the whole chain was seen. A chain is *not* whole when
/// a link fails to resolve or turns out not to be a class, or when a parent's
/// type arguments pass a type limit (recorded in `limits`); callers that
/// accumulate need that distinction, callers that search do not.
fn walk_chain<T>(
    lookup: impl Fn(&MangledName) -> Option<TypeSymbol>,
    limits: &TypeLimits,
    start: &MangledName,
    start_args: &[Type],
    mut visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>) -> ControlFlow<Option<T>>,
) -> (Option<T>, bool) {
    let mut cur = Some((start.clone(), start_args.to_vec()));
    let mut seen: Vec<MangledName> = Vec::new();
    while let Some((m, cur_args)) = cur {
        if seen.contains(&m) {
            return (None, false);
        }
        seen.push(m.clone());
        let Some(sym) = lookup(&m) else {
            return (None, false);
        };
        let TypeKind::Class { generics, .. } = &sym.kind else {
            return (None, false);
        };
        if generics.len() != cur_args.len() {
            return (None, false);
        }
        let bindings: BTreeMap<String, Type> = generics
            .iter()
            .cloned()
            .zip(cur_args.iter().cloned())
            .collect();
        if let ControlFlow::Break(answer) = visit(&sym, &bindings) {
            return (answer, true);
        }
        cur = match parent_hop(&sym, &bindings, limits) {
            Ok(hop) => hop,
            Err(exceeded) => {
                limits.record(exceeded);
                return (None, false);
            }
        };
    }
    (None, true)
}

/// A class's own type parameters bound to themselves — the standpoint of the
/// class's own declaration, where `T` still means `T`.
fn identity_bindings(sym: &TypeSymbol) -> BTreeMap<String, Type> {
    let TypeKind::Class { generics, .. } = &sym.kind else {
        return BTreeMap::new();
    };
    generics
        .iter()
        .map(|g| (g.clone(), Type::TypeVar(g.clone())))
        .collect()
}

/// Stand-in constructor signature for a class that inherits from an
/// unresolvable parent: variadic, at the type that silences argument
/// diagnostics.
fn erased_ctor_rest_param() -> Param {
    Param {
        name: "args".to_string(),
        ty: Type::Array(Box::new(Type::Error)),
        default: None,
        rest: true,
    }
}

/// Visit every class in an `extends` chain from `start`, accumulating rather
/// than searching. `None` means the chain broke — a symbol that could not be
/// resolved, or a non-class in the middle — which callers must not mistake for
/// a complete walk.
pub(super) fn for_each_class_in_chain(
    lookup: impl Fn(&MangledName) -> Option<TypeSymbol>,
    limits: &TypeLimits,
    start: &MangledName,
    start_args: &[Type],
    mut visit: impl FnMut(&TypeSymbol, &BTreeMap<String, Type>),
) -> Option<()> {
    let (_, whole) = walk_chain(lookup, limits, start, start_args, |sym, bindings| {
        visit(sym, bindings);
        ControlFlow::<Option<()>>::Continue(())
    });
    whole.then_some(())
}

/// One hop up an `extends` chain: the parent's mangled name paired with the
/// clause's type arguments resolved through the child's bindings. `None` for a
/// root class or a non-class symbol.
pub(super) fn parent_hop(
    sym: &TypeSymbol,
    bindings: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Result<Option<(MangledName, Vec<Type>)>, TypeTooLarge> {
    let TypeKind::Class {
        extends: Some(extends),
        ..
    } = &sym.kind
    else {
        return Ok(None);
    };
    let args = extends
        .args
        .iter()
        .map(|a| substitute_typevars(a, bindings, limits))
        .collect::<Result<_, _>>()?;
    Ok(Some((extends.parent.clone(), args)))
}

/// A method signature with the declaring class's type parameters substituted
/// from `bindings` — param types, return, and the guard predicate's asserted
/// type. Method-level generics and doc pass through unchanged.
fn substitute_method_sig(
    sig: &MethodSig,
    bindings: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Result<MethodSig, TypeTooLarge> {
    use super::generic::substitute_typevars;
    Ok(MethodSig {
        generics: sig.generics.clone(),
        params: sig
            .params
            .iter()
            .map(|p| {
                Ok(Param {
                    ty: substitute_typevars(&p.ty, bindings, limits)?,
                    ..p.clone()
                })
            })
            .collect::<Result<_, TypeTooLarge>>()?,
        ret: substitute_typevars(&sig.ret, bindings, limits)?,
        predicate: match &sig.predicate {
            Some(p) => Some(crate::TypePredicate {
                parameter_index: p.parameter_index,
                asserted_type: substitute_typevars(&p.asserted_type, bindings, limits)?,
            }),
            None => None,
        },
        doc: sig.doc.clone(),
    })
}

/// A class's own instance field `name`, if it is backed by a payload slot.
/// `None` when an accessor of that name owns the property instead: an accessor
/// appears in `fields` for typing but stores nothing, so
/// [`shadows_inherited_field`](crate::codegen::classes::shadows_inherited_field)
/// looks straight past it when laying out the payload. Every walk that reasons
/// about slot sharing has to skip the same entries, or an accessor anywhere in
/// a chain silently changes which declaration a redeclaration is checked
/// against.
fn data_field<'a>(sym: &'a TypeSymbol, name: &str) -> Option<&'a FieldSig> {
    let TypeKind::Class {
        fields, accessors, ..
    } = &sym.kind
    else {
        return None;
    };
    if accessors.iter().any(|a| a.name() == name) {
        return None;
    }
    fields.get(name)
}

/// One accessor half compared against the inherited declaration of the same
/// half: `subtype` must be assignable to `supertype`, and the two rendered
/// signatures are what the diagnostic prints.
struct AccessorComparison {
    /// Which half this compares, so the diagnostic lands on that declaration.
    half: AccessorKind,
    accessor: String,
    inherited: String,
    redeclared: String,
    subtype: Type,
    supertype: Type,
}

/// Build the comparison for `own` against `inherited`, both halves of the same
/// kind. A getter is covariant in its return — a parent-typed read must see a
/// value the inherited declaration admits — and a setter contravariant in its
/// parameter, since a parent-typed write hands over whatever the inherited
/// declaration accepts. `None` when the two halves don't match, which
/// [`Inferer::ancestor_accessor`] already rules out.
fn accessor_comparison(
    name: &str,
    own: &AccessorSig,
    inherited: &AccessorSig,
    opaque: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Result<Option<AccessorComparison>, TypeTooLarge> {
    Ok(match (own, inherited) {
        (
            AccessorSig::Getter { ret_ty: own, .. },
            AccessorSig::Getter {
                ret_ty: inherited, ..
            },
        ) => {
            let own = substitute_typevars(own, opaque, limits)?;
            Some(AccessorComparison {
                half: AccessorKind::Get,
                accessor: format!("get {name}"),
                inherited: format!("get {name}(): {inherited}"),
                redeclared: format!("get {name}(): {own}"),
                subtype: own,
                supertype: inherited.clone(),
            })
        }
        (
            AccessorSig::Setter { param: own, .. },
            AccessorSig::Setter {
                param: inherited, ..
            },
        ) => {
            let own = substitute_typevars(&own.ty, opaque, limits)?;
            Some(AccessorComparison {
                half: AccessorKind::Set,
                accessor: format!("set {name}"),
                inherited: format!("set {name}(v: {})", inherited.ty),
                redeclared: format!("set {name}(v: {own})"),
                subtype: inherited.ty.clone(),
                supertype: own,
            })
        }
        _ => None,
    })
}

/// Which kind of member `sym` declares `name` as, if any. Accessors are tested
/// first: an accessor's property also appears in `fields` for typing while
/// backing no data slot (see [`data_field`]), so reading `fields` first would
/// report every accessor as a field.
fn declared_member_kind(sym: &TypeSymbol, name: &str) -> Option<MemberKind> {
    let TypeKind::Class {
        fields,
        methods,
        accessors,
        ..
    } = &sym.kind
    else {
        return None;
    };
    if accessors.iter().any(|a| a.name() == name) {
        return Some(MemberKind::Accessor);
    }
    if methods.contains_key(name) {
        return Some(MemberKind::Method);
    }
    fields.contains_key(name).then_some(MemberKind::Field)
}

/// [`substitute_typevars`] applied through an accessor signature, so an
/// inherited accessor is compared at the type arguments the child's `extends`
/// clause instantiates.
fn substitute_accessor_sig(
    sig: &AccessorSig,
    bindings: &BTreeMap<String, Type>,
    limits: &TypeLimits,
) -> Result<AccessorSig, TypeTooLarge> {
    Ok(match sig {
        AccessorSig::Getter { name, ret_ty } => AccessorSig::Getter {
            name: name.clone(),
            ret_ty: substitute_typevars(ret_ty, bindings, limits)?,
        },
        AccessorSig::Setter { name, param } => AccessorSig::Setter {
            name: name.clone(),
            param: Param {
                ty: substitute_typevars(&param.ty, bindings, limits)?,
                ..param.clone()
            },
        },
    })
}

/// How a visibility renders as the source keyword that produces it. `public` is
/// the default and usually written implicitly, but a diagnostic naming the fix
/// has to spell it.
fn visibility_keyword(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Private => "private",
    }
}

/// What a field reads as: its declared type, widened to `T | null` when
/// optional. Shared by [`Inferer::class_field_read_ty`] and the shadowing
/// check, so the latter compares the types access sites actually see.
fn field_read_ty(field: &FieldSig) -> Type {
    if field.optional {
        Type::union(vec![field.ty.clone(), Type::Null])
    } else {
        field.ty.clone()
    }
}

/// Build the `(params) => ret` function form of a method signature for the
/// override-compatibility check (param-contravariant, return-covariant via
/// [`super::assignable`]).
fn method_fn_type(sig: &MethodSig) -> Type {
    Type::Function {
        params: sig.params.iter().map(|p| p.ty.clone()).collect(),
        ret: Box::new(sig.ret.clone()),
        predicate: None,
        has_rest: sig.params.last().is_some_and(|p| p.rest),
    }
}

/// Push a fresh scope and bind the body's parameters (using the resolved
/// signature types). Mirrors the parameter-binding loop in `infer_functions`.
/// Caller owns the matching `scopes.pop()`.
fn bind_params_for_body(
    tc: &mut Inferer<'_>,
    params: &[crate::ParamDecl],
    sig_params: &[Param],
    bindings: &BTreeMap<String, Type>,
) -> Result<Vec<TypedParam>, CompilerFailure> {
    if params.len() != sig_params.len() {
        return Err(super::inference_failure(
            "constructor parameter/signature length mismatch",
        ));
    }
    tc.scopes.push();
    // Scope types go through the body instantiation (TypeVar → GenericParam);
    // the returned TypedParams keep the raw signature forms codegen expects.
    for (p, sp) in params.iter().zip(sig_params.iter()) {
        let ty = substitute_typevars(&sp.ty, bindings, &tc.type_limits)
            .map_err(type_limit_at(p.name.span))?;
        tc.scopes
            .insert(p.name.name.clone(), ty, false, p.name.span);
    }
    Ok(params
        .iter()
        .zip(sig_params.iter())
        .map(|(p, sp)| TypedParam {
            name: p.name.clone(),
            ty: sp.ty.clone(),
            boxed: false,
            rest: sp.rest,
            default: sp.default.clone(),
        })
        .collect())
}

/// Signature-space scan for a `TypeVar` mention, over the one child traversal
/// that covers every `Type` variant, so this can't drift from the enum.
fn signature_mentions_typevar(ty: &Type, name: &str) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if matches!(ty, Type::TypeVar(var) if var == name) {
            return true;
        }
        crate::type_size::for_each_child(ty, |child| pending.push(child));
    }
    false
}

/// Re-fetch the class's bound symbol for the export surface (it was registered
/// in `bind_class`; `add_typed_type_decl` records it into the module's exports).
fn sym_for_export(tc: &Inferer<'_>, name: &Ident) -> Result<TypeSymbol, CompilerFailure> {
    tc.types.lookup(&name.name).cloned().ok_or_else(|| {
        super::inference_failure("missing class symbol from signature pass").with_span(name.span)
    })
}

fn type_mentions_erased_parameter(ty: &Type) -> bool {
    match ty.peel() {
        Type::TypeVar(_) | Type::GenericParam { .. } => true,
        Type::Array(elem) => type_mentions_erased_parameter(elem),
        Type::Tuple(elems) | Type::Union(elems) => elems.iter().any(type_mentions_erased_parameter),
        Type::Object { fields, index } => {
            index
                .as_ref()
                .is_some_and(|i| type_mentions_erased_parameter(&i.value))
                || fields
                    .values()
                    .any(|field| type_mentions_erased_parameter(&field.ty))
        }
        Type::Function { params, ret, .. } => {
            params.iter().any(type_mentions_erased_parameter) || type_mentions_erased_parameter(ret)
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().any(type_mentions_erased_parameter),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{run, run_clean};

    /// The source text a diagnostic's span covers.
    fn spanned<'a>(source: &'a str, diag: &crate::Diagnostic) -> &'a str {
        &source[diag.span.start as usize..diag.span.end as usize]
    }

    /// `get p` and `set p` are one property and one candidate, but each half is
    /// compared against the inherited accessor on its own — so each mismatch has
    /// to point its caret at the half that is wrong. The fixture harness matches
    /// only `Diagnostic.message`, so nothing there can pin this.
    #[test]
    fn an_incompatible_setter_reports_on_the_setter_not_the_getter() {
        let source = r#"
            class Base {
              private n: string = "b";
              get p(): string { return this.n; }
              set p(x: string | null) { this.n = x === null ? "n" : x; }
            }
            class Sub extends Base {
              get p(): string { return "child"; }
              set p(x: string) { this.q = x; }
              private q: string = "";
            }
            export function main(): string { return "x"; }
            "#;
        let (_, diags) = run(source);
        let reported: Vec<&crate::Diagnostic> = diags
            .iter()
            .filter(|d| {
                d.message
                    .contains("is not compatible with the inherited accessor")
            })
            .collect();
        assert_eq!(
            reported.len(),
            1,
            "only the setter is incompatible: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            reported[0].message.contains("`set p`"),
            "names the setter: {}",
            reported[0].message
        );
        let at = spanned(source, reported[0]);
        assert!(
            at.starts_with("set p"),
            "caret sits on the setter, not the getter above it: {at:?}"
        );
    }

    /// See [`an_incompatible_setter_reports_on_the_setter_not_the_getter`] for
    /// why this is a unit test rather than a fixture.
    #[test]
    fn both_accessor_halves_report_separately() {
        let source = r#"
            class Base {
              private n: string = "b";
              get p(): string { return this.n; }
              set p(x: string | null) { this.n = x === null ? "n" : x; }
            }
            class Sub extends Base {
              get p(): string | null { return null; }
              set p(x: string) { this.q = x; }
              private q: string = "";
            }
            export function main(): string { return "x"; }
            "#;
        let (_, diags) = run(source);
        let spans: Vec<&str> = diags
            .iter()
            .filter(|d| {
                d.message
                    .contains("is not compatible with the inherited accessor")
            })
            .map(|d| spanned(source, d))
            .collect();
        assert_eq!(
            spans.len(),
            2,
            "one diagnostic per incompatible half: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            spans.iter().any(|at| at.starts_with("get p")),
            "one caret on the getter: {spans:?}"
        );
        assert!(
            spans.iter().any(|at| at.starts_with("set p")),
            "one caret on the setter: {spans:?}"
        );
    }

    #[test]
    fn static_members_bind_into_the_statics_maps() {
        let ta = run_clean(
            r#"
            export class Calc {
              static readonly MAX: number = 10;
              private static key(): number { return 1; }
              static make(n: number): number { return Calc.key() + n; }
              make(): number { return 0; }
            }
            "#,
        );
        // A static method lands in `ta.functions` under `Calc#static#name`
        // (disjoint from the instance method `make`, which is vtable-bound and
        // absent from the top-level functions); a static field becomes a module
        // global under the same key scheme.
        let fn_mangles: Vec<&str> = ta
            .functions
            .iter()
            .map(|f| f.mangled_name.as_str())
            .collect();
        assert!(
            fn_mangles.iter().any(|m| m.ends_with("Calc#static#make")),
            "{fn_mangles:?}"
        );
        assert!(
            fn_mangles.iter().any(|m| m.ends_with("Calc#static#key")),
            "{fn_mangles:?}"
        );
        assert!(
            ta.globals
                .iter()
                .any(|g| g.mangled_name.as_str().ends_with("Calc#static#MAX")),
        );
        // Only the public statics reach the export surface.
        let export_names: Vec<&str> = ta.exports.iter().map(|e| e.public_name.as_str()).collect();
        assert!(
            export_names.iter().any(|m| m.ends_with("Calc#static#make")),
            "{export_names:?}"
        );
        assert!(
            export_names.iter().any(|m| m.ends_with("Calc#static#MAX")),
            "{export_names:?}"
        );
        assert!(
            !export_names.iter().any(|m| m.ends_with("Calc#static#key")),
            "{export_names:?}"
        );
    }

    #[test]
    fn duplicate_static_member_diagnoses() {
        let (_, diags) =
            run("class C { static f(): number { return 1; } static readonly f: number = 2; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate static member `f`")),
            "got: {diags:?}"
        );
    }

    #[test]
    fn static_referencing_class_generic_diagnoses() {
        let (_, diags) = run("class Box<T> { static id(x: T): number { return 1; } }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("static member `id` cannot reference class type parameter `T`")),
            "got: {diags:?}"
        );
    }

    #[test]
    fn constructor_calls_share_argument_diagnostics() {
        let (_, diags) = run(r#"
            class Point {
                constructor(readonly x: number, readonly y: number = 0, ...labels: string[]) {}
                move(dx: number, dy: number = 0): number { return dx + dy; }
            }
            class Child extends Point {
                constructor() { super(); }
                bad(): number { return super.move(); }
            }
            function main(): void { new Point(); new Point("wrong"); }
        "#);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::Severity::Error)
            .collect();
        assert_eq!(errors.len(), 4, "{errors:?}");
        let arity: Vec<_> = errors
            .iter()
            .filter(|d| d.message.contains("argument(s)"))
            .collect();
        assert_eq!(arity.len(), 3, "{errors:?}");
        assert!(arity.iter().all(|d| !d.help.is_empty()), "{errors:?}");
        assert!(
            arity
                .iter()
                .filter(|d| d.message.contains("constructor"))
                .all(|d| d
                    .help
                    .iter()
                    .any(|h| h.contains("y: number = 0, ...labels: string[]"))),
            "{errors:?}"
        );
        assert!(
            arity.iter().any(|d| d
                .help
                .iter()
                .any(|h| h.contains("Point.move(dx: number, dy: number = 0)"))),
            "{errors:?}"
        );
    }

    const ANIMAL: &str = r#"
        class Animal {
          name: string;
          private sound: string;
          readonly species: string;

          constructor(name: string, sound: string, species: string) {
            this.name = name;
            this.sound = sound;
            this.species = species;
          }

          speak(): string {
            return this.name + " says " + this.sound;
          }
        }
    "#;

    #[test]
    fn animal_class_typechecks() {
        run_clean(ANIMAL);
    }

    #[test]
    fn dog_extends_animal_with_super() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              private tricks: string[];
              constructor(name: string) {{
                super(name, "woof", "canis familiaris");
                this.tricks = [];
              }}
              learn(trick: string): void {{
                this.tricks.push(trick);
              }}
            }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn super_method_call_typechecks() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              constructor(name: string) {{ super(name, "woof", "dog"); }}
              speak(): string {{ return super.speak() + " (bark)"; }}
            }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn implicit_constructor_inherits_parent_signature() {
        // `Dog` declares no constructor, so `new Dog(...)` must check against
        // `Animal`'s constructor signature.
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              bark(): string {{ return this.name; }}
            }}
            function make(): Dog {{ return new Dog("rex", "woof", "dog"); }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn subclass_constructor_without_super_rejected() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              private tricks: number;
              constructor() {{ this.tricks = 0; }}
            }}
        "#
        );
        let (_, diags) = run(&src);
        assert!(
            diags.iter().any(|d| d.message.contains("must call `super")),
            "expected a missing-super diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn read_before_super_rejected() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              private tricks: number;
              constructor(name: string) {{
                this.tricks = 0;
                super(name, "woof", "dog");
              }}
            }}
        "#
        );
        let (_, diags) = run(&src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("before accessing `this`")),
            "expected a read-before-super diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn super_method_outside_subclass_rejected() {
        let src = r#"
            class Animal {
              name: string;
              constructor(name: string) { this.name = name; }
              speak(): string { return super.speak(); }
            }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("only valid inside a subclass")),
            "expected a super-outside-subclass diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn super_unknown_method_rejected() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              constructor(name: string) {{ super(name, "woof", "dog"); }}
              speak(): string {{ return super.nope(); }}
            }}
        "#
        );
        let (_, diags) = run(&src);
        assert!(
            diags.iter().any(|d| d.message.contains("no method `nope`")),
            "expected an unknown-super-method diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn new_constructs_instance_type() {
        let src = format!(
            r#"{ANIMAL}
            function make(): Animal {{
              return new Animal("rex", "woof", "dog");
            }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn subclass_instance_assignable_to_parent() {
        let src = format!(
            r#"{ANIMAL}
            class Dog extends Animal {{
              constructor(name: string) {{ super(name, "woof", "dog"); }}
            }}
            function adopt(): Animal {{ return new Dog("rex"); }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn class_implements_satisfied_interface() {
        let src = r#"
            interface Named { name(): string; }
            class Person implements Named {
              private who: string;
              constructor(who: string) { this.who = who; }
              name(): string { return this.who; }
            }
        "#;
        run_clean(src);
    }

    #[test]
    fn private_field_visible_within_declaring_module() {
        // Module-scoped privacy: a sibling function in the same module may read
        // a private field (single-file = single module).
        let src = format!(
            r#"{ANIMAL}
            function noise(a: Animal): string {{ return a.sound; }}
        "#
        );
        run_clean(&src);
    }

    #[test]
    fn readonly_write_outside_constructor_rejected() {
        let src = r#"
            class Box {
              readonly value: number;
              constructor(v: number) { this.value = v; }
              reset(): void { this.value = 0; }
            }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags.iter().any(|d| d.message.contains("readonly")),
            "expected a readonly diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn bad_override_rejected() {
        let src = r#"
            class Base { value(): number { return 1; } }
            class Sub extends Base { value(): string { return "x"; } }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags.iter().any(|d| d.message.contains("override")),
            "expected an override diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn extends_cycle_rejected() {
        let src = r#"
            class A extends B { }
            class B extends A { }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("circular inheritance")),
            "expected a cycle diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn extends_non_class_rejected() {
        let src = r#"
            interface Shape { area(): number; }
            class Circle extends Shape { }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("can only `extends` another class")),
            "expected an extends-non-class diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn unresolved_parent_silences_dependent_diagnostics() {
        let src = r#"
            interface Named { name(): string; }
            class Base extends Nope implements Named { }
            class Sub extends Base {
              set(): void { this.count = 5; }
              call(): number { return this.inherited() + super.bump(); }
            }
            export function main(): string {
              const s = new Sub(1, 2);
              return Base.make() + s.label;
            }
        "#;
        let (_, diags) = run(src);
        assert_eq!(
            diags.len(),
            1,
            "only the `extends` clause itself should be reported, got: {diags:?}"
        );
        assert!(
            diags[0].message.contains("unknown type `Nope`"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn unresolved_parent_keeps_unrelated_diagnostics() {
        let src = r#"
            class Broken extends Nope {
              go(): number { const x: number = "oops"; return this.whatever(); }
            }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `number`, got `string`")),
            "an error unrelated to inheritance must survive, got: {diags:?}"
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("whatever")),
            "the member miss is unknowable, not wrong, got: {diags:?}"
        );
    }

    #[test]
    fn unresolved_parent_leaves_no_error_typed_follow_on() {
        let src = r#"
            class Broken extends Nope {
              chain(): void { const x = this.a?.b; }
              coalesce(): number { return this.n ?? 1; }
            }
        "#;
        let (_, diags) = run(src);
        assert!(
            !diags.iter().any(|d| d.message.contains("<error>")),
            "silencing the miss must not leave a diagnostic naming the poisoned \
             type, got: {diags:?}"
        );
    }

    #[test]
    fn unresolved_parent_silences_implements_on_child_declared_first() {
        let src = r#"
            interface Named { name(): string; }
            class Sub extends Base implements Named { }
            class Base extends Nope { }
        "#;
        let (_, diags) = run(src);
        assert_eq!(
            diags.len(),
            1,
            "conformance is checked after every signature binds, so declaration \
             order must not resurrect the cascade, got: {diags:?}"
        );
        assert!(
            diags[0].message.contains("unknown type `Nope`"),
            "got: {diags:?}"
        );
    }

    #[test]
    fn implements_still_rejected_when_a_later_parent_lacks_the_member() {
        let src = r#"
            interface Named { name(): string; }
            class Sub extends Base implements Named { }
            class Base { other(): number { return 1; } }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("does not implement")
                    && d.message.contains("missing member `name`")),
            "deferring the check must not silence a genuinely unsatisfied \
             interface, got: {diags:?}"
        );
    }

    #[test]
    fn implements_satisfied_by_a_later_declared_parent() {
        let src = r#"
            interface Named { name(): string; }
            class Sub extends Base implements Named { }
            class Base { name(): string { return "b"; } }
        "#;
        let (_, diags) = run(src);
        assert!(diags.is_empty(), "got: {diags:?}");
    }

    #[test]
    fn resolved_parent_still_reports_missing_members() {
        let src = r#"
            class Base { m(): number { return 1; } }
            class Sub extends Base { go(): number { return this.nope(); } }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("field `nope` does not exist on `Sub`")),
            "a known parent chain still knows what is missing, got: {diags:?}"
        );
    }

    #[test]
    fn unsatisfied_implements_rejected() {
        let src = r#"
            interface Named { name(): string; }
            class Anon implements Named { }
        "#;
        let (_, diags) = run(src);
        let diag = diags
            .iter()
            .find(|d| d.message.contains("does not implement"))
            .unwrap_or_else(|| panic!("expected an implements diagnostic, got: {diags:?}"));
        assert!(
            diag.message.contains("missing member `name`"),
            "expected the message to name the missing member, got: {}",
            diag.message
        );
        assert!(
            diag.help.iter().any(|h| h.contains("interface Named")),
            "expected the help to lift the interface shape, got: {:?}",
            diag.help
        );
    }

    #[test]
    fn implements_wrong_signature_rejected() {
        let src = r#"
            interface Named { name(): string; }
            class Bad implements Named {
              name(): number { return 1; }
            }
        "#;
        let (_, diags) = run(src);
        let diag = diags
            .iter()
            .find(|d| d.message.contains("does not implement"))
            .unwrap_or_else(|| panic!("expected an implements diagnostic, got: {diags:?}"));
        assert!(
            diag.message
                .contains("member `name` has an incompatible signature"),
            "expected the message to flag the incompatible member, got: {}",
            diag.message
        );
        assert!(
            diag.help.iter().any(|h| h.contains("Bad.name")),
            "expected the help to lift the class's actual signature, got: {:?}",
            diag.help
        );
    }

    #[test]
    fn implements_wrong_arity_rejected() {
        // A method may declare fewer parameters, as in TypeScript, but not more.
        let src = r#"
            interface Greeter { greet(n: number): number; }
            class Bad implements Greeter {
              greet(n: number, m: number): number { return n + m; }
            }
        "#;
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("does not implement")
                    && d.message
                        .contains("member `greet` has an incompatible signature")),
            "expected a wrong-arity implements diagnostic, got: {diags:?}"
        );
    }
}

#[cfg(test)]
mod invariant_tests {
    use super::*;

    #[test]
    fn missing_class_metadata_stops_descriptor_and_export_construction() {
        super::super::test_support::with_inferer(|tc| {
            let name = Ident {
                name: "Missing".into(),
                span: Span::at(crate::FileId(0)),
            };
            assert!(matches!(
                sym_for_export(tc, &name),
                Err(CompilerFailure::Internal { .. })
            ));
            let ty = Type::class_ref(
                crate::Package("missing".into()),
                "Missing",
                crate::mangle::package_symbol("missing", "Missing"),
                Vec::new(),
            );
            assert!(matches!(
                tc.record_runtime_type_test(&ty),
                Err(CompilerFailure::Internal { .. })
            ));
            assert!(!tc.typed_ast.runtime_class_fields.contains_key(&ty));
        });
    }
}

#[cfg(test)]
mod review_regressions {
    use super::super::test_support::{run, with_source_inferer};
    use super::*;

    #[test]
    fn missing_guarded_field_stops_runtime_metadata_construction() {
        with_source_inferer("class C {}", |tc| {
            assert!(tc.signatures().unwrap());
            let symbol = tc.types.lookup_mut("C").unwrap();
            let mangled = symbol.mangled_name.clone();
            let TypeKind::Class {
                narrowing_checks, ..
            } = &mut symbol.kind
            else {
                panic!("class signature");
            };
            narrowing_checks.insert(
                "missing".into(),
                crate::FieldNarrowingCheck {
                    declaration: Some(mangled.clone()),
                    test: crate::FieldNarrowingTest::NonNull,
                    minimal_test_target: None,
                    message: "test guard".into(),
                },
            );
            let ty = Type::class_ref(
                crate::Package(tc.package_name.into()),
                "C",
                mangled,
                Vec::new(),
            );
            assert!(matches!(
                tc.record_runtime_type_test(&ty),
                Err(CompilerFailure::Internal { .. })
            ));
            assert!(!tc.typed_ast.runtime_field_guards.contains_key(&ty));
            assert!(!tc.typed_ast.runtime_class_fields.contains_key(&ty));
        });
    }

    #[test]
    fn duplicate_methods_and_inheritance_cycles_remain_source_errors() {
        for source in [
            "class C { value: number = 1; m(): number { return 1; } } class C {} function main(): number { return 0; }",
            "class C { value: number = 1; } interface C { x: number; } function main(): number { return 0; }",
            "class C { m(): number { return 1; } m(x: number): number { return x; } } function main(): number { return 0; }",
            "class C { static m(): number { return 1; } static m(x: number): number { return x; } } function main(): number { return 0; }",
            "class A extends B { constructor() { super(); } } class B extends A { constructor() { super(); } } function main(): number { const a = new A(); return 0; }",
        ] {
            let (_, diagnostics) = run(source);
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
            );
            assert!(
                !diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.message.contains("internal compiler failure")),
                "{diagnostics:?}"
            );
        }
    }

    #[test]
    fn missing_class_and_member_signatures_are_fatal_without_source_errors() {
        let source = "class C { value: number = 1; m(): number { return 1; } static s(): number { return 2; } }";
        for missing in ["class", "field", "method", "static", "parameter-property"] {
            let source = if missing == "parameter-property" {
                "class C { constructor(public value: number) {} }"
            } else {
                source
            };
            with_source_inferer(source, |tc| {
                assert!(tc.signatures().unwrap());
                assert!(tc.diagnostics.is_empty());
                if missing == "class" {
                    tc.types = super::super::TypeNamespace::new();
                } else {
                    let TypeKind::Class {
                        fields,
                        methods,
                        statics,
                        ..
                    } = &mut tc.types.lookup_mut("C").unwrap().kind
                    else {
                        panic!("class signature");
                    };
                    match missing {
                        "field" | "parameter-property" => fields.clear(),
                        "method" => methods.clear(),
                        "static" => statics.clear(),
                        _ => panic!("test case"),
                    }
                }
                assert!(
                    matches!(tc.infer_classes(), Err(CompilerFailure::Internal { .. })),
                    "{missing}"
                );
                assert!(
                    tc.typed_ast.types.is_empty(),
                    "failed class must not be published"
                );
            });
        }
    }
}
