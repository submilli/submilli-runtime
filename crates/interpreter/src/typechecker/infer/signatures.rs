//! Signature pass: resolve and register top-level function and interface declarations.

use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Diagnostic, Ident, InterfaceMember, Intrinsic, MethodSig, Param, PropertySig, Severity, Span,
    StmtKind, Type, TypeKind, TypeSymbol, ValueKind,
};

use super::void_value::ValuePosition;
use super::{Inferer, ValueEntry};
use crate::runtime::prelude::number::global_constant_value;

/// What an identifier in default position denotes. One classification for every
/// arm that can meet one, so no two arms can reach different messages for the
/// same identifier — `x = Infinity` and `x = -Infinity` under a parameter named
/// `Infinity` must both name the parameter.
enum DefaultIdent {
    /// A sibling parameter. Defaults are evaluated in the function's own scope,
    /// so this wins over any outer binding of the same name.
    Parameter,
    /// `Infinity` / `NaN` — the two prelude globals whose value is known at
    /// signature time, so they can fold to the literal the host signatures
    /// already carry. That fold is what makes a lifted `end: number = Infinity`
    /// paste back. The value comes from the prelude's own table.
    Global(f64),
    Unresolved,
}

fn classify_default_ident(ident: &Ident, param_names: &BTreeSet<&str>) -> DefaultIdent {
    if param_names.contains(ident.name.as_str()) {
        return DefaultIdent::Parameter;
    }
    match global_constant_value(&ident.name) {
        Some(v) => DefaultIdent::Global(v),
        None => DefaultIdent::Unresolved,
    }
}

impl<'a> Inferer<'a> {
    pub(super) fn signatures(&mut self) -> Result<bool, CompilerFailure> {
        self.pending_index_checks = Some(Vec::new());
        // Enums first — their bodies are value initializers, never type
        // references, so they have no forward-reference concern and
        // their names must be visible before any other body resolves.
        let top_level: Vec<_> = self.ast.top_level.clone();
        for stmt_id in &top_level {
            let stmt = self
                .ast
                .try_stmt(*stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            if let StmtKind::EnumDecl { name, members, doc } = stmt.kind {
                self.bind_enum(name, members, doc)?;
            }
        }
        // Forward-declare every interface + alias name before resolving
        // any body, so a body can reference any user type regardless of
        // source order — recursive, forward, and mutually-recursive
        // references all resolve. Duplicate + intrinsic-name detection
        // runs in the pre-pass; the returned `skip` set names the decls
        // that hit one of those so the body binders below skip them.
        let skip = self.pre_register_type_names(&top_level)?;
        self.rejected_class_names = skip.clone();
        if !self.bind_interfaces_in_order(&top_level, &skip)? {
            return Ok(false);
        }
        // Class signatures. Names are forward-declared, so `extends` /
        // field+method types may reference any class or interface.
        for stmt_id in &top_level {
            let stmt = self
                .ast
                .try_stmt(*stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            if let StmtKind::ClassDecl {
                name,
                generics,
                extends,
                implements,
                members,
                doc,
            } = stmt.kind
            {
                if skip.contains(&name.name) {
                    continue;
                }
                let span = name.span;
                self.bind_class(name, generics, extends, implements, members, doc)?;
                self.type_size_checkpoint(Some(span))?;
            }
        }
        // Reject `extends` cycles and incompatible overrides once every class
        // signature is bound (so the parent chain is fully resolvable), then
        // check `implements` conformance against those complete chains.
        self.check_class_inheritance(&top_level)?;
        self.check_pending_implements()?;
        for stmt_id in &top_level {
            let stmt = self
                .ast
                .try_stmt(*stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            if let StmtKind::Function {
                name,
                generics,
                params,
                return_type,
                type_predicate,
                doc,
                ..
            } = stmt.kind
            {
                if Intrinsic::from_name(&name.name).is_some() {
                    self.error(
                        name.span,
                        format!(
                            "`{}` is a reserved compiler intrinsic and cannot be redeclared",
                            name.name,
                        ),
                    );
                    continue;
                }
                // Push generics for name visibility only — no body instantiation.
                let generic_names: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
                self.push_signature_generics(generic_names.clone());
                let resolved_params: Vec<Param> = self.resolve_params(&params)?;
                let (resolved_return, resolved_predicate) = match (&return_type, &type_predicate) {
                    (Some(annot), _) => (self.resolve_type(annot)?, None),
                    (None, Some(pred)) => {
                        let resolved = self.resolve_type_predicate(pred, &resolved_params)?;
                        (Type::Boolean, resolved)
                    }
                    (None, None) => {
                        return Err(super::inference_failure(
                            "function declaration has no return annotation or predicate",
                        ));
                    }
                };
                self.pop_signature_generics();
                self.type_size_checkpoint(Some(name.span))?;
                self.bind_top(
                    &name,
                    ValueKind::Function {
                        generics: generic_names,
                        params: resolved_params,
                        ret: resolved_return,
                        type_predicate: resolved_predicate,
                        doc,
                    },
                )?;
            }
        }
        // Drain any aliases never referenced by an interface / function
        // signature so declared-but-unused aliases still resolve their
        // body (validating it + mirroring onto the typed AST). Already-
        // resolved aliases are no-ops.
        let pending: Vec<String> = self.pending_aliases.keys().cloned().collect();
        for name in pending {
            self.resolve_alias_body(&name)?;
        }
        self.validate_bound_interfaces(&top_level, &skip)?;
        self.check_pending_indexes();
        Ok(true)
    }

    /// forward-declare every interface + alias name with a
    /// placeholder symbol so bodies resolved later can reference any
    /// type regardless of source order. Runs the duplicate-declaration
    /// and intrinsic-name checks (the binders no longer do, since after
    /// this pass every name is already present). Returns the set of
    /// names that hit a duplicate / intrinsic error, which the body
    /// binders skip. Enums are bound in full before this pass, so an
    /// enum name already present here is a genuine prior declaration.
    fn pre_register_type_names(
        &mut self,
        top_level: &[crate::StmtId],
    ) -> Result<BTreeSet<String>, CompilerFailure> {
        let mut skip: BTreeSet<String> = BTreeSet::new();
        for stmt_id in top_level {
            let stmt = self
                .ast
                .try_stmt(*stmt_id)
                .map_err(super::arena_failure)?
                .clone();
            match stmt.kind {
                StmtKind::InterfaceDecl { name, generics, .. } => {
                    if self.types.contains(&name.name) {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: name.span,
                            message: format!("duplicate declaration of interface `{}`", name.name,),
                            help: Vec::new(),
                            notes: Vec::new(),
                        });
                        skip.insert(name.name.clone());
                        continue;
                    }
                    let generic_names: Vec<String> =
                        generics.iter().map(|g| g.name.clone()).collect();
                    let mangled = self.mangle_top_symbol(&name.name)?;
                    self.types.insert(
                        name.name.clone(),
                        self.package_name.to_string(),
                        TypeSymbol {
                            name: name.name.clone(),
                            mangled_name: mangled,
                            declaration_span: name.span,
                            kind: TypeKind::Interface {
                                index: None,
                                generics: generic_names,
                                methods: BTreeMap::new(),
                                properties: BTreeMap::new(),
                                dispatch: crate::Dispatch::VTable,
                                doc: None,
                            },
                        },
                    );
                }
                StmtKind::ClassDecl { name, generics, .. } => {
                    if self.types.contains(&name.name) {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: name.span,
                            message: format!("duplicate declaration of class `{}`", name.name),
                            help: Vec::new(),
                            notes: Vec::new(),
                        });
                        skip.insert(name.name.clone());
                        continue;
                    }
                    let generic_names: Vec<String> =
                        generics.iter().map(|g| g.name.clone()).collect();
                    let mangled = self.mangle_top_symbol(&name.name)?;
                    self.types.insert(
                        name.name.clone(),
                        self.package_name.to_string(),
                        TypeSymbol {
                            name: name.name.clone(),
                            mangled_name: mangled,
                            declaration_span: name.span,
                            kind: TypeKind::Class {
                                generics: generic_names,
                                fields: BTreeMap::new(),
                                narrowing_checks: BTreeMap::new(),
                                methods: BTreeMap::new(),
                                method_visibility: BTreeMap::new(),
                                accessors: Vec::new(),
                                constructor: Vec::new(),
                                constructor_visibility: crate::Visibility::Public,
                                statics: BTreeMap::new(),
                                static_visibility: BTreeMap::new(),
                                static_fields: BTreeMap::new(),
                                extends: None,
                                implements: Vec::new(),
                                doc: None,
                            },
                        },
                    );
                }
                StmtKind::TypeAliasDecl {
                    name,
                    generics,
                    ty,
                    doc,
                } => {
                    if self.reject_intrinsic_name(&name) {
                        skip.insert(name.name.clone());
                        continue;
                    }
                    if let Some(prev) = self.types.lookup(&name.name) {
                        let prev_span = prev.declaration_span;
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: name.span,
                            message: format!("duplicate declaration of type `{}`", name.name,),
                            help: Vec::new(),
                            notes: vec![(prev_span, "previously declared here".into())],
                        });
                        skip.insert(name.name.clone());
                        continue;
                    }
                    let generic_names: Vec<String> =
                        generics.iter().map(|g| g.name.clone()).collect();
                    let mangled = self.mangle_top_symbol(&name.name)?;
                    self.types.insert(
                        name.name.clone(),
                        self.package_name.to_string(),
                        TypeSymbol {
                            name: name.name.clone(),
                            mangled_name: mangled,
                            declaration_span: name.span,
                            kind: TypeKind::Alias {
                                generics: generic_names.clone(),
                                // Placeholder body — filled in by
                                // `resolve_alias_body` on first reference.
                                ty: Type::Error,
                                doc: doc.clone(),
                            },
                        },
                    );
                    self.pending_aliases.insert(
                        name.name.clone(),
                        super::PendingAlias {
                            name,
                            generics: generic_names,
                            annotation: ty,
                            doc,
                        },
                    );
                }
                _ => {}
            }
        }
        Ok(skip)
    }

    /// `toString` and `toJson` fill the universal vtable slots that `String(x)`,
    /// string interpolation and `JSON.stringify` call, whose shape is fixed at
    /// `(): string`, so a class or interface may declare them only so.
    pub(super) fn check_conversion_method(
        &mut self,
        owner: &str,
        name: &Ident,
        params: &[Param],
        generics: &[String],
        ret: &crate::Type,
    ) {
        if !matches!(name.name.as_str(), "toString" | "toJson") {
            return;
        }
        if params.is_empty() && generics.is_empty() && self.returns_string(ret) {
            return;
        }
        self.error_with_help(
            name.span,
            format!(
                "{owner} method `{}` must have signature `(): string`",
                name.name
            ),
            vec![format!(
                "`{}` overrides the built-in conversion used by `String(x)`, \
                 string interpolation, and `JSON.stringify`; declare it as \
                 `{}(): string` or pick another method name",
                name.name, name.name
            )],
        );
    }

    /// An interface property named `toString` or `toJson` is called by the
    /// same conversions as the method, so it must have exactly the method's
    /// type. It may be optional: an object without it converts as a plain
    /// object.
    fn check_conversion_property(&mut self, name: &Ident, ty: &crate::Type) {
        let Some(expected) = super::reserved::override_field_signature(&name.name) else {
            return;
        };
        if self.is_conversion_function(ty) {
            return;
        }
        self.error(
            name.span,
            format!(
                "interface property `{}` must have type `{expected}` (got `{ty}`)",
                name.name
            ),
        );
    }

    /// Whether `ty` is a function the conversions can call: no parameters and
    /// a return that is always a string.
    fn is_conversion_function(&self, ty: &crate::Type) -> bool {
        let crate::Type::Function {
            params,
            ret,
            has_rest: false,
            ..
        } = ty.peel()
        else {
            return false;
        };
        params.is_empty() && self.returns_string(ret)
    }

    /// Whether a conversion returning `ret` always yields a string. Any string
    /// subtype qualifies, as it does for an assignment; a type parameter
    /// doesn't, since an instance may bind it to a non-string.
    fn returns_string(&self, ret: &crate::Type) -> bool {
        !mentions_generic_param(ret)
            && super::assignable(ret, &crate::Type::String, self.resolver())
    }

    pub(super) fn bind_interface(
        &mut self,
        name: Ident,
        generics: Vec<Ident>,
        members: Vec<InterfaceMember>,
        extends: Vec<crate::TypeAnnotation>,
        doc: Option<crate::DocComment>,
    ) -> Result<(), CompilerFailure> {
        // Duplicate detection happened in `pre_register_type_names`;
        // this binder is only reached for a non-duplicate name and
        // overwrites that name's forward-declared placeholder.
        let generic_names: Vec<String> = generics.iter().map(|g| g.name.clone()).collect();
        self.push_signature_generics(generic_names.clone());
        let mut method_sigs: BTreeMap<String, MethodSig> = BTreeMap::new();
        let mut property_sigs: BTreeMap<String, PropertySig> = BTreeMap::new();
        let mut typed_members: Vec<crate::TypedInterfaceMember> = Vec::new();
        let mut index = None;
        for base in extends {
            self.inherit_interface(&base, &mut method_sigs, &mut property_sigs, &mut index)?;
        }
        let mut own_names = BTreeSet::new();
        let mut own_index = false;
        for member in members {
            match member {
                InterfaceMember::IndexSignature(annotation) => {
                    let resolved = self.resolve_index_signature(&annotation)?;
                    if own_index {
                        self.error(annotation.span, "duplicate string index signature".into());
                    }
                    own_index = true;
                    index = Some(resolved);
                }
                InterfaceMember::Method {
                    name: m_name,
                    generics: m_generics,
                    params,
                    return_type,
                    span: _,
                    doc: m_doc,
                } => {
                    if !own_names.insert(m_name.name.clone()) {
                        // `@call` is the call-signature sentinel — give it a distinct duplicate message.
                        let message = if m_name.name == "@call" {
                            format!("duplicate call signature on interface `{}`", name.name,)
                        } else {
                            format!(
                                "duplicate member `{}` on interface `{}`",
                                m_name.name, name.name,
                            )
                        };
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: m_name.span,
                            message,
                            help: Vec::new(),
                            notes: Vec::new(),
                        });
                        continue;
                    }
                    // Method generics shadowing interface generics are rejected: methods have
                    // no body to walk, so no unique body-form IDs are available for substitution.
                    let mut shadow_rejected = false;
                    for g in &m_generics {
                        if generic_names.iter().any(|i| i == &g.name) {
                            self.diagnostics.push(Diagnostic {
                                severity: Severity::Error,
                                span: g.span,
                                message: format!(
                                    "method generic `{}` shadows interface generic `{}`",
                                    g.name, g.name,
                                ),
                                help: Vec::new(),
                                notes: Vec::new(),
                            });
                            shadow_rejected = true;
                        }
                    }
                    let m_generic_names: Vec<String> =
                        m_generics.iter().map(|g| g.name.clone()).collect();
                    // Even on shadow rejection, keep resolving the
                    // sig so any downstream type errors surface in
                    // one pass.
                    self.push_signature_generics(m_generic_names.clone());
                    let resolved_params: Vec<Param> = self.resolve_params(&params)?;
                    let resolved_ret = self.resolve_type(&return_type)?;
                    self.pop_signature_generics();
                    if shadow_rejected {
                        continue;
                    }
                    self.check_conversion_method(
                        "interface",
                        &m_name,
                        &resolved_params,
                        &m_generic_names,
                        &resolved_ret,
                    );
                    let typed_params: Vec<crate::TypedParam> = params
                        .iter()
                        .zip(resolved_params.iter())
                        .map(|(p, rp)| crate::TypedParam {
                            name: p.name.clone(),
                            ty: rp.ty.clone(),
                            boxed: false,
                            rest: rp.rest,
                            default: rp.default.clone(),
                        })
                        .collect();
                    typed_members.push(crate::TypedInterfaceMember::Method {
                        name: m_name.clone(),
                        generics: m_generic_names.clone(),
                        params: typed_params,
                        return_type: resolved_ret.clone(),
                        doc: m_doc.clone(),
                    });
                    property_sigs.remove(&m_name.name);
                    method_sigs.insert(
                        m_name.name,
                        MethodSig {
                            generics: m_generic_names,
                            params: resolved_params,
                            ret: resolved_ret,
                            predicate: None,
                            doc: m_doc,
                        },
                    );
                }
                InterfaceMember::Property {
                    name: p_name,
                    ty,
                    optional,
                    readonly,
                    span: _,
                    doc: p_doc,
                } => {
                    if !own_names.insert(p_name.name.clone()) {
                        self.diagnostics.push(Diagnostic {
                            severity: Severity::Error,
                            span: p_name.span,
                            message: format!(
                                "duplicate member `{}` on interface `{}`",
                                p_name.name, name.name,
                            ),
                            help: Vec::new(),
                            notes: Vec::new(),
                        });
                        continue;
                    }
                    // `JSON.stringify` calls `toJson` through the vtable, which an
                    // absent one would leave null. An absent `toString` falls back
                    // to `[object Object]`, as JavaScript does.
                    if optional && p_name.name == "toJson" {
                        self.error(p_name.span, format!("`{}` cannot be optional", p_name.name));
                    }
                    let resolved_ty = self.resolve_value_type(&ty, ValuePosition::FieldType)?;
                    self.check_conversion_property(&p_name, &resolved_ty);
                    typed_members.push(crate::TypedInterfaceMember::Property {
                        name: p_name.clone(),
                        ty: resolved_ty.clone(),
                        readonly,
                        optional,
                        doc: p_doc.clone(),
                    });
                    method_sigs.remove(&p_name.name);
                    property_sigs.insert(
                        p_name.name,
                        PropertySig {
                            ty: resolved_ty,
                            readonly,
                            optional,
                            intrinsic: false,
                            doc: p_doc,
                        },
                    );
                }
            }
        }
        self.pop_signature_generics();
        let typed_decl = crate::TypedTypeDecl::Interface(crate::TypedInterfaceDecl {
            property_names: property_sigs.keys().cloned().collect(),
            name: name.clone(),
            generics: generic_names.clone(),
            members: typed_members,
            index: index.clone(),
            doc: doc.clone(),
        });
        let mangled = self.mangle_top_symbol(&name.name)?;
        let symbol = TypeSymbol {
            name: name.name.clone(),
            mangled_name: mangled,
            declaration_span: name.span,
            kind: TypeKind::Interface {
                index,
                generics: generic_names,
                methods: method_sigs,
                properties: property_sigs,
                // User-declared interfaces use vtable dispatch; the prelude opts into direct/static dispatch.
                dispatch: crate::Dispatch::VTable,
                doc,
            },
        };
        self.add_typed_type_decl(typed_decl, symbol.clone())?;
        self.types
            .insert(name.name.clone(), self.package_name.to_string(), symbol);

        Ok(())
    }

    pub(super) fn reject_intrinsic_name(&mut self, name: &Ident) -> bool {
        if Intrinsic::from_name(&name.name).is_some() {
            self.error(
                name.span,
                format!(
                    "`{}` is a reserved compiler intrinsic and cannot be redeclared",
                    name.name,
                ),
            );
            true
        } else {
            false
        }
    }

    /// Resolves a whole parameter list. The list is resolved together because a
    /// default value has to know the *sibling* parameter names: a default is
    /// evaluated in the function's own scope, so an identifier naming a
    /// parameter refers to that parameter, never to a global of the same name.
    pub(super) fn resolve_params(
        &mut self,
        params: &[crate::ParamDecl],
    ) -> Result<Vec<Param>, CompilerFailure> {
        self.check_parameter_arity(params)?;
        self.resolve_parameter_types(params)
    }

    /// Resolve slot types/defaults without imposing a callable ABI. Constructors
    /// use this directly because they have no closure slot of their own.
    pub(super) fn resolve_parameter_types(
        &mut self,
        params: &[crate::ParamDecl],
    ) -> Result<Vec<Param>, CompilerFailure> {
        self.report_duplicate_params(params.iter().map(|p| &p.name));
        let names: BTreeSet<&str> = params.iter().map(|p| p.name.name.as_str()).collect();
        params
            .iter()
            .map(|p| self.resolve_param(p, &names))
            .collect::<Result<_, _>>()
    }

    /// Reject a parameter name declared twice in one list, anchored at the
    /// second occurrence and pointing back at the first.
    ///
    /// The later binding wins, so the earlier parameter is unreachable — the
    /// body can never name it, and the failure mode is a wrong value rather than
    /// an error. Takes the names alone so the two lists that do not reach
    /// [`resolve_params`](Self::resolve_params) — an arrow's parameters, and a
    /// function-*type* annotation's, whose names are documentary but still have
    /// to be distinct — can call it with what they have.
    pub(super) fn report_duplicate_params<'p>(
        &mut self,
        names: impl IntoIterator<Item = &'p Ident>,
    ) {
        let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
        for ident in names {
            let name = ident.name.as_str();
            match seen.get(name) {
                Some(prev) => self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: ident.span,
                    message: format!("duplicate parameter `{name}`"),
                    help: vec![format!(
                        "the later `{name}` wins, so the first is unreachable — rename one"
                    )],
                    notes: vec![(*prev, "previously declared here".to_string())],
                }),
                None => {
                    seen.insert(name, ident.span);
                }
            }
        }
    }

    fn resolve_param(
        &mut self,
        p: &crate::ParamDecl,
        param_names: &BTreeSet<&str>,
    ) -> Result<Param, CompilerFailure> {
        let ty =
            p.ty.as_ref()
                .map(|t| self.resolve_value_type(t, ValuePosition::Parameter))
                .transpose()?
                .unwrap_or(Type::Error);
        // Parser already rejects rest + default and non-trailing rest; here we check the
        // annotation is present and resolves to an array.
        if p.rest {
            if p.ty.is_none() {
                self.error(
                    p.name.span,
                    format!(
                        "rest parameter `{}` requires a type annotation",
                        p.name.name,
                    ),
                );
            // NOT peeled: an aliased array type passes this gate but the
            // rest-param lowering still dispatches on the unpeeled type and
            // emits a scalar where a ref is expected.
            } else if ty.rest_element().is_none() && !matches!(ty, Type::Error) {
                self.error(
                    p.name.span,
                    format!(
                        "rest parameter `{}` type must be an array, got `{}`",
                        p.name.name, ty,
                    ),
                );
            }
            return Ok(Param::rest(p.name.name.clone(), ty));
        }
        let default = p
            .default
            .map(|expr_id| self.resolve_default(expr_id, &ty, &p.name.name, param_names))
            .transpose()?
            .flatten();
        Ok(match default {
            Some(d) => Param::with_default(p.name.name.clone(), ty, d),
            None => Param::new(p.name.name.clone(), ty),
        })
    }

    fn resolve_default(
        &mut self,
        expr_id: crate::ExprId,
        param_ty: &Type,
        param_name: &str,
        param_names: &BTreeSet<&str>,
    ) -> Result<Option<crate::DefaultValue>, CompilerFailure> {
        use crate::{DefaultValue, EnumVariantValue, ExprKind};
        let expr = self
            .ast
            .try_expr(expr_id)
            .map_err(super::arena_failure)?
            .clone();
        let span = expr.span;
        let (value, value_ty): (DefaultValue, Type) = match expr.kind {
            ExprKind::Number(n) => (DefaultValue::Number(n), Type::Number),
            ExprKind::String(s) => (DefaultValue::String(s), Type::String),
            ExprKind::Boolean(b) => (DefaultValue::Boolean(b), Type::Boolean),
            ExprKind::Null => (DefaultValue::Null, Type::Null),
            // Parser lowers `-N` into `Unary { Neg, Number(n) }` rather than a plain Number.
            ExprKind::Unary {
                op: crate::ast::UnOp::Neg,
                operand,
            } => {
                let folded = match &self
                    .ast
                    .try_expr(operand)
                    .map_err(super::arena_failure)?
                    .kind
                {
                    ExprKind::Number(n) => Some(*n),
                    ExprKind::Identifier(ident) => {
                        match classify_default_ident(ident, param_names) {
                            DefaultIdent::Parameter => {
                                let name = ident.name.clone();
                                return Ok(self.reject_parameter_default(span, &name));
                            }
                            DefaultIdent::Global(v) => Some(v),
                            DefaultIdent::Unresolved => None,
                        }
                    }
                    _ => None,
                };
                let Some(v) = folded else {
                    return Ok(self.reject_non_literal_default(span));
                };
                (DefaultValue::Number(-v), Type::Number)
            }
            ExprKind::ArrayLiteral { ref elements } if elements.is_empty() => {
                if !matches!(param_ty.peel(), Type::Array(_)) {
                    self.error_with_help(
                        span,
                        format!(
                            "default value `[]` is only valid for array \
                             parameters; parameter `{param_name}` is `{param_ty}`",
                        ),
                        Vec::new(),
                    );
                    return Ok(None);
                }
                return Ok(Some(DefaultValue::EmptyArray));
            }
            ExprKind::FieldAccess { receiver, ref name } => {
                let recv_kind = self
                    .ast
                    .try_expr(receiver)
                    .map_err(super::arena_failure)?
                    .kind
                    .clone();
                if let ExprKind::Identifier(recv_ident) = recv_kind {
                    // A sibling parameter of the enum's name shadows it, the same
                    // way it shadows a global constant.
                    if param_names.contains(recv_ident.name.as_str()) {
                        return Ok(self.reject_parameter_default(span, &recv_ident.name));
                    }
                    if let Some(sym) = self.lookup_named_type(&recv_ident.name) {
                        let enum_name = recv_ident.name.clone();
                        let variant_name = name.name.clone();
                        let enum_mangled = sym.mangled_name.clone();
                        let enum_package = self.type_package(&recv_ident.name);
                        match &sym.kind {
                            crate::TypeKind::NumberEnum { variants, .. } => {
                                if let Some((_, value)) =
                                    variants.iter().find(|(v, _)| v == &variant_name)
                                {
                                    (
                                        DefaultValue::EnumVariant {
                                            enum_mangled: enum_mangled.clone(),
                                            variant: variant_name,
                                            value: EnumVariantValue::Number(*value),
                                        },
                                        Type::number_enum(enum_package, enum_name, enum_mangled),
                                    )
                                } else {
                                    self.error(
                                        name.span,
                                        format!(
                                            "no variant `{variant_name}` on enum `{enum_name}`",
                                        ),
                                    );
                                    return Ok(None);
                                }
                            }
                            crate::TypeKind::StringEnum { variants, .. } => {
                                if let Some((_, value)) =
                                    variants.iter().find(|(v, _)| v == &variant_name)
                                {
                                    (
                                        DefaultValue::EnumVariant {
                                            enum_mangled: enum_mangled.clone(),
                                            variant: variant_name,
                                            value: EnumVariantValue::String(value.clone()),
                                        },
                                        Type::string_enum(enum_package, enum_name, enum_mangled),
                                    )
                                } else {
                                    self.error(
                                        name.span,
                                        format!(
                                            "no variant `{variant_name}` on enum `{enum_name}`",
                                        ),
                                    );
                                    return Ok(None);
                                }
                            }
                            _ => return Ok(self.reject_non_literal_default(span)),
                        }
                    } else {
                        return Ok(self.reject_non_literal_default(span));
                    }
                } else {
                    return Ok(self.reject_non_literal_default(span));
                }
            }
            ExprKind::Identifier(ref ident) => {
                let folded = match classify_default_ident(ident, param_names) {
                    DefaultIdent::Parameter => {
                        return Ok(self.reject_parameter_default(span, &ident.name));
                    }
                    DefaultIdent::Global(v) => Some(v),
                    DefaultIdent::Unresolved => None,
                };
                let Some(v) = folded else {
                    // Global inference runs after signatures, so top-level const references
                    // can't be resolved here yet.
                    self.error_with_help(
                        span,
                        "top-level `const` references as defaults are not yet supported"
                            .to_string(),
                        vec![
                            "use a literal (number, string, boolean, null, `[]`, \
                             `Infinity`, `NaN`) or an enum variant (`EnumName.Variant`)"
                                .to_string(),
                        ],
                    );
                    return Ok(None);
                };
                (DefaultValue::Number(v), Type::Number)
            }
            _ => return Ok(self.reject_non_literal_default(span)),
        };
        // `assignable` treats a type variable as a wildcard, so the check below
        // would accept any literal a type variable could stand for and let the
        // caller pick the type argument — `f<T>(v: T = 5)` called as
        // `f<string>()` would reach the body holding a number. The literal has
        // to be accepted by something the caller can't choose.
        let Some(concrete_ty) = without_type_vars(param_ty) else {
            self.error_with_help(
                span,
                format!(
                    "parameter `{param_name}` cannot have a default value: its type \
                     `{param_ty}` is chosen by the caller"
                ),
                vec![
                    "drop the default, or give the parameter a type that doesn't depend on \
                     a type parameter"
                        .to_string(),
                ],
            );
            return Ok(None);
        };
        if !super::assignable::assignable(&value_ty, &concrete_ty, self.resolver()) {
            // Narrowing the type variable out of a union makes the plain
            // "not assignable" message read as false — `5` obviously fits
            // `T | null` — so name the part that has to accept the value.
            let help = if concrete_ty == *param_ty {
                Vec::new()
            } else {
                vec![format!(
                    "the caller picks what a type parameter stands for, so the default has to \
                     fit `{concrete_ty}` — the rest of `{param_ty}`"
                )]
            };
            self.error_with_help(
                span,
                format!(
                    "default value of type `{value_ty}` is not assignable to \
                     parameter type `{param_ty}`",
                ),
                help,
            );
            return Ok(None);
        }
        Ok(Some(value))
    }

    /// A default that names another parameter. Reported apart from the
    /// const-reference gate because this is a permanent rule, not a missing
    /// feature: spec.md says a default may not reference another parameter, and
    /// "not yet supported" would tell a reader to wait for a release.
    fn reject_parameter_default(&mut self, span: Span, name: &str) -> Option<crate::DefaultValue> {
        self.error_with_help(
            span,
            format!("a default value cannot reference the parameter `{name}`"),
            vec![format!(
                "defaults are evaluated in the function's own scope, so `{name}` here is the \
                 parameter, not an outer binding; use a literal or an enum variant instead"
            )],
        );
        None
    }

    fn reject_non_literal_default(&mut self, span: Span) -> Option<crate::DefaultValue> {
        self.error_with_help(
            span,
            "default value must be a literal or enum variant".to_string(),
            vec![
                "accepted forms: number / string / boolean / null literal, \
                 `[]`, `Infinity`, `NaN`, or `EnumName.Variant`"
                    .to_string(),
            ],
        );
        None
    }

    pub(super) fn bind_top(
        &mut self,
        name: &Ident,
        kind: ValueKind,
    ) -> Result<(), CompilerFailure> {
        if let Some(existing) = self.top_symbols.get(&name.name) {
            let prev_span = existing.declaration_span;
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: name.span,
                message: format!("duplicate declaration of `{}`", name.name),
                help: vec![],
                notes: vec![(prev_span, "previously declared here".to_string())],
            });
            return Ok(());
        }
        let mangled_name = self.mangle_top_symbol(&name.name)?;
        self.top_symbols.insert(
            name.name.clone(),
            ValueEntry {
                declaration_span: name.span,
                package_name: self.package_name.to_string(),
                symbol_name: name.name.clone(),
                kind,
                mangled_name,
            },
        );

        Ok(())
    }
}

/// The part of `ty` a caller can't choose: `ty` itself, or — for a union —
/// its non-type-variable members. `None` when nothing is left, i.e. the type
/// is a type variable or a union of them.
///
/// Returns `ty` unchanged when it holds no type variable, so a caller can read
/// "something was stripped" off the result differing from what it passed in.
fn without_type_vars(ty: &Type) -> Option<Type> {
    fn is_type_var(ty: &Type) -> bool {
        matches!(ty.peel(), Type::TypeVar(_) | Type::GenericParam { .. })
    }
    if is_type_var(ty) {
        return None;
    }
    match ty.peel() {
        Type::Union(members) => {
            let concrete: Vec<Type> = members
                .iter()
                .filter(|m| !is_type_var(m))
                .cloned()
                .collect();
            if concrete.len() == members.len() {
                return Some(ty.clone());
            }
            (!concrete.is_empty()).then(|| Type::union(concrete))
        }
        _ => Some(ty.clone()),
    }
}

/// Whether `ty` is a type parameter or a union with one among its members.
fn mentions_generic_param(ty: &crate::Type) -> bool {
    match ty.peel() {
        crate::Type::TypeVar(_) | crate::Type::GenericParam { .. } => true,
        crate::Type::Union(members) => members.iter().any(mentions_generic_param),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{run, run_clean};

    use crate::{PackageDeclaration, Param, Type, ValueKind};

    /// `resolve_params` has five call sites and a signature can be resolved by
    /// more than one pass, so "reported once" is the property at risk — and it
    /// is a *count*, which no `expect-error` fixture directive can express.
    #[test]
    fn a_duplicate_parameter_reports_once_per_list() {
        for source in [
            "function dup(a: number, a: number): number { return a; }\n\
             function main(): string { return dup(1, 2).toString(); }",
            "function main(): string { let f = (a: number, a: number): number => a; return \"x\"; }",
            "function main(): string { let f: (a: number, a: number) => number = null; return \"x\"; }",
            "class C { m(a: number, a: number): number { return a; } }\n\
             function main(): string { return \"x\"; }",
            "interface I { m(a: number, a: number): number; }\n\
             function main(): string { return \"x\"; }",
        ] {
            let (_, diags) = run(source);
            let reported = diags
                .iter()
                .filter(|d| d.message == "duplicate parameter `a`")
                .count();
            assert_eq!(
                reported, 1,
                "expected one report for:\n{source}\ngot {diags:?}"
            );
        }
    }

    /// Three of a name is two duplicates, each pointing back at the first — not
    /// one report, and not a chain where the second points at the first and the
    /// third at the second.
    #[test]
    fn three_of_a_name_reports_twice_against_the_first() {
        let (_, diags) = run(
            "function three(h: number, h: number, h: number): number { return h; }\n\
             function main(): string { return \"x\"; }",
        );
        let reports: Vec<&crate::Diagnostic> = diags
            .iter()
            .filter(|d| d.message == "duplicate parameter `h`")
            .collect();
        assert_eq!(reports.len(), 2, "got {diags:?}");
        let first_note = reports[0].notes.first().map(|(span, _)| *span);
        assert_eq!(
            first_note,
            reports[1].notes.first().map(|(span, _)| *span),
            "both reports should point back at the first declaration",
        );
    }

    #[test]
    fn empty_program() {
        let ta = run_clean("");
        assert!(PackageDeclaration::from_typed_ast(&ta).values.is_empty());
    }

    #[test]
    fn function_signature_in_definitions() {
        let ta = run_clean("function f(a: number, b: string): boolean { }");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        let sym = reg.values.get("f").expect("f not found");
        match &sym.kind {
            ValueKind::Function { params, ret, .. } => {
                assert_eq!(
                    params,
                    &[Param::new("a", Type::Number), Param::new("b", Type::String),]
                );
                assert_eq!(*ret, Type::Boolean);
            }
            other => panic!("expected Function, got {other:?}"),
        }
    }

    #[test]
    fn function_void_return() {
        let ta = run_clean("function f(): void { }");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { params, ret, .. } => {
                assert!(params.is_empty());
                assert_eq!(*ret, Type::Void);
            }
            _ => panic!("expected Function"),
        }
    }

    #[test]
    fn let_with_annotation_in_definitions() {
        let ta = run_clean("let x: number = 1;");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("x").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Number),
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn const_with_annotation_in_definitions() {
        let ta = run_clean(r#"const y: string = "hi";"#);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("y").unwrap().kind {
            ValueKind::Const { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn multiple_top_level_decls_in_definitions() {
        let ta = run_clean(r#"function f(): void { } let x: number = 1; const y: string = "hi";"#);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        assert_eq!(reg.values.len(), 3);
        assert!(reg.values.contains_key("f"));
        assert!(reg.values.contains_key("x"));
        assert!(reg.values.contains_key("y"));
    }

    #[test]
    fn default_on_a_type_parameter_slot_diagnoses() {
        let (_, diags) = run("function f<T>(v: T = 5): void { }");
        assert_eq!(
            diags[0].message,
            "parameter `v` cannot have a default value: its type `T` is chosen by the caller",
        );
    }

    /// The default has to fit the part of the union the caller can't pick, and
    /// the help says which part that is — plain "not assignable to `T | null`"
    /// reads as false, since `5` obviously fits.
    #[test]
    fn default_against_the_concrete_arm_of_a_type_parameter_union() {
        let (_, diags) = run("function f<T>(v: T | null = 5): void { }");
        assert_eq!(
            diags[0].message,
            "default value of type `number` is not assignable to parameter type `null | T`",
        );
        assert_eq!(diags[0].help.len(), 1);
        assert!(
            diags[0].help[0].contains("has to fit `null`"),
            "help should name the arm the default must fit, got {:?}",
            diags[0].help[0],
        );
    }

    /// A union with no type parameter in it takes the plain message — the help
    /// above would point at a type parameter that isn't there.
    #[test]
    fn default_against_a_plain_union_has_no_type_parameter_help() {
        let (_, diags) = run("type NumOrStr = number | string;\n\
             function f(v: NumOrStr = true): void { }");
        assert_eq!(
            diags[0].message,
            "default value of type `boolean` is not assignable to parameter type `NumOrStr`",
        );
        assert!(diags[0].help.is_empty(), "got help: {:?}", diags[0].help);
    }

    #[test]
    fn duplicate_function_diagnoses() {
        let (_, diags) = run("function f(): void { } function f(): void { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "duplicate declaration of `f`");
        assert_eq!(diags[0].notes.len(), 1);
        assert_eq!(diags[0].notes[0].1, "previously declared here");
    }

    #[test]
    fn duplicate_let_const_collision_diagnoses() {
        let (_, diags) = run("let x: number = 1; const x: number = 2;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "duplicate declaration of `x`");
    }

    #[test]
    fn top_level_expr_stmt_ignored_in_definitions() {
        let ta = run_clean("42;");
        assert!(PackageDeclaration::from_typed_ast(&ta).values.is_empty());
    }

    #[test]
    fn if_at_top_level_ignored_in_definitions() {
        let ta = run_clean("if (true) { }");
        assert!(PackageDeclaration::from_typed_ast(&ta).values.is_empty());
    }

    #[test]
    fn forward_references_supported() {
        let ta = run_clean("function a(): void { } function b(): void { }");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        assert!(reg.values.contains_key("a"));
        assert!(reg.values.contains_key("b"));
    }

    #[test]
    fn definitions_iteration_order_is_deterministic() {
        let ta = run_clean("function b(): void { } function a(): void { }");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        let names: Vec<&str> = reg.values.keys().map(std::string::String::as_str).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn definitions_from_typed_ast_round_trip() {
        let src = r#"function f(a: number, b: string): boolean { return true; } let x: number = 1; const y: string = "hi";"#;
        let ta = run_clean(src);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        assert_eq!(reg.values.len(), 3);
        match &reg.values.get("f").unwrap().kind {
            ValueKind::Function { params, ret, .. } => {
                assert_eq!(
                    params,
                    &[Param::new("a", Type::Number), Param::new("b", Type::String),]
                );
                assert_eq!(*ret, Type::Boolean);
            }
            _ => panic!("expected Function"),
        }
        match &reg.values.get("x").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Number),
            _ => panic!("expected Let"),
        }
        match &reg.values.get("y").unwrap().kind {
            ValueKind::Const { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn definitions_from_typed_ast_skips_non_decls() {
        let ta = run_clean("42; if (true) { } let x: number = 1;");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        assert_eq!(reg.values.len(), 1);
        assert!(reg.values.contains_key("x"));
    }

    #[test]
    fn redeclaring_number_emits_duplicate_binding() {
        let (_, diags) =
            run("function Number(s: string): number { return 0; } function main(): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate") || d.message.contains("already")),
            "expected duplicate-binding diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn redeclaring_string_emits_duplicate_binding() {
        let (_, diags) = run("let String: number = 42; function main(): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate") || d.message.contains("already")),
            "expected duplicate-binding diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn cannot_redeclare_console_as_function() {
        let (_, diags) = run("function console(): void { } function main(): void { }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate declaration of `console`")),
            "expected duplicate-declaration diagnostic, got: {diags:?}"
        );
    }
}
