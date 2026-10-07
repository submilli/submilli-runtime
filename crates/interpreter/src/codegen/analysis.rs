use std::collections::BTreeSet;

use crate::codegen::bigint_pool::BigIntPool;
use crate::codegen::closures::{ClosureMeta, ClosureSig};
use crate::codegen::dependency_usage::DependencyUsage;
use crate::codegen::function_adapters::AdapterMeta;
use crate::codegen::string_pool::StringPool;
use crate::codegen::{bounds, cast_check, throw};
use crate::{
    AccessorKind, BinOp, ClosureBody, ExprId, Intrinsic, PostfixTarget, Shape, StmtId, Type,
    TypedAst, TypedChainPart, TypedExprKind, TypedObjectFieldSource, TypedStmtKind,
    TypedSwitchValue,
};

pub struct CodegenAnalysis {
    pub string_pool: StringPool,
    pub bigint_pool: BigIntPool,
    pub dependency_usage: DependencyUsage,
    pub closure_metas: Vec<ClosureMeta>,
    pub adapter_metas: Vec<AdapterMeta>,
    /// Closure shapes named by any type the module mentions; see
    /// [`crate::codegen::closures`] for how this fits the other sources.
    pub mentioned_closure_sigs: Vec<ClosureSig>,
    pub extra_field_names: Vec<String>,
    adapter_seen: BTreeSet<crate::MangledName>,
}

impl CodegenAnalysis {
    pub fn collect(
        ta: &TypedAst,
        dependencies: &[&crate::PackageDeclaration],
    ) -> Result<Self, crate::compiler_error::CompilerFailure> {
        let mut analysis = Self {
            string_pool: StringPool::default(),
            bigint_pool: BigIntPool::default(),
            dependency_usage: DependencyUsage::empty(),
            closure_metas: Vec::new(),
            adapter_metas: Vec::new(),
            mentioned_closure_sigs: Vec::new(),
            extra_field_names: Vec::new(),
            adapter_seen: BTreeSet::new(),
        };

        // Generated serializers also cover dependency shapes discovered later.
        analysis.dependency_usage.note_member(crate::mangle::extend(
            &crate::mangle::prelude("ObjectConstructor"),
            "#toJson",
        ));

        for (id, ty) in &ta.runtime_source_types {
            let span = ta
                .try_expr(*id)
                .map_err(crate::codegen::arena_failure)?
                .span;
            analysis
                .visit_type(ty)
                .map_err(|error| error.with_span(span))?;
            analysis
                .string_pool
                .intern_text(&cast_check::error_prefix(ty));
        }
        if !ta.runtime_source_types.is_empty() {
            for text in cast_check::TYPE_TAG_STRINGS {
                analysis.string_pool.intern_text(text);
            }
            for name in [
                "numeric",
                "inc",
                "dec",
                "to_number",
                "to_index",
                "to_string",
                "member",
                "invoke",
                "property",
            ] {
                analysis
                    .dependency_usage
                    .note_value(crate::mangle::prelude(&format!("__value_{name}")));
            }
        }
        for (id, types) in &ta.runtime_chain_types {
            let span = ta
                .try_expr(*id)
                .map_err(crate::codegen::arena_failure)?
                .span;
            for ty in types {
                let ty = crate::typechecker::infer::narrowing::strip_null(ty);
                analysis.visit_type_at(&ty, span)?;
                analysis
                    .string_pool
                    .intern_text(&cast_check::error_prefix(&ty));
            }
        }

        for shape in &ta.shapes {
            if matches!(shape, Shape::Object { .. }) {
                analysis
                    .dependency_usage
                    .collect_typed_object_stringify_host_value();
            }
            analysis.dependency_usage.note_shape(shape.clone());
        }
        for g in &ta.globals {
            analysis.visit_type_at(&g.ty, g.span)?;
        }
        let init_guards = super::init_guard::guarded_globals(ta)?;
        if !init_guards.is_empty() {
            analysis
                .dependency_usage
                .note_type(crate::mangle::prelude("ReferenceError"));
        }
        for guard in init_guards {
            analysis.string_pool.intern_text(&guard.message);
        }

        for f in &ta.functions {
            for p in &f.params {
                analysis
                    .visit_type(&p.ty)
                    .map_err(|error| error.with_span(p.name.span))?;
            }
            analysis.visit_type_at(&f.return_type, f.span)?;
            analysis.walk_stmt(ta, f.body)?;
        }
        for &stmt_id in &ta.top_level_statements {
            analysis.walk_stmt(ta, stmt_id)?;
        }
        for stmt_id in ta.class_body_roots() {
            analysis.walk_stmt(ta, stmt_id)?;
        }
        for expr_id in ta.class_field_initializers() {
            analysis.walk_expr(ta, expr_id)?;
        }
        if !ta.runtime_class_fields.is_empty() {
            analysis
                .mentioned_closure_sigs
                .push(super::field_guards::signature());
        }
        analysis.note_narrowing_checks(ta)?;
        for test in ta.runtime_type_tests.values() {
            analysis.note_narrowing_test(test)?;
        }
        analysis.note_dependency_narrowing_checks(dependencies)?;

        // Imported class members can add closure adapters after dependency
        // selection, even when the source mentions no function-valued types.
        for name in [
            "__value_defaults_fit",
            "__value_invoke_defaults",
            "Number#toString",
            "string_concat",
        ] {
            analysis
                .dependency_usage
                .note_value(crate::mangle::prelude(name));
        }

        Ok(analysis)
    }

    /// A narrowed field's read guard throws a constant message and, when its test
    /// is structural, tests a shape; both need the same interning and imports a
    /// `Cast` gets, and neither is reachable from any expression, so no walk above
    /// finds them.
    fn note_narrowing_checks(
        &mut self,
        ta: &TypedAst,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for decl in &ta.types {
            let crate::TypedTypeDecl::Class(class) = decl else {
                continue;
            };
            if class
                .methods
                .iter()
                .any(|method| matches!(method.name.name.as_str(), "toString" | "toJson"))
            {
                self.string_pool
                    .intern_text(&cast_check::error_prefix(&Type::String));
                for tag in cast_check::TYPE_TAG_STRINGS {
                    self.string_pool.intern_text(tag);
                }
            }
            for check in class
                .fields
                .iter()
                .filter_map(|f| f.narrowing_check.as_ref())
            {
                self.note_narrowing_check(check)?;
            }
        }
        Ok(())
    }

    fn note_dependency_narrowing_checks(
        &mut self,
        dependencies: &[&crate::PackageDeclaration],
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        for package in dependencies {
            for symbol in package.runtime_types.values().chain(package.types.values()) {
                let crate::TypeKind::Class {
                    narrowing_checks, ..
                } = &symbol.kind
                else {
                    continue;
                };
                for check in narrowing_checks.values() {
                    self.note_narrowing_check(check)?;
                }
            }
        }
        Ok(())
    }

    fn note_narrowing_check(
        &mut self,
        check: &crate::FieldNarrowingCheck,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.string_pool.intern_text(&check.message);
        self.note_narrowing_test(&check.test)?;

        Ok(())
    }

    fn note_narrowing_test(
        &mut self,
        test: &crate::FieldNarrowingTest,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        match test {
            crate::FieldNarrowingTest::Shape(shape) => {
                self.visit_type(shape)?;
                self.note_shape_member_names(shape);
            }
            crate::FieldNarrowingTest::Interface(test) => {
                let shape = Type::Object {
                    index: test.index.clone(),
                    fields: test.members.clone(),
                };
                self.visit_type(&shape)?;
                self.note_shape_member_names(&shape);
            }
            crate::FieldNarrowingTest::NonNull
            | crate::FieldNarrowingTest::Substituted
            | crate::FieldNarrowingTest::Representation => {}
        };
        Ok(())
    }

    /// The per-name `$string` globals the shape's field scan reads: for each
    /// property, its own name and its `get <prop>` accessor name, plus the getter
    /// closure sig the accessor branch casts to. A recorded shape reaches neither
    /// `ta.shapes` nor any expression, so a member named nowhere else in the
    /// module has no global unless this registers it — and the field-scan lookup
    /// panics rather than importing one on demand.
    ///
    /// The accessor half is interned off the *test*, not off any accessor
    /// declaration, for the reason [`Self::note_shaped_property_access`] gives:
    /// whether a value is accessor-backed is a whole-program fact, and the module
    /// running the cast need not see the class that declares the accessor.
    ///
    /// `runtime_type_is_testable` defines the recursively structural `Shape`
    /// cases. Interface descriptors are registered separately by
    /// `note_narrowing_test`, including nested member names. The arms below are
    /// therefore the remaining shape forms that can nest an object; a new
    /// nesting arm in `emit_structural_test` needs one here too.
    fn note_shape_member_names(&mut self, ty: &Type) {
        match ty.peel() {
            Type::Object { fields, index } => {
                if let Some(index) = index {
                    self.note_record_helpers();
                    self.note_shape_member_names(&index.value);
                }
                if !fields.is_empty() {
                    self.mentioned_closure_sigs.push(
                        crate::codegen::classes::accessor_closure_sig(AccessorKind::Get),
                    );
                }
                for (name, field) in fields {
                    self.extra_field_names.push(name.clone());
                    self.extra_field_names
                        .push(crate::codegen::classes::accessor_getter_name(name));
                    self.note_shape_member_names(&field.ty);
                }
            }
            Type::Array(elem) => self.note_shape_member_names(elem),
            Type::Tuple(elems) => {
                for e in elems {
                    self.note_shape_member_names(e);
                }
            }
            Type::Union(members) => {
                for m in members {
                    self.note_shape_member_names(m);
                }
            }
            _ => {}
        }
    }

    /// Every type the module mentions funnels through here. `dependency_usage`
    /// needs it to import the symbols the type names; closure codegen needs the
    /// function-typed positions inside it. These are post-substitution types, so
    /// a generic instantiated with `void` contributes the sig it lowers to
    /// rather than the one its declaration spells.
    ///
    /// Call this rather than `dependency_usage.collect_type` directly —
    /// bypassing it drops the closure half silently, and the failure surfaces as
    /// an internal compiler failure far from the omission.
    fn visit_type_at(
        &mut self,
        ty: &Type,
        span: crate::Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.visit_type(ty).map_err(|error| error.with_span(span))
    }

    fn visit_type(&mut self, ty: &Type) -> Result<(), crate::compiler_error::CompilerFailure> {
        if let Type::Object {
            index: Some(index), ..
        } = ty.peel()
        {
            self.note_record_helpers();
            self.visit_type(&index.value)?;
        }
        self.dependency_usage.collect_type(ty);
        crate::codegen::closures::walk_type(ty, &mut self.mentioned_closure_sigs)?;

        Ok(())
    }

    fn note_record_helpers(&mut self) {
        for helper in ["#getField", "#setField", "#recordValues", "#hasField"] {
            self.dependency_usage.note_member(crate::mangle::extend(
                &crate::mangle::prelude("ObjectConstructor"),
                helper,
            ));
        }
    }

    fn walk_stmt(
        &mut self,
        ta: &TypedAst,
        id: StmtId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.note_stmt_pre(ta, id)?;
        match &ta.try_stmt(id).map_err(crate::codegen::arena_failure)?.kind {
            TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => {
                self.walk_expr(ta, *value)?;
            }
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.walk_expr(ta, *condition)?;
                self.walk_stmt(ta, *then_block)?;
                if let Some(else_block) = else_block {
                    self.walk_stmt(ta, *else_block)?;
                }
            }
            TypedStmtKind::While { condition, body } => {
                self.walk_expr(ta, *condition)?;
                self.walk_stmt(ta, *body)?;
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.walk_stmt(ta, *init)?;
                }
                if let Some(condition) = condition {
                    self.walk_expr(ta, *condition)?;
                }
                if let Some(update) = update {
                    self.walk_stmt(ta, *update)?;
                }
                self.walk_stmt(ta, *body)?;
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.walk_expr(ta, *iter)?;
                self.walk_stmt(ta, *body)?;
            }
            TypedStmtKind::DoWhile { body, condition } => {
                self.walk_stmt(ta, *body)?;
                self.walk_expr(ta, *condition)?;
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.walk_expr(ta, *discriminant)?;
                for case in cases {
                    self.walk_stmt(ta, case.body)?;
                }
                if let Some(default) = default {
                    self.walk_stmt(ta, *default)?;
                }
            }
            TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
            TypedStmtKind::Return(value) => {
                if let Some(value) = value {
                    self.walk_expr(ta, *value)?;
                }
            }
            TypedStmtKind::Expr(expr) => self.walk_expr(ta, *expr)?,
            TypedStmtKind::Block(stmts) => {
                for &stmt in stmts {
                    self.walk_stmt(ta, stmt)?;
                }
            }
            TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => self.walk_expr(ta, *value)?,
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => {
                self.walk_expr(ta, *receiver)?;
                self.walk_expr(ta, *value)?;
            }
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.walk_expr(ta, *receiver)?;
                self.walk_expr(ta, *index)?;
                self.walk_expr(ta, *value)?;
            }
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                self.walk_expr(ta, *source)?;
                self.walk_stmt(ta, *body)?;
            }
            TypedStmtKind::Throw { value } => self.walk_expr(ta, *value)?,
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.walk_stmt(ta, *body)?;
                for clause in catches {
                    self.walk_stmt(ta, clause.body)?;
                }
                if let Some(finally) = finally {
                    self.walk_stmt(ta, *finally)?;
                }
            }
        }
        self.note_stmt_post(ta, id)?;
        Ok(())
    }

    fn walk_expr(
        &mut self,
        ta: &TypedAst,
        id: ExprId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let expr = ta.try_expr(id).map_err(crate::codegen::arena_failure)?;
        self.note_expr_pre(ta, id)
            .map_err(|error| error.with_span(expr.span))?;
        let _: () = match &expr.kind {
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. } => {
                for &arg in args {
                    self.walk_expr(ta, arg)?;
                }
            }
            TypedExprKind::CallClosure { callee, args } => {
                self.walk_expr(ta, *callee)?;
                for &arg in args {
                    self.walk_expr(ta, arg)?;
                }
            }
            TypedExprKind::GenericCall {
                args, type_args, ..
            } => {
                self.mentioned_closure_sigs
                    .push(super::field_guards::signature());
                for ty in type_args {
                    self.visit_type(ty)?;
                }
                for arg in args {
                    self.walk_expr(ta, arg.expr)?;
                }
            }
            TypedExprKind::Closure { body, .. } => match *body {
                ClosureBody::Expr(expr) => self.walk_expr(ta, expr)?,
                ClosureBody::Block(stmt) => self.walk_stmt(ta, stmt)?,
            },
            TypedExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(ta, *lhs)?;
                self.walk_expr(ta, *rhs)?;
            }
            TypedExprKind::EffectThen { effect, result } => {
                self.walk_expr(ta, *effect)?;
                self.walk_expr(ta, *result)?;
            }
            TypedExprKind::Sequence { stmts, result } => {
                for &stmt in stmts {
                    self.walk_stmt(ta, stmt)?;
                }
                self.walk_expr(ta, *result)?;
            }
            TypedExprKind::Unary { operand, .. }
            | TypedExprKind::TypeofTag { value: operand, .. }
            | TypedExprKind::InstanceOf { value: operand, .. }
            | TypedExprKind::NonNullAssert { value: operand } => {
                self.walk_expr(ta, *operand)?;
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.walk_expr(ta, *receiver)?;
                for &arg in args {
                    self.walk_expr(ta, arg)?;
                }
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.walk_expr(ta, *receiver)?;
                for arg in args {
                    self.walk_expr(ta, arg.expr)?;
                }
            }
            TypedExprKind::ObjectLiteral { members, .. } => {
                for member in members {
                    for expression in member.expressions() {
                        self.walk_expr(ta, expression)?;
                    }
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                for elem in elements {
                    self.walk_expr(ta, elem.expr_id())?;
                }
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                for &elem in elements {
                    self.walk_expr(ta, elem)?;
                }
            }
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
                self.walk_expr(ta, *receiver)?;
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                self.walk_expr(ta, *receiver)?;
                self.walk_expr(ta, *index)?;
            }
            TypedExprKind::Narrowed { source, inner, .. } => {
                self.walk_expr(ta, *source)?;
                self.walk_expr(ta, *inner)?;
            }
            TypedExprKind::Ternary { cond, then_, else_ } => {
                self.walk_expr(ta, *cond)?;
                self.walk_expr(ta, *then_)?;
                self.walk_expr(ta, *else_)?;
            }
            TypedExprKind::NullishCoalesce { lhs, rhs } => {
                self.walk_expr(ta, *lhs)?;
                self.walk_expr(ta, *rhs)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.walk_expr(ta, *base)?;
                for part in parts {
                    match part {
                        TypedChainPart::Index { idx, .. } => self.walk_expr(ta, *idx)?,
                        TypedChainPart::Call { args, .. }
                        | TypedChainPart::MethodCall { args, .. } => {
                            for &arg in args {
                                self.walk_expr(ta, arg)?;
                            }
                        }
                        TypedChainPart::Field { .. }
                        | TypedChainPart::InterfaceProperty { .. }
                        | TypedChainPart::NonNull { .. } => {}
                    }
                }
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                PostfixTarget::Field { receiver, .. } => self.walk_expr(ta, *receiver)?,
                PostfixTarget::Index {
                    receiver, index, ..
                } => {
                    self.walk_expr(ta, *receiver)?;
                    self.walk_expr(ta, *index)?;
                }
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {}
            },
            TypedExprKind::Cast { value, .. } => self.walk_expr(ta, *value)?,
            TypedExprKind::Number(_)
            | TypedExprKind::BigInt(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::This
            | TypedExprKind::Regex { .. }
            | TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::GlobalRef { .. }
            | TypedExprKind::FunctionRef { .. }
            | TypedExprKind::NumberEnumMember { .. }
            | TypedExprKind::StringEnumMember { .. } => {}
        };
        Ok(())
    }

    fn note_stmt_pre(
        &mut self,
        ta: &TypedAst,
        id: StmtId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let stmt = ta.try_stmt(id).map_err(crate::codegen::arena_failure)?;
        let _: () = match &stmt.kind {
            TypedStmtKind::Let { ty, .. } | TypedStmtKind::Const { ty, .. } => {
                self.visit_type_at(ty, stmt.span)?;
            }
            TypedStmtKind::ForOf { element_ty, .. } => {
                self.visit_type_at(element_ty, stmt.span)?;
            }
            TypedStmtKind::Switch {
                discriminant_ty,
                cases,
                ..
            } => {
                self.visit_type_at(discriminant_ty, stmt.span)?;
                for case in cases {
                    for value in &case.values {
                        if let TypedSwitchValue::Enum { enum_name, .. } = value {
                            self.dependency_usage.note_type(enum_name.clone());
                        }
                    }
                }
            }
            TypedStmtKind::AssignLocal { target_ty, .. } => {
                self.visit_type_at(target_ty, stmt.span)?;
            }
            TypedStmtKind::AssignGlobal {
                mangled, target_ty, ..
            } => {
                self.dependency_usage.note_value(mangled.clone());
                self.visit_type_at(target_ty, stmt.span)?;
            }
            TypedStmtKind::AssignField { receiver, name, .. } => {
                self.extra_field_names.push(name.name.clone());
                self.note_shaped_property_access(
                    ta.source_type(*receiver)
                        .map_err(crate::codegen::arena_failure)?,
                    &name.name,
                    AccessorKind::Set,
                );
            }
            TypedStmtKind::AssignIndex {
                receiver, elem_ty, ..
            } => {
                if ta
                    .source_type(*receiver)
                    .map_err(crate::codegen::arena_failure)?
                    .is_structural_object()
                {
                    self.note_record_helpers();
                }
                self.visit_type_at(elem_ty, stmt.span)?;
                self.note_index_check();
            }
            TypedStmtKind::NarrowRegion { cast_info, .. } => {
                self.visit_type_at(&cast_info.from_ty, stmt.span)?;
                self.visit_type_at(&cast_info.to_ty, stmt.span)?;
            }
            TypedStmtKind::Try { catches, .. } => {
                // A cross-package subclass used only in a catch annotation
                // still needs its class reconstructed.
                for clause in catches {
                    self.visit_type_at(&clause.ty, stmt.span)?;
                }
            }
            TypedStmtKind::If { .. }
            | TypedStmtKind::While { .. }
            | TypedStmtKind::For { .. }
            | TypedStmtKind::DoWhile { .. }
            | TypedStmtKind::Break
            | TypedStmtKind::Continue
            | TypedStmtKind::ReboxLocal { .. }
            | TypedStmtKind::Return(_)
            | TypedStmtKind::Expr(_)
            | TypedStmtKind::Block(_)
            | TypedStmtKind::Throw { .. } => {}
        };
        Ok(())
    }

    fn note_stmt_post(
        &mut self,
        ta: &TypedAst,
        id: StmtId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let _: () = if let TypedStmtKind::Switch { cases, .. } =
            &ta.try_stmt(id).map_err(crate::codegen::arena_failure)?.kind
        {
            for case in cases {
                for value in &case.values {
                    match value {
                        TypedSwitchValue::String { value, .. } => {
                            self.string_pool.intern_text(value);
                        }
                        TypedSwitchValue::Enum {
                            value: crate::EnumVariantPayload::String(text),
                            ..
                        } => {
                            self.string_pool.intern_text(text);
                        }
                        _ => {}
                    }
                }
            }
        };
        Ok(())
    }

    fn note_expr_pre(
        &mut self,
        ta: &TypedAst,
        id: ExprId,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let expr = ta.try_expr(id).map_err(crate::codegen::arena_failure)?;
        if matches!(
            expr.kind,
            TypedExprKind::IndexAccess { .. }
                | TypedExprKind::Binary {
                    op: crate::BinOp::In,
                    ..
                }
        ) {
            self.note_record_helpers();
        }
        if matches!(expr.kind, TypedExprKind::PostfixUnary { .. }) {
            self.note_record_helpers();
        }
        self.visit_type(&expr.ty)
            .map_err(|error| error.with_span(expr.span))?;
        let _: () = match &expr.kind {
            TypedExprKind::String(text) => {
                self.string_pool.record_expr(id, text);
            }
            TypedExprKind::StringEnumMember {
                enum_mangled,
                value,
                ..
            } => {
                self.string_pool.record_expr(id, value);
                self.dependency_usage.note_type(enum_mangled.clone());
            }
            TypedExprKind::BigInt(digits) => {
                self.bigint_pool.intern_digits(digits)?;
                self.dependency_usage
                    .collect_bigint_host_value("fromNumber");
            }
            TypedExprKind::GlobalRef { mangled, .. } => {
                self.dependency_usage.note_value(mangled.clone());
            }
            TypedExprKind::FunctionRef { name, mangled } => {
                self.dependency_usage.note_value(mangled.clone());
                if self.adapter_seen.insert(mangled.clone()) {
                    self.adapter_metas.push(AdapterMeta {
                        name: name.name.clone(),
                        mangled: mangled.clone(),
                        signature: expr.ty.clone(),
                    });
                }
            }
            TypedExprKind::NumberEnumMember { enum_mangled, .. } => {
                self.dependency_usage.note_type(enum_mangled.clone());
            }
            TypedExprKind::Binary { op, lhs, .. } => {
                if let Some(name) = op.bitwise_name() {
                    self.dependency_usage
                        .note_value(crate::mangle::prelude(&format!("__value_{name}")));
                }
                if matches!(op, BinOp::Pow) && matches!(expr.ty.peel(), Type::Number) {
                    self.dependency_usage
                        .note_value(crate::runtime::prelude::math::math_key("pow"));
                }
                if matches!(expr.ty.peel(), Type::BigInt) {
                    match op {
                        BinOp::Add => self.dependency_usage.collect_bigint_host_value("add"),
                        BinOp::Sub => self.dependency_usage.collect_bigint_host_value("sub"),
                        BinOp::Mul => self.dependency_usage.collect_bigint_host_value("mul"),
                        BinOp::Div => self.dependency_usage.collect_bigint_host_value("div"),
                        BinOp::Rem => self.dependency_usage.collect_bigint_host_value("mod"),
                        BinOp::Pow => self.dependency_usage.collect_bigint_host_value("pow"),
                        BinOp::Eq
                        | BinOp::BitAnd
                        | BinOp::BitOr
                        | BinOp::BitXor
                        | BinOp::Shl
                        | BinOp::Shr
                        | BinOp::UnsignedShr
                        | BinOp::NotEq
                        | BinOp::Lt
                        | BinOp::Gt
                        | BinOp::Le
                        | BinOp::Ge
                        | BinOp::And
                        | BinOp::Or
                        | BinOp::In
                        | BinOp::NullishCoalesce => {}
                    }
                }
                if matches!(
                    ta.try_expr(*lhs)
                        .map_err(crate::codegen::arena_failure)?
                        .ty
                        .peel(),
                    Type::BigInt
                ) && matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
                ) {
                    self.dependency_usage.collect_bigint_host_value("cmp");
                }
                if matches!(op, BinOp::In)
                    && let TypedExprKind::String(name) = &ta
                        .try_expr(*lhs)
                        .map_err(crate::codegen::arena_failure)?
                        .kind
                {
                    self.extra_field_names.push(name.clone());
                    self.extra_field_names
                        .push(crate::codegen::classes::accessor_getter_name(name));
                    self.extra_field_names
                        .push(crate::codegen::classes::accessor_setter_name(name));
                    for kind in [AccessorKind::Get, AccessorKind::Set] {
                        self.mentioned_closure_sigs
                            .push(crate::codegen::classes::accessor_closure_sig(kind));
                    }
                }
            }
            TypedExprKind::Unary { op, operand } => {
                if matches!(op, crate::UnOp::BitNot) {
                    self.dependency_usage
                        .note_value(crate::mangle::prelude("__value_bitnot"));
                }
                let operand_ty = ta
                    .try_expr(*operand)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .peel();
                if matches!(op, crate::UnOp::Neg) && matches!(operand_ty, Type::BigInt) {
                    self.dependency_usage.collect_bigint_host_value("neg");
                }
                if matches!(op, crate::UnOp::Pos) && operand_ty.is_string_shaped() {
                    self.dependency_usage.collect_number_coercion_host_value();
                }
            }
            TypedExprKind::Call { mangled, .. } | TypedExprKind::GenericCall { mangled, .. } => {
                self.dependency_usage.note_value(mangled.clone());
            }
            TypedExprKind::McpCall { server, tool, .. } => {
                self.dependency_usage
                    .note_value(crate::mangle::host(&format!("@mcp/{server}"), tool));
                self.dependency_usage
                    .note_value(crate::mangle::host(crate::runtime::MCP_MODULE_NAME, "call"));
                if !matches!(expr.ty.peel(), Type::String) {
                    self.dependency_usage.collect_json_host_values();
                }
            }
            TypedExprKind::IntrinsicCall { kind, .. } => match kind {
                Intrinsic::JsonParse | Intrinsic::JsonStringify => {
                    self.dependency_usage.collect_json_host_values();
                }
                Intrinsic::BigIntFromString => {
                    self.dependency_usage
                        .collect_bigint_host_value("fromString");
                }
                Intrinsic::Assert => {}
            },
            TypedExprKind::MethodCall {
                receiver,
                iface,
                name,
                args,
                ..
            } => {
                self.note_method_call(
                    ta.source_type(*receiver)
                        .map_err(crate::codegen::arena_failure)?,
                    iface,
                    name,
                    args.len(),
                    &expr.ty,
                )?;
            }
            TypedExprKind::GenericMethodCall {
                receiver,
                iface,
                name,
                args,
                ..
            } => {
                self.note_method_call(
                    ta.source_type(*receiver)
                        .map_err(crate::codegen::arena_failure)?,
                    iface,
                    name,
                    args.len(),
                    &expr.ty,
                )?;
            }
            TypedExprKind::InterfacePropertyAccess { iface, name, .. } => {
                self.dependency_usage
                    .note_member(crate::mangle::extend(iface, &name.name));
            }
            TypedExprKind::ObjectLiteral { fields, members } => {
                if members
                    .iter()
                    .any(|m| matches!(m, crate::TypedObjectMember::Computed { .. }))
                {
                    self.note_record_helpers();
                }
                if members.iter().any(|member| {
                    matches!(
                        member,
                        crate::TypedObjectMember::Spread { .. }
                            | crate::TypedObjectMember::Computed { .. }
                    )
                }) {
                    self.dependency_usage.note_member(crate::mangle::extend(
                        &crate::mangle::prelude("ObjectConstructor"),
                        "#spread",
                    ));
                }
                for field in fields {
                    let mut source = Some(&field.source);
                    while let Some(TypedObjectFieldSource::Spread {
                        source_ty,
                        field_name,
                        fallback,
                        ..
                    }) = source
                    {
                        self.visit_type(source_ty)?;
                        // A spread read by name looks its field up by name and tests
                        // the value against the field's type, which reads names too.
                        self.extra_field_names.push(field_name.clone());
                        self.note_shape_member_names(source_ty);
                        source = fallback.as_deref();
                    }
                }
            }
            TypedExprKind::ArrayLiteral { element_ty, .. } => {
                self.visit_type(element_ty)?;
            }
            TypedExprKind::TupleLiteral { element_types, .. } => {
                for ty in element_types {
                    self.visit_type(ty)?;
                }
            }
            TypedExprKind::FieldAccess { receiver, name } => {
                self.extra_field_names.push(name.name.clone());
                self.note_shaped_property_access(
                    ta.source_type(*receiver)
                        .map_err(crate::codegen::arena_failure)?,
                    &name.name,
                    AccessorKind::Get,
                );
            }
            TypedExprKind::IndexAccess { .. } => {
                self.note_index_check();
            }
            TypedExprKind::Closure {
                runtime_generics,
                params,
                captured,
                body,
                return_type,
                ..
            } => {
                self.closure_metas.push(ClosureMeta {
                    this_type: ta.closure_this.get(&id).cloned(),
                    self_name: ta.closure_names.get(&id).cloned(),
                    runtime_generics: runtime_generics.clone(),
                    expr_id: id,
                    signature: expr.ty.clone(),
                    captured: captured.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    return_type: return_type.clone(),
                });
                for p in params {
                    self.visit_type(&p.ty)?;
                }
                self.visit_type(return_type)?;
            }
            TypedExprKind::Narrowed { cast_info, .. } => {
                self.visit_type(&cast_info.from_ty)?;
                self.visit_type(&cast_info.to_ty)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.note_chain_parts(ta, id, *base, parts)?;
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                PostfixTarget::Local { .. } => {
                    self.note_postfix_target(&expr.ty)?;
                }
                PostfixTarget::Global { mangled, .. } => {
                    self.dependency_usage.note_value(mangled.clone());
                    self.note_postfix_target(&expr.ty)?;
                }
                PostfixTarget::Field { receiver, name, .. } => {
                    self.note_postfix_target(&expr.ty)?;
                    let receiver_ty = ta
                        .source_type(*receiver)
                        .map_err(crate::codegen::arena_failure)?;
                    self.note_shaped_property_access(receiver_ty, &name.name, AccessorKind::Get);
                    self.note_shaped_property_access(receiver_ty, &name.name, AccessorKind::Set);
                }
                PostfixTarget::Index { elem_ty, .. } => {
                    self.note_postfix_target(elem_ty)?;
                    self.note_index_check();
                }
            },
            TypedExprKind::NonNullAssert { .. } => {
                self.string_pool.intern_text(throw::NON_NULL_ASSERT_MESSAGE);
            }
            TypedExprKind::Cast {
                target_ty, check, ..
            } => {
                self.string_pool
                    .intern_text(&cast_check::error_prefix(target_ty));
                for tag in cast_check::TYPE_TAG_STRINGS {
                    self.string_pool.intern_text(tag);
                }
                self.visit_type(target_ty)?;
                if let Some(check) = check {
                    self.visit_type(check)?;
                    self.note_shape_member_names(check);
                }
            }
            TypedExprKind::Regex { source, flags } => {
                self.string_pool.intern_text(source);
                self.string_pool.intern_text(flags);
            }
            TypedExprKind::CallClosure { callee, .. } => {
                self.dependency_usage
                    .note_value(crate::mangle::prelude("__value_invoke_defaults"));
                self.string_pool.intern_text(&cast_check::error_prefix(
                    ta.source_type(*callee)
                        .map_err(crate::codegen::arena_failure)?,
                ));
                for tag in cast_check::TYPE_TAG_STRINGS {
                    self.string_pool.intern_text(tag);
                }
            }
            TypedExprKind::InstanceOf { class, .. } => {
                // `ref.test (ref $Foo)` needs the class's type (and its rec group, for an
                // imported class) pulled into the dependency set; `expr.ty` is just `boolean`.
                self.visit_type(class)?;
            }
            TypedExprKind::Number(_)
            | TypedExprKind::Boolean(_)
            | TypedExprKind::Null
            | TypedExprKind::This
            | TypedExprKind::EffectThen { .. }
            | TypedExprKind::Sequence { .. }
            | TypedExprKind::LocalRef { .. }
            | TypedExprKind::LocalNarrowRef { .. }
            | TypedExprKind::SuperCtorCall { .. }
            | TypedExprKind::SuperMethodCall { .. }
            | TypedExprKind::TypeofTag { .. }
            | TypedExprKind::Ternary { .. }
            | TypedExprKind::NullishCoalesce { .. } => {}
        };
        Ok(())
    }

    /// A method call notes its member for imports and, when it dispatches
    /// through an object shape, the sig that dispatch builds.
    fn note_method_call(
        &mut self,
        receiver_ty: &Type,
        iface: &crate::MangledName,
        name: &crate::Ident,
        arity: usize,
        ret: &Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.dependency_usage
            .note_member(crate::mangle::extend(iface, &name.name));
        self.string_pool.intern_text("Value is not callable");
        self.string_pool
            .intern_text("Unbound function has no this receiver");
        self.extra_field_names.push(name.name.clone());
        self.note_shape_dispatch(receiver_ty, arity, ret)?;

        Ok(())
    }

    /// A method reached through an `$ObjectShape` receiver dispatches as a
    /// closure field, and codegen builds that closure's sig from the *call
    /// site* — arity plus return type — rather than from any type in scope
    /// (`emit_interface_method_via_shape_with_receiver_on_stack`). Nothing else
    /// records it: `walk_type` on an `InterfaceRef` visits type arguments only,
    /// and a `void` return contributes no type at all.
    ///
    /// `InterfaceRef` only, unlike [`Self::note_shaped_property_access`]: a method on
    /// an anonymous object type is a function-typed *field*, so it lowers to a
    /// property read followed by a closure call, never to method dispatch.
    fn note_shape_dispatch(
        &mut self,
        receiver_ty: &Type,
        arity: usize,
        ret: &Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.string_pool.intern_text("Value is not callable");
        self.string_pool
            .intern_text("Unbound function has no this receiver");
        // An optional chain dispatches on the non-null half of its receiver.
        let receiver_ty = crate::typechecker::infer::narrowing::strip_null(receiver_ty);
        if !matches!(receiver_ty.peel(), Type::InterfaceRef { .. }) {
            return Ok(());
        }
        self.mentioned_closure_sigs
            .push(ClosureSig::of(arity, ret)?);

        Ok(())
    }

    /// Each step's receiver is the previous step's result, so a chain has to be
    /// read in order to know what any one step dispatches on.
    fn note_chain_parts(
        &mut self,
        ta: &TypedAst,
        id: ExprId,
        base: ExprId,
        parts: &[TypedChainPart],
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let source_types = ta.runtime_chain_types.get(&id);
        let mut receiver_ty = ta
            .source_type(base)
            .map_err(crate::codegen::arena_failure)?;
        for (index, part) in parts.iter().enumerate() {
            if let TypedChainPart::MethodCall { iface, name, .. }
            | TypedChainPart::InterfaceProperty { iface, name, .. } = part
            {
                self.string_pool.intern_text(iface.as_str());
                self.string_pool.intern_text(&name.name);
            }
            match part {
                TypedChainPart::Field {
                    name, result_ty, ..
                } => {
                    self.extra_field_names.push(name.name.clone());
                    self.visit_type(result_ty)?;
                    self.note_shaped_property_access(receiver_ty, &name.name, AccessorKind::Get);
                }
                TypedChainPart::InterfaceProperty {
                    iface,
                    name,
                    result_ty,
                    span,
                    ..
                } => {
                    if ta.authored_call_arguments(*span).is_some()
                        && !iface.as_str().starts_with("submilli:")
                    {
                        for helper in ["__value_member", "__value_invoke"] {
                            self.dependency_usage
                                .note_value(crate::mangle::prelude(helper));
                        }
                    }
                    self.extra_field_names.push(name.name.clone());
                    self.dependency_usage
                        .note_member(crate::mangle::extend(iface, &name.name));
                    self.visit_type(result_ty)?;
                }
                TypedChainPart::Index { result_ty, .. } => {
                    self.note_record_helpers();
                    self.visit_type(result_ty)?;
                    // A chain index is bounds-checked like any other, so it
                    // needs the same message in the pool.
                    self.note_index_check();
                }
                TypedChainPart::Call { result_ty, .. } => {
                    self.dependency_usage
                        .note_value(crate::mangle::prelude("__value_invoke_defaults"));
                    self.visit_type(result_ty)?;
                }
                TypedChainPart::NonNull { result_ty, .. } => {
                    self.visit_type(result_ty)?;
                    self.string_pool.intern_text(throw::NON_NULL_ASSERT_MESSAGE);
                }
                TypedChainPart::MethodCall {
                    iface,
                    name,
                    args,
                    result_ty,
                    span,
                    ..
                } => {
                    if ta.authored_call_arguments(*span).is_some()
                        && !iface.as_str().starts_with("submilli:")
                    {
                        for helper in ["__value_member", "__value_invoke"] {
                            self.dependency_usage
                                .note_value(crate::mangle::prelude(helper));
                        }
                    }
                    self.extra_field_names.push(name.name.clone());
                    self.dependency_usage
                        .note_member(crate::mangle::extend(iface, &name.name));
                    self.visit_type(result_ty)?;
                    self.note_shape_dispatch(receiver_ty, args.len(), result_ty)?;
                }
            }
            receiver_ty = source_types.map_or_else(|| part.result_ty(), |types| &types[index + 1]);
        }
        Ok(())
    }

    /// A property access on an `$ObjectShape` receiver emits every branch it
    /// might take: the payload-slot read (or write), a dispatch to the synthetic
    /// `get <prop>` / `set <prop>` method for the accessor case, and — for a
    /// write — the read-only-property message the getter-backed branch throws.
    /// The accessor branch needs its closure sig even when the program declares
    /// no accessor at all, since the branch is emitted on the field-name scan
    /// alone.
    ///
    /// The `get <prop>` / `set <prop>` `$string` globals are interned off the
    /// *access*, not off a declaration: whether a value is accessor-backed is a
    /// whole-program fact, and a library that declares an interface and reads it
    /// never sees the consumer's accessor implementation.
    fn note_shaped_property_access(&mut self, receiver_ty: &Type, prop: &str, kind: AccessorKind) {
        if matches!(kind, AccessorKind::Set) {
            self.dependency_usage.note_member(crate::mangle::extend(
                &crate::mangle::prelude("ObjectConstructor"),
                "#insertField",
            ));
        }
        self.mentioned_closure_sigs
            .push(super::field_guards::signature());
        if !is_shaped_receiver(receiver_ty) && !matches!(receiver_ty.peel(), Type::ClassRef { .. })
        {
            return;
        }
        self.string_pool.intern_text("Value is not callable");
        self.string_pool
            .intern_text("Unbound function has no this receiver");
        // Both directions need the getter's name and sig: a write scans the
        // `get <prop>` slot to tell a read-only property from an absent one.
        self.extra_field_names
            .push(crate::codegen::classes::accessor_getter_name(prop));
        self.mentioned_closure_sigs
            .push(crate::codegen::classes::accessor_closure_sig(
                AccessorKind::Get,
            ));
        if matches!(kind, AccessorKind::Set) {
            self.extra_field_names
                .push(crate::codegen::classes::accessor_setter_name(prop));
            self.mentioned_closure_sigs
                .push(crate::codegen::classes::accessor_closure_sig(
                    AccessorKind::Set,
                ));
            // A write also emits the read-only-property throw, for the case
            // where the receiver turns out to be getter-backed.
            self.string_pool
                .intern_text(throw::READ_ONLY_PROPERTY_MESSAGE);
        }
    }

    fn note_index_check(&mut self) {
        self.string_pool.intern_text(bounds::INDEX_OOB_MESSAGE);
        for ty in [Type::Array(Box::new(Type::Unknown)), Type::Uint8Array] {
            self.string_pool.intern_text(&cast_check::error_prefix(&ty));
        }
        for tag in cast_check::TYPE_TAG_STRINGS {
            self.string_pool.intern_text(tag);
        }
    }

    fn note_postfix_target(
        &mut self,
        ty: &Type,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        self.dependency_usage.collect_postfix_bigint_ops(ty);
        self.visit_type(ty)?;

        Ok(())
    }
}

/// Whether a property access on this receiver lowers to the `$ObjectShape`
/// field-name scan, which is what carries the accessor branch. `infer_field_access`
/// admits a union of object shapes as well as a single one.
fn is_shaped_receiver(ty: &Type) -> bool {
    let ty = crate::typechecker::infer::narrowing::strip_null(ty);
    match ty.peel() {
        Type::InterfaceRef { .. } | Type::Object { .. } => true,
        Type::Union(members) => members
            .iter()
            .any(|m| matches!(m.peel(), Type::InterfaceRef { .. } | Type::Object { .. })),
        _ => false,
    }
}
