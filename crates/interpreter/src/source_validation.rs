//! Validates source metadata before inference consumes a caller-supplied AST.

use crate::{FileId, Span, ast::*, source::SourceError};

pub(crate) fn validate(ast: &Ast, source: &str, file: FileId) -> Result<(), SourceError> {
    SourceError::check_source_len(source.len())?;
    let mut validator = Validator {
        source,
        file,
        annotations: Vec::new(),
    };
    for export in &ast.exported_decls {
        validator.span(export.export_span)?;
    }
    for origin in ast.pattern_origins.values() {
        validator.span(origin.pattern_span)?;
    }
    for bindings in ast.for_of_pattern_bindings.values() {
        validator.idents(bindings)?;
    }
    for expr in ast.source_expressions() {
        validator.expression_metadata(expr)?;
        match &expr.kind {
            ExprKind::Call { type_args, .. } | ExprKind::New { type_args, .. } => {
                for annotation in type_args.iter().flatten() {
                    validator.annotation(annotation)?;
                }
            }
            ExprKind::FunctionExpression { this_type, .. } => validator.optional(this_type)?,
            ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                ..
            } => {
                validator.params(params)?;
                validator.optional(return_type)?;
                validator.predicate(type_predicate)?;
            }
            ExprKind::As { ty, .. } | ExprKind::InstanceOf { ty, .. } => {
                validator.annotation(ty)?;
            }
            ExprKind::OptionalChain { parts, .. } => {
                for part in parts {
                    validator.span(part.span())?;
                    if let ChainPart::Call { type_args, .. } = part {
                        for annotation in type_args.iter().flatten() {
                            validator.annotation(annotation)?;
                        }
                    }
                }
            }
            ExprKind::Assign { op_span, .. } => validator.span(*op_span)?,
            _ => {}
        }
    }
    for stmt in ast.source_statements() {
        validator.statement_metadata(stmt)?;
        match &stmt.kind {
            StmtKind::Let { ty, .. }
            | StmtKind::Const { ty, .. }
            | StmtKind::LetPattern { ty, .. }
            | StmtKind::ConstPattern { ty, .. }
            | StmtKind::ObjectRest { ty, .. }
            | StmtKind::ForOf { ty, .. }
            | StmtKind::ForOfPattern { ty, .. } => validator.optional(ty)?,
            StmtKind::Function {
                params,
                return_type,
                type_predicate,
                ..
            } => {
                validator.params(params)?;
                validator.optional(return_type)?;
                validator.predicate(type_predicate)?;
            }
            StmtKind::Try { catches, .. } => {
                for catch in catches {
                    validator.span(catch.span)?;
                    validator.optional(&catch.ty)?;
                }
            }
            StmtKind::InterfaceDecl {
                extends, members, ..
            } => {
                for annotation in extends {
                    validator.annotation(annotation)?;
                }
                for member in members {
                    validator.interface_member(member)?;
                }
            }
            StmtKind::ClassDecl {
                extends,
                implements,
                members,
                ..
            } => {
                validator.optional(extends)?;
                for annotation in implements {
                    validator.annotation(annotation)?;
                }
                for member in members {
                    validator.class_member(member)?;
                }
            }
            StmtKind::TypeAliasDecl { ty, .. } => validator.annotation(ty)?,
            StmtKind::CompoundAssign { op_span, .. }
            | StmtKind::CompoundAssignField { op_span, .. }
            | StmtKind::CompoundAssignIndex { op_span, .. } => validator.span(*op_span)?,
            _ => {}
        }
    }
    validator.validate_annotations()
}

struct Validator<'a> {
    source: &'a str,
    file: FileId,
    annotations: Vec<&'a TypeAnnotation>,
}

impl<'a> Validator<'a> {
    fn expression_metadata(&self, expr: &Expr) -> Result<(), SourceError> {
        self.span(expr.span)?;
        match &expr.kind {
            ExprKind::Identifier(name) | ExprKind::FieldAccess { name, .. } => {
                self.span(name.span)?;
            }
            ExprKind::ObjectLiteral { members } => {
                for member in members {
                    match member {
                        ObjectLiteralMember::Field(field) => self.span(field.name.span)?,
                        ObjectLiteralMember::Spread { span, .. } => self.span(*span)?,
                        ObjectLiteralMember::Computed { .. } => {}
                    }
                }
            }
            ExprKind::ArrayLiteral { elements } => {
                for element in elements {
                    if let ArrayLiteralElement::Spread { span, .. } = element {
                        self.span(*span)?;
                    }
                }
            }
            ExprKind::FunctionExpression {
                name: Some(name), ..
            } => self.span(name.span)?,
            ExprKind::OptionalChain { parts, .. } => {
                for part in parts {
                    if let ChainPart::Field { name, .. } = part {
                        self.span(name.span)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn statement_metadata(&self, stmt: &Stmt) -> Result<(), SourceError> {
        self.span(stmt.span)?;
        match &stmt.kind {
            StmtKind::Let { name, doc, .. } | StmtKind::Const { name, doc, .. } => {
                self.span(name.span)?;
                self.doc(doc)?;
            }
            StmtKind::ObjectRest {
                name, doc, exclude, ..
            } => {
                self.span(name.span)?;
                self.idents(exclude)?;
                self.doc(doc)?;
            }
            StmtKind::LetPattern { binding, doc, .. }
            | StmtKind::ConstPattern { binding, doc, .. } => {
                self.binding(binding)?;
                self.doc(doc)?;
            }
            StmtKind::ForOf { name, .. } => self.span(name.span)?,
            StmtKind::ForOfPattern { binding, .. } => self.binding(binding)?,
            StmtKind::Function {
                name,
                generics,
                doc,
                ..
            }
            | StmtKind::InterfaceDecl {
                name,
                generics,
                doc,
                ..
            }
            | StmtKind::ClassDecl {
                name,
                generics,
                doc,
                ..
            }
            | StmtKind::TypeAliasDecl {
                name,
                generics,
                doc,
                ..
            } => {
                self.span(name.span)?;
                self.idents(generics)?;
                self.doc(doc)?;
            }
            StmtKind::EnumDecl { name, members, doc } => {
                self.span(name.span)?;
                self.doc(doc)?;
                for member in members {
                    self.span(member.span)?;
                    self.span(member.name.span)?;
                    self.doc(&member.doc)?;
                    if let Some(value) = &member.value {
                        self.span(value.span())?;
                    }
                }
            }
            StmtKind::Assign { target, .. } | StmtKind::CompoundAssign { target, .. } => {
                self.span(target.span)?;
            }
            StmtKind::AssignField { field_name, .. }
            | StmtKind::CompoundAssignField { field_name, .. } => self.span(field_name.span)?,
            StmtKind::Switch { cases, default, .. } => {
                for case in cases {
                    self.span(case.span)?;
                }
                if let Some(default) = default {
                    self.span(default.span)?;
                }
            }
            StmtKind::Try { catches, .. } => {
                for catch in catches {
                    self.span(catch.binding.span)?;
                }
            }
            StmtKind::Import {
                module_span,
                kind,
                doc,
                ..
            } => {
                self.span(*module_span)?;
                self.doc(doc)?;
                match kind {
                    ImportKind::Named(specs) => self.specifiers(specs)?,
                    ImportKind::Namespace { local_name } => self.span(local_name.span)?,
                }
            }
            StmtKind::ExportFrom { specs, source, doc } => {
                self.specifiers(specs)?;
                self.doc(doc)?;
                if let Some((_, span)) = source {
                    self.span(*span)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn specifiers(&self, specs: &[ImportSpecifier]) -> Result<(), SourceError> {
        for spec in specs {
            self.span(spec.imported_name.span)?;
            self.span(spec.local_name.span)?;
        }
        Ok(())
    }

    fn idents(&self, names: &[Ident]) -> Result<(), SourceError> {
        for name in names {
            self.span(name.span)?;
        }
        Ok(())
    }

    fn modifiers(&self, modifiers: &ClassModifiers) -> Result<(), SourceError> {
        for span in [
            modifiers.visibility_span,
            modifiers.readonly,
            modifiers.static_span,
        ]
        .into_iter()
        .flatten()
        {
            self.span(span)?;
        }
        Ok(())
    }

    fn binding(&self, binding: &Binding) -> Result<(), SourceError> {
        self.span(binding.span())?;
        match binding {
            Binding::Object { fields, rest, .. } => {
                for field in fields {
                    self.span(field.span)?;
                    self.span(field.source.span)?;
                    self.span(field.local.span)?;
                }
                if let Some(rest) = rest {
                    self.span(rest.span)?;
                }
            }
            Binding::Array { elems, rest, .. } => {
                for name in elems.iter().flatten().chain(rest.iter()) {
                    self.span(name.span)?;
                }
            }
        }
        Ok(())
    }

    fn interface_metadata(&self, member: &InterfaceMember) -> Result<(), SourceError> {
        match member {
            InterfaceMember::IndexSignature(index) => self.span(index.span)?,
            InterfaceMember::Method {
                name,
                generics,
                doc,
                ..
            } => {
                self.span(name.span)?;
                self.idents(generics)?;
                self.doc(doc)?;
            }
            InterfaceMember::Property { name, doc, .. } => {
                self.span(name.span)?;
                self.doc(doc)?;
            }
        }
        Ok(())
    }

    fn class_metadata(&self, member: &ClassMember) -> Result<(), SourceError> {
        match member {
            ClassMember::Field {
                name,
                modifiers,
                doc,
                ..
            }
            | ClassMember::Accessor {
                name,
                modifiers,
                doc,
                ..
            } => {
                self.span(name.span)?;
                self.modifiers(modifiers)?;
                self.doc(doc)?;
            }
            ClassMember::Method {
                name,
                modifiers,
                generics,
                doc,
                ..
            } => {
                self.span(name.span)?;
                self.modifiers(modifiers)?;
                self.idents(generics)?;
                self.doc(doc)?;
            }
            ClassMember::Constructor { doc, .. } => self.doc(doc)?,
        }
        Ok(())
    }

    fn doc(&self, doc: &Option<crate::DocComment>) -> Result<(), SourceError> {
        let Some(doc) = doc else {
            return Ok(());
        };
        self.span(doc.span)?;
        for param in &doc.params {
            self.span(param.tag_span)?;
            self.span(param.name_span)?;
        }
        if let Some(returns) = &doc.returns {
            self.span(returns.tag_span)?;
        }
        for text in doc
            .throws
            .iter()
            .chain(doc.examples.iter())
            .chain(doc.deprecated.iter())
        {
            self.span(text.tag_span)?;
        }
        for tag in &doc.unknown_tags {
            self.span(tag.tag_span)?;
        }
        for capability in &doc.capabilities {
            self.span(capability.tag_span)?;
            self.span(capability.capability_span)?;
            for binding in &capability.bindings {
                self.span(binding.field_span)?;
                let span = match &binding.kind {
                    crate::DocCapabilityBindingKind::Parameter { span, .. }
                    | crate::DocCapabilityBindingKind::Type { span, .. }
                    | crate::DocCapabilityBindingKind::Literal { span, .. } => *span,
                };
                self.span(span)?;
            }
            for diagnostic in &capability.diagnostics {
                self.span(diagnostic.span)?;
            }
        }
        Ok(())
    }

    fn span(&self, span: Span) -> Result<(), SourceError> {
        span.text(self.source, self.file).map(|_| ())
    }

    fn name(&self, name: &Ident) -> Result<(), SourceError> {
        let text = name.span.text(self.source, self.file)?;
        if text != name.name {
            return Err(SourceError::InvalidSpan {
                span: name.span,
                reason: "type name does not match its source",
            });
        }
        Ok(())
    }

    fn annotation(&mut self, annotation: &'a TypeAnnotation) -> Result<(), SourceError> {
        self.annotations
            .try_reserve(1)
            .map_err(SourceError::Allocation)?;
        self.annotations.push(annotation);
        Ok(())
    }

    fn optional(&mut self, annotation: &'a Option<TypeAnnotation>) -> Result<(), SourceError> {
        if let Some(annotation) = annotation {
            self.annotation(annotation)?;
        }
        Ok(())
    }

    fn predicate(
        &mut self,
        predicate: &'a Option<TypePredicateAnnotation>,
    ) -> Result<(), SourceError> {
        if let Some(predicate) = predicate {
            self.span(predicate.span)?;
            self.span(predicate.param.span)?;
            self.annotation(&predicate.asserted)?;
        }
        Ok(())
    }

    fn params(&mut self, params: &'a [ParamDecl]) -> Result<(), SourceError> {
        for param in params {
            self.span(param.name.span)?;
            if let Some(binding) = &param.pattern {
                self.binding(binding)?;
            }
            if let Some(modifiers) = &param.modifiers {
                self.modifiers(modifiers)?;
            }
            self.optional(&param.ty)?;
        }
        Ok(())
    }

    fn interface_member(&mut self, member: &'a InterfaceMember) -> Result<(), SourceError> {
        self.interface_metadata(member)?;
        match member {
            InterfaceMember::IndexSignature(index) => self.annotation(&index.value)?,
            InterfaceMember::Method {
                params,
                return_type,
                span,
                ..
            } => {
                self.span(*span)?;
                self.params(params)?;
                self.annotation(return_type)?;
            }
            InterfaceMember::Property { ty, span, .. } => {
                self.span(*span)?;
                self.annotation(ty)?;
            }
        }
        Ok(())
    }

    fn class_member(&mut self, member: &'a ClassMember) -> Result<(), SourceError> {
        self.class_metadata(member)?;
        match member {
            ClassMember::Field { ty, span, .. } => {
                self.span(*span)?;
                self.annotation(ty)?;
            }
            ClassMember::Method {
                params,
                return_type,
                span,
                ..
            } => {
                self.span(*span)?;
                self.params(params)?;
                self.annotation(return_type)?;
            }
            ClassMember::Constructor { params, span, .. } => {
                self.span(*span)?;
                self.params(params)?;
            }
            ClassMember::Accessor {
                param,
                return_type,
                span,
                ..
            } => {
                self.span(*span)?;
                if let Some(param) = param {
                    self.params(std::slice::from_ref(param))?;
                }
                self.optional(return_type)?;
            }
        }
        Ok(())
    }

    fn validate_annotations(&mut self) -> Result<(), SourceError> {
        while let Some(annotation) = self.annotations.pop() {
            self.span(annotation.span)?;
            match &annotation.kind {
                TypeAnnotationKind::Name { name, args } => {
                    self.name(name)?;
                    for argument in args {
                        self.annotation(argument)?;
                    }
                }
                TypeAnnotationKind::Qualified { path, args } => {
                    self.path(path, 2, annotation.span)?;
                    for argument in args {
                        self.annotation(argument)?;
                    }
                }
                TypeAnnotationKind::TypeOf { path } => self.path(path, 1, annotation.span)?,
                TypeAnnotationKind::Array(inner)
                | TypeAnnotationKind::Readonly(inner)
                | TypeAnnotationKind::KeyOf(inner) => self.annotation(inner)?,
                TypeAnnotationKind::Tuple(members) | TypeAnnotationKind::Union(members) => {
                    for member in members {
                        self.annotation(member)?;
                    }
                }
                TypeAnnotationKind::Object { fields, index } => {
                    for field in fields {
                        self.span(field.name.span)?;
                        self.annotation(&field.ty)?;
                    }
                    if let Some(index) = index {
                        self.span(index.span)?;
                        self.annotation(&index.value)?;
                    }
                }
                TypeAnnotationKind::Function {
                    params,
                    return_type,
                } => {
                    for param in params {
                        self.span(param.name.span)?;
                        self.annotation(&param.ty)?;
                    }
                    self.annotation(return_type)?;
                }
                TypeAnnotationKind::StringLiteral(_)
                | TypeAnnotationKind::NumberLiteral(_)
                | TypeAnnotationKind::BooleanLiteral(_) => {}
            }
        }
        Ok(())
    }

    fn path(&self, path: &[Ident], minimum: usize, span: Span) -> Result<(), SourceError> {
        if path.len() < minimum {
            return Err(SourceError::InvalidSpan {
                span,
                reason: "type path is missing name segments",
            });
        }
        for name in path {
            self.name(name)?;
        }
        Ok(())
    }
}
