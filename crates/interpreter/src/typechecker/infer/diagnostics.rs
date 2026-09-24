//! Diagnostic-emitting helpers and the wrappers around the
//! `.d.ts`-style definition / signature lifts used by B1 / B2.
//!
//! Also hosts the "did you mean" closest-match callers (B3) — they
//! delegate to [`crate::did_you_mean`] but need access to `self.types`
//! and `self.scopes`, so the helper sits here on the `Inferer`.

use crate::{Diagnostic, ExprId, MethodSig, Severity, Span, Type, TypeKind, ValueKind};

use super::format_signature::SignatureKind;
use super::lookup::FieldWrite;
use crate::did_you_mean;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;

use super::{Inferer, format_definition, format_signature, narrowing};

/// Add-on payload for a diagnostic: free-form `help:` lines plus
/// span-anchored notes. Used by the narrowing-invalidation hint
/// (plan 75.17a) to attach both a fix-shape help and a "this killed
/// you" pointer to a primary diagnostic.
type DiagnosticAddon = (Vec<String>, Vec<(Span, String)>);

/// Whether a type *spells* `null` — the signal that a failing read is about a
/// missing narrowing rather than an unrelated mismatch. Deliberately narrower
/// than `infer::expr`'s `type_admits_null`, which answers the semantic question.
fn spells_null(ty: &Type) -> bool {
    match ty.peel() {
        Type::Null => true,
        Type::Union(members) => members.iter().any(spells_null),
        // `unknown` admits null too, but its callers gate on assignability and
        // `unknown` is assignable to nothing, so reporting it here would never
        // reach a hint. Explaining a missing `unknown` guard needs its own
        // route, not this one.
        _ => false,
    }
}

/// Which side of the assignment a member access sits on. A write carries the
/// operator, because the rewrite it names has to be the edit the reader makes —
/// `x!.f = …` is a different operation from `x.f++`.
#[derive(Clone, Copy)]
enum MemberSide {
    Read,
    Write(Option<RwOp>),
}

/// How a write to `target` is spelled, for a rewrite the reader can paste.
/// `None` is a plain assignment.
fn write_form(target: &str, op: Option<RwOp>) -> String {
    match op {
        None => format!("{target} = …"),
        Some(rw @ RwOp::Postfix(_)) => format!("{target}{}", rw.text()),
        Some(rw) => format!("{target} {} …", rw.text()),
    }
}

/// What a member name resolves to on a receiver with `null` removed. A method
/// has to be *called*, so its rewrites carry the call; a bare `x?.m` is a method
/// reference, which is not a value in this language.
#[derive(Clone, Copy)]
enum NonNullAccess {
    Field,
    /// The arity is carried rather than assumed: `a!.map()` is a fix that does
    /// not compile, and it drops whatever the reader already passed.
    Method {
        takes_args: bool,
    },
}

impl NonNullAccess {
    fn call_suffix(self) -> &'static str {
        match self {
            NonNullAccess::Field => "",
            NonNullAccess::Method { takes_args: false } => "()",
            NonNullAccess::Method { takes_args: true } => "(…)",
        }
    }
}

/// Which string-coercion site rejected a nullable value — selects the
/// rewrite shape shown by [`Inferer::nullable_string_fix_help`].
pub(super) enum NullableStringContext {
    Interpolation,
    ToStringCall,
}

/// A read-modify-write operator: `x.f += v` or `x.f++`. Holds the operator
/// itself rather than its renderings, so a diagnostic cannot name one operator
/// while applying another's accept rule.
#[derive(Clone, Copy)]
pub(super) enum RwOp {
    Compound(crate::BinOp),
    /// Only `Inc` / `Dec` reach here — `infer_postfix` sends `NonNullAssert`
    /// down its own path before any target is resolved.
    Postfix(crate::PostfixOp),
}

impl RwOp {
    /// How the source spells the operator.
    pub(super) fn text(self) -> String {
        match self {
            RwOp::Compound(binary) => format!("{}=", super::stmt::binary_op_text(binary)),
            RwOp::Postfix(crate::PostfixOp::Dec) => "--".to_string(),
            RwOp::Postfix(_) => "++".to_string(),
        }
    }

    /// The binary operator a written-out rewrite applies.
    pub(super) fn sign(self) -> &'static str {
        match self {
            RwOp::Compound(binary) => super::stmt::binary_op_text(binary),
            RwOp::Postfix(crate::PostfixOp::Dec) => "-",
            RwOp::Postfix(_) => "+",
        }
    }

    /// Whether the operator is defined for `ty` at all. A field it rejects
    /// regardless of `null` is not a nullability problem, and saying otherwise
    /// sends the reader after a guard that changes nothing.
    fn accepts(self, ty: &Type) -> bool {
        // Arithmetic widens literals; the result cannot be stored back into
        // a literal-only field, even after a null guard.
        let is_literal =
            |ty: &Type| matches!(ty.peel(), Type::NumberLiteral(_) | Type::StringLiteral(_));
        if is_literal(ty)
            || matches!(ty.peel(), Type::Union(members) if members.iter().all(is_literal))
        {
            return false;
        }
        match self {
            RwOp::Compound(binary) => super::stmt::compound_arith_result(binary, ty, ty).is_some(),
            // `++` / `--` write `x.f ± 1` back, so they need what that binary
            // form needs, including a compatible write-back type.
            RwOp::Postfix(_) => {
                super::stmt::compound_arith_result(crate::BinOp::Add, ty, &Type::Number).is_some()
                    || super::stmt::compound_arith_result(crate::BinOp::Add, ty, &Type::BigInt)
                        .is_some()
            }
        }
    }
}

/// A position that compares a value rather than testing it for truthiness.
/// Sibling of [`ValuePosition`](super::resolve_type::ValuePosition) rather than
/// a shared enum — the two sentence frames differ.
#[derive(Clone, Copy)]
pub(super) enum ComparisonPosition {
    EqualityOperand,
    SwitchDiscriminant,
}

impl std::fmt::Display for ComparisonPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ComparisonPosition::EqualityOperand => "an equality operand",
            ComparisonPosition::SwitchDiscriminant => "a `switch` discriminant",
        })
    }
}

impl<'a> Inferer<'a> {
    pub(super) fn error(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            help: vec![],
            notes: vec![],
        });
    }

    /// Like [`Self::error`] but attaches one or more `help:` blocks
    /// (free-form, no span anchor). Span-anchored secondaries belong
    /// in [`Diagnostic::notes`] — this is for lifted type/signature
    /// definitions and similar fix-shape text the LLM needs but that
    /// doesn't point anywhere in the source.
    pub(super) fn error_with_help(&mut self, span: Span, message: String, help: Vec<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            help,
            notes: vec![],
        });
    }

    /// A `void`/`never` expression where a value is compared rather than
    /// tested for truthiness — a `switch` discriminant or an `===`/`!==`
    /// operand. Worded separately from
    /// [`error_non_condition_type`](Self::error_non_condition_type) because
    /// neither position is a condition, and truthiness is not what fails.
    pub(super) fn error_non_comparable_type(
        &mut self,
        span: Span,
        ty: &Type,
        position: ComparisonPosition,
    ) {
        self.error_with_help(
            span,
            format!("cannot compare `{ty}`: {position} must be a value"),
            vec![format!(
                "`{ty}` produces no value to compare. Call something that returns \
                 one, or drop the comparison and call this as its own statement."
            )],
        );
    }

    /// Only `void`/`never` reach this since JS truthiness landed — every
    /// value-bearing type is condition-compatible. Comparison positions use
    /// [`error_non_comparable_type`](Self::error_non_comparable_type) instead.
    pub(super) fn error_non_condition_type(&mut self, span: Span, ty: &Type) {
        let help = vec![
            "conditions need a value to test for truthiness; this expression \
             produces none. Call something that returns a value, or restructure \
             the condition."
                .to_string(),
        ];
        self.error_with_help(
            span,
            format!("expected a value in this condition, got `{ty}`"),
            help,
        );
    }

    /// Like [`Self::error_with_help`] but also attaches span-anchored
    /// secondary notes. Used by diagnostic sites that lift both a
    /// fix-shape help block and a "this is what killed you" pointer
    /// at another location — currently the property-path narrowing
    /// tombstone surfaces (plan 75.17a).
    pub(super) fn error_with_help_and_notes(
        &mut self,
        span: Span,
        message: String,
        help: Vec<String>,
        notes: Vec<(Span, String)>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            help,
            notes,
        });
    }

    /// Plan 75.17a: build the `(help, notes)` add-on for a failing
    /// field / method / index access whose receiver lost a narrowing.
    /// Returns `None` when there's nothing relevant to surface — the
    /// caller then emits the bare diagnostic.
    ///
    /// Strategy: derive the receiver's [`narrowing::ReferencePath`]
    /// and look it up first in the tombstone stack (a narrowing was
    /// installed and then killed), then in the closure-mutator set
    /// (the narrowing was refused outright under stability rule 5).
    /// The two sources are mutually exclusive in practice — a path
    /// can't have both a live tombstone and a captured-mutator root
    /// at the same time without something else having gone wrong.
    pub(super) fn narrowing_invalidation_hint(&self, receiver: ExprId) -> Option<DiagnosticAddon> {
        let receiver_expr = self.typed_ast.expr(receiver);
        if let Some(hint) = self.getter_narrowing_hint(&receiver_expr.kind) {
            return Some(hint);
        }
        let path = self.expr_to_reference_path(receiver_expr)?;
        self.narrowing_hint_for_path(&path)
    }

    /// The operand whose `null` is what breaks this site: it admits `null`, and
    /// `accepts` says dropping `null` would make the site legal. Callers must supply
    /// that second half — advice that still fails when followed verbatim is worse
    /// than no advice, and a nullable operand sitting beside an unrelated mismatch
    /// is the common case, not the rare one.
    pub(super) fn nullable_culprit(
        &self,
        operands: &[(ExprId, &Type)],
        accepts: impl Fn(&Type) -> bool,
    ) -> Option<ExprId> {
        operands.iter().find_map(|(expr, ty)| {
            if !spells_null(ty) {
                return None;
            }
            let non_null = super::narrow_scopes::non_null_form((*ty).clone())?;
            accepts(&non_null).then_some(*expr)
        })
    }

    /// [`error_with_help`](Self::error_with_help) plus the narrowing hint for
    /// `culprit` — the operand a caller has already established is nullable *and*
    /// the reason this site fails (see [`nullable_culprit`](Self::nullable_culprit)).
    /// `None` emits the bare diagnostic.
    pub(super) fn error_with_narrowing_hint(
        &mut self,
        span: Span,
        message: String,
        mut help: Vec<String>,
        culprit: Option<ExprId>,
    ) {
        let mut notes = Vec::new();
        if let Some((extra_help, extra_notes)) =
            culprit.and_then(|e| self.narrowing_invalidation_hint(e))
        {
            help.extend(extra_help);
            notes.extend(extra_notes);
        }
        self.error_with_help_and_notes(span, message, help, notes);
    }

    /// The culprit for a binary operator, given the operator's own rule for which
    /// operand pairs it accepts. `accepts` must be the *same* rule the arm used to
    /// decide it had an error — "the two sides now agree" is not enough, since
    /// `number[] + number[]` agrees and is still rejected.
    pub(super) fn nullable_binary_culprit(
        &self,
        lhs: (ExprId, &Type),
        rhs: (ExprId, &Type),
        accepts: impl Fn(&Type, &Type) -> bool,
    ) -> Option<ExprId> {
        self.nullable_culprit(&[lhs], |non_null| accepts(non_null, rhs.1))
            .or_else(|| self.nullable_culprit(&[rhs], |non_null| accepts(lhs.1, non_null)))
            .or_else(|| {
                // Both sides nullable: neither one-sided probe can match, yet a
                // single guard may still cover both (`s + s`, or two variables
                // guarded together). Try dropping `null` from both.
                let rhs_non_null = super::narrow_scopes::non_null_form(rhs.1.clone())?;
                self.nullable_culprit(&[lhs], |non_null| accepts(non_null, &rhs_non_null))
            })
    }

    /// Why a path isn't narrowed here, most specific cause first. A killed
    /// narrowing, a closure boundary and a refused shape are different stories;
    /// whichever applies must win consistently, or the same program point gets
    /// contradictory advice depending on which diagnostic site fired.
    pub(super) fn narrowing_hint_for_path(
        &self,
        path: &narrowing::ReferencePath,
    ) -> Option<DiagnosticAddon> {
        if let Some(reason) = self.lookup_tombstone(path) {
            return Some(self.invalidation_reason_hint(path, &reason));
        }
        if self.path_root_is_captured_mutator(path) {
            return Some(self.captured_mutator_hint(path));
        }
        if self.narrowed_in_suspended_frame(path) {
            return Some(self.closure_boundary_hint(path));
        }
        None
    }

    /// Explains a nullable read on a path whose guard the engine cannot carry
    /// to this point, for diagnostic sites that fire while inferring the
    /// expression rather than on a later read of it.
    ///
    /// Fires only when dropping `null` would actually satisfy `want` — that is
    /// what separates "you need a guard here" from an unrelated mismatch that
    /// merely happens to involve a nullable value.
    pub(super) fn narrowing_refusal_hint(
        &self,
        kind: &crate::TypedExprKind,
        got: &Type,
        want: &Type,
    ) -> Option<DiagnosticAddon> {
        if !spells_null(got) {
            return None;
        }
        let non_null = super::narrow_scopes::non_null_form(got.clone())?;
        if !super::assignable::assignable(&non_null, want, self.resolver()) {
            return None;
        }
        if let Some(hint) = self.getter_narrowing_hint(kind) {
            return Some(hint);
        }
        let path = self.kind_to_reference_path(kind)?;
        self.narrowing_hint_for_path(&path)
    }

    fn getter_narrowing_hint(&self, kind: &crate::TypedExprKind) -> Option<DiagnosticAddon> {
        let state = self.kind_to_reference_path_state(kind)?;
        if !state.contains_getter {
            return None;
        }
        let rendered = state.path.render();
        let tmp = self.fresh_hint_binding();
        Some((
            vec![format!(
                "`{rendered}` is getter-backed, so each read may return a different value and its guard cannot narrow later reads. Bind one read to a local `const` first: `const {tmp} = {rendered};` then guard `{tmp}`."
            )],
            Vec::new(),
        ))
    }

    /// A binding name the suggested rewrite can introduce without colliding
    /// with something already in scope — `const v = v` is not advice.
    fn fresh_hint_binding(&self) -> String {
        (0..)
            .map(|n| {
                if n == 0 {
                    "v".to_string()
                } else {
                    format!("v{n}")
                }
            })
            .find(|name| self.scopes.get(name).is_none())
            .unwrap_or_else(|| "v".to_string())
    }

    /// The path is narrowed outside the closure we are currently inferring, but
    /// isn't one of the stable depth-0 narrowings that cross the
    /// boundary (stability rule 9).
    fn closure_boundary_hint(&self, path: &narrowing::ReferencePath) -> DiagnosticAddon {
        let rendered = path.render();
        let tmp = self.fresh_hint_binding();
        // Always the hoist form: a `let` could also just be declared `const`,
        // but a parameter can't, and both are `BindingId::Local` here.
        (
            vec![format!(
                "narrowing on `{rendered}` does not cross a closure boundary — \
                 only a directly narrowed binding with no later or nested-function \
                 writes keeps its narrowing inside a closure body. Bind it to a `const` \
                 first: `const {tmp} = {rendered}; if ({tmp} !== null) {{ … }}` — or \
                 re-narrow inside the closure.",
            )],
            Vec::new(),
        )
    }

    fn invalidation_reason_hint(
        &self,
        path: &narrowing::ReferencePath,
        reason: &narrowing::InvalidationReason,
    ) -> DiagnosticAddon {
        let rendered = path.render();
        // Neither help offers an `if (x !== null)` example: the dropped narrowing
        // may be a discriminant or `typeof` one, where that edit does nothing. The
        // primary diagnostic already names the form this type needs.
        let (help, note_text) = match reason {
            narrowing::InvalidationReason::ShapeUnrebuildable { .. } => (
                format!(
                    "cannot preserve this guard on `{rendered}` because its source cannot be \
                     rebuilt here. Bind the value to a local `const` and guard that instead."
                ),
                String::new(),
            ),
            narrowing::InvalidationReason::Write { .. } => (
                format!(
                    "narrowing on `{rendered}` was dropped by the write — \
                     re-narrow it after the write to read it again."
                ),
                // Names no path: the write may be to a *prefix* of `rendered`
                // (`o = …` drops `o.a`), and the span already points at it.
                format!("this write invalidates the narrowing on `{rendered}`"),
            ),
            narrowing::InvalidationReason::Reassignment { .. } => (
                format!(
                    "narrowing on `{rendered}` was dropped by the reassignment — \
                     re-narrow it after the reassignment to read it again."
                ),
                format!("this reassignment invalidates the narrowing on `{rendered}`"),
            ),
            narrowing::InvalidationReason::CapturedMutator { .. } => {
                // Should not be reachable via lookup_tombstone — the
                // captured-mutator case is detected via
                // `path_root_is_captured_mutator` below. Fall back to
                // a generic message just in case.
                (
                    format!("narrowing on `{rendered}` was refused (stability rule 5)."),
                    "narrowing refused here".to_string(),
                )
            }
        };
        let mut notes = Vec::new();
        if let Some(span) = reason.span() {
            notes.push((span, note_text));
        }
        (vec![help], notes)
    }

    /// Leads with the read-into-`const`-and-write-back rewrite rather than
    /// "drop the reassignment", because the reassignment is often the very
    /// statement being diagnosed — `v += 1` or `v = v + 1` inside the closure.
    /// There, dropping it and declaring `v` as `const` are both refusals to write
    /// the program. The hoist form compiles either way.
    fn captured_mutator_hint(&self, path: &narrowing::ReferencePath) -> DiagnosticAddon {
        let root_name = match &path.root {
            narrowing::BindingId::Local { name, .. } => name.clone(),
            narrowing::BindingId::Global(_) | narrowing::BindingId::This => path.render(),
        };
        let tmp = self.fresh_hint_binding();
        (
            vec![format!(
                "narrowing on `{root_name}` was refused because a closure body \
                 reassigns `{root_name}` — stability rule 5. Read it into a \
                 `const` and write back: `const {tmp} = {root_name}; \
                 if ({tmp} !== null) {{ {root_name} = {tmp}…; }}` — or, if \
                 `{root_name}` never needs to change, declare it `const`."
            )],
            Vec::new(),
        )
    }

    /// Wrapper around [`format_definition::format_definition`] that threads the
    /// FQN registry (with `self.types` as fallback) so a library-typed value's
    /// shape is lifted into help even when the interface name was never imported.
    pub(super) fn format_definition(&self, ty: &Type) -> String {
        format_definition::format_definition(ty, &self.types, &self.type_registry)
    }

    /// The type's definition lifted as a `help:` block, empty when the lift is
    /// only the type's own `Display`. Every caller already prints the type in the
    /// message line, so a union (and the other arms with no members to list)
    /// would otherwise echo it back as advice.
    pub(super) fn definition_help(&self, ty: &Type) -> Vec<String> {
        let lifted = self.format_definition(ty);
        if lifted == ty.peel().to_string() {
            return Vec::new();
        }
        vec![lifted]
    }

    /// Look up the doc comment for a top-level function by name.
    /// Returns `None` for non-functions or unknown names. Used by B2
    /// signature lifts to thread the function's JSDoc into the help
    /// block..
    pub(super) fn lookup_function_doc(&self, name: &str) -> Option<crate::DocComment> {
        self.top_symbols
            .get(name)
            .and_then(|entry| match &entry.kind {
                ValueKind::Function { doc, .. } => doc.clone(),
                _ => None,
            })
    }

    /// Builds the substitution a method lift renders with — empty for every
    /// other kind — and hands it to the formatter.
    ///
    /// Used by the B2 diagnostic sites (method / generic-call arity, type-arg,
    /// argument-type mismatches) that need to lift a callable's signature into a
    /// one-line `help:` block. A method lift substitutes at the args the member
    /// was actually looked up with — see
    /// [`method_lift_substitution`](Self::method_lift_substitution).
    pub(super) fn format_signature(&self, kind: format_signature::SignatureKind<'_>) -> String {
        let substitution = match &kind {
            SignatureKind::Method {
                receiver_ty, name, ..
            } => self.method_lift_substitution(receiver_ty, name),
            _ => TypeParamSubstitution::new(),
        };
        format_signature::format_signature(kind, &substitution)
    }

    /// The type-parameter table a method lift renders with: re-asks
    /// [`find_method`](Inferer::find_method) for the bindings it resolves at the
    /// call site, rather than zipping a second table from the receiver's own
    /// declaration.
    ///
    /// An inherited method is written in its *declaring* class's parameter
    /// names. `class StringBox extends Box<string>` declares no generics at all,
    /// so a receiver-side table is empty and the lift prints `Box`'s raw `T`;
    /// `class Flip<A, B> extends Pair<B, A>` binds `A`/`B` while the signature
    /// spells `K`/`V`, so even a non-empty one misses. Only the chain walk that
    /// found the method resolved `Flip<string, number>` to `Pair<number,
    /// string>`, and it hands the answer back.
    fn method_lift_substitution(&self, receiver_ty: &Type, name: &str) -> TypeParamSubstitution {
        match self.find_method(receiver_ty, name) {
            Some((_, bindings, ..)) => TypeParamSubstitution::from_bindings(bindings),
            None => TypeParamSubstitution::new(),
        }
    }

    /// Suggest the closest identifier (across local scopes + top
    /// symbols) to `query`. Used by B3's "unresolved identifier"
    /// diagnostics to attach a "did you mean `X`?" help block.
    pub(super) fn closest_local_or_global(&self, query: &str) -> Option<String> {
        let locals: Vec<&str> = self.scopes.all_names().collect();
        let globals = self.top_symbols.keys().map(String::as_str);
        did_you_mean::closest_match(query, locals.into_iter().chain(globals)).map(String::from)
    }

    /// Suggest the closest type name to `query` from the four
    /// primitive keywords + every name in `self.types`. Used by B3's
    /// "unknown type" diagnostic.
    pub(super) fn closest_type_name(&self, query: &str) -> Option<String> {
        const BUILTINS: [&str; 5] = ["number", "string", "boolean", "void", "never"];
        let user_types: Vec<&str> = self.types.iter_names().collect();
        did_you_mean::closest_match(query, BUILTINS.into_iter().chain(user_types)).map(String::from)
    }

    /// Report a field that does not exist on `receiver_ty` — the single place
    /// that decides whether such a miss is reportable at all. A class whose
    /// `extends` clause never resolved could have inherited the field from the
    /// parent we failed to find, so the miss there is unknowable rather than
    /// wrong, and the clause's own diagnostic stands alone.
    pub(super) fn report_missing_field(&mut self, span: Span, receiver_ty: &Type, name: &str) {
        if self.receiver_inherits_unresolved_parent(receiver_ty) {
            return;
        }
        let help = self.interface_member_miss_help(receiver_ty, name);
        self.error_with_help(
            span,
            format!("field `{name}` does not exist on `{receiver_ty}`"),
            help,
        );
    }

    /// Reports a failing member *read* that is really a method named without
    /// its call, answering whether it did. `false` means the name is not a
    /// method and the caller still owes a diagnostic — same shape as
    /// [`report_static_on_instance`](Self::report_static_on_instance).
    ///
    /// Read positions only. In a write the fix is not "call it", so assignment
    /// goes straight to [`report_missing_field`](Self::report_missing_field).
    pub(super) fn try_report_method_reference(
        &mut self,
        span: Span,
        receiver_ty: &Type,
        name: &str,
    ) -> bool {
        let Some((sig, ..)) = self.find_method(receiver_ty, name) else {
            return false;
        };
        self.report_method_reference(span, receiver_ty, name, &sig);
        true
    }

    /// A method named where a value is expected. A bare instance-method
    /// reference is not a value in this language, on any receiver, so the fix is
    /// always to call it and the signature lift shows what to pass.
    pub(super) fn report_method_reference(
        &mut self,
        span: Span,
        receiver_ty: &Type,
        name: &str,
        sig: &MethodSig,
    ) {
        // Rendered with the receiver's generic bindings applied, so the
        // parameters shown are the ones the caller actually has to pass.
        let help = self.format_signature(SignatureKind::Method {
            receiver_ty,
            name,
            sig,
        });
        self.error_with_help(
            span,
            format!("method `{name}` must be called — a method reference is not a value"),
            vec![
                help,
                "to pass it as a value, wrap the call in an arrow".to_string(),
            ],
        );
    }

    /// Reports a failing member *write* that is really an assignment to a method,
    /// answering whether it did. `false` means the name is not a method and the
    /// caller still owes a diagnostic — the write-position twin of
    /// [`try_report_method_reference`](Self::try_report_method_reference).
    ///
    /// Without it a class receiver reports the name as missing and then suggests
    /// that same name back, and an interface receiver calls the method a readonly
    /// property; both leave the reader to guess that methods are not assignable
    /// at all.
    pub(super) fn try_report_method_assignment(
        &mut self,
        span: Span,
        receiver_ty: &Type,
        name: &str,
    ) -> bool {
        let Some((sig, ..)) = self.find_method(receiver_ty, name) else {
            return false;
        };
        let help = self.format_signature(SignatureKind::Method {
            receiver_ty,
            name,
            sig: &sig,
        });
        self.error_with_help(
            span,
            format!("cannot assign to method `{name}` on `{receiver_ty}`"),
            vec![
                help,
                "methods are fixed at their declaration; only a field of function type \
                 can be reassigned"
                    .to_string(),
            ],
        );
        true
    }

    /// Rejects a nullable or optional field as a read-modify-write target
    /// (`x.f += v`, `x.f++`), answering whether it did.
    ///
    /// Only when dropping `null` would actually make the operator legal — the
    /// same rule [`nullable_culprit`](Self::nullable_culprit) enforces for the
    /// operator diagnostics. A `boolean`, array, or literal-union field admits
    /// no `+=` whether or not it is nullable, and blaming the null there sends
    /// the reader after a guard that changes nothing; those fall through to the
    /// operator's own message, which names the real problem.
    ///
    /// The rewrite it names is the written-out assignment rather than a guard,
    /// because narrowing does not reach a read-modify-write target: an `if`
    /// around the statement leaves the target's own read un-narrowed, so the
    /// advice that works everywhere else is wrong here.
    pub(super) fn try_report_nullable_rw_target(
        &mut self,
        receiver_span: Span,
        receiver_ty: &Type,
        name: &crate::Ident,
        field_ty: &Type,
        optional: bool,
        op: RwOp,
    ) -> bool {
        // Compare against the *peeled* type: `strip_null` peels, so an alias of
        // a non-nullable type would otherwise differ from its own declaration
        // and read as nullable.
        let non_null = narrowing::strip_null(field_ty);
        if !optional && non_null == *field_ty.peel() {
            return false;
        }
        if !op.accepts(&non_null) {
            return false;
        }
        // `++` / `--` mean exactly ±1, so the rewrite names the whole statement.
        // A compound assignment's right-hand side is whatever the caller wrote,
        // and guessing it would hand back code computing the wrong thing.
        let operand = match op {
            RwOp::Postfix(_) if matches!(non_null.peel(), Type::BigInt) => "1n",
            RwOp::Postfix(_) => "1",
            RwOp::Compound(_) => "…",
        };
        let rewrite = format!("{} {operand}", op.sign());
        let shape = if optional { "optional" } else { "nullable" };
        let recv = self.rewritable_snippet(receiver_span);
        let op_text = op.text();
        self.error_with_help(
            name.span,
            format!(
                "`{}` on `{receiver_ty}` is {shape}; `{op_text}` requires a non-null field",
                name.name,
            ),
            vec![format!(
                "narrowing does not reach the target of `{op_text}`; write the assignment out: \
                 `if ({recv}.{n} !== null) {{ {recv}.{n} = {recv}.{n} {rewrite}; }}`",
                n = name.name
            )],
        );
        true
    }

    /// A field read that misses on an object literal's own shape. Deliberately
    /// not [`report_missing_field`](Self::report_missing_field): that one's help
    /// routes a structural receiver to the prelude `Object` interface and would
    /// list *its* members. What the reader needs here is the shape they wrote.
    pub(super) fn report_missing_object_field(
        &mut self,
        span: Span,
        receiver_ty: &Type,
        name: &str,
    ) {
        let help = vec![self.format_definition(receiver_ty)];
        self.error_with_help(
            span,
            format!("field `{name}` does not exist on `{receiver_ty}`"),
            help,
        );
    }

    /// A field read on a receiver that carries no fields at all. Attaches the
    /// type's definition plus, when the receiver lost a narrowing, the hint
    /// naming what killed it. A nullable receiver gets the guard rewrite *instead
    /// of* the definition dump — the definition is not what is wrong with it.
    pub(super) fn report_non_object_field_read(
        &mut self,
        span: Span,
        receiver: ExprId,
        receiver_ty: &Type,
        name: &str,
    ) {
        let recv_span = self.typed_ast.expr(receiver).span;
        let recv_path = self.expr_to_reference_path(self.typed_ast.expr(receiver));
        let nullable = self.nullable_receiver_fix(
            recv_span,
            recv_path.as_ref(),
            receiver_ty,
            name,
            MemberSide::Read,
        );
        let (message, mut help) = match nullable {
            Some(fix) => (
                format!(
                    "cannot read field `{name}` on `{receiver_ty}`: the receiver can be `null`"
                ),
                vec![fix],
            ),
            None if receiver_ty.interface_routing().is_some()
                && !matches!(
                    receiver_ty.peel(),
                    Type::Number | Type::Boolean | Type::BooleanLiteral(_)
                ) =>
            {
                (
                    format!("field `{name}` does not exist on `{receiver_ty}`"),
                    self.interface_member_miss_help(receiver_ty, name),
                )
            }
            None => (
                format!("cannot read field `{name}` on non-object type `{receiver_ty}`"),
                self.definition_help(receiver_ty),
            ),
        };
        let mut notes: Vec<(Span, String)> = Vec::new();
        if let Some((extra_help, extra_notes)) = self.narrowing_invalidation_hint(receiver) {
            help.extend(extra_help);
            notes.extend(extra_notes);
        }
        self.error_with_help_and_notes(span, message, help, notes);
    }

    /// A field read on a union whose members otherwise carry fields, where one
    /// member can't back it. Which member and *why* are both load-bearing: told
    /// "does not exist" about a member that visibly declares the name, a reader
    /// goes looking for the wrong fix.
    pub(super) fn report_union_field_miss(
        &mut self,
        span: Span,
        receiver_path: Option<&narrowing::ReferencePath>,
        receiver_ty: &Type,
        missing: &Type,
        name: &str,
        miss: super::lookup::MemberFieldMiss,
    ) {
        use super::lookup::MemberFieldMiss;
        let message = match miss {
            MemberFieldMiss::Absent => format!(
                "field `{name}` does not exist on all members of `{receiver_ty}` \
                 (missing on `{missing}`)"
            ),
            MemberFieldMiss::Method => {
                format!(
                    "`{name}` is a method on `{missing}`, and a union receiver reads only fields"
                )
            }
            MemberFieldMiss::HostDispatched => format!(
                "`{name}` on `{missing}` is a built-in property, which a union receiver cannot reach"
            ),
        };
        let mut help = vec![
            self.format_definition(missing),
            union_narrow_help(receiver_ty),
        ];
        let mut notes: Vec<(Span, String)> = Vec::new();
        if let Some((extra_help, extra_notes)) =
            receiver_path.and_then(|p| self.narrowing_hint_for_path(p))
        {
            help.extend(extra_help);
            notes.extend(extra_notes);
        }
        self.error_with_help_and_notes(span, message, help, notes);
    }

    /// A member *write* whose receiver carries no assignable field map — the
    /// dead end `=`, `+=`, and `++` all fall to when
    /// [`assignment_target_fields`](Self::assignment_target_fields) returns
    /// `None`.
    ///
    /// A union is the one receiver here with a fix to name. `assignment_target_fields`
    /// answers `None` for *every* union, whether or not its members agree about
    /// the field, so per-member classification here is the only thing that can
    /// say which reason applies.
    ///
    /// The value the write was given is not typed here — callers that need the
    /// shape to check it against ask [`write_target_ty`](Self::write_target_ty).
    pub(super) fn report_unassignable_field_target(
        &mut self,
        recv_span: Span,
        name: &crate::Ident,
        receiver_ty: &Type,
        recv_path: Option<&narrowing::ReferencePath>,
        rw_op: Option<RwOp>,
    ) {
        use super::lookup::MemberFieldMiss;
        // The same gate the read side applies before it treats a receiver as a
        // union of shapes. A union carrying `null` or a primitive fails for a
        // different reason — that member has no fields at all — and the
        // narrow-to-one-member advice below would name `instanceof` at a
        // receiver whose fix is a null check.
        let members = match receiver_ty.peel() {
            Type::Union(members) if members.iter().all(Self::is_field_bearing) => members,
            _ => {
                return self.report_non_union_field_write(
                    recv_span,
                    name,
                    receiver_ty,
                    recv_path,
                    rw_op,
                );
            }
        };
        let field = &name.name;
        let miss = members.iter().find_map(|m| {
            self.union_member_field_read_ty(m, field)
                .err()
                .map(|reason| (m.clone(), reason))
        });
        let (message, missing) = match miss {
            Some((missing, MemberFieldMiss::Absent)) => (
                format!(
                    "field `{field}` does not exist on all members of `{receiver_ty}` \
                     (missing on `{missing}`)"
                ),
                Some(missing),
            ),
            Some((missing, MemberFieldMiss::Method)) => (
                format!(
                    "`{field}` is a method on `{missing}`; methods are fixed at their declaration"
                ),
                Some(missing),
            ),
            Some((missing, MemberFieldMiss::HostDispatched)) => (
                format!(
                    "`{field}` on `{missing}` is a built-in property, which a union receiver \
                     cannot assign through"
                ),
                Some(missing),
            ),
            // Every member backs the field, so it is the write itself that has
            // nowhere to go: the members may lay the field out at different
            // slots and there is no union-receiver field write to scan them.
            None => (
                format!(
                    "cannot assign to `{field}` through `{receiver_ty}`: a union receiver has \
                     no single field layout to write"
                ),
                None,
            ),
        };
        let mut help = Vec::new();
        match missing {
            Some(m) => {
                help.push(self.format_definition(&m));
                help.push(union_narrow_help(receiver_ty));
            }
            // Every member carries the field, so the shared narrow-first advice
            // is wrong here: `"f" in x` discriminates nothing.
            None => help.push(self.union_write_fix_help(
                recv_span,
                field,
                receiver_ty,
                members,
                recv_path,
                rw_op,
            )),
        }
        self.error_with_help(name.span, message, help);
    }

    /// The shape a write to `field` on this receiver is typed against, whatever
    /// rejected the write — including where no edit could make it legal, since a
    /// `readonly` field still says what a value written to it would have to be.
    /// `None` only when no member names a shape.
    ///
    /// `null` comes off first: adding `| null` to a receiver must not change what
    /// its value is checked against. Stripping is identity on a null-free type,
    /// so both reporters can share this.
    pub(super) fn write_target_ty(&self, receiver_ty: &Type, field: &str) -> Option<Type> {
        match narrowing::strip_null(receiver_ty) {
            Type::Union(members) => self.members_agree_on_write_ty(&members, field),
            single => self.union_member_field_write(&single, field).map(|w| w.ty),
        }
    }

    /// Every write receiver that is not a union of field-bearing members — a
    /// primitive, a nullable, or a union one of whose members carries no fields.
    /// None of them is helped by the `instanceof` advice
    /// [`report_unassignable_field_target`](Self::report_unassignable_field_target)
    /// gives a union.
    ///
    /// `null` is the one with a fix of its own, so it is answered here and
    /// anchored at the field, as the union messages are.
    fn report_non_union_field_write(
        &mut self,
        recv_span: Span,
        name: &crate::Ident,
        receiver_ty: &Type,
        recv_path: Option<&narrowing::ReferencePath>,
        rw_op: Option<RwOp>,
    ) {
        match self.nullable_receiver_fix(
            recv_span,
            recv_path,
            receiver_ty,
            &name.name,
            MemberSide::Write(rw_op),
        ) {
            Some(fix) => self.error_with_help(
                name.span,
                format!(
                    "cannot assign to field `{}` of `{receiver_ty}`: the receiver can be `null`",
                    name.name,
                ),
                vec![fix],
            ),
            // Nothing to narrow and no member to name, so this keeps the
            // receiver anchor; the union messages name the field and anchor at
            // it, as the read side does.
            None => self.error(
                recv_span,
                format!("cannot assign to field of `{receiver_ty}`"),
            ),
        }
    }

    /// How to reach a non-null receiver, for a member access on either side of
    /// the assignment. `None` when the null is not what blocks the access.
    ///
    /// Claimed only when removing `null` makes *this* access legal — see
    /// [`access_after_null`](Self::access_after_null). Advice that lands on a
    /// second rejection is worse than the generic message it replaces, and it
    /// also costs the reader the type dump that message carries.
    fn nullable_receiver_fix(
        &self,
        recv_span: Span,
        recv_path: Option<&narrowing::ReferencePath>,
        receiver_ty: &Type,
        field: &str,
        side: MemberSide,
    ) -> Option<String> {
        if !spells_null(receiver_ty) {
            return None;
        }
        let access = self.access_after_null(&narrowing::strip_null(receiver_ty), field, side)?;
        let recv = self.rewritable_snippet(recv_span);
        let member = format!("{field}{}", access.call_suffix());
        let (nonnull_rewrite, guard_body) = match side {
            MemberSide::Read => (
                format!(
                    "read it as `{recv}?.{member}`, or assert non-null with `{recv}!.{member}`"
                ),
                format!("{{ … {recv}.{member} … }}"),
            ),
            MemberSide::Write(op) => (
                format!(
                    "assert non-null with `{}`",
                    write_form(&format!("{recv}!.{member}"), op),
                ),
                format!("{{ {} }}", write_form(&format!("{recv}.{member}"), op),),
            ),
        };
        // `!` and `?.` rewrite the access itself, so they hold whatever the
        // receiver is. A guard is different: it installs a narrowing, and only a
        // place can hold one — the same rule `union_write_fix_help` follows, and
        // the reason it asks for a `const` binding rather than printing a test
        // that compiles and then narrows nothing.
        let bind_first = format!(
            "bind {} to a `const` first and guard that, or {nonnull_rewrite}",
            match self.rewritable_text(recv_span) {
                Some(place) => format!("`{place}`"),
                None => "the receiver".to_string(),
            },
        );
        Some(match self.receiver_place(recv_path) {
            ReceiverPlace::Narrowable => {
                format!("guard first — `if ({recv} !== null) {guard_body}` — or {nonnull_rewrite}")
            }
            ReceiverPlace::BindFirst(reason) => format!("{reason} — {bind_first}"),
            // Some pathless receivers narrow fine and some cannot, so the advice
            // true of both is to bind first.
            ReceiverPlace::Unmodeled => bind_first,
        })
    }

    /// What `field` resolves to on the receiver once `null` is removed, or
    /// `None` when the non-null half rejects the access for its own reason.
    ///
    /// Field-*bearing* is not a strong enough test: a guard is only the fix if
    /// the access succeeds on the other side of it, and several shapes are
    /// field-bearing and fail anyway.
    fn access_after_null(
        &self,
        stripped: &Type,
        field: &str,
        side: MemberSide,
    ) -> Option<NonNullAccess> {
        let backs_field =
            |receiver: &Type| self.union_member_field_read_ty(receiver, field).is_ok();
        match (side, stripped) {
            (MemberSide::Read, Type::Union(members)) => members
                .iter()
                .all(backs_field)
                .then_some(NonNullAccess::Field),
            (MemberSide::Read, single) => {
                if let Some((sig, ..)) = self.find_method(single, field) {
                    return Some(NonNullAccess::Method {
                        // A defaulted or rest parameter need not be passed, so a
                        // rewrite showing `(…)` for one would ask the reader to
                        // invent an argument list they do not owe.
                        takes_args: sig.params.iter().any(|p| p.default.is_none() && !p.rest),
                    });
                }
                // `lookup_interface_property` rather than `find_property`: the
                // latter declines a VTable-dispatched interface, and this asks
                // only whether the read resolves, not how it lowers. Without it
                // the three commonest nullable receivers in generated code —
                // `T[] | null`, `string | null`, `Map<K, V> | null` — miss the
                // gate on `.length` / `.size`, which are properties rather than
                // fields.
                (backs_field(single) || self.lookup_interface_property(single, field).is_some())
                    .then_some(NonNullAccess::Field)
            }
            (MemberSide::Write(_), Type::Union(_)) => None,
            // Permission, not shape.
            (MemberSide::Write(_), single) => self
                .union_member_field_writable(single, field)
                .then_some(NonNullAccess::Field),
        }
    }

    /// How to reach a writable single-member view of a union whose members all
    /// declare the field.
    ///
    /// Every branch names a form that actually compiles *in this module*; each
    /// constraint on that lives with the helper that checks it.
    fn union_write_fix_help(
        &self,
        recv_span: Span,
        field: &str,
        receiver_ty: &Type,
        members: &[Type],
        recv_path: Option<&narrowing::ReferencePath>,
        rw_op: Option<RwOp>,
    ) -> String {
        if !members
            .iter()
            .any(|m| self.union_member_field_writable(m, field))
        {
            return format!(
                "`{field}` is readonly on every member; no narrowing makes it writable"
            );
        }
        let recv = self.rewritable_snippet(recv_span);
        // Writable *and* spellable here: the two together are what make a named
        // member usable in the `as` or `instanceof` the help is about to print.
        let nameable: Vec<&Type> = members
            .iter()
            .filter(|m| self.union_member_field_writable(m, field) && self.name_resolves_here(m))
            .collect();
        let cast = self
            .members_agree_on_write_ty(members, field)
            .and_then(|_| {
                nameable
                    .iter()
                    .find(|m| self.is_legal_cast_target(receiver_ty, m))
            })
            .map(|m| {
                format!(
                    "write through a checked cast — `{}`",
                    write_form(&format!("({recv} as {m}).{field}"), rw_op),
                )
            });
        let guard = self.narrowing_guard(&recv, members, field, &nameable);
        let narrow_only = format!("narrow `{recv}` to one member first");
        let primary = match self.receiver_place(recv_path) {
            ReceiverPlace::BindFirst(reason) => {
                format!("{reason} — bind `{recv}` to a `const` first, then narrow that")
            }
            // Some pathless receivers narrow fine (`x!`) and some cannot (a call
            // result), so binding first is named only when a guard exists to
            // follow it — the one advice true of both.
            ReceiverPlace::Unmodeled if guard.is_some() => {
                format!("bind `{recv}` to a `const` first, then narrow that")
            }
            ReceiverPlace::Unmodeled => narrow_only,
            ReceiverPlace::Narrowable => match guard {
                Some(guard) => format!(
                    "narrow to one member first — `{guard} {{ {} }}`",
                    write_form(&format!("{recv}.{field}"), rw_op),
                ),
                None => narrow_only,
            },
        };
        or_cast(primary, cast.as_deref())
    }

    /// Syntax alone does not answer this: a plain identifier the narrowing
    /// engine refuses to install a view for — one a closure reassigns — reads as
    /// a place and narrows like an element, so naming a guard for it hands back
    /// the line that just failed.
    fn receiver_place(&self, path: Option<&narrowing::ReferencePath>) -> ReceiverPlace {
        let Some(path) = path else {
            return ReceiverPlace::Unmodeled;
        };
        if matches!(path.chain.last(), Some(narrowing::PathElem::Index(_))) {
            return ReceiverPlace::BindFirst("an element is not a place a guard can narrow");
        }
        if self.path_root_is_captured_mutator(path) {
            return ReceiverPlace::BindFirst(
                "a variable a closure reassigns cannot hold a narrowing",
            );
        }
        ReceiverPlace::Narrowable
    }

    /// The shape every member gives a write to `field`, or `None` where they
    /// disagree or no member carries the field.
    ///
    /// Gates the cast advice — a cast names one member, so offering one where the
    /// members disagree hands back a form that compiles and then rejects the
    /// value. It is also the type a rejected write checks its right-hand side
    /// against.
    ///
    /// Agreement is over the members a write could actually go through: a
    /// `readonly` member can never be the cast's target, so letting it veto the
    /// others costs the cast *and* the shape. Where no member is writable there
    /// is nothing to prefer, and every member's shape counts — the write is
    /// doomed either way, and the value still has to be typed against something.
    fn members_agree_on_write_ty(&self, members: &[Type], field: &str) -> Option<Type> {
        let writes: Vec<FieldWrite> = members
            .iter()
            .filter_map(|m| self.union_member_field_write(m, field))
            .collect();
        let mut considered: Vec<&FieldWrite> = writes.iter().filter(|w| w.writable).collect();
        if considered.is_empty() {
            considered = writes.iter().collect();
        }
        let first = considered.first()?;
        considered
            .iter()
            .all(|w| w.ty == first.ty)
            .then(|| first.ty.clone())
    }

    /// The `if (…)` a reader can write to reach one member of this union, or
    /// `None` when no test distinguishes them and only a cast will do.
    ///
    /// A discriminant is read from *every* member — the test compares a field
    /// they all carry, so a member this module cannot name is no obstacle.
    /// `instanceof` has to name a class, so it draws from `nameable` only. It is
    /// preferred over the cast wherever such a class exists, including in a union
    /// that also holds interfaces, where it still narrows: a guard that picks the
    /// wrong member merely fails its test, while a cast that picks the wrong
    /// member traps.
    fn narrowing_guard(
        &self,
        recv: &str,
        members: &[Type],
        field: &str,
        nameable: &[&Type],
    ) -> Option<String> {
        if let Some((discriminant, variants)) = self.union_discriminant_with_nominals(members) {
            // Name a literal whose own member is writable — a bare `…` lets a
            // reader pick the branch where the field is `readonly`, which narrows
            // correctly and then rejects the write. Spellability is deliberately
            // not required: the literal is what the test compares, and it needs
            // no type name.
            let value = variants.iter().find_map(|(literal, idx)| {
                let member = members.get(idx.0 as usize)?;
                self.union_member_field_writable(member, field)
                    .then(|| render_literal(literal))
            });
            let value = value.unwrap_or_else(|| "…".to_string());
            return Some(format!("if ({recv}.{discriminant} === {value})"));
        }
        // `instanceof` tests the class itself — type arguments are erased, and
        // `instanceof A<string>` is rejected — so name the bare class.
        nameable.iter().find_map(|m| match m.peel() {
            Type::ClassRef { name, .. } => Some(format!("if ({recv} instanceof {name})")),
            _ => None,
        })
    }

    pub(super) fn interface_member_miss_help(&self, receiver_ty: &Type, name: &str) -> Vec<String> {
        let mut help = Vec::new();
        if let Some((mangled, _, iface_name, _)) = receiver_ty.interface_routing()
            && let Some(sym) = self.lookup_structural_type(&mangled, iface_name)
        {
            let candidates: Vec<&str> = match &sym.kind {
                TypeKind::Interface {
                    methods,
                    properties,
                    ..
                } => methods
                    .keys()
                    .chain(properties.keys())
                    .map(String::as_str)
                    .collect(),
                // Suggest only public members at miss sites; private members of
                // a class from another module are invisible (docs/classes.md §3).
                TypeKind::Class {
                    methods,
                    method_visibility,
                    fields,
                    ..
                } => methods
                    .keys()
                    .filter(|m| {
                        method_visibility.get(*m).copied() != Some(crate::Visibility::Private)
                    })
                    .chain(
                        fields
                            .iter()
                            .filter(|(_, f)| f.visibility != crate::Visibility::Private)
                            .map(|(n, _)| n),
                    )
                    .map(String::as_str)
                    .collect(),
                _ => Vec::new(),
            };
            if let Some(suggestion) = did_you_mean::closest_match(name, candidates) {
                help.push(format!("did you mean `{suggestion}`?"));
            }
        }
        help.push(self.format_definition(receiver_ty));
        help
    }

    /// Reject binding a `void` value (`void` is a return type only — it has
    /// no Wasm lowering). Checks the top-level type and direct union members
    /// only, so nested return-position voids (`() => void`) stay legal.
    /// Returns whether it errored.
    pub(super) fn reject_void_binding(&mut self, ty: &Type, span: Span) -> bool {
        let mentions_void = ty.carries_void();
        if mentions_void {
            self.error_with_help(
                span,
                "cannot bind a `void` value".to_string(),
                vec![
                    "`void` is a return type only — it carries no value. Call the \
                     expression as a statement instead of binding its result."
                        .to_string(),
                ],
            );
        }
        mentions_void
    }

    /// Build the fix trio for a nullable value in a string context —
    /// the runtime-checked `!` assert, a `??` fallback, and narrowing —
    /// each rewritten against the offending expression's source text so
    /// the model can apply one verbatim.
    pub(super) fn nullable_string_fix_help(
        &self,
        expr_span: Span,
        context: NullableStringContext,
    ) -> Vec<String> {
        let expr = self.rewritable_snippet(expr_span);
        let (asserted, defaulted) = match context {
            NullableStringContext::Interpolation => (
                format!("`${{{expr}!}}`"),
                format!("`${{{expr} ?? \"fallback\"}}`"),
            ),
            NullableStringContext::ToStringCall => (
                format!("`{expr}!.toString()`"),
                format!("`({expr} ?? \"fallback\").toString()`"),
            ),
        };
        vec![format!(
            "use a non-null value:\n\
             \x20 {asserted} — assert non-null (throws `Error` at runtime if null)\n\
             \x20 {defaulted} — provide a fallback\n\
             \x20 or narrow first: `if ({expr} !== null) {{ … }}`"
        )]
    }

    /// Source text of `span`, ready to take a postfix `!` or a trailing
    /// `?? …`: simple reference paths pass through verbatim, other
    /// single-line expressions are parenthesized to stay
    /// precedence-correct, and anything unrenderable falls back to `x`.
    fn rewritable_snippet(&self, span: Span) -> String {
        self.rewritable_text(span)
            .unwrap_or_else(|| "x".to_string())
    }

    /// Source text of `span` if it can be rendered at all, `None` otherwise.
    ///
    /// [`rewritable_snippet`](Self::rewritable_snippet) substitutes `x` for the
    /// `None`, which reads correctly inside a code sample (`x?.f`) and wrongly
    /// in prose that points at a binding — "bind `x` to a `const`" names a
    /// variable the reader does not have. Advice of the second kind asks here.
    fn rewritable_text(&self, span: Span) -> Option<String> {
        let text = self.source.get(span.start as usize..span.end as usize)?;
        if text.is_empty() || text.len() > 60 || text.contains('\n') {
            return None;
        }
        let is_path_char =
            |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '[' | ']');
        if text.chars().all(is_path_char) {
            Some(text.to_string())
        } else {
            Some(format!("({text})"))
        }
    }

    /// validate a condition expression's type, preferring
    /// the "narrow first" diagnostic when the type is `unknown`
    /// (forcing the LLM toward `typeof` / `x === null` /
    /// `Array.isArray(x)` rather than puzzling over a generic
    /// "expected boolean" mismatch). Falls back to the standard
    /// boolean-compatibility check for all other non-condition types.
    pub(super) fn check_condition_ty(&mut self, ty: &Type, span: Span) {
        if matches!(ty.peel(), Type::Unknown) {
            self.error_with_help(
                span,
                "cannot use `unknown` as a condition".to_string(),
                vec![
                    "narrow first with `typeof x === \"…\"`, `x === null`, \
                     or `Array.isArray(x)` before using as a condition"
                        .to_string(),
                ],
            );
        } else if !super::narrowing::condition_compatible(ty) {
            self.error_non_condition_type(span, ty);
        }
    }
}

/// The narrow-first advice every union member-access diagnostic ends on. One
/// definition so the read and write sides cannot name different fixes for the
/// same receiver.
fn union_narrow_help(receiver_ty: &Type) -> String {
    format!(
        "narrow `{receiver_ty}` to one member first — `instanceof`, a discriminant \
         field, or `\"<field>\" in x`"
    )
}

/// How a receiver relates to the guards that could narrow it.
enum ReceiverPlace {
    /// A place the narrowing engine will refuse a view for, carrying the reason
    /// so the help can say which refusal this is. Binding to a `const` escapes
    /// every one of them.
    BindFirst(&'static str),
    /// No path form at all. Some of these narrow fine (`x!`) and some cannot
    /// (a call result); the path model does not say which, so nothing about the
    /// receiver may be asserted.
    Unmodeled,
    /// A path a guard can bind a shadow for — an identifier or a field path.
    Narrowable,
}

/// Appends the cast as an alternative to whatever fix was named first. One
/// definition so the two sites that offer it cannot punctuate it differently.
fn or_cast(primary: String, cast: Option<&str>) -> String {
    match cast {
        Some(cast) => format!("{primary}, or {cast}"),
        None => primary,
    }
}

/// A discriminant's literal value, spelled as source. Only the three literal
/// kinds a discriminant can take reach here.
fn render_literal(literal: &narrowing::LiteralValue) -> String {
    match literal {
        narrowing::LiteralValue::String(s) => format!("{s:?}"),
        narrowing::LiteralValue::Number(n) => crate::runtime::number::format_number_js(n.0),
        narrowing::LiteralValue::Boolean(b) => b.to_string(),
    }
}

#[cfg(test)]
mod dropped_guard_tests {
    use super::super::test_support::run;

    const TYPES: &str = "class Leaf { z: number | null = 3; }\n\
        class Element { y: Leaf | null = new Leaf(); }\n\
        class Holder { elems: Element[] = [new Element()]; }\n";

    #[test]
    fn dropped_guard_hint_stays_in_its_branch() {
        for (condition, guarded_arm) in [
            (
                "h.elems[0].y !== null && h.elems[0].y.z !== null",
                "thenValue",
            ),
            (
                "h.elems[0].y === null || h.elems[0].y.z === null",
                "elseValue",
            ),
        ] {
            let source = format!(
                "{TYPES}
                function main(): void {{
                    const h = new Holder();
                    if ({condition}) {{
                        const thenValue: number = h.elems[0].y.z;
                    }} else {{
                        const elseValue: number = h.elems[0].y.z;
                    }}
                    const afterValue: number = h.elems[0].y.z;
                }}"
            );
            let (_, diagnostics) = run(&source);
            let hints: Vec<_> = diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic
                        .help
                        .iter()
                        .any(|help| help.contains("cannot preserve this guard"))
                })
                .collect();
            assert_eq!(hints.len(), 1, "{diagnostics:?}");
            let guarded_line = source
                .lines()
                .find(|line| line.contains(guarded_arm))
                .unwrap();
            let expected_start = source.find(guarded_line).unwrap();
            assert!(
                hints[0].span.start as usize >= expected_start,
                "{diagnostics:?}"
            );
            assert!(
                (hints[0].span.start as usize) < expected_start + guarded_line.len(),
                "{diagnostics:?}"
            );
        }
    }

    #[test]
    fn early_return_keeps_the_surviving_outcomes_drop() {
        let source = format!(
            "{TYPES}
            function main(): number {{
                const h = new Holder();
                if (h.elems[0].y === null || h.elems[0].y.z === null) return 0;
                return h.elems[0].y.z;
            }}"
        );
        let (_, diagnostics) = run(&source);
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .help
                    .iter()
                    .any(|help| help.contains("cannot preserve this guard"))
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn branch_join_keeps_only_a_common_dropped_refinement() {
        for (else_guard, expected_hint) in [
            (
                "if (h.elems[0].y === null || h.elems[0].y.z === null) return 0;",
                true,
            ),
            ("", false),
        ] {
            let source = format!(
                "{TYPES}
                function read(h: Holder, flag: boolean): number {{
                    if (flag) {{
                        if (h.elems[0].y === null || h.elems[0].y.z === null) return 0;
                    }} else {{ {else_guard} }}
                    return h.elems[0].y.z;
                }}"
            );
            let (_, diagnostics) = run(&source);
            let has_hint = diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .help
                    .iter()
                    .any(|help| help.contains("cannot preserve this guard"))
            });
            assert_eq!(has_hint, expected_hint, "{diagnostics:?}");
        }
    }

    #[test]
    fn condition_without_a_branch_cannot_leak_a_drop() {
        let source = format!(
            "{TYPES}
            function first(): void {{
                const h = new Holder();
                do {{}} while (h.elems[0].y !== null && h.elems[0].y.z !== null);
                const read = (): number => h.elems[0].y.z;
            }}
            function second(): number {{
                const h = new Holder();
                return h.elems[0].y.z;
            }}"
        );
        let (_, diagnostics) = run(&source);
        assert!(!diagnostics.is_empty());
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .help
                    .iter()
                    .any(|help| help.contains("cannot preserve this guard"))
            }),
            "{diagnostics:?}"
        );
    }
}
