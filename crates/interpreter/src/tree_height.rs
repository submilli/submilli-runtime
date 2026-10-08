//! Structural height limits for syntax and typed trees.
//!
//! The parser's recursion budget does not bound tree height: operator, postfix
//! and statement-level chains are parsed iteratively, and inference lowers flat
//! syntax such as template literals into nested operations. Every later compiler
//! walk recurses over these trees, so their height is measured iteratively and
//! rejected before a recursive phase can consume them.

use crate::ast::{
    ArrowBody, ChainPart, ClassMember, ExprKind, InterfaceMember, ParamDecl, StmtKind,
    TypeAnnotation, TypeAnnotationKind,
};
use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::typed_ast::{
    ClosureBody, PostfixTarget, TypedChainPart, TypedExprKind, TypedObjectFieldSource,
    TypedStmtKind,
};
use std::ops::Range;

use crate::{Ast, ExprId, Span, StmtId, TypedAst};

/// Height of parsed syntax, counting expressions, statements and type annotations.
pub const MAX_SYNTAX_HEIGHT: u32 = 256;

/// Height of the typed tree after inference and lowering, which may add wrapper
/// nodes and expand flat syntax such as template literal substitutions.
pub const MAX_TYPED_HEIGHT: u32 = 1024;

/// Rejects parsed syntax whose recursive walks could exhaust the compiler stack.
pub fn check_syntax(ast: &Ast) -> Result<(), CompilerFailure> {
    measure(
        &SyntaxTree(ast),
        MAX_SYNTAX_HEIGHT,
        CompilerStage::Parse,
        |span| CompilerFailure::Limit {
            stage: CompilerStage::Parse,
            span: Some(span),
            message: format!(
                "syntax nesting exceeds the compiler limit of {MAX_SYNTAX_HEIGHT} levels"
            ),
            help: vec![
                "split long operator chains or deeply nested code into intermediate declarations"
                    .into(),
            ],
        },
    )
}

/// Rejects a typed tree whose recursive walks could exhaust the compiler stack.
pub fn check_typed(ta: &TypedAst, stage: CompilerStage) -> Result<(), CompilerFailure> {
    measure(&TypedTree(ta), MAX_TYPED_HEIGHT, stage, |span| {
        CompilerFailure::Limit {
            stage,
            span: Some(span),
            message: format!(
                "lowered expression nesting exceeds the compiler limit of {MAX_TYPED_HEIGHT} levels"
            ),
            help: vec![
                "split long operator chains, template literals or spreads into intermediate declarations"
                    .into(),
            ],
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    Expr(ExprId),
    Stmt(StmtId),
}

/// A node's children in the arena plus its owned height: levels the compiler
/// recurses through that have no arena node of their own. These are type
/// annotations, spread fallback links, and optional-chain links, which are
/// flat in the tree but nested by code generation.
struct NodeInfo {
    span: Span,
    children: Vec<Child>,
    owned_height: u32,
}

/// `offset` counts the owned levels, such as spread fallback or optional-chain
/// links, that the compiler recurses through before reaching the child.
#[derive(Clone, Copy)]
struct Child {
    node: Node,
    offset: u32,
}

impl Child {
    fn direct(node: Node) -> Self {
        Self { node, offset: 0 }
    }
}

trait Tree {
    fn expr_ids(&self) -> Result<Range<u32>, CompilerFailure>;
    fn stmt_ids(&self) -> Result<Range<u32>, CompilerFailure>;
    fn node(&self, node: Node) -> Result<NodeInfo, CompilerFailure>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Measured {
    Unvisited,
    InProgress,
    Done {
        height: u32,
        /// The tallest child counting its offset. The node's owned levels
        /// may still be taller.
        deepest: Option<Node>,
        owned_height: u32,
    },
}

struct Frame {
    node: Node,
    info: NodeInfo,
    next_child: usize,
    /// The child being measured on the frame above this one.
    pending: Option<Child>,
    child_height: u32,
    /// The tallest child so far, counting its offset. The first of equally
    /// tall children is kept, so a report starts at the earlier one.
    deepest: Option<Node>,
}

impl Frame {
    fn include(&mut self, child: Child, height: u32) {
        let height = height.saturating_add(child.offset);
        if self.deepest.is_none() || height > self.child_height {
            self.child_height = height;
            self.deepest = Some(child.node);
        }
    }
}

struct Heights {
    exprs: Vec<Measured>,
    stmts: Vec<Measured>,
}

impl Heights {
    fn get(&self, node: Node) -> Option<Measured> {
        match node {
            Node::Expr(id) => self.exprs.get(id.0 as usize).copied(),
            Node::Stmt(id) => self.stmts.get(id.0 as usize).copied(),
        }
    }

    fn slot(&mut self, node: Node) -> Option<&mut Measured> {
        match node {
            Node::Expr(id) => self.exprs.get_mut(id.0 as usize),
            Node::Stmt(id) => self.stmts.get_mut(id.0 as usize),
        }
    }
}

/// Computes each node's height once with an explicit stack, so measuring a
/// too-deep tree cannot itself recurse. Shared children are visited once.
fn measure(
    tree: &impl Tree,
    limit: u32,
    stage: CompilerStage,
    exceeded: impl Fn(Span) -> CompilerFailure,
) -> Result<(), CompilerFailure> {
    let expr_ids = tree.expr_ids().map_err(|error| error.with_stage(stage))?;
    let stmt_ids = tree.stmt_ids().map_err(|error| error.with_stage(stage))?;
    let mut heights = Heights {
        exprs: vec![Measured::Unvisited; expr_ids.len()],
        stmts: vec![Measured::Unvisited; stmt_ids.len()],
    };
    let roots = expr_ids
        .map(|id| Node::Expr(ExprId(id)))
        .chain(stmt_ids.map(|id| Node::Stmt(StmtId(id))));
    // Measuring continues past the first node over the limit, which is often
    // deep inside the expression a user has to split, so the report can start
    // from the tallest node with its whole path measured. A parent is taller
    // than its children, so the tallest node is a root; the first of equally
    // tall roots is reported.
    let mut tallest: Option<(Node, u32)> = None;
    for root in roots {
        if heights.get(root) != Some(Measured::Unvisited) {
            continue;
        }
        measure_from(tree, root, &mut heights, stage)?;
        if let Some(Measured::Done { height, .. }) = heights.get(root)
            && height > limit
            && tallest.is_none_or(|(_, tallest_height)| height > tallest_height)
        {
            tallest = Some((root, height));
        }
    }
    match tallest {
        Some((node, _)) => Err(exceeded(reported_span(tree, &heights, node, stage)?)),
        None => Ok(()),
    }
}

fn measure_from(
    tree: &impl Tree,
    root: Node,
    heights: &mut Heights,
    stage: CompilerStage,
) -> Result<(), CompilerFailure> {
    let mut stack = vec![enter(tree, root, heights, stage)?];
    while let Some(frame) = stack.last_mut() {
        if let Some(&child) = frame.info.children.get(frame.next_child) {
            frame.next_child += 1;
            match heights.get(child.node) {
                // An out-of-range child fails in the arena lookup with its ID.
                Some(Measured::Unvisited) | None => {
                    let child_frame = enter(tree, child.node, heights, stage)?;
                    frame.pending = Some(child);
                    stack.push(child_frame);
                }
                Some(Measured::InProgress) => {
                    return Err(internal(stage, "tree node is its own descendant"));
                }
                Some(Measured::Done { height, .. }) => frame.include(child, height),
            }
            continue;
        }
        let Some(frame) = stack.pop() else {
            break;
        };
        let height = frame
            .child_height
            .max(frame.info.owned_height)
            .saturating_add(1);
        let Some(stored) = heights.slot(frame.node) else {
            return Err(internal(stage, "tree node is outside its arena"));
        };
        *stored = Measured::Done {
            height,
            deepest: frame.deepest,
            owned_height: frame.info.owned_height,
        };
        if let Some(parent) = stack.last_mut() {
            let child = parent
                .pending
                .take()
                .ok_or_else(|| internal(stage, "tree walk lost its parent's child"))?;
            parent.include(child, height);
        }
    }
    Ok(())
}

/// The tallest node over the limit is often a statement or closure that only
/// wraps the construct that made the tree tall. Along its deepest path, an
/// expression with no segment open (the start of the path, or the first
/// expression below a statement) opens a segment of levels that ends at the
/// next statement. An expression whose owned levels, such as optional-chain
/// links, decide its height ends the segment there with those levels counted,
/// and the expression below it opens the next. The expression with the longest
/// segment is what a user has to split; ties go to the outermost, and with no
/// expression on the path the node over the limit is reported. The span marks
/// where that expression starts, since a tall one can span many lines.
fn reported_span(
    tree: &impl Tree,
    heights: &Heights,
    over_limit: Node,
    stage: CompilerStage,
) -> Result<Span, CompilerFailure> {
    let span_of = |node| {
        tree.node(node)
            .map(|info| info.span)
            .map_err(|error| error.with_stage(stage))
    };
    let mut longest = Longest {
        span: span_of(over_limit)?,
        levels: 0,
    };
    let mut segment = None;
    let mut next = Some(over_limit);
    // The path ends at a node without children: a statement, which closes the
    // open segment, or an expression, whose owned levels decide its height.
    while let Some(node) = next {
        let Some(Measured::Done {
            height,
            deepest,
            owned_height,
        }) = heights.get(node)
        else {
            return Err(internal(
                stage,
                "a node on the deepest path was not measured",
            ));
        };
        segment = match (node, segment) {
            (Node::Stmt(_), Some(open)) => {
                longest.offer(open, height);
                None
            }
            (Node::Stmt(_), None) => None,
            (Node::Expr(_), open) => {
                let open = match open {
                    Some(open) => open,
                    None => Segment {
                        expression: span_of(node)?,
                        height,
                    },
                };
                if owns_its_height(height, owned_height) {
                    longest.offer(open, 0);
                    None
                } else {
                    Some(open)
                }
            }
        };
        next = deepest;
    }
    Ok(Span {
        end: longest.span.start,
        ..longest.span
    })
}

/// Whether a node's owned levels are at least as tall as each of its children
/// (counting offsets), so they alone account for its height.
fn owns_its_height(height: u32, owned_height: u32) -> bool {
    height == owned_height.saturating_add(1)
}

#[derive(Clone, Copy)]
struct Segment {
    expression: Span,
    height: u32,
}

struct Longest {
    span: Span,
    levels: u32,
}

impl Longest {
    /// Offers a segment ending at `end_height`. Outer segments are offered
    /// first, so a tie keeps the outer one.
    fn offer(&mut self, segment: Segment, end_height: u32) {
        let levels = segment.height.saturating_sub(end_height);
        if levels > self.levels {
            self.span = segment.expression;
            self.levels = levels;
        }
    }
}

fn enter(
    tree: &impl Tree,
    node: Node,
    heights: &mut Heights,
    stage: CompilerStage,
) -> Result<Frame, CompilerFailure> {
    let info = tree.node(node).map_err(|error| error.with_stage(stage))?;
    let Some(stored) = heights.slot(node) else {
        return Err(internal(stage, "tree node is outside its arena"));
    };
    *stored = Measured::InProgress;
    Ok(Frame {
        node,
        info,
        next_child: 0,
        pending: None,
        child_height: 0,
        deepest: None,
    })
}

/// Arena ID iterators are dense ranges starting at zero.
fn id_range(mut ids: impl DoubleEndedIterator<Item = u32>) -> Range<u32> {
    let end = ids.next_back().map_or(0, |last| last.saturating_add(1));
    0..end
}

fn syntax_arena_error(error: crate::arena::ArenaError) -> CompilerFailure {
    error.into_compiler_failure(CompilerStage::Parse)
}

fn typed_arena_error(error: crate::arena::ArenaError) -> CompilerFailure {
    error.into_compiler_failure(CompilerStage::Infer)
}

fn internal(stage: CompilerStage, message: &str) -> CompilerFailure {
    CompilerFailure::Internal {
        stage,
        span: None,
        message: message.into(),
    }
}

struct SyntaxTree<'a>(&'a Ast);

impl Tree for SyntaxTree<'_> {
    fn expr_ids(&self) -> Result<Range<u32>, CompilerFailure> {
        let ids = self.0.expr_ids().map_err(syntax_arena_error)?;
        Ok(id_range(ids.map(|id| id.0)))
    }

    fn stmt_ids(&self) -> Result<Range<u32>, CompilerFailure> {
        let ids = self.0.stmt_ids().map_err(syntax_arena_error)?;
        Ok(id_range(ids.map(|id| id.0)))
    }

    fn node(&self, node: Node) -> Result<NodeInfo, CompilerFailure> {
        let mut children = SyntaxChildren::default();
        let span = match node {
            Node::Expr(id) => {
                let expr = self.0.try_expr(id).map_err(syntax_arena_error)?;
                children.expr_kind(&expr.kind);
                expr.span
            }
            Node::Stmt(id) => {
                let stmt = self.0.try_stmt(id).map_err(syntax_arena_error)?;
                children.stmt_kind(&stmt.kind);
                stmt.span
            }
        };
        if let Some(failure) = children.failure {
            return Err(failure.with_span(span));
        }
        Ok(NodeInfo {
            span,
            children: children
                .nodes
                .into_iter()
                .map(Child::direct)
                .chain(children.behind_owned)
                .collect(),
            owned_height: children.owned_height,
        })
    }
}

#[derive(Default)]
struct SyntaxChildren {
    failure: Option<CompilerFailure>,
    nodes: Vec<Node>,
    /// Children reached through owned levels, measured with that offset.
    behind_owned: Vec<Child>,
    owned_height: u32,
}

impl SyntaxChildren {
    fn expr(&mut self, id: ExprId) {
        self.nodes.push(Node::Expr(id));
    }

    fn exprs(&mut self, ids: &[ExprId]) {
        self.nodes.extend(ids.iter().copied().map(Node::Expr));
    }

    fn stmt(&mut self, id: StmtId) {
        self.nodes.push(Node::Stmt(id));
    }

    fn annotation(&mut self, ty: &TypeAnnotation) {
        self.annotation_at(ty, 0);
    }

    fn annotation_at(&mut self, ty: &TypeAnnotation, offset: u32) {
        if self.failure.is_some() {
            return;
        }
        match annotation_height(ty) {
            Ok(height) => self.owned_height = self.owned_height.max(height.saturating_add(offset)),
            Err(failure) => self.failure = Some(failure),
        }
    }

    fn annotations<'a>(&mut self, types: impl IntoIterator<Item = &'a TypeAnnotation>) {
        for ty in types {
            self.annotation(ty);
        }
    }

    fn params(&mut self, params: &[ParamDecl]) {
        for param in params {
            self.annotations(&param.ty);
            self.nodes.extend(param.default.map(Node::Expr));
        }
    }

    /// Code generation nests each chain link inside the previous one, so the
    /// link at depth `n` and the operands it owns sit `n` levels down.
    fn chain_parts(&mut self, parts: &[ChainPart]) {
        let mut depth = 0u32;
        for part in parts {
            depth = depth.saturating_add(1);
            self.owned_height = self.owned_height.max(depth);
            match part {
                ChainPart::Field { .. } | ChainPart::NonNull { .. } => {}
                ChainPart::Index { idx, .. } => self.expr_at(*idx, depth),
                ChainPart::Call {
                    args, type_args, ..
                } => {
                    for arg in args {
                        self.expr_at(*arg, depth);
                    }
                    for ty in type_args.iter().flatten() {
                        self.annotation_at(ty, depth);
                    }
                }
            }
        }
    }

    fn expr_at(&mut self, id: ExprId, offset: u32) {
        self.behind_owned.push(Child {
            node: Node::Expr(id),
            offset,
        });
    }

    fn expr_kind(&mut self, kind: &ExprKind) {
        match kind {
            ExprKind::Number(_)
            | ExprKind::BigInt(_)
            | ExprKind::String(_)
            | ExprKind::Boolean(_)
            | ExprKind::Null
            | ExprKind::Identifier(_)
            | ExprKind::This
            | ExprKind::ThisOutsideReceiver
            | ExprKind::Super
            | ExprKind::Regex { .. } => {}
            ExprKind::Binary { lhs, rhs, .. } => self.exprs(&[*lhs, *rhs]),
            ExprKind::Unary { operand, .. }
            | ExprKind::Paren(operand)
            | ExprKind::Typeof { operand }
            | ExprKind::Delete { operand }
            | ExprKind::PostfixUnary { operand, .. } => self.expr(*operand),
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
                self.expr(*callee);
                self.annotations(type_args.iter().flatten());
                self.exprs(args);
            }
            ExprKind::ObjectLiteral { members } => {
                for member in members {
                    self.nodes.extend(member.expressions().map(Node::Expr));
                }
            }
            ExprKind::ArrayLiteral { elements } => {
                self.nodes
                    .extend(elements.iter().map(|element| Node::Expr(element.value())));
            }
            ExprKind::FieldAccess { receiver, .. } => self.expr(*receiver),
            ExprKind::IndexAccess { receiver, index } => self.exprs(&[*receiver, *index]),
            ExprKind::FunctionExpression {
                this_type,
                function,
                ..
            } => {
                self.annotations(this_type);
                self.expr(*function);
            }
            ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body,
            } => {
                self.params(params);
                self.annotations(return_type);
                self.annotations(type_predicate.iter().map(|predicate| &predicate.asserted));
                match body {
                    ArrowBody::Expr(id) => self.expr(*id),
                    ArrowBody::Block(id) => self.stmt(*id),
                }
            }
            ExprKind::TemplateLiteral { exprs, .. } => self.exprs(exprs),
            ExprKind::Ternary { cond, then_, else_ } => self.exprs(&[*cond, *then_, *else_]),
            ExprKind::OptionalChain { base, parts } => {
                self.expr(*base);
                self.chain_parts(parts);
            }
            ExprKind::Assign { target, value, .. } => self.exprs(&[*target, *value]),
            ExprKind::As { expr, ty } => {
                self.expr(*expr);
                self.annotation(ty);
            }
            ExprKind::InstanceOf { value, ty } => {
                self.expr(*value);
                self.annotation(ty);
            }
        }
    }

    fn stmt_kind(&mut self, kind: &StmtKind) {
        match kind {
            StmtKind::Let { ty, value, .. }
            | StmtKind::Const { ty, value, .. }
            | StmtKind::LetPattern { ty, value, .. }
            | StmtKind::ConstPattern { ty, value, .. }
            | StmtKind::ObjectRest {
                ty, source: value, ..
            } => {
                self.annotations(ty);
                self.expr(*value);
            }
            StmtKind::Function {
                params,
                return_type,
                type_predicate,
                body,
                ..
            } => {
                self.params(params);
                self.annotations(return_type);
                self.annotations(type_predicate.iter().map(|predicate| &predicate.asserted));
                self.stmt(*body);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(*condition);
                self.stmt(*then_block);
                self.nodes.extend(else_block.map(Node::Stmt));
            }
            StmtKind::While { condition, body } | StmtKind::DoWhile { body, condition } => {
                self.expr(*condition);
                self.stmt(*body);
            }
            StmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.nodes.extend(init.map(Node::Stmt));
                self.nodes.extend(condition.map(Node::Expr));
                self.nodes.extend(update.map(Node::Stmt));
                self.stmt(*body);
            }
            StmtKind::ForOf { ty, iter, body, .. }
            | StmtKind::ForOfPattern { ty, iter, body, .. } => {
                self.annotations(ty);
                self.expr(*iter);
                self.stmt(*body);
            }
            StmtKind::Switch {
                discriminant,
                cases,
                default,
            } => {
                self.expr(*discriminant);
                for case in cases {
                    self.exprs(&case.values);
                    self.stmt(case.body);
                }
                self.nodes
                    .extend(default.iter().map(|default| Node::Stmt(default.body)));
            }
            StmtKind::Break
            | StmtKind::Continue
            | StmtKind::EnumDecl { .. }
            | StmtKind::Import { .. }
            | StmtKind::ExportFrom { .. } => {}
            StmtKind::Return(value) => self.nodes.extend(value.map(Node::Expr)),
            StmtKind::Throw { value } | StmtKind::Expr(value) => self.expr(*value),
            StmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.stmt(*body);
                for catch in catches {
                    self.annotations(&catch.ty);
                    self.stmt(catch.body);
                }
                self.nodes.extend(finally.map(Node::Stmt));
            }
            StmtKind::Block(stmts) => self.nodes.extend(stmts.iter().copied().map(Node::Stmt)),
            StmtKind::Assign { value, .. } | StmtKind::CompoundAssign { value, .. } => {
                self.expr(*value);
            }
            StmtKind::AssignField {
                receiver, value, ..
            }
            | StmtKind::CompoundAssignField {
                receiver, value, ..
            } => self.exprs(&[*receiver, *value]),
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
            } => self.exprs(&[*receiver, *index, *value]),
            StmtKind::InterfaceDecl {
                extends, members, ..
            } => {
                self.annotations(extends);
                for member in members {
                    match member {
                        InterfaceMember::IndexSignature(signature) => {
                            self.annotation(&signature.value);
                        }
                        InterfaceMember::Method {
                            params,
                            return_type,
                            ..
                        } => {
                            self.params(params);
                            self.annotation(return_type);
                        }
                        InterfaceMember::Property { ty, .. } => self.annotation(ty),
                    }
                }
            }
            StmtKind::ClassDecl {
                extends,
                implements,
                members,
                ..
            } => {
                self.annotations(extends);
                self.annotations(implements);
                for member in members {
                    self.class_member(member);
                }
            }
            StmtKind::TypeAliasDecl { ty, .. } => self.annotation(ty),
        }
    }

    fn class_member(&mut self, member: &ClassMember) {
        match member {
            ClassMember::Field {
                ty, initializer, ..
            } => {
                self.annotation(ty);
                self.nodes.extend(initializer.map(Node::Expr));
            }
            ClassMember::Method {
                params,
                return_type,
                body,
                ..
            } => {
                self.params(params);
                self.annotation(return_type);
                self.stmt(*body);
            }
            ClassMember::Constructor { params, body, .. } => {
                self.params(params);
                self.stmt(*body);
            }
            ClassMember::Accessor {
                param,
                return_type,
                body,
                ..
            } => {
                self.params(param.as_deref().map_or(&[], std::slice::from_ref));
                self.annotations(return_type);
                self.stmt(*body);
            }
        }
    }
}

/// Annotations are boxed values, measured with an explicit stack for the same
/// reason as arena nodes.
fn annotation_height(root: &TypeAnnotation) -> Result<u32, CompilerFailure> {
    measure_annotation(root, u64::MAX, u32::MAX).map(|extent| extent.depth)
}

/// Nodes in `root`, counted without recursion; stops once past `max_nodes`.
pub(crate) fn annotation_nodes(
    root: &TypeAnnotation,
    max_nodes: u64,
) -> Result<u64, CompilerFailure> {
    measure_annotation(root, max_nodes, u32::MAX).map(|extent| extent.nodes)
}

fn measure_annotation(
    root: &TypeAnnotation,
    max_nodes: u64,
    max_depth: u32,
) -> Result<crate::type_size::TypeExtent, CompilerFailure> {
    crate::type_walk::measure(root, max_nodes, max_depth, AnnotationChildren::new).map_err(|_| {
        CompilerFailure::Internal {
            stage: CompilerStage::Parse,
            span: Some(root.span),
            message: "could not allocate annotation traversal frames".into(),
        }
    })
}

struct AnnotationChildren<'a> {
    members: std::slice::Iter<'a, TypeAnnotation>,
    fields: std::slice::Iter<'a, crate::ast::TypeAnnotationField>,
    trailing: Option<&'a TypeAnnotation>,
}

impl<'a> AnnotationChildren<'a> {
    fn new(ty: &'a TypeAnnotation) -> Self {
        let mut children = Self {
            members: [].iter(),
            fields: [].iter(),
            trailing: None,
        };
        match &ty.kind {
            TypeAnnotationKind::Name { args, .. }
            | TypeAnnotationKind::Qualified { args, .. }
            | TypeAnnotationKind::Tuple(args)
            | TypeAnnotationKind::Union(args) => {
                children.members = args.iter();
            }
            TypeAnnotationKind::StringLiteral(_)
            | TypeAnnotationKind::NumberLiteral(_)
            | TypeAnnotationKind::BigIntLiteral(_)
            | TypeAnnotationKind::BooleanLiteral(_)
            | TypeAnnotationKind::TypeOf { .. } => {}
            TypeAnnotationKind::Array(inner)
            | TypeAnnotationKind::Readonly(inner)
            | TypeAnnotationKind::KeyOf(inner) => children.trailing = Some(inner),
            TypeAnnotationKind::Object { fields, index } => {
                children.fields = fields.iter();
                children.trailing = index.as_ref().map(|index| &index.value);
            }
            TypeAnnotationKind::Function {
                params,
                return_type,
            } => {
                children.fields = params.iter();
                children.trailing = Some(return_type);
            }
        }
        children
    }
}

impl<'a> Iterator for AnnotationChildren<'a> {
    type Item = &'a TypeAnnotation;

    fn next(&mut self) -> Option<Self::Item> {
        self.trailing
            .take()
            .or_else(|| self.fields.next_back().map(|field| &field.ty))
            .or_else(|| self.members.next_back())
    }
}

struct TypedTree<'a>(&'a TypedAst);

impl Tree for TypedTree<'_> {
    fn expr_ids(&self) -> Result<Range<u32>, CompilerFailure> {
        let ids = self.0.expr_ids().map_err(typed_arena_error)?;
        Ok(id_range(ids.map(|id| id.0)))
    }

    fn stmt_ids(&self) -> Result<Range<u32>, CompilerFailure> {
        let ids = self.0.stmt_ids().map_err(typed_arena_error)?;
        Ok(id_range(ids.map(|id| id.0)))
    }

    fn node(&self, node: Node) -> Result<NodeInfo, CompilerFailure> {
        let mut children = TypedChildren::default();
        let span = match node {
            Node::Expr(id) => {
                let expr = self.0.try_expr(id).map_err(typed_arena_error)?;
                children.expr_kind(&expr.kind);
                expr.span
            }
            Node::Stmt(id) => {
                let stmt = self.0.try_stmt(id).map_err(typed_arena_error)?;
                children.stmt_kind(&stmt.kind);
                stmt.span
            }
        };
        Ok(NodeInfo {
            span,
            children: children
                .nodes
                .into_iter()
                .map(Child::direct)
                .chain(children.behind_owned)
                .collect(),
            owned_height: children.owned_height,
        })
    }
}

#[derive(Default)]
struct TypedChildren {
    nodes: Vec<Node>,
    /// Children reached through owned levels, measured with that offset.
    behind_owned: Vec<Child>,
    owned_height: u32,
}

impl TypedChildren {
    fn expr(&mut self, id: ExprId) {
        self.nodes.push(Node::Expr(id));
    }

    fn exprs(&mut self, ids: &[ExprId]) {
        self.nodes.extend(ids.iter().copied().map(Node::Expr));
    }

    fn stmt(&mut self, id: StmtId) {
        self.nodes.push(Node::Stmt(id));
    }

    /// Code generation nests each chain link inside the previous one, so the
    /// link at depth `n` and the operands it owns sit `n` levels down.
    fn chain_parts(&mut self, parts: &[TypedChainPart]) {
        let mut depth = 0u32;
        for part in parts {
            depth = depth.saturating_add(1);
            self.owned_height = self.owned_height.max(depth);
            match part {
                TypedChainPart::Field { .. }
                | TypedChainPart::InterfaceProperty { .. }
                | TypedChainPart::NonNull { .. } => {}
                TypedChainPart::Index { idx, .. } => self.expr_at(*idx, depth),
                TypedChainPart::Call { args, .. } | TypedChainPart::MethodCall { args, .. } => {
                    for arg in args {
                        self.expr_at(*arg, depth);
                    }
                }
            }
        }
    }

    fn expr_at(&mut self, id: ExprId, offset: u32) {
        self.behind_owned.push(Child {
            node: Node::Expr(id),
            offset,
        });
    }

    /// Spread fallbacks form a boxed chain inside one object literal field; a
    /// walk descends the chain before reaching its terminal expression.
    fn field_source(&mut self, source: &TypedObjectFieldSource) {
        let mut chain_height = 0u32;
        let mut next = Some(source);
        while let Some(source) = next {
            chain_height = chain_height.saturating_add(1);
            next = match source {
                TypedObjectFieldSource::Literal(id) | TypedObjectFieldSource::Absent(id) => {
                    self.expr_at(*id, chain_height);
                    None
                }
                TypedObjectFieldSource::Spread { fallback, .. } => fallback.as_deref(),
            };
        }
        self.owned_height = self.owned_height.max(chain_height);
    }

    fn expr_kind(&mut self, kind: &TypedExprKind) {
        match kind {
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
            TypedExprKind::EffectThen { effect, result } => self.exprs(&[*effect, *result]),
            TypedExprKind::Sequence { stmts, result } => {
                self.nodes.extend(stmts.iter().copied().map(Node::Stmt));
                self.expr(*result);
            }
            TypedExprKind::Binary { lhs, rhs, .. }
            | TypedExprKind::NullishCoalesce { lhs, rhs } => self.exprs(&[*lhs, *rhs]),
            TypedExprKind::Unary { operand, .. } => self.expr(*operand),
            TypedExprKind::Call { args, .. }
            | TypedExprKind::McpCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. }
            | TypedExprKind::SuperCtorCall { args, .. }
            | TypedExprKind::SuperMethodCall { args, .. } => self.exprs(args),
            TypedExprKind::CallClosure { callee, args } => {
                self.expr(*callee);
                self.exprs(args);
            }
            TypedExprKind::GenericCall { args, .. } => {
                self.nodes
                    .extend(args.iter().map(|argument| Node::Expr(argument.expr)));
            }
            TypedExprKind::MethodCall { receiver, args, .. } => {
                self.expr(*receiver);
                self.exprs(args);
            }
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                self.expr(*receiver);
                self.nodes
                    .extend(args.iter().map(|argument| Node::Expr(argument.expr)));
            }
            TypedExprKind::ObjectLiteral { members, fields } => {
                for member in members {
                    self.nodes.extend(member.expressions().map(Node::Expr));
                }
                for field in fields {
                    self.field_source(&field.source);
                }
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                self.nodes
                    .extend(elements.iter().map(|element| Node::Expr(element.expr_id())));
            }
            TypedExprKind::TupleLiteral { elements, .. } => self.exprs(elements),
            TypedExprKind::FieldAccess { receiver, .. }
            | TypedExprKind::InterfacePropertyAccess { receiver, .. } => self.expr(*receiver),
            TypedExprKind::IndexAccess { receiver, index } => self.exprs(&[*receiver, *index]),
            TypedExprKind::Closure { body, .. } => match body {
                ClosureBody::Expr(id) => self.expr(*id),
                ClosureBody::Block(id) => self.stmt(*id),
            },
            TypedExprKind::TypeofTag { value, .. }
            | TypedExprKind::NonNullAssert { value }
            | TypedExprKind::Cast { value, .. }
            | TypedExprKind::InstanceOf { value, .. } => self.expr(*value),
            TypedExprKind::Narrowed { source, inner, .. } => self.exprs(&[*source, *inner]),
            TypedExprKind::Ternary { cond, then_, else_ } => self.exprs(&[*cond, *then_, *else_]),
            TypedExprKind::OptionalChain { base, parts } => {
                self.expr(*base);
                self.chain_parts(parts);
            }
            TypedExprKind::PostfixUnary { target, .. } => match target {
                PostfixTarget::Local { .. } | PostfixTarget::Global { .. } => {}
                PostfixTarget::Field { receiver, .. } => self.expr(*receiver),
                PostfixTarget::Index {
                    receiver, index, ..
                } => self.exprs(&[*receiver, *index]),
            },
        }
    }

    fn stmt_kind(&mut self, kind: &TypedStmtKind) {
        match kind {
            TypedStmtKind::Let { value, .. }
            | TypedStmtKind::Const { value, .. }
            | TypedStmtKind::Throw { value }
            | TypedStmtKind::Expr(value)
            | TypedStmtKind::AssignLocal { value, .. }
            | TypedStmtKind::AssignGlobal { value, .. } => self.expr(*value),
            TypedStmtKind::ReboxLocal { .. } | TypedStmtKind::Break | TypedStmtKind::Continue => {}
            TypedStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr(*condition);
                self.stmt(*then_block);
                self.nodes.extend(else_block.map(Node::Stmt));
            }
            TypedStmtKind::While { condition, body }
            | TypedStmtKind::DoWhile { body, condition } => {
                self.expr(*condition);
                self.stmt(*body);
            }
            TypedStmtKind::For {
                init,
                condition,
                update,
                body,
            } => {
                self.nodes.extend(init.map(Node::Stmt));
                self.nodes.extend(condition.map(Node::Expr));
                self.nodes.extend(update.map(Node::Stmt));
                self.stmt(*body);
            }
            TypedStmtKind::ForOf { iter, body, .. } => {
                self.expr(*iter);
                self.stmt(*body);
            }
            TypedStmtKind::Switch {
                discriminant,
                cases,
                default,
                ..
            } => {
                self.expr(*discriminant);
                self.nodes.extend(
                    cases
                        .iter()
                        .flat_map(crate::TypedSwitchCase::label_comparisons)
                        .map(Node::Expr),
                );
                self.nodes
                    .extend(cases.iter().map(|case| Node::Stmt(case.body)));
                self.nodes.extend(default.map(Node::Stmt));
            }
            TypedStmtKind::Return(value) => self.nodes.extend(value.map(Node::Expr)),
            TypedStmtKind::Try {
                body,
                catches,
                finally,
            } => {
                self.stmt(*body);
                self.nodes
                    .extend(catches.iter().map(|catch| Node::Stmt(catch.body)));
                self.nodes.extend(finally.map(Node::Stmt));
            }
            TypedStmtKind::Block(stmts) => self.nodes.extend(stmts.iter().copied().map(Node::Stmt)),
            TypedStmtKind::AssignField {
                receiver, value, ..
            } => self.exprs(&[*receiver, *value]),
            TypedStmtKind::AssignIndex {
                receiver,
                index,
                value,
                ..
            } => self.exprs(&[*receiver, *index, *value]),
            TypedStmtKind::NarrowRegion { source, body, .. } => {
                self.expr(*source);
                self.stmt(*body);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::parse_script;
    use crate::{Expr, FileId, Type, TypedExpr};

    fn annotation(kind: TypeAnnotationKind) -> TypeAnnotation {
        TypeAnnotation {
            kind,
            span: Span::at(FileId(3)),
        }
    }

    fn annotation_field(ty: TypeAnnotation) -> crate::ast::TypeAnnotationField {
        crate::ast::TypeAnnotationField {
            name: crate::Ident {
                name: "field".into(),
                span: ty.span,
            },
            ty,
            optional: false,
            readonly: false,
            rest: false,
            method: false,
        }
    }

    #[test]
    fn wide_annotations_use_only_ancestor_frames() {
        let root = annotation(TypeAnnotationKind::Tuple(vec![
            annotation(
                TypeAnnotationKind::BooleanLiteral(true)
            );
            100_000
        ]));
        for budget in [0, 1, 7, 100_001] {
            let (nodes, observed) =
                crate::type_walk::tests::observe(None, || annotation_nodes(&root, budget).unwrap());
            assert_eq!(nodes, 100_001.min(budget + 1));
            assert!(observed.peak_frames <= 1);
            assert!(observed.reservations <= 1);
        }
        let (height, observed) =
            crate::type_walk::tests::observe(None, || annotation_height(&root).unwrap());
        assert_eq!(height, 2);
        assert_eq!(observed.peak_frames, 1);
    }

    #[test]
    fn mixed_annotations_keep_depth_counts_and_lifo_order() {
        let leaf = annotation(TypeAnnotationKind::BooleanLiteral(true));
        let deep = (1..20).fold(leaf.clone(), |inner, _| {
            annotation(TypeAnnotationKind::Array(Box::new(inner)))
        });
        let root = annotation(TypeAnnotationKind::Tuple(vec![
            annotation(TypeAnnotationKind::Tuple(vec![leaf; 100_000])),
            deep,
        ]));
        let (extent, observed) = crate::type_walk::tests::observe(None, || {
            measure_annotation(&root, u64::MAX, u32::MAX).unwrap()
        });
        assert_eq!(
            extent,
            crate::type_size::TypeExtent {
                nodes: 100_022,
                depth: 21
            }
        );
        assert_eq!(observed.peak_frames, 20);
        assert_eq!(
            measure_annotation(&root, 3, u32::MAX).unwrap(),
            crate::type_size::TypeExtent { nodes: 4, depth: 4 }
        );
        let (failure, _) = crate::type_walk::tests::observe(Some(1), || annotation_height(&root));
        assert!(matches!(failure, Err(CompilerFailure::Internal { .. })));
    }

    #[test]
    fn annotation_variants_preserve_children_and_measurement() {
        let first = annotation(TypeAnnotationKind::StringLiteral("first".into()));
        let second = annotation(TypeAnnotationKind::BooleanLiteral(false));
        let pair = vec![first.clone(), second.clone()];
        let name = crate::Ident {
            name: "T".into(),
            span: first.span,
        };
        let cases = vec![
            (
                TypeAnnotationKind::Name {
                    name: name.clone(),
                    args: pair.clone(),
                },
                pair.clone(),
            ),
            (
                TypeAnnotationKind::Qualified {
                    path: vec![name.clone(), name],
                    args: pair.clone(),
                },
                pair.clone(),
            ),
            (TypeAnnotationKind::Tuple(pair.clone()), pair.clone()),
            (TypeAnnotationKind::Union(pair.clone()), pair.clone()),
            (
                TypeAnnotationKind::Array(Box::new(first.clone())),
                vec![first.clone()],
            ),
            (
                TypeAnnotationKind::Readonly(Box::new(first.clone())),
                vec![first.clone()],
            ),
            (
                TypeAnnotationKind::KeyOf(Box::new(first.clone())),
                vec![first.clone()],
            ),
            (
                TypeAnnotationKind::Object {
                    fields: vec![annotation_field(first.clone())],
                    index: Some(Box::new(crate::IndexSignatureAnnotation {
                        value: second.clone(),
                        readonly: false,
                        span: second.span,
                    })),
                },
                pair.clone(),
            ),
            (
                TypeAnnotationKind::Function {
                    params: vec![annotation_field(first.clone())],
                    return_type: Box::new(second.clone()),
                },
                pair,
            ),
            (TypeAnnotationKind::TypeOf { path: Vec::new() }, Vec::new()),
            (TypeAnnotationKind::StringLiteral("leaf".into()), Vec::new()),
            (
                TypeAnnotationKind::NumberLiteral(crate::types::LiteralF64(1.0)),
                Vec::new(),
            ),
            (TypeAnnotationKind::BooleanLiteral(true), Vec::new()),
        ];
        for (kind, mut expected) in cases {
            let root = annotation(kind);
            let nodes = expected.len() as u64 + 1;
            let height = if expected.is_empty() { 1 } else { 2 };
            expected.reverse();
            assert_eq!(
                AnnotationChildren::new(&root).cloned().collect::<Vec<_>>(),
                expected
            );
            assert_eq!(annotation_nodes(&root, u64::MAX).unwrap(), nodes);
            assert_eq!(annotation_height(&root).unwrap(), height);
        }
    }

    #[test]
    fn annotation_failure_terminates_syntax_validation() {
        let root = annotation(TypeAnnotationKind::Array(Box::new(annotation(
            TypeAnnotationKind::BooleanLiteral(true),
        ))));
        let (count, _) = crate::type_walk::tests::observe(Some(0), || annotation_nodes(&root, 10));
        assert!(
            matches!(count, Err(CompilerFailure::Internal { span: Some(s), .. }) if s == root.span)
        );
        // Parse before injection, so the assertion targets the public syntax check.
        let source = "type T = [number, string];";
        let mut lexer = crate::asi::Asi::new(source, FileId(3));
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token();
            let eof = matches!(token.kind, crate::TokenKind::Eof);
            tokens.push(token);
            if eof {
                break;
            }
        }
        let (ast, diagnostics) = crate::parser::parse_checked(source, tokens, FileId(3)).unwrap();
        assert!(diagnostics.is_empty());
        let (result, _) = crate::type_walk::tests::observe(Some(0), || check_syntax(&ast));
        assert!(matches!(
            result,
            Err(CompilerFailure::Internal {
                stage: CompilerStage::Parse,
                span: Some(_),
                ..
            })
        ));
        assert_eq!(check_syntax(&ast), Ok(()));
    }

    /// The syntax-height diagnostic `parse_script` reports for `source`, if any.
    fn syntax_limit(source: &str) -> Option<crate::Diagnostic> {
        parse_script(source, FileId(0))
            .diagnostics()
            .iter()
            .find(|d| d.message.contains("syntax nesting"))
            .cloned()
    }

    fn sum(operands: usize) -> String {
        // Function, block and return statements add three levels to the chain.
        format!(
            "function main(): number {{ return {}; }}",
            vec!["1"; operands].join(" + ")
        )
    }

    #[test]
    fn syntax_height_accepts_the_limit_and_rejects_one_more_level() {
        let at_limit = MAX_SYNTAX_HEIGHT as usize - 3;
        let chain_start = "function main(): number { return ".len() as u32;
        assert!(syntax_limit(&sum(at_limit)).is_none());
        // One level over, the function statement is the tallest node over the
        // limit; the report follows the deepest path to the chain itself.
        let diagnostic = syntax_limit(&sum(at_limit + 1)).expect("one level over is rejected");
        assert_eq!(diagnostic.span.start, chain_start);
        assert!(!diagnostic.help.is_empty());
        let diagnostic = syntax_limit(&sum(10 * at_limit)).expect("a long chain is rejected");
        assert_eq!(diagnostic.span.start, chain_start);
    }

    #[test]
    fn syntax_height_counts_postfix_chains_and_annotations() {
        let calls = format!(
            "function main(): number {{ const f = (): unknown => f; f{}; return 1; }}",
            "()".repeat(MAX_SYNTAX_HEIGHT as usize)
        );
        assert!(syntax_limit(&calls).is_some());
        let nested_type = format!(
            "function main(): number {{ const x: {}number{} | null = null; return 1; }}",
            "{ v: ".repeat(40),
            " }".repeat(40)
        );
        let parsed = parse_script(&nested_type, FileId(0));
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics());
    }

    /// `const y = <base>?.a.a…<tail>;` with `links` field links before `tail`.
    fn optional_fields(base: &str, links: usize, tail: &str) -> String {
        format!(
            "function main(): number {{ const y = {base}?.a{}{tail}; return 1; }}",
            ".a".repeat(links.saturating_sub(1))
        )
    }

    fn longest_accepted_field_chain() -> usize {
        let longest = (1..=MAX_SYNTAX_HEIGHT as usize)
            .take_while(|&links| syntax_limit(&optional_fields("x", links, "")).is_none())
            .last()
            .expect("a one-link chain is accepted");
        assert!(longest < MAX_SYNTAX_HEIGHT as usize - 1);
        longest
    }

    #[test]
    fn syntax_height_counts_each_optional_chain_link() {
        let longest = longest_accepted_field_chain();
        let over = optional_fields("x", longest + 1, "");
        let diagnostic = syntax_limit(&over).expect("one more link is rejected");
        assert_eq!(diagnostic.span.start as usize, over.find("x?.").unwrap());
        assert!(syntax_limit(&optional_fields("x", 20_000, "")).is_some());
    }

    #[test]
    fn optional_chain_operands_sit_below_their_link() {
        let longest = longest_accepted_field_chain();
        // `.f(…)` is two links; the argument is measured under the second.
        assert!(syntax_limit(&optional_fields("x", longest - 3, ".f(1)")).is_none());
        assert!(syntax_limit(&optional_fields("x", longest - 3, ".f(- 1)")).is_some());
        assert!(syntax_limit(&optional_fields("x", longest - 2, "[0]")).is_none());
        assert!(syntax_limit(&optional_fields("x", longest - 2, "[- 0]")).is_some());
    }

    #[test]
    fn a_long_expression_is_reported_rather_than_a_closure_it_contains() {
        let longest = longest_accepted_field_chain();
        let closure = "((): number => { const q = 1; return q; })()";
        let annotated = "((): number => { const q: number = 1; return q; })()";
        let plus_chain = |operands: usize| vec!["1"; operands].join(" + ");
        let closure_led_sum = |inner: usize, outer: usize| {
            format!(
                "function main(): number {{ return ((): number => {{ {{ return {}; }} }})(){}; }}",
                plus_chain(inner),
                " + 1".repeat(outer)
            )
        };
        // Each case pairs a source with the text its caret must start at.
        let cases = [
            // The chain's links make the function statement too tall.
            (optional_fields(closure, longest + 1, ""), closure),
            // The chain itself is over the limit.
            (optional_fields(closure, 20_000, ""), closure),
            // Block closures as the last call argument and index.
            (
                optional_fields("x", longest + 1, ".f(() => { return 1; })"),
                "x?.",
            ),
            (
                optional_fields("x", 20_000, "[(() => { return 1; })()]"),
                "x?.",
            ),
            // Sums led by a closure start where the closure does, with or
            // without an annotation on the closure's local.
            (
                format!(
                    "function main(): number {{ return {closure}{}; }}",
                    " + 1".repeat(400)
                ),
                closure,
            ),
            (
                format!(
                    "function main(): number {{ return {annotated}{}; }}",
                    " + 1".repeat(400)
                ),
                annotated,
            ),
            // A long outer sum wins over a shorter one inside its closure.
            (closure_led_sum(130, 10_000), "((): number"),
            // 203 inner operands tie with the outer sum, which wins; one more
            // and the inner sum is the longer.
            (closure_led_sum(203, 200), "((): number"),
            (closure_led_sum(204, 200), "1 + 1"),
            // The chain's links push the function over the limit, so its
            // segment counts them all rather than stopping at the sum inside
            // its base.
            (
                optional_fields(
                    &format!("((): number => {{ {{ return {}; }} }})()", plus_chain(200)),
                    longest + 1,
                    "",
                ),
                "((): number",
            ),
            // The chain's links tie with its base's height; they still decide
            // it, so the chain is reported.
            (
                optional_fields(
                    &format!(
                        "((): number => {{ {{ return {}; }} }})()",
                        plus_chain(longest - 5)
                    ),
                    longest + 1,
                    "",
                ),
                "((): number",
            ),
            // Of two equally tall statements, the first is reported.
            (
                format!(
                    "function main(): number {{ const a = {0}; const b = {0}; return 1; }}",
                    plus_chain(300)
                ),
                "1 + 1",
            ),
            // Of two equally tall functions, the first is reported.
            (
                format!(
                    "function f(): number {{ return {0}; }} function g(): number {{ return {0}; }}",
                    plus_chain(400)
                ),
                "1 + 1",
            ),
        ];
        for (case, (source, marker)) in cases.iter().enumerate() {
            let diagnostic = syntax_limit(source).expect("the long expression is rejected");
            let start = source.find(marker).unwrap();
            assert_eq!(diagnostic.span.start as usize, start, "case {case}");
        }
    }

    #[test]
    fn syntax_cycles_are_internal_failures() {
        let mut ast = Ast::new();
        let span = Span::at(FileId(0));
        ast.try_push_expr(Expr {
            kind: ExprKind::Paren(ExprId(0)),
            span,
        })
        .unwrap();
        assert!(matches!(
            check_syntax(&ast),
            Err(CompilerFailure::Internal {
                stage: CompilerStage::Parse,
                ..
            })
        ));
        let mut ast = Ast::new();
        ast.try_push_expr(Expr {
            kind: ExprKind::Paren(ExprId(7)),
            span,
        })
        .unwrap();
        assert!(matches!(
            check_syntax(&ast),
            Err(CompilerFailure::Internal { .. })
        ));
    }

    #[test]
    fn typed_height_is_measured_iteratively_and_rejects_cycles() {
        let span = Span::at(FileId(0));
        let mut ta = TypedAst::new();
        let mut previous = ta
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(1.0),
                span,
                ty: Type::Number,
            })
            .unwrap();
        for _ in 1..MAX_TYPED_HEIGHT {
            previous = ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Unary {
                        op: crate::UnOp::Neg,
                        operand: previous,
                    },
                    span,
                    ty: Type::Number,
                })
                .unwrap();
        }
        check_typed(&ta, CompilerStage::Codegen).unwrap();
        ta.try_push_expr(TypedExpr {
            kind: TypedExprKind::Unary {
                op: crate::UnOp::Neg,
                operand: previous,
            },
            span,
            ty: Type::Number,
        })
        .unwrap();
        assert!(matches!(
            check_typed(&ta, CompilerStage::Codegen),
            Err(CompilerFailure::Limit {
                stage: CompilerStage::Codegen,
                ..
            })
        ));

        let mut ta = TypedAst::new();
        ta.try_push_expr(TypedExpr {
            kind: TypedExprKind::NonNullAssert { value: ExprId(0) },
            span,
            ty: Type::Number,
        })
        .unwrap();
        assert!(matches!(
            check_typed(&ta, CompilerStage::Infer),
            Err(CompilerFailure::Internal {
                stage: CompilerStage::Infer,
                ..
            })
        ));
    }

    /// A negation chain of `levels` typed nodes, returning its root.
    fn negations(ta: &mut TypedAst, levels: u32) -> ExprId {
        let span = Span::at(FileId(0));
        let mut expr = ta
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Number(1.0),
                span,
                ty: Type::Number,
            })
            .unwrap();
        for _ in 1..levels {
            expr = ta
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Unary {
                        op: crate::UnOp::Neg,
                        operand: expr,
                    },
                    span,
                    ty: Type::Number,
                })
                .unwrap();
        }
        expr
    }

    fn object_with_fallback_chain(links: u32, terminal_levels: u32) -> TypedAst {
        let mut ta = TypedAst::new();
        let terminal = negations(&mut ta, terminal_levels);
        let mut source = TypedObjectFieldSource::Literal(terminal);
        for _ in 1..links {
            source = TypedObjectFieldSource::Spread {
                source_index: 0,
                field_name: "a".into(),
                source_ty: Type::Number,
                fallback: Some(Box::new(source)),
            };
        }
        ta.try_push_expr(TypedExpr {
            kind: TypedExprKind::ObjectLiteral {
                members: Vec::new(),
                fields: vec![crate::TypedObjectFieldOrigin {
                    name: crate::Ident {
                        name: "a".into(),
                        span: Span::at(FileId(0)),
                    },
                    source,
                    optional: true,
                    ty: Type::Number,
                }],
            },
            span: Span::at(FileId(0)),
            ty: Type::Unknown,
        })
        .unwrap();
        ta
    }

    #[test]
    fn spread_fallback_links_add_to_the_terminal_expression_height() {
        // Object node + fallback links + terminal chain.
        let within = MAX_TYPED_HEIGHT - 1 - 600;
        check_typed(
            &object_with_fallback_chain(600, within),
            CompilerStage::Codegen,
        )
        .unwrap();
        assert!(matches!(
            check_typed(
                &object_with_fallback_chain(600, within + 1),
                CompilerStage::Codegen
            ),
            Err(CompilerFailure::Limit { .. })
        ));
    }
}
