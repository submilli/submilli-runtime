//! Pre-infer pass lifting `interface`, `type` and `enum` declarations out of
//! blocks to module scope.
//!
//! TypeScript scopes a type declared in a block to that block: it is visible in
//! the whole block, shadows outer names, and may use the type parameters of the
//! enclosing function, method or class. The typechecker registers named types
//! for the whole module only, so this pass resolves local type names lexically,
//! once, and hands inference ordinary top-level declarations:
//!
//! - each local type gets a module-unique key, its source name followed by
//!   [`MARKER`] and the start of its declaring block, and every reference that
//!   resolves to it is renamed to the key;
//! - the outer type parameters it uses, directly or through other local types,
//!   are lifted into leading type parameters of its own, and every reference
//!   passes them explicitly (`Box` becomes `Box%12%1<T>`, the last segment
//!   counting the lifted parameters, which diagnostics leave out);
//! - the declaration moves from its block to [`Ast::top_level`].
//!
//! A local type is referenced only inside its block, so each block settles its
//! own declarations before its statements are walked: it collects them, notes
//! what each uses, settles the lifted parameters over the references among
//! them, and rewrites them.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::compiler_limits::MAX_LIFTED_TYPE_ARGUMENTS;
use crate::{
    ArrowBody, Ast, ChainPart, ClassMember, Diagnostic, ExprId, ExprKind, Ident, InterfaceMember,
    LoweredLocalTypes, ObjectLiteralMember, ParamDecl, Severity, Span, StmtId, StmtKind,
    SwitchCase, SwitchDefault, TypeAnnotation, TypeAnnotationKind,
};

/// Separates a local type's source name from the rest of its key. No
/// identifier, package or mangled name contains it.
const MARKER: char = '%';

/// The source name of a type: a local type's key without its suffix.
pub(crate) fn display_type_name(name: &str) -> &str {
    name.split_once(MARKER).map_or(name, |(source, _)| source)
}

/// The type arguments, or parameters, of the type named `name` that the source
/// writes: all but those lifted from a local type's enclosing scope.
pub(crate) fn written_type_args<'a, T>(name: &str, args: &'a [T]) -> &'a [T] {
    args.get(lifted_type_params(name)..).unwrap_or_default()
}

/// Whether `name` is the key of a type declared in a block.
pub(crate) fn is_local_type_name(name: &str) -> bool {
    name.contains(MARKER)
}

fn lifted_type_params(name: &str) -> usize {
    name.split(MARKER)
        .nth(2)
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

/// Lift the module's local type declarations. Returns the diagnostics for
/// local types that can't be lifted or clash with another name, and the scope
/// of each lifted type.
pub(crate) fn lower(ast: &mut Ast) -> Result<LoweredLocalTypes, CompilerFailure> {
    // The walk recurses as deep as the syntax, so a module without local types
    // skips it; finding them needs no recursion.
    if first_unlifted_type_decl(ast)?.is_none() {
        return Ok(LoweredLocalTypes::default());
    }
    // Pattern lowering wraps some bodies in blocks, so measure the height the
    // walk recurses through again.
    crate::tree_height::check_syntax(ast)
        .map_err(|failure| failure.with_stage(CompilerStage::Infer))?;
    let mut lifter = Lifter::default();
    for id in ast.top_level.clone() {
        lifter.walk_stmt(ast, id)?;
    }
    ast.top_level.extend(lifter.hoisted);
    let scopes = lifter
        .decls
        .iter()
        .map(|decl| (decl.key.clone(), decl.scope))
        .collect();
    Ok(LoweredLocalTypes {
        diagnostics: lifter.diagnostics,
        scopes,
    })
}

/// A type declaration outside the module's top level, if there is one: a type
/// declared in a block, before this pass lifts it.
pub(crate) fn first_unlifted_type_decl(ast: &Ast) -> Result<Option<Span>, CompilerFailure> {
    let top_level: HashSet<StmtId> = ast.top_level.iter().copied().collect();
    for id in ast.stmt_ids().map_err(arena_failure)? {
        let stmt = ast.try_stmt(id).map_err(arena_failure)?;
        if is_type_decl(&stmt.kind) && !top_level.contains(&id) {
            return Ok(Some(stmt.span));
        }
    }
    Ok(None)
}

fn is_type_decl(kind: &StmtKind) -> bool {
    matches!(
        kind,
        StmtKind::InterfaceDecl { .. } | StmtKind::TypeAliasDecl { .. } | StmtKind::EnumDecl { .. }
    )
}

fn arena_failure(error: crate::arena::ArenaError) -> CompilerFailure {
    error.into_compiler_failure(CompilerStage::Infer)
}

/// A local type declaration, as an index into `Lifter::decls`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DeclId(usize);

/// A type parameter, as an index into `Lifter::type_params`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TypeParamId(usize);

/// What a type name in scope refers to.
#[derive(Clone, Copy)]
enum TypeBinding {
    TypeParam(TypeParamId),
    Local(DeclId),
}

/// What a function declares in the scope its body block shares.
#[derive(Clone, Copy, Default)]
struct Signature<'a> {
    type_params: &'a [crate::TypeParameterAnnotation],
    params: &'a [String],
}

/// The names one scope declares, to take out of scope when it ends.
#[derive(Default)]
struct Frame {
    type_names: Vec<String>,
    value_names: Vec<String>,
    /// How many type parameters were in scope when it was entered: it declares
    /// those pushed after.
    type_params_len: usize,
}

struct LocalDecl {
    /// The module-unique name the declaration and its references take.
    key: String,
    /// The name of its first declaration.
    span: Span,
    /// The block, or a switch's arms from its first label, it is visible in.
    scope: Span,
    is_enum: bool,
    /// The outer type parameters it takes as leading parameters, outermost
    /// first, which is in id order.
    lifted: Vec<LiftedParam>,
}

struct LiftedParam {
    id: TypeParamId,
    /// Its name inside the declaration.
    name: String,
    /// The span of its declaration, whose text is its source name, for the
    /// lifted parameter and the arguments passed for it.
    span: Span,
}

/// The local declaration whose annotations are being walked.
struct InDecl {
    decl: DeclId,
    /// Frames from this index on are the declaration's own.
    first_frame: usize,
    /// What the declaration uses, while that is being collected, before its
    /// lifted parameters are known; `None` while it is rewritten.
    uses: Option<DeclUses>,
}

#[derive(Default)]
struct DeclUses {
    /// Outer type parameters it uses: those it names, then also those of the
    /// local types it references.
    type_params: BTreeSet<TypeParamId>,
    /// Local types it references.
    local_types: BTreeSet<DeclId>,
    /// The type parameter names it declares, its own and its methods', which a
    /// lifted parameter must not take.
    declared_type_params: HashSet<String>,
}

enum TypeName {
    TypeParam {
        id: TypeParamId,
        frame: usize,
    },
    Local(DeclId),
    /// Declared at module level, built in, or unresolved.
    NotLocal,
}

enum ValueName {
    LocalEnum(DeclId),
    LocalValue {
        frame: usize,
    },
    /// Declared at module level, built in, or unresolved.
    NotLocal,
}

#[derive(Default)]
struct Lifter {
    frames: Vec<Frame>,
    /// Each type name in scope, with the frames that declare it, innermost
    /// last.
    type_bindings: HashMap<String, Vec<(usize, TypeBinding)>>,
    /// Each value name in scope, with the frames that declare it, innermost
    /// last.
    value_bindings: HashMap<String, Vec<usize>>,
    /// While a static member is walked, the frame of its class's type
    /// parameters, which it can't use.
    hidden_frame: Option<usize>,
    decls: Vec<LocalDecl>,
    /// The declaration of each type parameter in scope. A frame's are released
    /// when it ends: what lifts them is referenced only inside it.
    type_params: Vec<crate::TypeParameterAnnotation>,
    current: Option<InDecl>,
    /// Lifted parameters added to declarations and references.
    lifted_added: usize,
    /// Lifted parameters spread along references while settling them. A
    /// spread parameter is one argument the reference it crosses will pass, so
    /// this stays within the limit whenever `lifted_added` does.
    lifted_spread: usize,
    hoisted: Vec<StmtId>,
    diagnostics: Vec<Diagnostic>,
}

impl Lifter {
    // ---- statements ----

    fn walk_stmt(&mut self, ast: &mut Ast, id: StmtId) -> Result<(), CompilerFailure> {
        let stmt = ast.try_stmt(id).map_err(arena_failure)?;
        let span = stmt.span;
        let mut kind = stmt.kind.clone();
        match &mut kind {
            StmtKind::Block(_) => return self.walk_block(ast, id),
            StmtKind::Switch {
                discriminant,
                cases,
                default,
            } => {
                self.walk_expr(ast, *discriminant)?;
                return self.walk_switch(ast, span, cases, default.as_ref());
            }
            StmtKind::Let { ty, value, .. }
            | StmtKind::Const { ty, value, .. }
            | StmtKind::LetPattern { ty, value, .. }
            | StmtKind::ConstPattern { ty, value, .. }
            | StmtKind::ObjectRest {
                ty, source: value, ..
            } => {
                self.walk_opt_type(ty)?;
                self.walk_expr(ast, *value)?;
            }
            StmtKind::Function {
                generics,
                params,
                return_type,
                type_predicate,
                body,
                ..
            } => {
                self.push_type_params(generics);
                self.walk_constraints(generics)?;
                let names = self.enter_params(ast, params)?;
                self.walk_opt_type(return_type)?;
                if let Some(predicate) = type_predicate {
                    self.walk_type(&mut predicate.asserted)?;
                }
                let signature = Signature {
                    type_params: generics,
                    params: &names,
                };
                self.walk_body(ast, *body, signature)?;
                self.pop();
                self.pop();
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.walk_expr(ast, *condition)?;
                self.walk_block(ast, *then_block)?;
                if let Some(else_block) = else_block {
                    self.walk_block(ast, *else_block)?;
                }
            }
            StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
                self.walk_expr(ast, *condition)?;
                self.walk_block(ast, *body)?;
            }
            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                let mut names = Vec::new();
                for &init in init.iter() {
                    if let StmtKind::Let { name, .. } | StmtKind::Const { name, .. } =
                        &ast.try_stmt(init).map_err(arena_failure)?.kind
                    {
                        names.push(name.name.clone());
                    }
                }
                self.push_values(names);
                for &init in init.iter() {
                    self.walk_stmt(ast, init)?;
                }
                if let Some(condition) = condition {
                    self.walk_expr(ast, *condition)?;
                }
                if let Some(update) = update {
                    self.walk_stmt(ast, *update)?;
                }
                self.walk_block(ast, *body)?;
                self.pop();
            }
            StmtKind::ForOf {
                name,
                ty,
                iter,
                body,
                ..
            } => {
                self.walk_expr(ast, *iter)?;
                self.push_values(vec![name.name.clone()]);
                self.walk_opt_type(ty)?;
                self.walk_block(ast, *body)?;
                self.pop();
            }
            StmtKind::ForOfPattern { ty, iter, body, .. } => {
                self.walk_expr(ast, *iter)?;
                self.walk_opt_type(ty)?;
                self.walk_block(ast, *body)?;
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.walk_expr(ast, *value)?;
                }
            }
            StmtKind::Throw { value } | StmtKind::Expr(value) => self.walk_expr(ast, *value)?,
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.walk_block(ast, *body)?;
                for catch in catches.iter_mut() {
                    self.push_values(vec![catch.binding.name.clone()]);
                    if let Some(ty) = &mut catch.ty {
                        self.walk_class_test_target(ty)?;
                    }
                    self.walk_block(ast, catch.body)?;
                    self.pop();
                }
                if let Some(finally) = finally {
                    self.walk_block(ast, *finally)?;
                }
            }
            StmtKind::Assign { target, value } | StmtKind::CompoundAssign { target, value, .. } => {
                // Writing a local enum is an error inference reports once the
                // target names it.
                self.rename_local_enum(target);
                self.walk_expr(ast, *value)?;
            }
            StmtKind::AssignField {
                receiver, value, ..
            }
            | StmtKind::CompoundAssignField {
                receiver, value, ..
            } => {
                self.walk_expr(ast, *receiver)?;
                self.walk_expr(ast, *value)?;
            }
            StmtKind::AssignIndex {
                receiver,
                index,
                value,
            }
            | StmtKind::CompoundAssignIndex {
                receiver,
                index,
                value,
                ..
            } => {
                self.walk_expr(ast, *receiver)?;
                self.walk_expr(ast, *index)?;
                self.walk_expr(ast, *value)?;
            }
            StmtKind::ClassDecl {
                generics,
                extends,
                implements,
                members,
                ..
            } => {
                let class_frame = self.push_type_params(generics);
                self.walk_constraints(generics)?;
                self.walk_opt_type(extends)?;
                for ty in implements.iter_mut() {
                    self.walk_type(ty)?;
                }
                for member in members.iter_mut() {
                    self.walk_class_member(ast, member, class_frame)?;
                }
                self.pop();
            }
            // A type declaration outside a block is a top-level one, which
            // can't see local names; a block's own are walked by the block.
            StmtKind::InterfaceDecl { .. }
            | StmtKind::TypeAliasDecl { .. }
            | StmtKind::EnumDecl { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Import { .. }
            | StmtKind::ExportFrom { .. } => {}
        }
        ast.try_stmt_mut(id).map_err(arena_failure)?.kind = kind;
        Ok(())
    }

    /// Walk a class member. A static one can't use the type parameters of its
    /// class, declared in `class_frame`.
    fn walk_class_member(
        &mut self,
        ast: &mut Ast,
        member: &mut ClassMember,
        class_frame: usize,
    ) -> Result<(), CompilerFailure> {
        let is_static = match member {
            ClassMember::Field { modifiers, .. }
            | ClassMember::Method { modifiers, .. }
            | ClassMember::Accessor { modifiers, .. } => modifiers.static_span.is_some(),
            ClassMember::Constructor { .. } => false,
        };
        // A class is declared at the top level, so it is walked inside a static
        // member only when parser recovery keeps a nested one; its members then
        // put back the outer member's hidden frame.
        let outer_hidden = self.hidden_frame;
        if is_static {
            self.hidden_frame = Some(class_frame);
        }
        match member {
            ClassMember::Field {
                ty, initializer, ..
            } => {
                self.walk_type(ty)?;
                if let Some(initializer) = initializer {
                    self.walk_expr(ast, *initializer)?;
                }
            }
            ClassMember::Method {
                generics,
                params,
                return_type,
                body,
                ..
            } => {
                self.push_type_params(generics);
                self.walk_constraints(generics)?;
                let names = self.enter_params(ast, params)?;
                self.walk_type(return_type)?;
                let signature = Signature {
                    type_params: generics,
                    params: &names,
                };
                self.walk_body(ast, *body, signature)?;
                self.pop();
                self.pop();
            }
            ClassMember::Constructor { params, body, .. } => {
                let names = self.enter_params(ast, params)?;
                let signature = Signature {
                    params: &names,
                    ..Signature::default()
                };
                self.walk_body(ast, *body, signature)?;
                self.pop();
            }
            ClassMember::Accessor {
                param,
                return_type,
                body,
                ..
            } => {
                let params = param
                    .as_deref_mut()
                    .map(std::slice::from_mut)
                    .unwrap_or_default();
                let names = self.enter_params(ast, params)?;
                self.walk_opt_type(return_type)?;
                let signature = Signature {
                    params: &names,
                    ..Signature::default()
                };
                self.walk_body(ast, *body, signature)?;
                self.pop();
            }
        }
        self.hidden_frame = outer_hidden;
        Ok(())
    }

    /// Walk a parameter list's annotations and defaults, with the parameters
    /// in scope. Returns the names it binds. The caller pops the scope this
    /// pushes.
    fn enter_params(
        &mut self,
        ast: &mut Ast,
        params: &mut [ParamDecl],
    ) -> Result<Vec<String>, CompilerFailure> {
        // A destructured parameter is lowered to a fresh name and statements,
        // kept beside the body, that bind the pattern's names from it.
        let mut names: Vec<String> = params.iter().map(|p| p.name.name.clone()).collect();
        let mut bindings = Vec::new();
        for param in params.iter() {
            for &binding in ast
                .parameter_bindings
                .get(&param.name.span)
                .into_iter()
                .flatten()
            {
                if let StmtKind::Let { name, .. }
                | StmtKind::Const { name, .. }
                | StmtKind::ObjectRest { name, .. } =
                    &ast.try_stmt(binding).map_err(arena_failure)?.kind
                {
                    names.push(name.name.clone());
                }
                bindings.push(binding);
            }
        }
        self.push_values(names.clone());
        for param in params.iter_mut() {
            self.walk_opt_type(&mut param.ty)?;
            if let Some(default) = param.default {
                self.walk_expr(ast, default)?;
            }
        }
        for binding in bindings {
            self.walk_stmt(ast, binding)?;
        }
        Ok(names)
    }

    // ---- scopes ----

    /// Walk a block in a scope of its own.
    fn walk_block(&mut self, ast: &mut Ast, id: StmtId) -> Result<(), CompilerFailure> {
        self.walk_body(ast, id, Signature::default())
    }

    /// Walk a function's body block, which shares its scope with what the
    /// function's `signature` declares.
    fn walk_body(
        &mut self,
        ast: &mut Ast,
        id: StmtId,
        signature: Signature<'_>,
    ) -> Result<(), CompilerFailure> {
        let stmt = ast.try_stmt(id).map_err(arena_failure)?;
        let scope = stmt.span;
        let StmtKind::Block(stmts) = &stmt.kind else {
            return self.walk_stmt(ast, id);
        };
        let stmts = stmts.clone();
        let decls = self.enter_scope(ast, scope, &stmts, signature)?;
        let kept = self.walk_statements(ast, &stmts)?;
        self.exit_scope(decls);
        ast.try_stmt_mut(id).map_err(arena_failure)?.kind = StmtKind::Block(kept);
        Ok(())
    }

    /// Walk a switch's arms in the one scope they share.
    fn walk_switch(
        &mut self,
        ast: &mut Ast,
        switch_span: Span,
        cases: &[SwitchCase],
        default: Option<&SwitchDefault>,
    ) -> Result<(), CompilerFailure> {
        // In `case 1: default:` both labels share one body.
        let mut arms: Vec<(StmtId, Vec<StmtId>)> = Vec::new();
        let mut seen = HashSet::new();
        for body in cases
            .iter()
            .map(|case| case.body)
            .chain(default.map(|d| d.body))
        {
            if !seen.insert(body) {
                continue;
            }
            // The parser makes every arm body a block.
            if let StmtKind::Block(stmts) = &ast.try_stmt(body).map_err(arena_failure)?.kind {
                arms.push((body, stmts.clone()));
            }
        }
        let all: Vec<StmtId> = arms.iter().flat_map(|(_, stmts)| stmts).copied().collect();
        let scope = arms_scope(switch_span, cases, default);
        let decls = self.enter_scope(ast, scope, &all, Signature::default())?;
        for case in cases {
            for &value in &case.values {
                self.walk_expr(ast, value)?;
            }
        }
        for (body, stmts) in arms {
            let kept = self.walk_statements(ast, &stmts)?;
            ast.try_stmt_mut(body).map_err(arena_failure)?.kind = StmtKind::Block(kept);
        }
        self.exit_scope(decls);
        Ok(())
    }

    /// Walk a scope's statements other than its type declarations, and return
    /// those statements: the declarations are hoisted.
    fn walk_statements(
        &mut self,
        ast: &mut Ast,
        stmts: &[StmtId],
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut kept = Vec::with_capacity(stmts.len());
        for &id in stmts {
            if is_type_decl(&ast.try_stmt(id).map_err(arena_failure)?.kind) {
                continue;
            }
            self.walk_stmt(ast, id)?;
            kept.push(id);
        }
        Ok(kept)
    }

    /// Enter the scope of a block's statements: declare its local types and
    /// values, then settle and rewrite the local types. Returns the type
    /// declarations, to hoist when the scope exits.
    fn enter_scope(
        &mut self,
        ast: &mut Ast,
        scope: Span,
        stmts: &[StmtId],
        signature: Signature<'_>,
    ) -> Result<Vec<StmtId>, CompilerFailure> {
        let mut types: HashMap<String, DeclId> = HashMap::new();
        let mut values: HashSet<String> = HashSet::new();
        let mut decl_stmts = Vec::new();
        let mut enums = Vec::new();
        for &id in stmts {
            match &ast.try_stmt(id).map_err(arena_failure)?.kind {
                StmtKind::InterfaceDecl { name, .. } | StmtKind::TypeAliasDecl { name, .. } => {
                    let decl = self.declare(&mut types, name, false, scope);
                    decl_stmts.push((id, decl));
                    self.check_type_param_clash(name, signature.type_params);
                }
                StmtKind::EnumDecl { name, .. } => {
                    let decl = self.declare(&mut types, name, true, scope);
                    decl_stmts.push((id, decl));
                    self.check_type_param_clash(name, signature.type_params);
                    enums.push(name.clone());
                }
                StmtKind::Let { name, .. }
                | StmtKind::Const { name, .. }
                | StmtKind::ObjectRest { name, .. }
                | StmtKind::Function { name, .. }
                | StmtKind::ClassDecl { name, .. } => {
                    values.insert(name.name.clone());
                }
                _ => {}
            }
        }
        self.check_enum_value_clash(&enums, &values, signature.params);
        self.push_frame(
            types
                .into_iter()
                .map(|(name, decl)| (name, TypeBinding::Local(decl)))
                .collect(),
            values.into_iter().collect(),
        );
        let mut uses = Vec::with_capacity(decl_stmts.len());
        for &(id, decl) in &decl_stmts {
            uses.push((decl, self.collect_uses(ast, id, decl)?));
        }
        self.settle_lifted_params(uses)?;
        for &(id, decl) in &decl_stmts {
            self.rewrite_decl(ast, id, decl)?;
        }
        Ok(decl_stmts.into_iter().map(|(id, _)| id).collect())
    }

    fn exit_scope(&mut self, decl_stmts: Vec<StmtId>) {
        self.pop();
        self.hoisted.extend(decl_stmts);
    }

    /// Declare a local type among a block's `types`. Declarations of one name
    /// in one block share a record, so they take one key and inference reports
    /// the duplicate.
    fn declare(
        &mut self,
        types: &mut HashMap<String, DeclId>,
        name: &Ident,
        is_enum: bool,
        scope: Span,
    ) -> DeclId {
        if let Some(&decl) = types.get(&name.name) {
            return decl;
        }
        let decl = DeclId(self.decls.len());
        self.decls.push(LocalDecl {
            key: format!("{}{MARKER}{}", name.name, scope.start),
            span: name.span,
            scope,
            is_enum,
            lifted: Vec::new(),
        });
        types.insert(name.name.clone(), decl);
        decl
    }

    /// An enum is also a value, so it can't share a name with a value its block
    /// declares or, in a function's body block, with a parameter.
    fn check_enum_value_clash(
        &mut self,
        enums: &[Ident],
        values: &HashSet<String>,
        params: &[String],
    ) {
        if enums.is_empty() {
            return;
        }
        let params: HashSet<&str> = params.iter().map(String::as_str).collect();
        for name in enums {
            if values.contains(&name.name) || params.contains(name.name.as_str()) {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: name.span,
                    message: format!("duplicate declaration of `{}`", name.name),
                    help: vec![
                        "an enum is also a value, so it can't share a name with a parameter of \
                         its function, or with a variable, function or class in the same block"
                            .to_string(),
                    ],
                    notes: Vec::new(),
                });
            }
        }
    }

    /// A function's type parameters and the types its body block declares
    /// share one scope, so one name can't be both.
    fn check_type_param_clash(
        &mut self,
        name: &Ident,
        fn_type_params: &[crate::TypeParameterAnnotation],
    ) {
        if let Some(param) = fn_type_params.iter().find(|p| p.name == name.name) {
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: name.span,
                message: format!("duplicate declaration of type `{}`", name.name),
                help: vec!["rename the type or the type parameter".to_string()],
                notes: vec![(param.span, "declared here as a type parameter".to_string())],
            });
        }
    }

    /// Enter a scope declaring type parameters `names`. Returns its frame.
    fn push_type_params(&mut self, names: &[crate::TypeParameterAnnotation]) -> usize {
        let first = self.type_params.len();
        let types = names
            .iter()
            .enumerate()
            .map(|(offset, name)| {
                let id = TypeParamId(first + offset);
                (name.name.clone(), TypeBinding::TypeParam(id))
            })
            .collect();
        let frame = self.push_frame(types, Vec::new());
        self.type_params.extend(names.iter().cloned());
        frame
    }

    fn push_values(&mut self, names: Vec<String>) {
        self.push_frame(Vec::new(), names);
    }

    /// Enter a scope declaring `types` and `values`. Returns its frame.
    fn push_frame(&mut self, types: Vec<(String, TypeBinding)>, values: Vec<String>) -> usize {
        let frame = self.frames.len();
        let mut type_names = Vec::with_capacity(types.len());
        for (name, binding) in types {
            self.type_bindings
                .entry(name.clone())
                .or_default()
                .push((frame, binding));
            type_names.push(name);
        }
        for name in &values {
            self.value_bindings
                .entry(name.clone())
                .or_default()
                .push(frame);
        }
        self.frames.push(Frame {
            type_names,
            value_names: values,
            type_params_len: self.type_params.len(),
        });
        frame
    }

    fn pop(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        for name in frame.type_names {
            if let Some(bindings) = self.type_bindings.get_mut(&name) {
                bindings.pop();
                if bindings.is_empty() {
                    self.type_bindings.remove(&name);
                }
            }
        }
        for name in frame.value_names {
            if let Some(bindings) = self.value_bindings.get_mut(&name) {
                bindings.pop();
                if bindings.is_empty() {
                    self.value_bindings.remove(&name);
                }
            }
        }
        self.type_params.truncate(frame.type_params_len);
    }

    /// The type bindings of `name` in scope, innermost first.
    fn type_bindings_of(&self, name: &str) -> impl Iterator<Item = (usize, TypeBinding)> + '_ {
        self.type_bindings
            .get(name)
            .into_iter()
            .flat_map(|bindings| bindings.iter().rev().copied())
            .filter(|&(frame, _)| Some(frame) != self.hidden_frame)
    }

    fn resolve_type(&self, name: &str) -> TypeName {
        match self.type_bindings_of(name).next() {
            Some((frame, TypeBinding::TypeParam(id))) => TypeName::TypeParam { id, frame },
            Some((_, TypeBinding::Local(decl))) => TypeName::Local(decl),
            None => TypeName::NotLocal,
        }
    }

    /// The nearest local enum named `name`, with the frame declaring it.
    fn local_enum(&self, name: &str) -> Option<(usize, DeclId)> {
        self.type_bindings_of(name)
            .find_map(|(frame, binding)| match binding {
                TypeBinding::Local(decl) if self.decl(decl).is_enum => Some((frame, decl)),
                _ => None,
            })
    }

    fn resolve_value(&self, name: &str) -> ValueName {
        let value = self
            .value_bindings
            .get(name)
            .and_then(|frames| frames.last().copied());
        match (value, self.local_enum(name)) {
            // In one frame, a value and an enum of one name are a reported
            // clash; the value wins.
            (Some(frame), Some((enum_frame, _))) if frame >= enum_frame => {
                ValueName::LocalValue { frame }
            }
            (_, Some((_, decl))) => ValueName::LocalEnum(decl),
            (Some(frame), None) => ValueName::LocalValue { frame },
            (None, None) => ValueName::NotLocal,
        }
    }

    // Ids are only made by `declare` and `push_type_params`, as indices into
    // the vectors they push to. `decls` never shrinks; a type parameter's id
    // is looked up only while its frame is on the stack.

    fn decl(&self, id: DeclId) -> &LocalDecl {
        &self.decls[id.0]
    }

    fn decl_mut(&mut self, id: DeclId) -> &mut LocalDecl {
        &mut self.decls[id.0]
    }

    fn type_param(&self, id: TypeParamId) -> &crate::TypeParameterAnnotation {
        &self.type_params[id.0]
    }

    // ---- local type declarations ----

    /// Collect what a local type declaration uses, before its lifted
    /// parameters are known. The walk changes nothing while it collects, so
    /// its copy of the declaration is dropped.
    fn collect_uses(
        &mut self,
        ast: &Ast,
        id: StmtId,
        decl: DeclId,
    ) -> Result<DeclUses, CompilerFailure> {
        let mut kind = ast.try_stmt(id).map_err(arena_failure)?.kind.clone();
        self.current = Some(InDecl {
            decl,
            first_frame: self.frames.len(),
            uses: Some(DeclUses::default()),
        });
        self.walk_decl_types(&mut kind)?;
        let mut visited = BTreeSet::new();
        loop {
            let parameters: Vec<_> = self
                .collecting_uses()
                .into_iter()
                .flat_map(|uses| uses.type_params.iter().copied())
                .collect();
            let mut progressed = false;
            for parameter in parameters {
                if visited.insert(parameter) {
                    progressed = true;
                    if let Some(mut constraint) = self.type_param(parameter).constraint.clone() {
                        self.walk_type(&mut constraint)?;
                    }
                }
            }
            if !progressed {
                break;
            }
        }
        let uses = self.current.take().and_then(|in_decl| in_decl.uses);
        Ok(uses.unwrap_or_default())
    }

    /// Rewrite a local type declaration under its key, with its lifted
    /// parameters, and rename what it references.
    fn rewrite_decl(
        &mut self,
        ast: &mut Ast,
        id: StmtId,
        decl: DeclId,
    ) -> Result<(), CompilerFailure> {
        let mut kind = ast.try_stmt(id).map_err(arena_failure)?.kind.clone();
        self.current = Some(InDecl {
            decl,
            first_frame: self.frames.len(),
            uses: None,
        });
        self.walk_decl_types(&mut kind)?;
        self.apply_key(&mut kind, decl)?;
        self.current = None;
        ast.try_stmt_mut(id).map_err(arena_failure)?.kind = kind;
        Ok(())
    }

    /// Walk the annotations of a local type declaration, with the type
    /// parameters it declares in scope. An enum's members are literals.
    fn walk_decl_types(&mut self, kind: &mut StmtKind) -> Result<(), CompilerFailure> {
        match kind {
            StmtKind::InterfaceDecl {
                generics,
                extends,
                members,
                ..
            } => {
                self.declare_own_type_params(generics);
                self.walk_constraints(generics)?;
                for ty in extends.iter_mut() {
                    self.walk_type(ty)?;
                }
                for member in members.iter_mut() {
                    match member {
                        InterfaceMember::IndexSignature(index) => {
                            self.walk_type(&mut index.key)?;
                            self.walk_type(&mut index.value)?;
                        }
                        InterfaceMember::Method {
                            generics,
                            params,
                            return_type,
                            ..
                        } => {
                            self.declare_own_type_params(generics);
                            self.walk_constraints(generics)?;
                            self.push_values(params.iter().map(|p| p.name.name.clone()).collect());
                            for param in params.iter_mut() {
                                self.walk_opt_type(&mut param.ty)?;
                            }
                            self.walk_type(return_type)?;
                            self.pop();
                            self.pop();
                        }
                        InterfaceMember::Property { ty, .. } => self.walk_type(ty)?,
                    }
                }
                self.pop();
            }
            StmtKind::TypeAliasDecl { generics, ty, .. } => {
                self.declare_own_type_params(generics);
                self.walk_constraints(generics)?;
                self.walk_type(ty)?;
                self.pop();
            }
            _ => {}
        }
        Ok(())
    }

    /// Push the type parameters a declaration declares, its own or a method's,
    /// and note their names while collecting its uses.
    fn declare_own_type_params(&mut self, names: &[crate::TypeParameterAnnotation]) {
        if let Some(uses) = self.collecting_uses() {
            uses.declared_type_params
                .extend(names.iter().map(|name| name.name.clone()));
        }
        self.push_type_params(names);
    }

    fn walk_constraints(
        &mut self,
        parameters: &mut [crate::TypeParameterAnnotation],
    ) -> Result<(), CompilerFailure> {
        for parameter in parameters {
            self.walk_opt_type(&mut parameter.constraint)?;
        }
        Ok(())
    }

    /// Where the uses of the declaration being walked go, while they are
    /// collected.
    fn collecting_uses(&mut self) -> Option<&mut DeclUses> {
        self.current
            .as_mut()
            .and_then(|in_decl| in_decl.uses.as_mut())
    }

    /// Rename a declaration to its key and prepend its lifted parameters.
    fn apply_key(&mut self, kind: &mut StmtKind, decl: DeclId) -> Result<(), CompilerFailure> {
        let (name, generics) = match kind {
            StmtKind::InterfaceDecl { name, generics, .. }
            | StmtKind::TypeAliasDecl { name, generics, .. } => (name, Some(generics)),
            StmtKind::EnumDecl { name, .. } => (name, None),
            _ => return Ok(()),
        };
        if let Some(generics) = generics {
            let count = self.decl(decl).lifted.len();
            charge(&mut self.lifted_added, count, name.span)?;
            let mut lifted: Vec<_> = self
                .decl(decl)
                .lifted
                .iter()
                .map(|param| {
                    let mut annotation = self.type_param(param.id).clone();
                    annotation.ident.name = param.name.clone();
                    annotation
                })
                .collect();
            self.walk_constraints(&mut lifted)?;
            generics.splice(0..0, lifted);
        }
        name.name.clone_from(&self.decl(decl).key);
        Ok(())
    }

    /// Settle the lifted parameters of a block's declarations: each lifts the
    /// outer type parameters it names, and those of the local types it
    /// references. Those of enclosing blocks are already settled; within the
    /// block, newly lifted parameters spread along references until none is
    /// added. Every parameter taken across a reference is charged.
    fn settle_lifted_params(
        &mut self,
        uses: Vec<(DeclId, DeclUses)>,
    ) -> Result<(), CompilerFailure> {
        // Declarations sharing a record pool their uses.
        let mut block: BTreeMap<DeclId, DeclUses> = BTreeMap::new();
        for (decl, decl_uses) in uses {
            let pooled = block.entry(decl).or_default();
            pooled.type_params.extend(decl_uses.type_params);
            pooled.local_types.extend(decl_uses.local_types);
            pooled
                .declared_type_params
                .extend(decl_uses.declared_type_params);
        }
        let in_block: BTreeSet<DeclId> = block.keys().copied().collect();
        let mut referenced_by: HashMap<DeclId, Vec<DeclId>> = HashMap::new();
        for (&decl, decl_uses) in &mut block {
            for target in std::mem::take(&mut decl_uses.local_types) {
                if in_block.contains(&target) {
                    referenced_by.entry(target).or_default().push(decl);
                    continue;
                }
                let (count, span) = (self.decl(target).lifted.len(), self.decl(decl).span);
                charge(&mut self.lifted_spread, count, span)?;
                let settled = self.decl(target).lifted.iter().map(|param| param.id);
                decl_uses.type_params.extend(settled);
            }
        }
        // A declaration is pending while it has parameters it hasn't spread
        // yet, so each parameter crosses each reference once.
        let mut unspread: BTreeMap<DeclId, Vec<TypeParamId>> = block
            .iter()
            .filter(|(_, decl_uses)| !decl_uses.type_params.is_empty())
            .map(|(&decl, decl_uses)| (decl, decl_uses.type_params.iter().copied().collect()))
            .collect();
        while let Some((decl, params)) = unspread.pop_first() {
            for &user in referenced_by.get(&decl).into_iter().flatten() {
                let span = self.decl(user).span;
                let Some(user_uses) = block.get_mut(&user) else {
                    continue;
                };
                charge(&mut self.lifted_spread, params.len(), span)?;
                let added: Vec<TypeParamId> = params
                    .iter()
                    .copied()
                    .filter(|&param| user_uses.type_params.insert(param))
                    .collect();
                if !added.is_empty() {
                    unspread.entry(user).or_default().extend(added);
                }
            }
        }
        for (decl, decl_uses) in block {
            let lifted =
                self.name_lifted_params(decl_uses.type_params, decl_uses.declared_type_params);
            let local = self.decl_mut(decl);
            if !lifted.is_empty() {
                local.key = format!("{}{MARKER}{}", local.key, lifted.len());
            }
            local.lifted = lifted;
        }
        Ok(())
    }

    /// Name the outer type parameters a declaration lifts: each keeps its own
    /// name, unless a type parameter the declaration declares, or another
    /// lifted one, already has it.
    fn name_lifted_params(
        &self,
        params: BTreeSet<TypeParamId>,
        mut taken: HashSet<String>,
    ) -> Vec<LiftedParam> {
        let mut lifted = Vec::with_capacity(params.len());
        for id in params {
            let param = self.type_param(id);
            let name = if taken.contains(&param.name) {
                format!("{}{MARKER}{}", param.name, id.0)
            } else {
                param.name.clone()
            };
            taken.insert(name.clone());
            lifted.push(LiftedParam {
                id,
                name,
                span: param.span,
            });
        }
        lifted
    }

    /// The name outer type parameter `id` has here: inside the declaration
    /// being rewritten, the name it was lifted under; elsewhere its own.
    fn type_param_name_here(&self, id: TypeParamId) -> String {
        if let Some(in_decl) = &self.current {
            let lifted = &self.decl(in_decl.decl).lifted;
            if let Ok(index) = lifted.binary_search_by_key(&id, |param| param.id) {
                return lifted[index].name.clone();
            }
        }
        self.type_param(id).name.clone()
    }

    /// The arguments a reference to `target` passes for its lifted parameters.
    ///
    /// Outside a local type, an argument names its parameter by its own name:
    /// nothing between a local type and its references declares another type
    /// parameter, since nested functions and arrows can't be generic, and a
    /// local type of that name is renamed to its key.
    fn lifted_args(
        &mut self,
        target: DeclId,
        at: &Ident,
    ) -> Result<Vec<TypeAnnotation>, CompilerFailure> {
        let count = self.decl(target).lifted.len();
        charge(&mut self.lifted_added, count, at.span)?;
        Ok(self
            .decl(target)
            .lifted
            .iter()
            .map(|param| TypeAnnotation {
                kind: TypeAnnotationKind::Name {
                    name: Ident {
                        name: self.type_param_name_here(param.id),
                        span: param.span,
                    },
                    args: Vec::new(),
                },
                span: param.span,
            })
            .collect())
    }

    // ---- type annotations ----

    fn walk_type(&mut self, ty: &mut TypeAnnotation) -> Result<(), CompilerFailure> {
        match &mut ty.kind {
            TypeAnnotationKind::Name { name, args } => {
                for arg in args.iter_mut() {
                    self.walk_type(arg)?;
                }
                match self.resolve_type(&name.name) {
                    TypeName::TypeParam { id, frame } => {
                        // Only a parameter from outside the local type being
                        // walked is lifted.
                        let Some(in_decl) = &self.current else {
                            return Ok(());
                        };
                        if frame >= in_decl.first_frame {
                            return Ok(());
                        }
                        if let Some(uses) = self.collecting_uses() {
                            uses.type_params.insert(id);
                        } else {
                            name.name = self.type_param_name_here(id);
                        }
                    }
                    TypeName::Local(target) => {
                        if let Some(uses) = self.collecting_uses() {
                            uses.local_types.insert(target);
                            return Ok(());
                        }
                        let lifted = self.lifted_args(target, name)?;
                        name.name.clone_from(&self.decl(target).key);
                        args.splice(0..0, lifted);
                    }
                    TypeName::NotLocal => {}
                }
            }
            TypeAnnotationKind::Qualified { path, args } => {
                for arg in args.iter_mut() {
                    self.walk_type(arg)?;
                }
                // `E.A` names a member of the nearest enum `E`: as in TypeScript,
                // a type or type parameter of that name doesn't hide it.
                if let Some(first) = path.first_mut()
                    && let Some((_, target)) = self.local_enum(&first.name)
                    && self.collecting_uses().is_none()
                {
                    first.name.clone_from(&self.decl(target).key);
                }
            }
            TypeAnnotationKind::TypeOf { path } => {
                if let Some(first) = path.first_mut() {
                    self.walk_typeof_root(first);
                }
            }
            TypeAnnotationKind::Array(inner)
            | TypeAnnotationKind::Optional(inner)
            | TypeAnnotationKind::Readonly(inner)
            | TypeAnnotationKind::KeyOf(inner) => self.walk_type(inner)?,
            TypeAnnotationKind::TemplateLiteral {
                substitutions: items,
                ..
            }
            | TypeAnnotationKind::Tuple(items)
            | TypeAnnotationKind::Union(items) => {
                for item in items.iter_mut() {
                    self.walk_type(item)?;
                }
            }
            TypeAnnotationKind::Object { fields, index } => {
                for field in fields.iter_mut() {
                    self.walk_type(&mut field.ty)?;
                }
                if let Some(index) = index {
                    self.walk_type(&mut index.key)?;
                    self.walk_type(&mut index.value)?;
                }
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                for param in params.iter_mut() {
                    self.walk_type(&mut param.ty)?;
                }
                self.walk_type(return_type)?;
            }
            TypeAnnotationKind::StringLiteral(_)
            | TypeAnnotationKind::NumberLiteral(_)
            | TypeAnnotationKind::BigIntLiteral(_)
            | TypeAnnotationKind::BooleanLiteral(_) => {}
        }
        Ok(())
    }

    /// `typeof x`: a local enum is renamed, and a local variable can't be named
    /// from a local type, which has no scope at module level to read it in.
    fn walk_typeof_root(&mut self, root: &mut Ident) {
        match self.resolve_value(&root.name) {
            ValueName::LocalEnum(target) => {
                if self.collecting_uses().is_none() {
                    root.name.clone_from(&self.decl(target).key);
                }
            }
            ValueName::LocalValue { frame } => {
                let Some(in_decl) = &self.current else {
                    return;
                };
                let from_outside = frame < in_decl.first_frame;
                // A local type is walked twice; report on the collecting walk
                // so the error appears once.
                if from_outside && self.collecting_uses().is_some() {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: root.span,
                        message: format!(
                            "a type declared in a block can't use `typeof` on the local `{}`",
                            root.name
                        ),
                        help: vec![
                            "write the type out, or declare the type at the top level of the \
                             module"
                                .to_string(),
                        ],
                        notes: Vec::new(),
                    });
                }
            }
            ValueName::NotLocal => {}
        }
    }

    fn walk_opt_type(&mut self, ty: &mut Option<TypeAnnotation>) -> Result<(), CompilerFailure> {
        match ty {
            Some(ty) => self.walk_type(ty),
            None => Ok(()),
        }
    }

    // ---- expressions ----

    fn walk_expr(&mut self, ast: &mut Ast, id: ExprId) -> Result<(), CompilerFailure> {
        let mut kind = ast.try_expr(id).map_err(arena_failure)?.kind.clone();
        match &mut kind {
            ExprKind::Identifier(name) => self.rename_local_enum(name),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(ast, *lhs)?;
                self.walk_expr(ast, *rhs)?;
            }
            ExprKind::Unary { operand, .. }
            | ExprKind::Paren(operand)
            | ExprKind::Void { operand }
            | ExprKind::Typeof { operand }
            | ExprKind::Delete { operand }
            | ExprKind::PostfixUnary { operand, .. } => self.walk_expr(ast, *operand)?,
            ExprKind::Call {
                callee,
                type_args,
                args,
            }
            | ExprKind::New {
                callee,
                type_args,
                args,
            } => {
                self.walk_expr(ast, *callee)?;
                self.walk_type_args(type_args)?;
                self.walk_exprs(ast, args)?;
            }
            ExprKind::ObjectLiteral { members } => {
                for member in members.iter() {
                    match member {
                        ObjectLiteralMember::Computed { key, value } => {
                            self.walk_expr(ast, *key)?;
                            self.walk_expr(ast, *value)?;
                        }
                        ObjectLiteralMember::Field(field) => self.walk_expr(ast, field.value)?,
                        ObjectLiteralMember::Spread { value, .. } => self.walk_expr(ast, *value)?,
                    }
                }
            }
            ExprKind::ArrayLiteral { elements } => {
                for element in elements.iter() {
                    self.walk_expr(ast, element.value())?;
                }
            }
            ExprKind::FieldAccess { receiver, .. } => self.walk_expr(ast, *receiver)?,
            ExprKind::IndexAccess { receiver, index } => {
                self.walk_expr(ast, *receiver)?;
                self.walk_expr(ast, *index)?;
            }
            ExprKind::FunctionExpression {
                name,
                this_type,
                function,
            } => {
                self.push_values(name.iter().map(|name| name.name.clone()).collect());
                self.walk_opt_type(this_type)?;
                self.walk_expr(ast, *function)?;
                self.pop();
            }
            ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body,
            } => {
                let names = self.enter_params(ast, params)?;
                self.walk_opt_type(return_type)?;
                if let Some(predicate) = type_predicate {
                    self.walk_type(&mut predicate.asserted)?;
                }
                match *body {
                    ArrowBody::Expr(body) => self.walk_expr(ast, body)?,
                    ArrowBody::Block(body) => {
                        let signature = Signature {
                            params: &names,
                            ..Signature::default()
                        };
                        self.walk_body(ast, body, signature)?;
                    }
                }
                self.pop();
            }
            ExprKind::TemplateLiteral { exprs, .. } => self.walk_exprs(ast, exprs)?,
            ExprKind::Ternary { cond, then_, else_ } => {
                self.walk_expr(ast, *cond)?;
                self.walk_expr(ast, *then_)?;
                self.walk_expr(ast, *else_)?;
            }
            ExprKind::OptionalChain { base, parts } => {
                self.walk_expr(ast, *base)?;
                for part in parts.iter_mut() {
                    match part {
                        ChainPart::Index { idx, .. } => self.walk_expr(ast, *idx)?,
                        ChainPart::Call {
                            args, type_args, ..
                        } => {
                            self.walk_exprs(ast, args)?;
                            self.walk_type_args(type_args)?;
                        }
                        ChainPart::Field { .. } | ChainPart::NonNull { .. } => {}
                    }
                }
            }
            ExprKind::Assign { target, value, .. } => {
                self.walk_expr(ast, *target)?;
                self.walk_expr(ast, *value)?;
            }
            ExprKind::As { expr, ty } => {
                self.walk_expr(ast, *expr)?;
                self.walk_type(ty)?;
            }
            ExprKind::InstanceOf { value, ty } => {
                self.walk_expr(ast, *value)?;
                self.walk_class_test_target(ty)?;
            }
            ExprKind::Number(_)
            | ExprKind::BigInt(_)
            | ExprKind::String(_)
            | ExprKind::Boolean(_)
            | ExprKind::Null
            | ExprKind::This
            | ExprKind::ThisOutsideReceiver
            | ExprKind::Super
            | ExprKind::Regex { .. } => {}
        }
        ast.try_expr_mut(id).map_err(arena_failure)?.kind = kind;
        Ok(())
    }

    fn walk_exprs(&mut self, ast: &mut Ast, ids: &[ExprId]) -> Result<(), CompilerFailure> {
        for &id in ids {
            self.walk_expr(ast, id)?;
        }
        Ok(())
    }

    fn walk_type_args(
        &mut self,
        type_args: &mut Option<Vec<TypeAnnotation>>,
    ) -> Result<(), CompilerFailure> {
        for ty in type_args.iter_mut().flatten() {
            self.walk_type(ty)?;
        }
        Ok(())
    }

    /// The right side of `instanceof` and a `catch` annotation name a class,
    /// which is a value. A local interface or alias there keeps its source
    /// name, so a class of the name wins, and inference resolves it to the
    /// local type only when there is no such class. A local enum, a value too,
    /// is renamed.
    fn walk_class_test_target(&mut self, ty: &mut TypeAnnotation) -> Result<(), CompilerFailure> {
        let TypeAnnotationKind::Name { name, args } = &mut ty.kind else {
            return self.walk_type(ty);
        };
        for arg in args.iter_mut() {
            self.walk_type(arg)?;
        }
        self.rename_local_enum(name);
        Ok(())
    }

    /// Rename a value reference that resolves to a local enum.
    fn rename_local_enum(&self, name: &mut Ident) {
        if let ValueName::LocalEnum(target) = self.resolve_value(&name.name) {
            name.name.clone_from(&self.decl(target).key);
        }
    }
}

/// The scope a switch's arms share: from its first label to its end, so its
/// discriminant is outside it.
fn arms_scope(switch_span: Span, cases: &[SwitchCase], default: Option<&SwitchDefault>) -> Span {
    let first_label = cases
        .iter()
        .map(|case| case.span.start)
        .chain(default.map(|default| default.span.start))
        .min();
    first_label
        .and_then(|start| Span::new(switch_span.file, start, switch_span.end).ok())
        .unwrap_or(switch_span)
}

/// Add `count` lifted parameters to `total`, which may not pass
/// [`MAX_LIFTED_TYPE_ARGUMENTS`]; `span` is where the last were added.
fn charge(total: &mut usize, count: usize, span: Span) -> Result<(), CompilerFailure> {
    *total = total.saturating_add(count);
    if *total <= MAX_LIFTED_TYPE_ARGUMENTS {
        return Ok(());
    }
    Err(CompilerFailure::Limit {
        stage: CompilerStage::Infer,
        span: Some(span),
        message: format!(
            "types declared in blocks pass their functions' type parameters more than \
             {MAX_LIFTED_TYPE_ARGUMENTS} times"
        ),
        help: vec![
            "declare the types at the top level of the module, with type parameters of their \
             own"
            .to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::{display_type_name, is_local_type_name, lower};
    use crate::{Asi, Ast, ExprKind, StmtKind, Token, TokenKind, TypeAnnotationKind, parse};

    fn parsed(source: &str) -> Ast {
        let mut asi = Asi::new(source, crate::FileId(0));
        let mut tokens: Vec<Token> = Vec::new();
        loop {
            let t = asi.next_token();
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let _ = asi.into_diagnostics();
        let (ast, diags) = parse(source, tokens, crate::FileId(0));
        assert!(diags.is_empty(), "unexpected parse diags: {diags:?}");
        ast
    }

    fn pipeline(source: &str) -> (Ast, Vec<crate::Diagnostic>) {
        // Pattern lowering ends with this pass.
        let mut ast = crate::lower_patterns(parsed(source)).unwrap();
        let diags = ast.lowered_local_types.take().unwrap().diagnostics;
        // A second run finds nothing left to lift.
        let before = ast.top_level.len();
        assert!(lower(&mut ast).unwrap().diagnostics.is_empty());
        assert_eq!(ast.top_level.len(), before);
        (ast, diags)
    }

    /// The hoisted declarations: source name and type parameters.
    fn hoisted(ast: &Ast) -> Vec<(String, Vec<String>)> {
        ast.top_level
            .iter()
            .filter_map(|&id| match &ast.try_stmt(id).unwrap().kind {
                StmtKind::InterfaceDecl { name, generics, .. }
                | StmtKind::TypeAliasDecl { name, generics, .. }
                    if is_local_type_name(&name.name) =>
                {
                    Some((
                        display_type_name(&name.name).to_string(),
                        generics.iter().map(|g| g.name.clone()).collect(),
                    ))
                }
                StmtKind::EnumDecl { name, .. } if is_local_type_name(&name.name) => {
                    Some((display_type_name(&name.name).to_string(), Vec::new()))
                }
                _ => None,
            })
            .collect()
    }

    fn render(ty: &crate::TypeAnnotation) -> String {
        match &ty.kind {
            TypeAnnotationKind::Name { name, args } if args.is_empty() => name.name.clone(),
            TypeAnnotationKind::Name { name, args } => format!(
                "{}<{}>",
                name.name,
                args.iter().map(render).collect::<Vec<_>>().join(", ")
            ),
            TypeAnnotationKind::Qualified { path, .. } => path
                .iter()
                .map(|segment| segment.name.as_str())
                .collect::<Vec<_>>()
                .join("."),
            TypeAnnotationKind::TypeOf { path } => format!(
                "typeof {}",
                path.iter()
                    .map(|segment| segment.name.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            ),
            TypeAnnotationKind::Array(inner) => format!("{}[]", render(inner)),
            other => format!("{other:?}"),
        }
    }

    /// The annotation of every annotated `let`/`const`, in arena order.
    fn binding_types(ast: &Ast) -> Vec<String> {
        (0..ast.stmts_len())
            .filter_map(
                |i| match &ast.try_stmt(crate::StmtId(i as u32)).unwrap().kind {
                    StmtKind::Let { ty: Some(ty), .. } | StmtKind::Const { ty: Some(ty), .. } => {
                        Some(render(ty))
                    }
                    _ => None,
                },
            )
            .collect()
    }

    /// The rendered right side of every `instanceof`, in arena order.
    fn instanceof_targets(ast: &Ast) -> Vec<String> {
        (0..ast.exprs_len())
            .filter_map(
                |i| match &ast.try_expr(crate::ExprId(i as u32)).unwrap().kind {
                    ExprKind::InstanceOf { ty, .. } => Some(render(ty)),
                    _ => None,
                },
            )
            .collect()
    }

    /// The receivers of `x.y` reads that are plain identifiers, in arena order.
    fn field_receivers(ast: &Ast) -> Vec<String> {
        (0..ast.exprs_len())
            .filter_map(|i| {
                let ExprKind::FieldAccess { receiver, .. } =
                    &ast.try_expr(crate::ExprId(i as u32)).unwrap().kind
                else {
                    return None;
                };
                match &ast.try_expr(*receiver).unwrap().kind {
                    ExprKind::Identifier(name) => Some(name.name.clone()),
                    _ => None,
                }
            })
            .collect()
    }

    /// The span of the lifted-parameter limit failure lowering `source` reports.
    fn limit_span(source: &str) -> crate::Span {
        match crate::lower_patterns(parsed(source)) {
            Err(crate::compiler_error::CompilerFailure::Limit {
                message,
                span: Some(span),
                ..
            }) if message.contains("type parameters more than") => span,
            other => panic!("expected the lifted-parameter limit, got {other:?}"),
        }
    }

    fn many_params(count: usize) -> String {
        (0..count)
            .map(|i| format!("T{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Where `name` starts in the `nth` occurrence of `context`, counting from 0.
    fn offset_in(source: &str, context: &str, nth: usize, name: &str) -> u32 {
        let (at, _) = source.match_indices(context).nth(nth).unwrap();
        (at + context.find(name).unwrap()) as u32
    }

    /// The `did you mean` suggestions compiling `source` reports.
    fn suggestions(source: &'static str) -> Vec<String> {
        crate::type_size::tests::on_compiler_stack(move || {
            let error = crate::compile::compile_script_checked(
                source,
                "suggest.ts",
                crate::FileId(0),
                &[],
                &[],
            )
            .unwrap_err();
            error
                .diagnostics
                .iter()
                .flat_map(|diag| diag.help.clone())
                .filter(|line| line.starts_with("did you mean"))
                .collect()
        })
    }

    #[test]
    fn leaves_a_module_without_local_types_alone() {
        let source =
            "interface Row { a: number }\nfunction f(): void { const r: Row = { a: 1 }; }\n";
        let (ast, diags) = pipeline(source);
        assert!(diags.is_empty());
        assert!(hoisted(&ast).is_empty());
        assert_eq!(binding_types(&ast), vec!["Row"]);
    }

    #[test]
    fn hoists_and_renames_a_local_interface() {
        let (ast, diags) = pipeline(
            "function main(): void {\n  const kept: Kept[] = [];\n  interface Kept { n: number }\n}\n",
        );
        assert!(diags.is_empty());
        assert_eq!(hoisted(&ast), vec![("Kept".to_string(), vec![])]);
        let types = binding_types(&ast);
        assert_eq!(types.len(), 1);
        assert!(
            types[0].starts_with("Kept%") && types[0].ends_with("[]"),
            "{types:?}"
        );
    }

    #[test]
    fn same_name_in_two_functions_gets_two_names() {
        let (ast, _) = pipeline(
            "function a(): void { interface Row { x: number } const r: Row = { x: 1 }; }\n\
             function b(): void { interface Row { y: string } const r: Row = { y: \"\" }; }\n",
        );
        let types = binding_types(&ast);
        assert_eq!(types.len(), 2);
        assert_ne!(types[0], types[1]);
    }

    #[test]
    fn inner_block_shadows_outer_local_and_outer_scope_stays_top_level() {
        let (ast, _) = pipeline(
            "interface Row { t: number }\n\
             function f(): void {\n  const a: Row = { t: 1 };\n  {\n    interface Row { i: number }\n    const b: Row = { i: 1 };\n  }\n}\n",
        );
        let types = binding_types(&ast);
        assert_eq!(types[0], "Row");
        assert!(types[1].starts_with("Row%"));
    }

    #[test]
    fn switch_arms_share_one_scope_and_a_shared_body_is_lifted_once() {
        let (ast, diags) = pipeline(
            "function f<T>(n: number, t: T): void {\n  switch (n) {\n    case 0:\n      interface A { t: T }\n      break;\n    case 1:\n    default:\n      const a: A = { t };\n  }\n}\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            hoisted(&ast),
            vec![("A".to_string(), vec!["T".to_string()])]
        );
        let types = binding_types(&ast);
        assert!(
            types[0].starts_with("A%") && types[0].ends_with("<T>"),
            "{types:?}"
        );
    }

    #[test]
    fn lifts_captured_type_parameters_transitively() {
        let (ast, _) = pipeline(
            "function f<T, U>(x: T): void {\n  interface A { v: T }\n  interface B<T> { a: A; own: T }\n  type C = number;\n  const b: B<string> = { a: { v: x }, own: \"\" };\n}\n",
        );
        let lifted = hoisted(&ast);
        assert_eq!(lifted[0], ("A".to_string(), vec!["T".to_string()]));
        // B captures the outer `T` through `A`, under a fresh name, since its
        // own `T` shadows it.
        assert_eq!(lifted[1].0, "B");
        assert_eq!(lifted[1].1.len(), 2);
        assert!(lifted[1].1[0].starts_with("T%"));
        assert_eq!(lifted[1].1[1], "T");
        assert_eq!(lifted[2], ("C".to_string(), vec![]));
        let types = binding_types(&ast);
        assert!(
            types[0].starts_with("B%") && types[0].ends_with("<T, string>"),
            "{types:?}"
        );
    }

    #[test]
    fn a_lifted_parameter_avoids_a_method_type_parameter_name() {
        let (ast, _) =
            pipeline("function f<T>(): void {\n  interface B { v: T; m<T>(t: T): T }\n}\n");
        let lifted = hoisted(&ast);
        assert_eq!(lifted[0].0, "B");
        assert!(lifted[0].1[0].starts_with("T%"), "{lifted:?}");
    }

    #[test]
    fn a_static_member_does_not_capture_the_class_type_parameters() {
        let (ast, _) = pipeline(
            "class C<T> {\n  static make(): void { interface S { v: T } }\n  get(): void { interface I { v: T } }\n}\n",
        );
        assert_eq!(
            hoisted(&ast),
            vec![
                ("S".to_string(), vec![]),
                ("I".to_string(), vec!["T".to_string()])
            ]
        );
    }

    #[test]
    fn renames_local_enum_references_unless_shadowed() {
        let (ast, _) = pipeline(
            "const E = 1;\nfunction f(): number {\n  enum E { A, B }\n  const x = E.B;\n  const a: E.A = E.A;\n  const t: typeof E = E;\n  {\n    const E = { B: 2 };\n    return E.B;\n  }\n}\n",
        );
        let receivers = field_receivers(&ast);
        assert_eq!(receivers.len(), 3);
        assert!(receivers[0].starts_with("E%") && receivers[1].starts_with("E%"));
        assert_eq!(receivers[2], "E");
        let types = binding_types(&ast);
        assert!(
            types[0].starts_with("E%") && types[0].ends_with(".A"),
            "{types:?}"
        );
        assert!(types[1].starts_with("typeof E%"), "{types:?}");
    }

    #[test]
    fn renames_an_assignment_to_a_local_enum() {
        let (ast, _) =
            pipeline("let E = 1;\nfunction f(): void {\n  enum E { A }\n  E = 2;\n  E += 1;\n}\n");
        let targets: Vec<String> = (0..ast.stmts_len())
            .filter_map(
                |i| match &ast.try_stmt(crate::StmtId(i as u32)).unwrap().kind {
                    StmtKind::Assign { target, .. } | StmtKind::CompoundAssign { target, .. } => {
                        Some(target.name.clone())
                    }
                    _ => None,
                },
            )
            .collect();
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().all(|t| t.starts_with("E%")), "{targets:?}");
    }

    #[test]
    fn same_name_declarations_in_one_block_share_a_key_and_their_captures() {
        let (ast, _) = pipeline(
            "function f<T, U>(): void {\n  interface A { x: T }\n  interface A { y: U }\n}\n",
        );
        let lifted = hoisted(&ast);
        let expected = ("A".to_string(), vec!["T".to_string(), "U".to_string()]);
        assert_eq!(lifted, vec![expected.clone(), expected]);
        let keys: Vec<&str> = ast
            .top_level
            .iter()
            .filter_map(|&id| match &ast.try_stmt(id).unwrap().kind {
                StmtKind::InterfaceDecl { name, .. } => Some(name.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], keys[1]);
    }

    #[test]
    fn a_qualified_enum_member_skips_a_nearer_type_of_the_name() {
        let (ast, _) = pipeline(
            "function f(): void {\n  enum E { A }\n  {\n    interface E { }\n    const v: E.A = E.A;\n  }\n}\n",
        );
        let types = binding_types(&ast);
        // The enum's key, not the interface's: they are declared in different
        // blocks.
        let enum_key = ast
            .top_level
            .iter()
            .find_map(|&id| match &ast.try_stmt(id).unwrap().kind {
                StmtKind::EnumDecl { name, .. } => Some(name.name.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(types, vec![format!("{enum_key}.A")]);
    }

    #[test]
    fn rejects_typeof_on_a_local_value() {
        let (_, diags) =
            pipeline("function f(): void {\n  const x = 1;\n  type X = typeof x;\n}\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`typeof`"), "{diags:?}");
    }

    #[test]
    fn rejects_a_body_type_named_like_a_type_parameter() {
        let (_, diags) = pipeline(
            "function f<T>(): void {\n  interface T { }\n  {\n    interface T { }\n  }\n}\n",
        );
        // Only the body block shares the type parameters' scope.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0]
                .message
                .contains("duplicate declaration of type `T`")
        );
    }

    #[test]
    fn rejects_an_enum_named_like_a_parameter_of_its_function() {
        let (_, diags) = pipeline(
            "function f(E: number): void {\n  enum E { A }\n}\n\
             const g = (K: number): void => {\n  enum K { A }\n};\n\
             const h = function ({ a: L }: { a: number }, ...M: number[]): void {\n  enum L { A }\n  enum M { A }\n};\n\
             const o = { m(N: number): void { enum N { A } } };\n\
             class C {\n  constructor(public P: number) { enum P { A } }\n  m(Q: number): void { enum Q { A } }\n  static s(R: number): void { enum R { A } }\n  set v(S: number) { enum S { A } }\n}\n\
             function patterns({ ...T }: { a: number }, [U]: number[], V = 1, { W = 2 }: { W?: number } = {}): void {\n  enum T { A }\n  enum U { A }\n  enum V { A }\n  enum W { A }\n}\n\
             function nested(Z: number): void {\n  {\n    enum Z { A }\n  }\n}\n",
        );
        // `nested`'s inner block enum shadows its parameter, as in TypeScript.
        let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
        let expected: Vec<String> = [
            "E", "K", "L", "M", "N", "P", "Q", "R", "S", "T", "U", "V", "W",
        ]
        .iter()
        .map(|name| format!("duplicate declaration of `{name}`"))
        .collect();
        assert_eq!(messages, expected, "{diags:?}");
    }

    #[test]
    fn suggests_a_local_type_only_where_it_is_in_scope() {
        let inside = suggestions(
            "function f<T>(t: T): void {\n  interface Point { x: T }\n  {\n    const p: Pont = { x: t };\n  }\n}\n\
             function main(): void {}\n",
        );
        assert_eq!(inside, vec!["did you mean `Point`?"]);
        let in_an_arm = suggestions(
            "function f(n: number): void {\n  switch (n) {\n    case 0:\n      interface Point { x: number }\n      break;\n    default:\n      const p: Pont = { x: 1 };\n  }\n}\n\
             function main(): void {}\n",
        );
        assert_eq!(in_an_arm, vec!["did you mean `Point`?"]);
        let outside = suggestions(
            "function f(): void {\n  interface Point { x: number }\n}\n\
             function g(): void {\n  const p: Pont = { x: 1 };\n}\n\
             function main(): void {}\n",
        );
        assert!(
            outside.iter().all(|line| !line.contains("Point")),
            "{outside:?}"
        );
        // The discriminant is outside the arms' scope.
        let discriminant = suggestions(
            "function f(x: unknown): void {\n  switch ((x as Pont).x) {\n    case 0:\n      interface Point { x: number }\n  }\n}\n\
             function main(): void {}\n",
        );
        assert!(discriminant.is_empty(), "{discriminant:?}");
    }

    #[test]
    fn rejects_an_enum_named_like_a_value_in_its_block() {
        let (_, diags) = pipeline("function f(): void {\n  const E = 1;\n  enum E { A }\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("duplicate declaration of `E`"));
    }

    // 300 parameters per lifted list: the 219th charge passes the limit.

    #[test]
    fn limits_the_lifted_arguments_references_pass() {
        let fields: String = (0..300).map(|i| format!("a{i}: T{i}; ")).collect();
        let uses: String = (0..300).map(|i| format!("let x{i}: A; ")).collect();
        let source = format!(
            "function f<{}>(): void {{ interface A {{ {fields}}} {uses}}}\n",
            many_params(300)
        );
        // `A`'s own declaration is charged first, then each reference.
        let span = limit_span(&source);
        assert_eq!(span.start, offset_in(&source, "let x217: A", 0, "A"));
    }

    #[test]
    fn limits_the_lifted_parameters_duplicates_receive() {
        let source = format!(
            "function f<{0}>(): void {{ type A = [{0}]; {1}}}\n",
            many_params(300),
            "type A = 0; ".repeat(300)
        );
        let span = limit_span(&source);
        assert_eq!(span.start, offset_in(&source, "type A = 0;", 217, "A"));
    }

    #[test]
    fn limits_the_parameters_spread_within_a_block() {
        // Each `A` references `E` in its own block, so settling spreads `E`'s
        // parameters to every one of them, before any reference is rewritten.
        let refs: String = (0..300).map(|i| format!("type A{i} = E; ")).collect();
        let source = format!(
            "function f<{0}>(): void {{ type E = [{0}]; {refs}}}\n",
            many_params(300)
        );
        let span = limit_span(&source);
        assert_eq!(span.start, offset_in(&source, "type A218 =", 0, "A218"));
    }

    #[test]
    fn limits_the_parameters_taken_from_an_enclosing_block() {
        let refs: String = (0..300).map(|i| format!("type A{i} = E; ")).collect();
        let source = format!(
            "function f<{0}>(): void {{ type E = [{0}]; {{ {refs}}} }}\n",
            many_params(300)
        );
        let span = limit_span(&source);
        assert_eq!(span.start, offset_in(&source, "type A218 =", 0, "A218"));
    }

    #[test]
    fn a_static_generic_method_lifts_only_its_own_type_parameters() {
        let (ast, _) =
            pipeline("class C<T> {\n  static m<U>(): void { interface S { u: U; t: T } }\n}\n");
        assert_eq!(
            hoisted(&ast),
            vec![("S".to_string(), vec!["U".to_string()])]
        );
    }

    #[test]
    fn a_qualified_enum_member_skips_a_nearer_alias_or_type_parameter() {
        let (ast, _) = pipeline(
            "function f(): void {\n  enum E { A }\n  {\n    type E = number;\n    const v: E.A = E.A;\n    interface I { m<E>(e: E.A): void }\n  }\n}\n",
        );
        let types = binding_types(&ast);
        assert!(
            types[0].starts_with("E%") && types[0].ends_with(".A"),
            "{types:?}"
        );
    }

    #[test]
    fn a_class_test_keeps_a_local_type_s_source_name() {
        let (ast, _) = pipeline(
            "class Shape { }\nfunction f(x: unknown): boolean {\n  interface Shape { kind: string }\n  return x instanceof Shape;\n}\n",
        );
        assert_eq!(instanceof_targets(&ast), vec!["Shape"]);
        // A local enum is a value, so it does hide the class.
        let (ast, _) = pipeline(
            "class E { }\nfunction f(x: unknown): boolean {\n  enum E { A }\n  return x instanceof E;\n}\n",
        );
        assert!(instanceof_targets(&ast)[0].starts_with("E%"));
        // So does a `catch` annotation, which names a class too.
        let (ast, _) = pipeline(
            "function f(): void {\n  interface Error { code: number }\n  try { } catch (e: Error) { }\n}\n",
        );
        let catch_types: Vec<String> = (0..ast.stmts_len())
            .filter_map(
                |i| match &ast.try_stmt(crate::StmtId(i as u32)).unwrap().kind {
                    StmtKind::Try { catches, .. } => catches[0].ty.as_ref().map(render),
                    _ => None,
                },
            )
            .collect();
        assert_eq!(catch_types, vec!["Error"]);
    }

    #[test]
    fn display_name_and_written_args_drop_the_lifted_suffix() {
        assert_eq!(display_type_name("Row%120"), "Row");
        assert_eq!(display_type_name("Row%120%2"), "Row");
        assert_eq!(display_type_name("Row"), "Row");
        assert_eq!(super::written_type_args("Row%120%2", &[1, 2, 3]), &[3]);
        assert_eq!(super::written_type_args("Row%120", &[1]), &[1]);
        assert_eq!(super::written_type_args("Row", &[1]), &[1]);
    }
}
