//! A callable the program declares has at most
//! [`MAX_CLOSURE_ARITY`](crate::compiler_limits::MAX_CLOSURE_ARITY) parameters,
//! the slots of the closure ABI.
//!
//! The rule reports where the callable is declared, so a package with one
//! fails its own compile and no script that uses it does. It needs no reach
//! analysis: substituting a function type changes its slot types, never their
//! count, and a rest parameter is one slot. A type argument can bring in a
//! function type, so the rule checks the types the program writes, type
//! arguments, bounds and `extends` clauses included. An `implements` clause's
//! arguments are not kept: a member that takes one restates it, and is
//! checked there. The rule checks only the program's own declarations; a
//! dependency's were checked when it was compiled. Codegen still checks every
//! signature it lowers, for declarations built by hand.
//!
//! A constructor is exempt: it has no closure slot of its own.

use std::collections::BTreeSet;

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::compiler_limits::{UnsupportedClosureArity, checked_closure_arity};
use crate::typechecker::rules::body_walk::{self, Visitor};
use crate::{
    Diagnostic, ExprId, GenericParameters, Severity, Span, Type, TypedAst, TypedClassAccessor,
    TypedClassDecl, TypedExprKind, TypedInterfaceDecl, TypedInterfaceMember, TypedParam, TypedStmt,
    TypedStmtKind, TypedTypeDecl,
};

pub(super) fn run(ta: &TypedAst, diagnostics: &mut Vec<Diagnostic>) -> Result<(), CompilerFailure> {
    let mut rule = Rule {
        ta,
        seen: BTreeSet::new(),
        reported: BTreeSet::new(),
        diagnostics,
    };
    // The places a type is written first, then the values that may hold a
    // copy of one: a local or global takes its initializer's type, a cast or
    // type argument may come from inference.
    rule.check_named_callables();
    body_walk::walk_program(ta, &mut Closures(&mut rule))?;
    rule.check_declaration_types();
    for global in &ta.globals {
        rule.check_held_type(&global.ty, global.name.span);
    }
    body_walk::walk_program(ta, &mut BodyTypes(&mut rule))
}

/// A function type's parameter types and return type.
type Signature = (Vec<Type>, Type);

struct Rule<'a> {
    ta: &'a TypedAst,
    /// Every signature of too many slots found so far.
    seen: BTreeSet<Signature>,
    /// Each place reported, with the signature reported there.
    reported: BTreeSet<(Span, Signature)>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Rule<'_> {
    /// The parameter lists of functions and methods.
    fn check_named_callables(&mut self) {
        let ta = self.ta;
        for function in &ta.functions {
            self.check_callable(&function.params, &function.return_type, function.name.span);
        }
        for declaration in &ta.types {
            match declaration {
                TypedTypeDecl::Interface(interface) => {
                    for member in &interface.members {
                        if let TypedInterfaceMember::Method {
                            name,
                            params,
                            return_type,
                            ..
                        } = member
                        {
                            self.check_callable(params, return_type, name.span);
                        }
                    }
                }
                TypedTypeDecl::Class(class) => {
                    for method in &class.methods {
                        self.check_callable(&method.params, &method.return_type, method.name.span);
                    }
                }
                TypedTypeDecl::NumberEnum(_)
                | TypedTypeDecl::StringEnum(_)
                | TypedTypeDecl::Alias(_) => {}
            }
        }
    }

    /// Every callable is reported, even one whose signature another shares.
    fn check_callable(&mut self, params: &[TypedParam], ret: &Type, span: Span) {
        if let Err(error) = checked_closure_arity(params.len()) {
            let signature: Signature = (params.iter().map(|p| p.ty.clone()).collect(), ret.clone());
            self.seen.insert(signature.clone());
            if self.reported.insert((span, signature)) {
                self.report(error, span);
            }
        }
    }

    /// The types written in declarations, past their parameter lists.
    fn check_declaration_types(&mut self) {
        let ta = self.ta;
        for declaration in &ta.types {
            match declaration {
                TypedTypeDecl::Interface(interface) => self.check_interface_types(interface),
                TypedTypeDecl::Class(class) => self.check_class_types(class),
                TypedTypeDecl::Alias(alias) => {
                    self.check_generics(&alias.generics, alias.name.span);
                    // Past the alias that names this declaration, to its body.
                    self.check_type(alias.ty.peel(), alias.name.span);
                }
                TypedTypeDecl::NumberEnum(_) | TypedTypeDecl::StringEnum(_) => {}
            }
        }
        for function in &ta.functions {
            let span = function.name.span;
            self.check_generics(&function.generics, span);
            self.check_parameter_types(&function.params);
            self.check_type(&function.return_type, span);
            if let Some(predicate) = &function.type_predicate {
                self.check_type(&predicate.asserted_type, span);
            }
        }
    }

    fn check_interface_types(&mut self, interface: &TypedInterfaceDecl) {
        let span = interface.name.span;
        self.check_generics(&interface.generics, span);
        for base in &interface.extends {
            self.check_type(base, span);
        }
        for member in &interface.members {
            match member {
                TypedInterfaceMember::Method {
                    name,
                    generics,
                    params,
                    return_type,
                    ..
                } => {
                    self.check_generics(generics, name.span);
                    self.check_parameter_types(params);
                    self.check_type(return_type, name.span);
                }
                TypedInterfaceMember::Property { name, ty, .. } => {
                    self.check_type(ty, name.span);
                }
            }
        }
        for index in crate::stored_index_signatures(&interface.index, &interface.more_indexes) {
            self.check_type(&index.value, span);
        }
    }

    fn check_class_types(&mut self, class: &TypedClassDecl) {
        let span = class.name.span;
        self.check_generics(&class.generics, span);
        for arg in class.extends.iter().flat_map(|parent| &parent.args) {
            self.check_type(arg, span);
        }
        if let Some(constructor) = &class.constructor {
            self.check_parameter_types(&constructor.params);
        }
        for field in &class.fields {
            self.check_type(&field.ty, field.name.span);
        }
        // A static field keeps no span of its own.
        for field in class.static_fields.values() {
            self.check_type(&field.ty, span);
        }
        for method in &class.methods {
            self.check_generics(&method.generics, method.name.span);
            self.check_parameter_types(&method.params);
            self.check_type(&method.return_type, method.name.span);
        }
        for accessor in &class.accessors {
            match accessor {
                TypedClassAccessor::Getter { name, ret_ty, .. } => {
                    self.check_type(ret_ty, name.span);
                }
                TypedClassAccessor::Setter { param, .. } => {
                    self.check_type(&param.ty, param.name.span);
                }
            }
        }
    }

    fn check_generics(&mut self, generics: &GenericParameters, span: Span) {
        for bound in generics.constraints.values() {
            self.check_type(bound, span);
        }
    }

    fn check_parameter_types(&mut self, params: &[TypedParam]) {
        for param in params {
            self.check_type(&param.ty, param.name.span);
        }
    }

    /// Reports each function type of too many slots that `root`, written at
    /// `span`, holds, once per place.
    fn check_type(&mut self, root: &Type, span: Span) {
        for (error, signature) in oversized_functions(root) {
            self.seen.insert(signature.clone());
            if self.reported.insert((span, signature)) {
                self.report(error, span);
            }
        }
    }

    /// Reports each function type of too many slots that `root` holds and
    /// no place has reported: the type of a value may repeat one written
    /// elsewhere.
    fn check_held_type(&mut self, root: &Type, span: Span) {
        for (error, signature) in oversized_functions(root) {
            if self.seen.insert(signature) {
                self.report(error, span);
            }
        }
    }

    fn report(&mut self, error: UnsupportedClosureArity, span: Span) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message: error.to_string(),
            help: vec!["group arguments into an object or a rest parameter".into()],
            notes: Vec::new(),
        });
    }
}

/// Checks each closure: its parameter list and the types it writes.
struct Closures<'r, 'a>(&'r mut Rule<'a>);

impl Visitor for Closures<'_, '_> {
    fn visit_expr(&mut self, id: ExprId, kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        let TypedExprKind::Closure {
            params,
            return_type,
            ..
        } = kind
        else {
            return Ok(());
        };
        let ta = self.0.ta;
        let span = closure_span(ta, id, params)?;
        self.0.check_callable(params, return_type, span);
        // Its own type, with its parameters, return and predicate.
        let ty = &ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?
            .ty;
        self.0.check_type(ty, span);
        Ok(())
    }
}

/// Checks the types of the values in bodies: of locals, casts and type
/// arguments.
struct BodyTypes<'r, 'a>(&'r mut Rule<'a>);

impl Visitor for BodyTypes<'_, '_> {
    fn visit_stmt(&mut self, stmt: &TypedStmt) -> Result<(), CompilerFailure> {
        let kind = &stmt.kind;
        if let TypedStmtKind::Let { name, ty, .. } | TypedStmtKind::Const { name, ty, .. } = kind {
            self.0.check_held_type(ty, name.span);
        }
        Ok(())
    }

    fn visit_expr(&mut self, id: ExprId, kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        let rule = &mut *self.0;
        let ta = rule.ta;
        match kind {
            TypedExprKind::Cast { target_ty, .. } => {
                let span = expr_span(ta, id)?;
                rule.check_held_type(target_ty, span);
            }
            TypedExprKind::GenericCall { type_args, .. }
            | TypedExprKind::GenericMethodCall { type_args, .. } => {
                let span = expr_span(ta, id)?;
                for ty in type_args {
                    rule.check_held_type(ty, span);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// The function types of too many slots in `root`. A named type is not
/// followed: its declaration is checked where it is declared.
fn oversized_functions(root: &Type) -> Vec<(UnsupportedClosureArity, Signature)> {
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::Alias { args, .. }
            | Type::InterfaceRef { args, .. }
            | Type::ClassRef { args, .. }
            | Type::AliasRef { args, .. } => pending.extend(args),
            Type::Function { params, ret, .. } => {
                if let Err(error) = checked_closure_arity(params.len()) {
                    found.push((error, (params.clone(), (**ret).clone())));
                }
                crate::type_size::for_each_child(ty, |child| pending.push(child));
            }
            _ => crate::type_size::for_each_child(ty, |child| pending.push(child)),
        }
    }
    found
}

/// A closure is reported over its parameter list.
fn closure_span(ta: &TypedAst, id: ExprId, params: &[TypedParam]) -> Result<Span, CompilerFailure> {
    let (Some(first), Some(last)) = (params.first(), params.last()) else {
        return expr_span(ta, id);
    };
    first
        .name
        .span
        .merge(last.name.span)
        .map_err(|error| error.into_compiler_failure(CompilerStage::Infer))
}

fn expr_span(ta: &TypedAst, id: ExprId) -> Result<Span, CompilerFailure> {
    Ok(ta
        .try_expr(id)
        .map_err(crate::typechecker::arena_failure)?
        .span)
}

#[cfg(test)]
mod tests {
    use crate::typechecker::rules::test_util::{infer_script, run_package};
    use crate::typechecker::rules::{check_package_submilli, check_script_submilli};

    fn parameters(arity: usize) -> String {
        (0..arity)
            .map(|i| format!("a{i}: number"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn limits(diagnostics: &[crate::Diagnostic]) -> usize {
        diagnostics
            .iter()
            .filter(|d| d.message.contains("parameter slots"))
            .count()
    }

    /// Another front end types the program and runs only the Submilli rules;
    /// they report the limit without Submilli's inference.
    #[test]
    fn submilli_rules_report_the_limit() {
        let source = format!(
            "interface I {{ f({}): number; }} function main(): void {{}}",
            parameters(256)
        );
        let (typed_ast, inference) = infer_script(&source);
        assert_eq!(limits(&inference), 0, "{inference:?}");
        assert_eq!(limits(&check_script_submilli(&typed_ast, &[]).unwrap()), 1);

        let source = format!("export type F = ({}) => void;", parameters(256));
        let (typed_ast, package, _) = run_package("arity", &[("lib", &source)], &[]);
        assert_eq!(
            limits(&check_package_submilli(&typed_ast, &package, &[]).unwrap()),
            1
        );
    }
}
