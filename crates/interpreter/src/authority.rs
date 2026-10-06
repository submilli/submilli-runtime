//! Deterministic package authority call graphs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::typechecker::rules::body_walk::{self, Visitor};
use crate::{
    ClosureBody, DocCapability, ExportKind, ExprId, MangledName, PackageDeclaration, Param,
    PostfixTarget, Sources, Span, StmtId, Type, TypeKind, TypedAst, TypedChainPart,
    TypedClassAccessor, TypedExprKind, TypedStmtKind, TypedTypeDecl, Visibility,
};

const MAX_CALLABLES: usize = 1 << 16;
const MAX_EDGES: usize = 1 << 18;
const MAX_ROUTE_EFFECTS: usize = 1 << 18;
const MAX_WITNESS_WORK: usize = 1 << 24;

#[derive(Clone, Copy)]
struct AnalysisLimits {
    callables: usize,
    edges: usize,
    route_effects: usize,
    witness_work: usize,
}

impl Default for AnalysisLimits {
    fn default() -> Self {
        Self {
            callables: MAX_CALLABLES,
            edges: MAX_EDGES,
            route_effects: MAX_ROUTE_EFFECTS,
            witness_work: MAX_WITNESS_WORK,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityMap {
    pub callables: Vec<AuthorityCallable>,
    pub edges: Vec<AuthorityEdge>,
    pub routes: Vec<AuthorityRoute>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityCallable {
    pub id: String,
    pub kind: AuthorityCallableKind,
    pub name: String,
    pub span: AuthoritySpan,
    pub exposure: AuthorityExposure,
    pub direct_effects: Vec<AuthorityEffect>,
    pub transitive_effects: Vec<AuthorityEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityCallableKind {
    Function,
    StaticMethod,
    Method,
    Constructor,
    Getter,
    Setter,
    Closure,
    FunctionValue,
    ModuleInitializer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityExposure {
    Public,
    Private,
    Initialization,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthoritySpan {
    pub path: String,
    pub start: AuthorityPosition,
    pub end: AuthorityPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorityPosition {
    pub byte: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorityEffect {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    pub sink: AuthoritySpan,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub known_bindings: BTreeMap<String, String>,
    pub unresolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityEdge {
    pub caller: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub span: AuthoritySpan,
    pub unresolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityRoute {
    pub callable: String,
    pub effects: Vec<AuthorityRouteEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityRouteEffect {
    pub effect: AuthorityEffect,
    pub witness: Vec<AuthorityWitnessStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityWitnessStep {
    pub caller: String,
    pub target: String,
    pub span: AuthoritySpan,
}

struct Root {
    statements: Vec<StmtId>,
    expressions: Vec<ExprId>,
}

struct Node {
    callable: AuthorityCallable,
    root: Root,
    raw_span: Span,
}

#[derive(Clone)]
struct RawEdge {
    caller: usize,
    target: Option<usize>,
    span: AuthoritySpan,
    unresolved: bool,
    reason: Option<String>,
}

struct Propagation {
    adjacency: Vec<Vec<(usize, usize)>>,
    component_of: Vec<usize>,
    component_effects: Vec<BTreeSet<AuthorityEffect>>,
}

#[derive(Clone, Copy)]
struct CapabilityCallee<'a> {
    params: &'a [Param],
    capabilities: &'a [DocCapability],
}

/// Build the authority graph for one already-typechecked package.
pub(crate) fn analyse<'a>(
    declaration: &'a PackageDeclaration,
    ta: &'a TypedAst,
    sources: &'a Sources,
    dependencies: impl Iterator<Item = &'a PackageDeclaration>,
) -> Result<AuthorityMap, CompilerFailure> {
    let mut builder = Builder::new(
        declaration,
        ta,
        sources,
        dependencies,
        AnalysisLimits::default(),
    )?;
    builder.scan()?;
    builder.finish()
}

struct Builder<'a> {
    declaration: &'a PackageDeclaration,
    ta: &'a TypedAst,
    sources: &'a Sources,
    local_class_declarations: BTreeMap<MangledName, &'a crate::TypedClassDecl>,
    nodes: Vec<Node>,
    named: BTreeMap<MangledName, usize>,
    methods: BTreeMap<(MangledName, String), usize>,
    accessors: BTreeMap<(MangledName, String, bool), usize>,
    classes: BTreeMap<MangledName, Option<MangledName>>,
    closure_nodes: BTreeMap<ExprId, usize>,
    closure_globals: BTreeMap<MangledName, usize>,
    known_functions: BTreeSet<MangledName>,
    known_methods: BTreeSet<(MangledName, String)>,
    dynamically_dispatched_methods: BTreeSet<(MangledName, String)>,
    capability_functions: BTreeMap<MangledName, CapabilityCallee<'a>>,
    capability_methods: BTreeMap<(MangledName, String), CapabilityCallee<'a>>,
    class_hierarchy: BTreeMap<MangledName, Option<MangledName>>,
    external_classes: BTreeMap<MangledName, &'a crate::TypeSymbol>,
    external_surfaces: BTreeMap<MangledName, Rc<ExternalClassSurface>>,
    edges: Vec<RawEdge>,
    exposure_edges: Vec<(usize, usize)>,
    direct_effect_count: usize,
    limits: AnalysisLimits,
}

impl<'a> Builder<'a> {
    fn new(
        declaration: &'a PackageDeclaration,
        ta: &'a TypedAst,
        sources: &'a Sources,
        dependencies: impl Iterator<Item = &'a PackageDeclaration>,
        limits: AnalysisLimits,
    ) -> Result<Self, CompilerFailure> {
        let local_class_declarations = ta
            .types
            .iter()
            .filter_map(|declaration| match declaration {
                TypedTypeDecl::Class(class) => Some((class.mangled_name.clone(), class)),
                _ => None,
            })
            .collect();
        let mut builder = Self {
            declaration,
            ta,
            sources,
            local_class_declarations,
            nodes: Vec::new(),
            named: BTreeMap::new(),
            methods: BTreeMap::new(),
            accessors: BTreeMap::new(),
            classes: BTreeMap::new(),
            closure_nodes: BTreeMap::new(),
            closure_globals: BTreeMap::new(),
            known_functions: BTreeSet::new(),
            known_methods: BTreeSet::new(),
            dynamically_dispatched_methods: BTreeSet::new(),
            capability_functions: BTreeMap::new(),
            capability_methods: BTreeMap::new(),
            class_hierarchy: BTreeMap::new(),
            external_classes: BTreeMap::new(),
            external_surfaces: BTreeMap::new(),
            edges: Vec::new(),
            exposure_edges: Vec::new(),
            direct_effect_count: 0,
            limits,
        };
        builder.index_local()?;
        for dependency in dependencies {
            builder.index_dependency_declaration(dependency);
        }
        builder.expose_inherited_members(&exports(ta, ExportKind::Type))?;
        builder.link_implicit_constructors()?;
        Ok(builder)
    }

    fn index_local(&mut self) -> Result<(), CompilerFailure> {
        let exported_functions = exports(self.ta, ExportKind::Function);
        let exported_globals = exports(self.ta, ExportKind::Global);
        let exported_classes = exports(self.ta, ExportKind::Type);
        let public_statics = public_static_functions(self.declaration, &exported_classes);

        self.index_functions(&exported_functions, &public_statics)?;
        self.index_classes(&exported_classes)?;
        self.index_closures()?;
        self.index_module_initializers(&exported_globals)?;
        Ok(())
    }

    fn index_functions(
        &mut self,
        exported_functions: &BTreeSet<MangledName>,
        public_statics: &BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        for function in &self.ta.functions {
            let kind = if function.mangled_name.as_str().contains("#static#") {
                AuthorityCallableKind::StaticMethod
            } else {
                AuthorityCallableKind::Function
            };
            let exposure = if exported_functions.contains(&function.mangled_name)
                || public_statics.contains(&function.mangled_name)
            {
                AuthorityExposure::Public
            } else {
                AuthorityExposure::Private
            };
            let index = self.push_node(
                function.mangled_name.to_string(),
                kind,
                function.name.name.clone(),
                function.span,
                exposure,
                vec![function.body],
                Vec::new(),
            )?;
            self.named.insert(function.mangled_name.clone(), index);
        }
        Ok(())
    }

    fn index_classes(
        &mut self,
        exported_classes: &BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        for declaration in &self.ta.types {
            let TypedTypeDecl::Class(class) = declaration else {
                continue;
            };
            self.classes
                .insert(class.mangled_name.clone(), class.extends.clone());
            self.class_hierarchy
                .insert(class.mangled_name.clone(), class.extends.clone());
            let class_public = exported_classes.contains(&class.mangled_name);
            let ctor_public = class_public && self.constructor_is_public(&class.mangled_name);
            let ctor_mangled = crate::mangle::extend(&class.mangled_name, "constructor");
            let mut ctor_statements = Vec::new();
            if let Some(constructor) = &class.constructor {
                ctor_statements.push(constructor.body);
            }
            let ctor_expressions = class
                .fields
                .iter()
                .filter_map(|field| field.initializer)
                .collect();
            let ctor_span = class
                .constructor
                .as_ref()
                .map_or(class.name.span, |constructor| constructor.span);
            let ctor = self.push_node(
                ctor_mangled.to_string(),
                AuthorityCallableKind::Constructor,
                format!("new {}", class.name.name),
                ctor_span,
                if ctor_public {
                    AuthorityExposure::Public
                } else {
                    AuthorityExposure::Private
                },
                ctor_statements,
                ctor_expressions,
            )?;
            self.named.insert(ctor_mangled, ctor);

            for method in &class.methods {
                self.dynamically_dispatched_methods
                    .insert((class.mangled_name.clone(), method.name.name.clone()));
                let id = crate::mangle::extend(&class.mangled_name, &method.name.name);
                let index = self.push_node(
                    id.to_string(),
                    AuthorityCallableKind::Method,
                    format!("{}.{}", class.name.name, method.name.name),
                    method.name.span,
                    if class_public && method.visibility == Visibility::Public {
                        AuthorityExposure::Public
                    } else {
                        AuthorityExposure::Private
                    },
                    vec![method.body],
                    Vec::new(),
                )?;
                self.methods.insert(
                    (class.mangled_name.clone(), method.name.name.clone()),
                    index,
                );
            }
            for accessor in &class.accessors {
                let is_setter = matches!(accessor, TypedClassAccessor::Setter { .. });
                let suffix = if is_setter { "set" } else { "get" };
                let id = format!("{}#{suffix}#{}", class.mangled_name, accessor.name().name);
                let index = self.push_node(
                    id,
                    if is_setter {
                        AuthorityCallableKind::Setter
                    } else {
                        AuthorityCallableKind::Getter
                    },
                    format!("{}.{}", class.name.name, accessor.name().name),
                    accessor.name().span,
                    if class_public && accessor.visibility() == Visibility::Public {
                        AuthorityExposure::Public
                    } else {
                        AuthorityExposure::Private
                    },
                    vec![accessor.body()],
                    Vec::new(),
                )?;
                self.accessors.insert(
                    (
                        class.mangled_name.clone(),
                        accessor.name().name.clone(),
                        is_setter,
                    ),
                    index,
                );
            }
        }
        Ok(())
    }

    fn index_closures(&mut self) -> Result<(), CompilerFailure> {
        let expr_ids: Vec<_> = self
            .ta
            .expr_ids()
            .map_err(crate::typechecker::arena_failure)?
            .collect();
        for id in expr_ids {
            let expression = self
                .ta
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            let TypedExprKind::Closure { body, .. } = &expression.kind else {
                continue;
            };
            let name = self
                .ta
                .nested_function_names
                .get(&id)
                .or_else(|| self.ta.closure_names.get(&id))
                .map_or_else(|| "anonymous closure".to_string(), |name| name.name.clone());
            let path = self
                .sources
                .get(expression.span.file)
                .map_or("<unknown>", |source| source.path.as_str());
            let node_id = format!(
                "closure:{path}:{}:{}",
                expression.span.start, expression.span.end
            );
            let (statements, expressions) = match body {
                ClosureBody::Block(body) => (vec![*body], Vec::new()),
                ClosureBody::Expr(result) => (Vec::new(), vec![*result]),
            };
            let index = self.push_node(
                node_id,
                AuthorityCallableKind::Closure,
                name,
                expression.span,
                AuthorityExposure::Private,
                statements,
                expressions,
            )?;
            self.closure_nodes.insert(id, index);
        }
        Ok(())
    }

    fn index_module_initializers(
        &mut self,
        exported_globals: &BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        let mut module_roots: BTreeMap<u32, Vec<_>> = BTreeMap::new();
        for statement in &self.ta.top_level_statements {
            let span = self
                .ta
                .try_stmt(*statement)
                .map_err(crate::typechecker::arena_failure)?
                .span;
            module_roots
                .entry(span.file.0)
                .or_default()
                .push(*statement);
            if let TypedStmtKind::AssignGlobal {
                ident,
                mangled,
                target_ty,
                value,
            } = &self
                .ta
                .try_stmt(*statement)
                .map_err(crate::typechecker::arena_failure)?
                .kind
            {
                self.index_function_global(
                    ident,
                    mangled,
                    target_ty,
                    *value,
                    exported_globals.contains(mangled),
                )?;
            }
        }
        for (file, statements) in module_roots {
            let file = crate::FileId(file);
            let span = self
                .ta
                .try_stmt(statements[0])
                .map_err(crate::typechecker::arena_failure)?
                .span;
            let path = self
                .sources
                .get(file)
                .map_or("<unknown>", |source| source.path.as_str());
            self.push_node(
                format!("module:{path}"),
                AuthorityCallableKind::ModuleInitializer,
                format!("module {path}"),
                span,
                AuthorityExposure::Initialization,
                statements,
                Vec::new(),
            )?;
        }
        Ok(())
    }

    fn index_function_global(
        &mut self,
        ident: &crate::Ident,
        mangled: &MangledName,
        target_ty: &Type,
        value: ExprId,
        exported: bool,
    ) -> Result<(), CompilerFailure> {
        let held = self.held_callable(value)?;
        if held.is_none() && !may_be_callable(target_ty) {
            return Ok(());
        }
        let exposure = if exported {
            AuthorityExposure::Public
        } else {
            AuthorityExposure::Private
        };
        let target = if self.ta.rebindable_globals.contains_key(mangled) {
            let target = self.push_node(
                format!("global-function:{mangled}"),
                AuthorityCallableKind::FunctionValue,
                ident.name.clone(),
                ident.span,
                exposure,
                Vec::new(),
                Vec::new(),
            )?;
            if let Some(held) = held {
                self.add_edge(target, held, ident.span)?;
                self.add_exposure_edge(target, held, ident.span)?;
            }
            self.add_unresolved(target, ident.span, "function-valued global may be rebound")?;
            target
        } else if let Some(held) = held {
            held
        } else {
            let target = self.push_node(
                format!("global-function:{mangled}"),
                AuthorityCallableKind::FunctionValue,
                ident.name.clone(),
                ident.span,
                exposure,
                Vec::new(),
                Vec::new(),
            )?;
            self.add_unresolved(
                target,
                ident.span,
                "function-valued global target is not directly recoverable",
            )?;
            target
        };
        self.closure_globals.insert(mangled.clone(), target);
        if exported {
            self.nodes[target].callable.exposure = AuthorityExposure::Public;
        }
        Ok(())
    }

    fn expose_inherited_members(
        &mut self,
        exported_classes: &BTreeSet<MangledName>,
    ) -> Result<(), CompilerFailure> {
        let mut inheritance_work = 0usize;
        for exported in exported_classes {
            let Some(class) = self.local_class_declarations.get(exported).copied() else {
                continue;
            };
            self.expose_local_instance_function_fields(exported, class, None)?;
            self.expose_static_function_fields(exported, class.name.span)?;
            let mut seen = InheritedNames {
                methods: class
                    .methods
                    .iter()
                    .map(|method| method.name.name.clone())
                    .collect(),
                accessors: class
                    .accessors
                    .iter()
                    .map(|accessor| {
                        (
                            accessor.name().name.clone(),
                            matches!(accessor, TypedClassAccessor::Setter { .. }),
                        )
                    })
                    .collect(),
                statics: own_static_names(self.ta, exported),
                fields: class
                    .fields
                    .iter()
                    .map(|field| field.name.name.clone())
                    .collect(),
            };
            let mut parent = class.extends.clone();
            for _ in 0..=self.class_hierarchy.len() {
                let Some(parent_name) = parent else {
                    break;
                };
                if let Some(parent_class) = self.local_class_declarations.get(&parent_name).copied()
                {
                    let static_count = parent_class
                        .static_methods
                        .len()
                        .saturating_add(parent_class.static_fields.len());
                    inheritance_work = checked_authority_budget(
                        inheritance_work,
                        parent_class
                            .methods
                            .len()
                            .saturating_add(parent_class.accessors.len())
                            .saturating_add(parent_class.fields.len())
                            .saturating_add(static_count),
                        self.limits.route_effects,
                        class.name.span,
                        "inherited-member work",
                    )?;
                    self.expose_local_inherited_methods(
                        parent_class,
                        &parent_name,
                        &mut seen.methods,
                    );
                    self.expose_local_inherited_accessors(
                        parent_class,
                        &parent_name,
                        &mut seen.accessors,
                    );
                    self.expose_local_instance_function_fields(
                        exported,
                        parent_class,
                        Some(&mut seen.fields),
                    )?;
                    self.expose_local_inherited_statics(
                        exported,
                        parent_class,
                        &parent_name,
                        class.name.span,
                        &mut seen.statics,
                    )?;
                    parent = parent_class.extends.clone();
                    continue;
                }
                let Some(surface) = self.external_surfaces.get(&parent_name).cloned() else {
                    break;
                };
                inheritance_work = checked_authority_budget(
                    inheritance_work,
                    surface.member_count(),
                    self.limits.route_effects,
                    class.name.span,
                    "inherited-member work",
                )?;
                self.expose_external_inherited_members(
                    exported,
                    &parent_name,
                    class.name.span,
                    surface.as_ref(),
                    &mut seen,
                )?;
                parent = surface.parent.clone();
            }
        }
        Ok(())
    }

    fn expose_local_instance_function_fields(
        &mut self,
        exported: &MangledName,
        class: &crate::TypedClassDecl,
        mut seen: Option<&mut BTreeSet<String>>,
    ) -> Result<(), CompilerFailure> {
        let fields = class
            .fields
            .iter()
            .filter_map(|field| {
                let newly_visible = seen
                    .as_deref_mut()
                    .is_none_or(|seen| seen.insert(field.name.name.clone()));
                (newly_visible
                    && field.visibility == Visibility::Public
                    && may_be_callable(&field.ty))
                .then_some((
                    field.name.name.clone(),
                    field.name.span,
                    field.initializer,
                    field.readonly,
                ))
            })
            .collect::<Vec<_>>();
        for (name, span, initializer, readonly) in fields {
            let initial_target = match initializer {
                Some(initializer) => self.held_callable(initializer)?,
                None => None,
            };
            let target = self.push_node(
                format!("instance-field:{exported}:{}:{name}", class.mangled_name),
                AuthorityCallableKind::FunctionValue,
                format!("{exported}.{name}"),
                span,
                AuthorityExposure::Public,
                Vec::new(),
                Vec::new(),
            )?;
            let reason = if let Some(initial_target) = initial_target {
                self.add_edge(target, initial_target, span)?;
                self.add_exposure_edge(target, initial_target, span)?;
                if readonly {
                    "readonly function field may be reassigned during construction"
                } else {
                    "public function field may be reassigned"
                }
            } else {
                "function-valued instance field target is not directly recoverable"
            };
            self.add_unresolved(target, span, reason)?;
        }
        Ok(())
    }

    fn expose_local_inherited_methods(
        &mut self,
        class: &crate::TypedClassDecl,
        owner: &MangledName,
        seen: &mut BTreeSet<String>,
    ) {
        for method in &class.methods {
            if method.visibility == Visibility::Public
                && seen.insert(method.name.name.clone())
                && let Some(target) = self.methods.get(&(owner.clone(), method.name.name.clone()))
            {
                self.nodes[*target].callable.exposure = AuthorityExposure::Public;
            }
        }
    }

    fn expose_local_inherited_accessors(
        &mut self,
        class: &crate::TypedClassDecl,
        owner: &MangledName,
        seen: &mut BTreeSet<(String, bool)>,
    ) {
        for accessor in &class.accessors {
            let setter = matches!(accessor, TypedClassAccessor::Setter { .. });
            let key = (accessor.name().name.clone(), setter);
            if accessor.visibility() == Visibility::Public
                && seen.insert(key.clone())
                && let Some(target) = self.accessors.get(&(owner.clone(), key.0, setter))
            {
                self.nodes[*target].callable.exposure = AuthorityExposure::Public;
            }
        }
    }

    fn expose_local_inherited_statics(
        &mut self,
        exported: &MangledName,
        class: &crate::TypedClassDecl,
        owner: &MangledName,
        span: Span,
        seen: &mut BTreeSet<String>,
    ) -> Result<(), CompilerFailure> {
        for (name, visibility) in &class.static_methods {
            if *visibility != Visibility::Private
                && seen.insert(name.clone())
                && let Some(target) = self.named.get(&crate::mangle::static_member(owner, name))
            {
                self.nodes[*target].callable.exposure = AuthorityExposure::Public;
            }
        }
        let function_fields = class
            .static_fields
            .iter()
            .filter(|(name, field)| {
                !seen.contains(*name)
                    && field.visibility == Visibility::Public
                    && may_be_callable(&field.ty)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        for name in function_fields {
            seen.insert(name.clone());
            if let Some(target) = self.static_function_target(owner, &name) {
                self.nodes[target].callable.exposure = AuthorityExposure::Public;
            } else {
                let target = self.push_node(
                    format!("inherited-static-field:{exported}:{owner}:{name}"),
                    AuthorityCallableKind::FunctionValue,
                    format!("{exported}.{name}"),
                    span,
                    AuthorityExposure::Public,
                    Vec::new(),
                    Vec::new(),
                )?;
                self.add_unresolved(
                    target,
                    span,
                    "static function field target is not directly recoverable",
                )?;
            }
        }
        Ok(())
    }

    fn expose_static_function_fields(
        &mut self,
        owner: &MangledName,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let Some(class) = self.local_class_declarations.get(owner).copied() else {
            return Ok(());
        };
        let names = class
            .static_fields
            .iter()
            .filter(|(_, field)| {
                field.visibility == Visibility::Public && may_be_callable(&field.ty)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        checked_authority_budget(
            self.nodes.len(),
            names.len(),
            self.limits.callables,
            span,
            "callable",
        )?;
        checked_authority_budget(
            self.direct_effect_count,
            names.len(),
            self.limits.route_effects,
            span,
            "direct effect",
        )?;
        for name in names {
            if let Some(target) = self.static_function_target(owner, &name) {
                self.nodes[target].callable.exposure = AuthorityExposure::Public;
            } else {
                let target = self.push_node(
                    format!("static-field:{owner}:{name}"),
                    AuthorityCallableKind::FunctionValue,
                    format!("{owner}.{name}"),
                    span,
                    AuthorityExposure::Public,
                    Vec::new(),
                    Vec::new(),
                )?;
                self.add_unresolved(
                    target,
                    span,
                    "static function field target is not directly recoverable",
                )?;
            }
        }
        Ok(())
    }

    fn static_function_target(&self, owner: &MangledName, name: &str) -> Option<usize> {
        let exact = crate::mangle::static_member(owner, name);
        self.closure_globals.get(&exact).copied()
    }

    fn expose_external_inherited_members(
        &mut self,
        exported: &MangledName,
        owner: &MangledName,
        span: Span,
        surface: &ExternalClassSurface,
        seen: &mut InheritedNames,
    ) -> Result<(), CompilerFailure> {
        for name in &surface.methods {
            if seen.methods.insert(name.clone()) {
                self.push_external_inherited_surface(
                    exported,
                    owner,
                    name,
                    AuthorityCallableKind::Method,
                    span,
                )?;
            }
        }
        for (name, setter) in &surface.accessors {
            if seen.accessors.insert((name.clone(), *setter)) {
                self.push_external_inherited_surface(
                    exported,
                    owner,
                    name,
                    if *setter {
                        AuthorityCallableKind::Setter
                    } else {
                        AuthorityCallableKind::Getter
                    },
                    span,
                )?;
            }
        }
        for name in &surface.function_fields {
            if seen.fields.insert(name.clone()) {
                self.push_external_inherited_surface(
                    exported,
                    owner,
                    name,
                    AuthorityCallableKind::FunctionValue,
                    span,
                )?;
            }
        }
        for name in &surface.statics {
            if seen.statics.insert(name.clone()) {
                self.push_external_inherited_surface(
                    exported,
                    owner,
                    name,
                    AuthorityCallableKind::StaticMethod,
                    span,
                )?;
            }
        }
        for name in &surface.static_function_fields {
            if seen.statics.insert(name.clone()) {
                self.push_external_inherited_surface(
                    exported,
                    owner,
                    name,
                    AuthorityCallableKind::FunctionValue,
                    span,
                )?;
            }
        }
        Ok(())
    }

    fn push_external_inherited_surface(
        &mut self,
        exported: &MangledName,
        owner: &MangledName,
        name: &str,
        kind: AuthorityCallableKind,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let index = self.push_node(
            format!("inherited:{exported}:{owner}:{kind:?}:{name}"),
            kind,
            format!("{exported}.{name}"),
            span,
            AuthorityExposure::Public,
            Vec::new(),
            Vec::new(),
        )?;
        self.add_external_inherited_capabilities(index, owner, name, kind, span)?;
        self.add_unresolved(
            index,
            span,
            "external inherited member body is not locally analyzable",
        )
    }

    fn add_external_inherited_capabilities(
        &mut self,
        index: usize,
        owner: &MangledName,
        name: &str,
        kind: AuthorityCallableKind,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let callee = match kind {
            AuthorityCallableKind::Method => self
                .capability_methods
                .get(&(owner.clone(), name.to_string()))
                .copied(),
            AuthorityCallableKind::StaticMethod => self
                .capability_functions
                .get(&crate::mangle::static_member(owner, name))
                .copied(),
            _ => None,
        };
        if let Some(callee) = callee {
            checked_authority_budget(
                self.direct_effect_count,
                callee.capabilities.len().saturating_add(1),
                self.limits.route_effects,
                span,
                "direct effect",
            )?;
            let mut effects = Vec::with_capacity(callee.capabilities.len());
            for capability in callee.capabilities {
                let derived =
                    crate::derive_call_site_capability(capability, callee.params, self.ta, &[])
                        .map_err(|error| {
                            error.fatal.unwrap_or_else(|| {
                        crate::typechecker::invariant_failure(
                            "inherited capability derivation failed without a typed failure",
                        )
                    })
                        })?;
                let unresolved = capability.bindings.iter().any(|binding| {
                    matches!(
                        binding.kind,
                        crate::DocCapabilityBindingKind::Parameter { .. }
                    )
                });
                effects.push(AuthorityEffect {
                    capability: Some(derived.capability),
                    sink: source_span(self.sources, span)?,
                    known_bindings: derived.known_bindings,
                    unresolved,
                    reason: unresolved.then(|| {
                        "some inherited capability bindings depend on caller arguments".to_string()
                    }),
                });
            }
            for effect in effects {
                self.push_effect(index, effect, span)?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_node(
        &mut self,
        id: String,
        kind: AuthorityCallableKind,
        name: String,
        span: Span,
        exposure: AuthorityExposure,
        statements: Vec<StmtId>,
        expressions: Vec<ExprId>,
    ) -> Result<usize, CompilerFailure> {
        check_limit(self.nodes.len(), self.limits.callables, span, "callable")?;
        let index = self.nodes.len();
        self.nodes.push(Node {
            callable: AuthorityCallable {
                id,
                kind,
                name,
                span: source_span(self.sources, span)?,
                exposure,
                direct_effects: Vec::new(),
                transitive_effects: Vec::new(),
            },
            root: Root {
                statements,
                expressions,
            },
            raw_span: span,
        });
        Ok(index)
    }

    fn link_implicit_constructors(&mut self) -> Result<(), CompilerFailure> {
        for declaration in &self.ta.types {
            let TypedTypeDecl::Class(class) = declaration else {
                continue;
            };
            let (None, Some(parent)) = (&class.constructor, &class.extends) else {
                continue;
            };
            let child = crate::mangle::extend(&class.mangled_name, "constructor");
            let parent = crate::mangle::extend(parent, "constructor");
            let Some(&caller) = self.named.get(&child) else {
                continue;
            };
            if let Some(&target) = self.named.get(&parent) {
                self.add_edge(caller, target, class.name.span)?;
            } else {
                self.add_unresolved(caller, class.name.span, "inherited constructor target")?;
            }
        }
        Ok(())
    }

    fn scan(&mut self) -> Result<(), CompilerFailure> {
        for caller in 0..self.nodes.len() {
            let statements = self.nodes[caller].root.statements.clone();
            let expressions = self.nodes[caller].root.expressions.clone();
            if self.nodes[caller].callable.kind == AuthorityCallableKind::Closure {
                for &expression in &expressions {
                    if let Some(target) = self.held_callable(expression)? {
                        let span = self
                            .ta
                            .try_expr(expression)
                            .map_err(crate::typechecker::arena_failure)?
                            .span;
                        self.add_exposure_edge(caller, target, span)?;
                    }
                }
            }
            let ta = self.ta;
            let mut visitor = CallVisitor {
                builder: self,
                caller,
            };
            body_walk::walk_roots(ta, statements, expressions, &mut visitor)?;
        }
        self.propagate_callable_exposure();
        self.edges.sort_by_key(|edge| edge_key(edge, &self.nodes));
        self.edges
            .dedup_by(|a, b| edge_key(a, &self.nodes) == edge_key(b, &self.nodes));
        Ok(())
    }

    fn add_exposure_edge(
        &mut self,
        caller: usize,
        target: usize,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        check_limit(
            self.exposure_edges.len(),
            self.limits.edges,
            span,
            "callable-exposure edge",
        )?;
        self.exposure_edges.push((caller, target));
        Ok(())
    }

    fn propagate_callable_exposure(&mut self) {
        let mut outgoing = vec![Vec::new(); self.nodes.len()];
        for &(caller, target) in &self.exposure_edges {
            outgoing[caller].push(target);
        }
        let mut pending = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| {
                (node.callable.exposure == AuthorityExposure::Public).then_some(index)
            })
            .collect::<VecDeque<_>>();
        while let Some(caller) = pending.pop_front() {
            for &target in &outgoing[caller] {
                if self.nodes[target].callable.exposure == AuthorityExposure::Private {
                    self.nodes[target].callable.exposure = AuthorityExposure::Public;
                    pending.push_back(target);
                }
            }
        }
    }

    fn add_edge(
        &mut self,
        caller: usize,
        target: usize,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        self.push_edge(
            RawEdge {
                caller,
                target: Some(target),
                span: source_span(self.sources, span)?,
                unresolved: false,
                reason: None,
            },
            span,
        )
    }

    fn add_unresolved(
        &mut self,
        caller: usize,
        span: Span,
        reason: &str,
    ) -> Result<(), CompilerFailure> {
        let effect = AuthorityEffect {
            capability: None,
            sink: source_span(self.sources, span)?,
            known_bindings: BTreeMap::new(),
            unresolved: true,
            reason: Some(reason.to_string()),
        };
        self.push_effect(caller, effect, span)?;
        self.push_edge(
            RawEdge {
                caller,
                target: None,
                span: source_span(self.sources, span)?,
                unresolved: true,
                reason: Some(reason.to_string()),
            },
            span,
        )
    }

    fn push_edge(&mut self, edge: RawEdge, span: Span) -> Result<(), CompilerFailure> {
        check_limit(self.edges.len(), self.limits.edges, span, "edge")?;
        self.edges.push(edge);
        Ok(())
    }

    fn push_effect(
        &mut self,
        caller: usize,
        effect: AuthorityEffect,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        check_limit(
            self.direct_effect_count,
            self.limits.route_effects,
            span,
            "direct effect",
        )?;
        self.direct_effect_count += 1;
        self.nodes[caller].callable.direct_effects.push(effect);
        Ok(())
    }

    fn add_capability_effects(
        &mut self,
        caller: usize,
        callee: CapabilityCallee<'_>,
        args: &[ExprId],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        for capability in callee.capabilities {
            let derived =
                crate::derive_call_site_capability(capability, callee.params, self.ta, args)
                    .map_err(|error| {
                        error.fatal.unwrap_or_else(|| {
                            crate::typechecker::invariant_failure(
                                "capability derivation failed without a typed failure",
                            )
                        })
                    })?;
            self.push_effect(
                caller,
                AuthorityEffect {
                    capability: Some(derived.capability),
                    sink: source_span(self.sources, span)?,
                    known_bindings: derived.known_bindings,
                    unresolved: false,
                    reason: None,
                },
                span,
            )?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<AuthorityMap, CompilerFailure> {
        for node in &mut self.nodes {
            node.callable.direct_effects.sort();
            node.callable.direct_effects.dedup();
        }
        let propagation = self.propagate_effects()?;
        for (index, component) in propagation.component_of.iter().enumerate() {
            self.nodes[index].callable.transitive_effects = propagation.component_effects
                [*component]
                .iter()
                .cloned()
                .collect();
        }
        let mut routes = self.build_routes(&propagation.adjacency)?;
        let edges = self.export_edges();
        let mut callables: Vec<_> = self.nodes.into_iter().map(|node| node.callable).collect();
        callables.sort_by(|a, b| a.id.cmp(&b.id));
        routes.sort_by(|a, b| a.callable.cmp(&b.callable));
        Ok(AuthorityMap {
            callables,
            edges,
            routes,
        })
    }

    fn propagate_effects(&self) -> Result<Propagation, CompilerFailure> {
        let adjacency = adjacency(self.nodes.len(), &self.edges);
        let (component_of, components) = strongly_connected_components(&adjacency);
        let mut component_effects = components
            .iter()
            .map(|members| {
                members
                    .iter()
                    .flat_map(|member| self.nodes[*member].callable.direct_effects.iter().cloned())
                    .collect::<BTreeSet<_>>()
            })
            .collect::<Vec<_>>();
        let (component_edges, topo) = component_dag(&component_of, components.len(), &self.edges);
        let first_span = self
            .nodes
            .first()
            .map_or(Span::at(crate::FileId(0)), |node| node.raw_span);
        let mut propagated_entries = component_effects
            .iter()
            .try_fold(0usize, |total, effects| total.checked_add(effects.len()))
            .ok_or_else(|| {
                limit(
                    first_span,
                    "authority analysis direct-effect count overflows the platform limit"
                        .to_string(),
                )
            })?;
        let mut propagation_work = 0usize;
        for component in topo.into_iter().rev() {
            let span = self.nodes[components[component][0]].raw_span;
            let mut downstream = BTreeSet::new();
            for target in &component_edges[component] {
                propagation_work = checked_authority_budget(
                    propagation_work,
                    component_effects[*target].len(),
                    self.limits.route_effects,
                    span,
                    "propagation work",
                )?;
                for effect in &component_effects[*target] {
                    if !component_effects[component].contains(effect) {
                        downstream.insert(effect.clone());
                    }
                }
            }
            propagated_entries = checked_authority_budget(
                propagated_entries,
                downstream.len(),
                self.limits.route_effects,
                span,
                "propagated effect",
            )?;
            component_effects[component].extend(downstream);
        }
        let mut route_effects = 0usize;
        for component in &component_of {
            let span = self.nodes[components[*component][0]].raw_span;
            route_effects = checked_authority_budget(
                route_effects,
                component_effects[*component].len(),
                self.limits.route_effects,
                span,
                "route effect",
            )?;
        }
        Ok(Propagation {
            adjacency,
            component_of,
            component_effects,
        })
    }

    fn build_routes(
        &self,
        adjacency: &[Vec<(usize, usize)>],
    ) -> Result<Vec<AuthorityRoute>, CompilerFailure> {
        let mut routes = Vec::new();
        let mut witness_work = 0usize;
        for (route, node) in self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.callable.exposure != AuthorityExposure::Private)
        {
            let mut effects = Vec::new();
            for effect in &node.callable.transitive_effects {
                effects.push(AuthorityRouteEffect {
                    effect: effect.clone(),
                    witness: witness(
                        route,
                        effect,
                        adjacency,
                        &self.edges,
                        &self.nodes,
                        &mut witness_work,
                        self.limits.witness_work,
                    )?,
                });
            }
            routes.push(AuthorityRoute {
                callable: node.callable.id.clone(),
                effects,
            });
        }
        Ok(routes)
    }

    fn export_edges(&self) -> Vec<AuthorityEdge> {
        self.edges
            .iter()
            .map(|edge| AuthorityEdge {
                caller: self.nodes[edge.caller].callable.id.clone(),
                target: edge
                    .target
                    .map(|target| self.nodes[target].callable.id.clone()),
                span: edge.span.clone(),
                unresolved: edge.unresolved,
                reason: edge.reason.clone(),
            })
            .collect()
    }

    fn index_dependency_declaration(&mut self, declaration: &'a PackageDeclaration) {
        self.index_dependency_functions(declaration);
        self.index_dependency_classes(declaration);
        self.index_dependency_interfaces(declaration);
    }

    fn index_dependency_functions(&mut self, declaration: &'a PackageDeclaration) {
        for symbol in declaration.values.values() {
            if let crate::ValueKind::Function { params, doc, .. } = &symbol.kind {
                self.known_functions.insert(symbol.mangled_name.clone());
                if let Some(doc) = doc.as_ref().filter(|doc| !doc.capabilities.is_empty()) {
                    self.capability_functions.insert(
                        symbol.mangled_name.clone(),
                        CapabilityCallee {
                            params,
                            capabilities: &doc.capabilities,
                        },
                    );
                }
            }
        }
    }

    fn index_dependency_classes(&mut self, declaration: &'a PackageDeclaration) {
        for symbol in declaration
            .runtime_types
            .values()
            .chain(declaration.types.values())
        {
            if matches!(symbol.kind, TypeKind::Class { .. }) {
                self.external_classes
                    .insert(symbol.mangled_name.clone(), symbol);
                if let Some(surface) = external_class_surface(symbol) {
                    self.external_surfaces
                        .insert(symbol.mangled_name.clone(), Rc::new(surface));
                }
            }
        }
        for class in package_classes(declaration) {
            self.class_hierarchy
                .insert(class.name.clone(), class.parent.clone());
            for (name, method) in class.methods {
                let key = (class.name.clone(), name.clone());
                self.known_methods.insert(key.clone());
                self.dynamically_dispatched_methods.insert(key);
                if let Some(doc) = method
                    .doc
                    .as_ref()
                    .filter(|doc| !doc.capabilities.is_empty())
                {
                    self.capability_methods.insert(
                        (class.name.clone(), name.clone()),
                        CapabilityCallee {
                            params: &method.params,
                            capabilities: &doc.capabilities,
                        },
                    );
                }
            }
            for (name, method) in class.statics {
                let mangled = crate::mangle::static_member(&class.name, name);
                self.known_functions.insert(mangled.clone());
                if let Some(doc) = method
                    .doc
                    .as_ref()
                    .filter(|doc| !doc.capabilities.is_empty())
                {
                    self.capability_functions.insert(
                        mangled,
                        CapabilityCallee {
                            params: &method.params,
                            capabilities: &doc.capabilities,
                        },
                    );
                }
            }
        }
    }

    fn index_dependency_interfaces(&mut self, declaration: &'a PackageDeclaration) {
        for symbol in declaration
            .runtime_types
            .values()
            .chain(declaration.types.values())
        {
            let TypeKind::Interface {
                methods, dispatch, ..
            } = &symbol.kind
            else {
                continue;
            };
            for (name, method) in methods {
                let key = (symbol.mangled_name.clone(), name.clone());
                self.known_methods.insert(key.clone());
                if *dispatch == crate::Dispatch::VTable {
                    self.dynamically_dispatched_methods.insert(key.clone());
                }
                if let Some(doc) = method
                    .doc
                    .as_ref()
                    .filter(|doc| !doc.capabilities.is_empty())
                {
                    self.capability_methods.insert(
                        key,
                        CapabilityCallee {
                            params: &method.params,
                            capabilities: &doc.capabilities,
                        },
                    );
                }
            }
        }
    }

    fn local_method(&self, class: &MangledName, name: &str) -> Option<usize> {
        let mut class = class.clone();
        for _ in 0..=self.classes.len() {
            if let Some(&target) = self.methods.get(&(class.clone(), name.to_string())) {
                return Some(target);
            }
            class = self.classes.get(&class)?.clone()?;
        }
        None
    }

    fn capability_method(&self, class: &MangledName, name: &str) -> Option<CapabilityCallee<'a>> {
        let mut class = class.clone();
        for _ in 0..=self.class_hierarchy.len() {
            if self
                .known_methods
                .contains(&(class.clone(), name.to_string()))
            {
                return self
                    .capability_methods
                    .get(&(class.clone(), name.to_string()))
                    .copied();
            }
            class = self.class_hierarchy.get(&class)?.clone()?;
        }
        None
    }

    fn known_method(&self, class: &MangledName, name: &str) -> bool {
        let mut class = class.clone();
        for _ in 0..=self.class_hierarchy.len() {
            if self
                .known_methods
                .contains(&(class.clone(), name.to_string()))
            {
                return true;
            }
            let Some(parent) = self.class_hierarchy.get(&class) else {
                return false;
            };
            let Some(parent) = parent else {
                return false;
            };
            class = parent.clone();
        }
        false
    }

    fn method_is_dynamic(&self, class: &MangledName, name: &str) -> bool {
        let mut class = class.clone();
        for _ in 0..=self.class_hierarchy.len() {
            let key = (class.clone(), name.to_string());
            if self.known_methods.contains(&key) || self.methods.contains_key(&key) {
                return self.dynamically_dispatched_methods.contains(&key);
            }
            let Some(Some(parent)) = self.class_hierarchy.get(&class) else {
                return false;
            };
            class = parent.clone();
        }
        false
    }

    fn external_accessor_exists(&self, class: &MangledName, name: &str, setter: bool) -> bool {
        let Some(symbol) = self.external_classes.get(class) else {
            return false;
        };
        let TypeKind::Class {
            fields, accessors, ..
        } = &symbol.kind
        else {
            return false;
        };
        fields
            .get(name)
            .is_none_or(|field| field.visibility == Visibility::Public)
            && accessors.iter().any(|accessor| match accessor {
                crate::AccessorSig::Getter {
                    name: candidate, ..
                } => !setter && candidate == name,
                crate::AccessorSig::Setter {
                    name: candidate, ..
                } => setter && candidate == name,
            })
    }

    fn held_callable(&self, mut id: ExprId) -> Result<Option<usize>, CompilerFailure> {
        for _ in 0..self.ta.exprs_len() {
            let expression = self
                .ta
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            match &expression.kind {
                TypedExprKind::Closure { .. } => {
                    return Ok(self.closure_nodes.get(&id).copied());
                }
                TypedExprKind::FunctionRef { mangled, .. } => {
                    return Ok(self.named.get(mangled).copied());
                }
                TypedExprKind::GlobalRef { mangled, .. } => {
                    return Ok(self.closure_globals.get(mangled).copied());
                }
                TypedExprKind::Cast { value, .. }
                | TypedExprKind::NonNullAssert { value }
                | TypedExprKind::Narrowed { inner: value, .. }
                | TypedExprKind::EffectThen { result: value, .. }
                | TypedExprKind::Sequence { result: value, .. } => id = *value,
                _ => return Ok(None),
            }
        }
        Err(crate::typechecker::invariant_failure(
            "closure value wrappers form a cycle",
        ))
    }

    fn constructor_is_public(&self, class: &MangledName) -> bool {
        source_type_symbol(self.declaration, class).is_none_or(|symbol| {
            !matches!(
                &symbol.kind,
                TypeKind::Class {
                    constructor_visibility: Visibility::Private,
                    ..
                }
            )
        })
    }
}

struct CallVisitor<'a, 'b> {
    builder: &'a mut Builder<'b>,
    caller: usize,
}

impl Visitor for CallVisitor<'_, '_> {
    fn visit_stmt(&mut self, kind: &TypedStmtKind) -> Result<(), CompilerFailure> {
        if let TypedStmtKind::AssignField { receiver, name, .. } = kind {
            self.accessor(*receiver, &name.name, true, name.span)?;
        }
        if let TypedStmtKind::Return(Some(value)) = kind
            && let Some(target) = self.builder.held_callable(*value)?
        {
            let span = self
                .builder
                .ta
                .try_expr(*value)
                .map_err(crate::typechecker::arena_failure)?
                .span;
            self.builder.add_exposure_edge(self.caller, target, span)?;
        }
        Ok(())
    }

    fn descend_into_closures(&self) -> bool {
        false
    }

    fn visit_expr(&mut self, id: ExprId, kind: &TypedExprKind) -> Result<(), CompilerFailure> {
        let span = self
            .builder
            .ta
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?
            .span;
        match kind {
            TypedExprKind::Call { mangled, args, .. } => self.direct_call(mangled, args, span)?,
            TypedExprKind::GenericCall { mangled, args, .. } => {
                let args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
                self.direct_call(mangled, &args, span)?;
            }
            TypedExprKind::MethodCall {
                iface, name, args, ..
            } => {
                self.method_call(iface, &name.name, args, span)?;
            }
            TypedExprKind::SuperMethodCall { owner, name, args } => {
                self.super_method_call(owner, &name.name, args, span)?;
            }
            TypedExprKind::GenericMethodCall {
                iface, name, args, ..
            } => {
                let args = args.iter().map(|arg| arg.expr).collect::<Vec<_>>();
                self.method_call(iface, &name.name, &args, span)?;
            }
            TypedExprKind::SuperCtorCall { parent, .. } => {
                let target = crate::mangle::extend(parent, "constructor");
                if let Some(&target) = self.builder.named.get(&target) {
                    self.builder.add_edge(self.caller, target, span)?;
                } else {
                    self.builder.add_unresolved(
                        self.caller,
                        span,
                        "external super-constructor target",
                    )?;
                }
            }
            TypedExprKind::CallClosure { callee, .. } => {
                if let Some(target) = self.closure_target(*callee)? {
                    self.builder.add_edge(self.caller, target, span)?;
                } else {
                    self.builder
                        .add_unresolved(self.caller, span, "dynamic closure target")?;
                }
            }
            TypedExprKind::McpCall { server, tool, .. } => {
                self.builder.push_effect(
                    self.caller,
                    AuthorityEffect {
                        capability: Some(format!("mcp.{server}")),
                        sink: source_span(self.builder.sources, span)?,
                        known_bindings: BTreeMap::from([
                            ("tool".to_string(), format!("\"{tool}\"")),
                            ("transport".to_string(), "\"streamable_http\"".to_string()),
                        ]),
                        unresolved: false,
                        reason: None,
                    },
                    span,
                )?;
            }
            TypedExprKind::FieldAccess { receiver, name }
            | TypedExprKind::InterfacePropertyAccess { receiver, name, .. } => {
                self.accessor(*receiver, &name.name, false, span)?;
            }
            TypedExprKind::PostfixUnary {
                target: PostfixTarget::Field { receiver, name, .. },
                ..
            } => {
                self.accessor(*receiver, &name.name, false, span)?;
                self.accessor(*receiver, &name.name, true, span)?;
            }
            TypedExprKind::OptionalChain { base, parts } => {
                self.optional_chain(*base, parts)?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl CallVisitor<'_, '_> {
    fn optional_chain(
        &mut self,
        base: ExprId,
        parts: &[TypedChainPart],
    ) -> Result<(), CompilerFailure> {
        let mut receiver_type = self
            .builder
            .ta
            .try_expr(base)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        for part in parts {
            match part {
                TypedChainPart::MethodCall {
                    iface,
                    name,
                    args,
                    result_ty,
                    span,
                    ..
                } => {
                    self.method_call(iface, &name.name, args, *span)?;
                    receiver_type = result_ty.clone();
                }
                TypedChainPart::Call {
                    result_ty, span, ..
                } => {
                    self.builder.add_unresolved(
                        self.caller,
                        *span,
                        "dynamic optional-call target",
                    )?;
                    receiver_type = result_ty.clone();
                }
                TypedChainPart::Field {
                    name,
                    result_ty,
                    span,
                    ..
                } => {
                    self.accessor_for_type(&receiver_type, &name.name, false, *span)?;
                    receiver_type = result_ty.clone();
                }
                TypedChainPart::InterfaceProperty {
                    result_ty, span, ..
                } => {
                    self.builder.add_unresolved(
                        self.caller,
                        *span,
                        "interface property may invoke an accessor",
                    )?;
                    receiver_type = result_ty.clone();
                }
                TypedChainPart::Index { result_ty, .. }
                | TypedChainPart::NonNull { result_ty, .. } => {
                    receiver_type = result_ty.clone();
                }
            }
        }
        Ok(())
    }

    fn direct_call(
        &mut self,
        mangled: &MangledName,
        args: &[ExprId],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if let Some(&target) = self.builder.named.get(mangled) {
            return self.builder.add_edge(self.caller, target, span);
        }
        if mangled.as_str() == "submilli:http#request" {
            return self.dynamic_http(args, span);
        }
        if let Some(callee) = self.builder.capability_functions.get(mangled).copied() {
            self.builder
                .add_capability_effects(self.caller, callee, args, span)?;
        } else if !self.builder.known_functions.contains(mangled) {
            self.builder
                .add_unresolved(self.caller, span, "unresolved direct call target")?;
        }
        Ok(())
    }

    fn dynamic_http(&mut self, args: &[ExprId], span: Span) -> Result<(), CompilerFailure> {
        let method = match args.first() {
            Some(id) => self
                .builder
                .ta
                .try_expr(*id)
                .map_err(crate::typechecker::arena_failure)?,
            None => return self.dynamic_http_effect(None, BTreeMap::new(), span),
        };
        let method = match &method.kind {
            TypedExprKind::String(method) => Some(method.to_ascii_lowercase()),
            _ => None,
        };
        let mut known_bindings = BTreeMap::new();
        if let Some(id) = args.get(1) {
            let url = self
                .builder
                .ta
                .try_expr(*id)
                .map_err(crate::typechecker::arena_failure)?;
            if let TypedExprKind::String(url) = &url.kind
                && let Ok(parsed) = url::Url::parse(url)
            {
                if let Some(host) = parsed.host_str() {
                    known_bindings.insert("host".to_string(), format!("\"{host}\""));
                }
                known_bindings.insert("path".to_string(), format!("\"{}\"", parsed.path()));
            }
        }
        self.dynamic_http_effect(method, known_bindings, span)
    }

    fn dynamic_http_effect(
        &mut self,
        method: Option<String>,
        known_bindings: BTreeMap<String, String>,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        self.builder.push_effect(
            self.caller,
            AuthorityEffect {
                capability: Some(method.as_ref().map_or_else(
                    || "http.<method>".to_string(),
                    |method| format!("http.{method}"),
                )),
                sink: source_span(self.builder.sources, span)?,
                known_bindings,
                unresolved: method.is_none(),
                reason: method
                    .is_none()
                    .then(|| "HTTP method is selected at runtime".to_string()),
            },
            span,
        )?;
        Ok(())
    }

    fn method_call(
        &mut self,
        iface: &MangledName,
        name: &str,
        args: &[ExprId],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let resolved = if let Some(target) = self.builder.local_method(iface, name) {
            self.builder.add_edge(self.caller, target, span)?;
            true
        } else if let Some(callee) = self.builder.capability_method(iface, name) {
            self.builder
                .add_capability_effects(self.caller, callee, args, span)?;
            true
        } else {
            self.builder.known_method(iface, name)
        };
        if !resolved {
            self.builder
                .add_unresolved(self.caller, span, "unresolved method target")?;
        } else if self.builder.method_is_dynamic(iface, name) {
            self.builder.add_unresolved(
                self.caller,
                span,
                "virtual method target may be overridden",
            )?;
        }
        Ok(())
    }

    fn super_method_call(
        &mut self,
        owner: &MangledName,
        name: &str,
        args: &[ExprId],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if let Some(&target) = self.builder.methods.get(&(owner.clone(), name.to_string())) {
            return self.builder.add_edge(self.caller, target, span);
        }
        if let Some(callee) = self
            .builder
            .capability_methods
            .get(&(owner.clone(), name.to_string()))
            .copied()
        {
            return self
                .builder
                .add_capability_effects(self.caller, callee, args, span);
        }
        if !self
            .builder
            .known_methods
            .contains(&(owner.clone(), name.to_string()))
        {
            self.builder
                .add_unresolved(self.caller, span, "unresolved super-method target")?;
        }
        Ok(())
    }

    fn closure_target(&self, id: ExprId) -> Result<Option<usize>, CompilerFailure> {
        self.builder.held_callable(id)
    }

    fn accessor(
        &mut self,
        receiver: ExprId,
        name: &str,
        setter: bool,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let receiver = self
            .builder
            .ta
            .try_expr(receiver)
            .map_err(crate::typechecker::arena_failure)?;
        self.accessor_for_type(&receiver.ty, name, setter, span)
    }

    fn accessor_for_type(
        &mut self,
        receiver: &Type,
        name: &str,
        setter: bool,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let receiver = crate::typechecker::infer::narrowing::strip_null(receiver);
        let Type::ClassRef { mangled, .. } = receiver.peel() else {
            if receiver.is_structural_object() {
                self.builder.add_unresolved(
                    self.caller,
                    span,
                    "structural property may invoke an accessor",
                )?;
            }
            return Ok(());
        };
        self.accessor_for_class(mangled, name, setter, span)
    }

    fn accessor_for_class(
        &mut self,
        class: &MangledName,
        name: &str,
        setter: bool,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let mut class = class.clone();
        for _ in 0..=self.builder.class_hierarchy.len() {
            if let Some(&target) =
                self.builder
                    .accessors
                    .get(&(class.clone(), name.to_string(), setter))
            {
                self.builder.add_edge(self.caller, target, span)?;
                self.builder.add_unresolved(
                    self.caller,
                    span,
                    "virtual accessor target may be overridden",
                )?;
                return Ok(());
            }
            if self.builder.external_accessor_exists(&class, name, setter) {
                self.builder.add_unresolved(
                    self.caller,
                    span,
                    "external accessor body is not locally analyzable",
                )?;
                return Ok(());
            }
            let Some(Some(parent)) = self.builder.class_hierarchy.get(&class) else {
                return Ok(());
            };
            class = parent.clone();
        }
        Ok(())
    }
}

fn exports(ta: &TypedAst, kind: ExportKind) -> BTreeSet<MangledName> {
    ta.exports
        .iter()
        .filter(|entry| entry.kind == kind)
        .map(|entry| entry.target.clone())
        .collect()
}

fn public_static_functions(
    declaration: &PackageDeclaration,
    exported_classes: &BTreeSet<MangledName>,
) -> BTreeSet<MangledName> {
    declaration
        .types
        .values()
        .filter(|symbol| exported_classes.contains(&symbol.mangled_name))
        .flat_map(|symbol| {
            let TypeKind::Class {
                statics,
                static_visibility,
                ..
            } = &symbol.kind
            else {
                return Vec::new();
            };
            statics
                .keys()
                .filter(|name| static_visibility.get(*name) != Some(&Visibility::Private))
                .map(|name| crate::mangle::static_member(&symbol.mangled_name, name))
                .collect()
        })
        .collect()
}

fn own_static_names(ta: &TypedAst, class: &MangledName) -> BTreeSet<String> {
    let prefix = format!("{class}#static#");
    ta.functions
        .iter()
        .map(|function| &function.mangled_name)
        .chain(ta.globals.iter().map(|global| &global.mangled_name))
        .filter_map(|mangled| mangled.as_str().strip_prefix(&prefix).map(str::to_string))
        .collect()
}

fn may_be_callable(ty: &Type) -> bool {
    match ty.peel() {
        Type::Function { .. } => true,
        Type::Union(members) => members.iter().any(may_be_callable),
        _ => false,
    }
}

fn source_type_symbol<'a>(
    declaration: &'a PackageDeclaration,
    mangled: &MangledName,
) -> Option<&'a crate::TypeSymbol> {
    declaration
        .types
        .values()
        .find(|symbol| &symbol.mangled_name == mangled)
        .or_else(|| declaration.type_symbol(mangled))
}

struct PackageClass<'a> {
    name: MangledName,
    methods: &'a BTreeMap<String, crate::MethodSig>,
    statics: &'a BTreeMap<String, crate::MethodSig>,
    parent: Option<MangledName>,
}

struct ExternalClassSurface {
    methods: Vec<String>,
    accessors: Vec<(String, bool)>,
    function_fields: Vec<String>,
    statics: Vec<String>,
    static_function_fields: Vec<String>,
    parent: Option<MangledName>,
}

impl ExternalClassSurface {
    fn member_count(&self) -> usize {
        self.methods
            .len()
            .saturating_add(self.accessors.len())
            .saturating_add(self.function_fields.len())
            .saturating_add(self.statics.len())
            .saturating_add(self.static_function_fields.len())
    }
}

struct InheritedNames {
    methods: BTreeSet<String>,
    accessors: BTreeSet<(String, bool)>,
    statics: BTreeSet<String>,
    fields: BTreeSet<String>,
}

fn external_class_surface(symbol: &crate::TypeSymbol) -> Option<ExternalClassSurface> {
    let TypeKind::Class {
        fields,
        methods,
        method_visibility,
        accessors,
        statics,
        static_visibility,
        static_fields,
        extends,
        ..
    } = &symbol.kind
    else {
        return None;
    };
    Some(ExternalClassSurface {
        methods: methods
            .keys()
            .filter(|name| method_visibility.get(*name) != Some(&Visibility::Private))
            .cloned()
            .collect(),
        accessors: accessors
            .iter()
            .filter(|accessor| {
                let name = match accessor {
                    crate::AccessorSig::Getter { name, .. }
                    | crate::AccessorSig::Setter { name, .. } => name,
                };
                fields
                    .get(name)
                    .is_none_or(|field| field.visibility == Visibility::Public)
            })
            .map(|accessor| match accessor {
                crate::AccessorSig::Getter { name, .. } => (name.clone(), false),
                crate::AccessorSig::Setter { name, .. } => (name.clone(), true),
            })
            .collect(),
        function_fields: fields
            .iter()
            .filter(|(_, field)| {
                field.visibility == Visibility::Public && may_be_callable(&field.ty)
            })
            .map(|(name, _)| name.clone())
            .collect(),
        statics: statics
            .keys()
            .filter(|name| static_visibility.get(*name) != Some(&Visibility::Private))
            .cloned()
            .collect(),
        static_function_fields: static_fields
            .iter()
            .filter(|(_, field)| {
                field.visibility == Visibility::Public && may_be_callable(&field.ty)
            })
            .map(|(name, _)| name.clone())
            .collect(),
        parent: extends.as_ref().map(|extends| extends.parent.clone()),
    })
}

fn package_classes(declaration: &PackageDeclaration) -> Vec<PackageClass<'_>> {
    declaration
        .runtime_types
        .values()
        .chain(declaration.types.values())
        .filter_map(|symbol| {
            if let TypeKind::Class {
                methods,
                statics,
                extends,
                ..
            } = &symbol.kind
            {
                Some(PackageClass {
                    name: symbol.mangled_name.clone(),
                    methods,
                    statics,
                    parent: extends.as_ref().map(|parent| parent.parent.clone()),
                })
            } else {
                None
            }
        })
        .collect()
}

fn source_span(sources: &Sources, span: Span) -> Result<AuthoritySpan, CompilerFailure> {
    let source = sources.get(span.file).ok_or_else(|| {
        crate::typechecker::invariant_failure("authority span names an unregistered source")
    })?;
    let (start_line, start_column) = source
        .line_index()
        .line_col(span.start)
        .map_err(|_| crate::typechecker::invariant_failure("authority span start is invalid"))?;
    let (end_line, end_column) = source
        .line_index()
        .line_col(span.end)
        .map_err(|_| crate::typechecker::invariant_failure("authority span end is invalid"))?;
    Ok(AuthoritySpan {
        path: source.path.as_str().to_string(),
        start: AuthorityPosition {
            byte: span.start,
            line: start_line,
            column: start_column,
        },
        end: AuthorityPosition {
            byte: span.end,
            line: end_line,
            column: end_column,
        },
    })
}

fn limit(span: Span, message: String) -> CompilerFailure {
    CompilerFailure::Limit {
        stage: CompilerStage::Infer,
        span: Some(span),
        message,
        help: vec![
            "split the package into smaller modules or reduce generated call sites".to_string(),
        ],
    }
}

fn check_limit(
    current: usize,
    maximum: usize,
    span: Span,
    subject: &str,
) -> Result<(), CompilerFailure> {
    if current < maximum {
        return Ok(());
    }
    Err(limit(
        span,
        format!("authority analysis exceeds the {subject} limit of {maximum}"),
    ))
}

fn checked_authority_budget(
    current: usize,
    additional: usize,
    maximum: usize,
    span: Span,
    subject: &str,
) -> Result<usize, CompilerFailure> {
    let Some(next) = current.checked_add(additional) else {
        return Err(limit(
            span,
            format!("authority analysis {subject} count overflows the platform limit"),
        ));
    };
    if next > maximum {
        return Err(limit(
            span,
            format!("authority analysis exceeds the {subject} limit of {maximum}"),
        ));
    }
    Ok(next)
}

fn edge_key(
    edge: &RawEdge,
    nodes: &[Node],
) -> (String, AuthoritySpan, Option<String>, bool, Option<String>) {
    (
        nodes[edge.caller].callable.id.clone(),
        edge.span.clone(),
        edge.target.map(|target| nodes[target].callable.id.clone()),
        edge.unresolved,
        edge.reason.clone(),
    )
}

fn adjacency(count: usize, edges: &[RawEdge]) -> Vec<Vec<(usize, usize)>> {
    let mut adjacency = vec![Vec::new(); count];
    for (edge_index, edge) in edges.iter().enumerate() {
        if let Some(target) = edge.target {
            adjacency[edge.caller].push((target, edge_index));
        }
    }
    adjacency
}

fn component_dag(
    component_of: &[usize],
    component_count: usize,
    edges: &[RawEdge],
) -> (Vec<BTreeSet<usize>>, Vec<usize>) {
    let mut component_edges = vec![BTreeSet::new(); component_count];
    let mut indegree = vec![0usize; component_count];
    for edge in edges {
        let Some(target) = edge.target else {
            continue;
        };
        let caller_component = component_of[edge.caller];
        let target_component = component_of[target];
        if caller_component != target_component
            && component_edges[caller_component].insert(target_component)
        {
            indegree[target_component] = indegree[target_component].saturating_add(1);
        }
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(component, degree)| (*degree == 0).then_some(component))
        .collect::<BTreeSet<_>>();
    let mut topo = Vec::with_capacity(component_count);
    while let Some(component) = ready.pop_first() {
        topo.push(component);
        for target in component_edges[component].iter().copied() {
            indegree[target] = indegree[target].saturating_sub(1);
            if indegree[target] == 0 {
                ready.insert(target);
            }
        }
    }
    (component_edges, topo)
}

fn strongly_connected_components(
    adjacency: &[Vec<(usize, usize)>],
) -> (Vec<usize>, Vec<Vec<usize>>) {
    let mut seen = vec![false; adjacency.len()];
    let mut finish = Vec::with_capacity(adjacency.len());
    for start in 0..adjacency.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![(start, 0usize)];
        while let Some((node, next)) = stack.last_mut() {
            if let Some((target, _)) = adjacency[*node].get(*next) {
                *next = next.saturating_add(1);
                if !seen[*target] {
                    seen[*target] = true;
                    stack.push((*target, 0));
                }
            } else {
                finish.push(*node);
                stack.pop();
            }
        }
    }
    let mut reverse = vec![Vec::new(); adjacency.len()];
    for (caller, targets) in adjacency.iter().enumerate() {
        for (target, _) in targets {
            reverse[*target].push(caller);
        }
    }
    reverse
        .iter_mut()
        .for_each(|sources| sources.sort_unstable());
    let mut component_of = vec![usize::MAX; adjacency.len()];
    let mut components = Vec::new();
    for start in finish.into_iter().rev() {
        if component_of[start] != usize::MAX {
            continue;
        }
        let component = components.len();
        let mut members = Vec::new();
        let mut pending = vec![start];
        component_of[start] = component;
        while let Some(node) = pending.pop() {
            members.push(node);
            for source in reverse[node].iter().rev().copied() {
                if component_of[source] == usize::MAX {
                    component_of[source] = component;
                    pending.push(source);
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }
    (component_of, components)
}

fn witness(
    root: usize,
    effect: &AuthorityEffect,
    adjacency: &[Vec<(usize, usize)>],
    edges: &[RawEdge],
    nodes: &[Node],
    work: &mut usize,
    maximum_work: usize,
) -> Result<Vec<AuthorityWitnessStep>, CompilerFailure> {
    if nodes[root]
        .callable
        .direct_effects
        .binary_search(effect)
        .is_ok()
    {
        return Ok(Vec::new());
    }
    let traversal_bound = nodes.len().checked_add(edges.len()).ok_or_else(|| {
        limit(
            nodes[root].raw_span,
            "authority witness-work count overflows the platform limit".to_string(),
        )
    })?;
    *work = checked_authority_budget(
        *work,
        traversal_bound,
        maximum_work,
        nodes[root].raw_span,
        "witness work",
    )?;
    let mut previous: Vec<Option<(usize, usize)>> = vec![None; nodes.len()];
    let mut seen = BTreeSet::from([root]);
    let mut pending = VecDeque::from([root]);
    let mut found = None;
    while let Some(node) = pending.pop_front() {
        for (target, edge_index) in &adjacency[node] {
            if !seen.insert(*target) {
                continue;
            }
            previous[*target] = Some((node, *edge_index));
            if nodes[*target]
                .callable
                .direct_effects
                .binary_search(effect)
                .is_ok()
            {
                found = Some(*target);
                break;
            }
            pending.push_back(*target);
        }
        if found.is_some() {
            break;
        }
    }
    let Some(mut current) = found else {
        return Ok(Vec::new());
    };
    let mut path = Vec::new();
    while current != root {
        let Some((caller, edge_index)) = previous[current] else {
            return Ok(Vec::new());
        };
        path.push(AuthorityWitnessStep {
            caller: nodes[caller].callable.id.clone(),
            target: nodes[current].callable.id.clone(),
            span: edges[edge_index].span.clone(),
        });
        current = caller;
    }
    path.reverse();
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ModulePath, PackageSourceModule, Param, Type, ValueKind, ValueSymbol, compile_package,
    };

    fn compile(modules: &[(&str, &str)]) -> AuthorityMap {
        compile_with_dependencies(modules, &[])
    }

    fn compile_with_dependencies(
        modules: &[(&str, &str)],
        dependencies: &[&PackageDeclaration],
    ) -> AuthorityMap {
        let modules = modules
            .iter()
            .map(|(path, source)| PackageSourceModule {
                path: ModulePath::from(*path),
                source,
            })
            .collect::<Vec<_>>();
        compile_package(
            "@test/authority",
            ModulePath::from("lib"),
            &modules,
            dependencies,
        )
        .unwrap_or_else(|diagnostics| panic!("compile failed: {diagnostics:#?}"))
        .authority_map
    }

    fn route<'a>(map: &'a AuthorityMap, suffix: &str) -> &'a AuthorityRoute {
        map.routes
            .iter()
            .find(|route| route.callable.ends_with(suffix))
            .unwrap_or_else(|| panic!("missing route ending in {suffix}: {:#?}", map.routes))
    }

    #[test]
    fn direct_and_recursive_helpers_propagate_one_http_effect_with_a_witness() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:http";
                function first(url: string): void { second(url); }
                function second(url: string): void { if (url !== "") { first(""); } get(url); }
                export function fetch(): void { first("https://example.com/data"); }
            "#,
        )]);
        let route = route(&map, "#fetch");
        let effect = route
            .effects
            .iter()
            .find(|effect| effect.effect.capability.as_deref() == Some("http.get"))
            .expect("transitive HTTP effect");
        assert_eq!(effect.witness.len(), 2, "{effect:#?}");
        assert!(effect.witness[0].target.ends_with("#first"));
        assert!(effect.witness[1].target.ends_with("#second"));
    }

    #[test]
    fn root_reexport_is_public_but_unreachable_private_helper_is_not_a_route() {
        let map = compile(&[
            ("lib", "export { send } from \"./internal\";"),
            (
                "internal",
                r#"
                    import { get } from "submilli:secrets";
                    export function send(): void { get("TOKEN"); }
                    function hidden(): void { get("HIDDEN"); }
                "#,
            ),
        ]);
        assert!(
            map.routes
                .iter()
                .any(|route| route.callable.ends_with("#send"))
        );
        assert!(
            !map.routes
                .iter()
                .any(|route| route.callable.ends_with("#hidden"))
        );
        assert!(route(&map, "#send").effects.iter().any(|effect| {
            effect.effect.capability.as_deref() == Some("secrets.get")
                && effect.effect.known_bindings.get("name") == Some(&"\"TOKEN\"".to_string())
        }));
    }

    #[test]
    fn dynamic_closure_and_http_method_are_explicit_uncertainties() {
        let map = compile(&[(
            "lib",
            r#"
                import { request } from "submilli:http";
                export function run(method: string, callback: () => void): void {
                    callback();
                    request(method, "https://example.com/v1");
                }
            "#,
        )]);
        let route = route(&map, "#run");
        assert!(route.effects.iter().any(|effect| {
            effect.effect.capability.as_deref() == Some("http.<method>") && effect.effect.unresolved
        }));
        assert!(route.effects.iter().any(|effect| {
            effect.effect.capability.is_none()
                && effect.effect.reason.as_deref() == Some("dynamic closure target")
        }));
        assert!(
            map.edges
                .iter()
                .any(|edge| edge.unresolved && edge.target.is_none())
        );
    }

    #[test]
    fn static_filesystem_mcp_and_tagged_dependency_sinks_are_reported() {
        let dependency_source = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: r#"
                /** @capability vendor.send { queue: $queue } */
                export function send(queue: string): void {}
            "#,
        }];
        let dependency = compile_package(
            "@vendor/queue",
            ModulePath::from("lib"),
            &dependency_source,
            &[],
        )
        .expect("dependency compiles")
        .declaration;

        let mut mcp = PackageDeclaration::with_package("@mcp/issues");
        mcp.mcp_server = Some("issues".to_string());
        mcp.values.insert(
            "create".to_string(),
            ValueSymbol {
                name: "create".to_string(),
                mangled_name: crate::mangle::package_symbol("@mcp/issues", "create"),
                declaration_span: Span::at(crate::FileId::MCP),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("title", Type::String)],
                    ret: Type::Unknown,
                    type_predicate: None,
                    doc: None,
                },
            },
        );

        let map = compile_with_dependencies(
            &[(
                "lib",
                r#"
                    import { readText } from "submilli:fs";
                    import issues from "@mcp/issues";
                    import { send } from "@vendor/queue";
                    export class Client {
                        static run(): void {
                            readText("/workspace/input.txt");
                            issues.create("bug");
                            send("urgent");
                        }
                    }
                "#,
            )],
            &[&dependency, &mcp],
        );
        let route = map
            .routes
            .iter()
            .find(|route| route.callable.contains("#static#run"))
            .expect("public static route");
        let capabilities = route
            .effects
            .iter()
            .filter_map(|effect| effect.effect.capability.as_deref())
            .collect::<BTreeSet<_>>();
        assert!(capabilities.contains("fs.read"), "{capabilities:?}");
        assert!(capabilities.contains("mcp.issues"), "{capabilities:?}");
        assert!(capabilities.contains("vendor.send"), "{capabilities:?}");
    }

    #[test]
    fn accessor_reads_and_writes_link_to_getter_and_setter_bodies() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                class Vault {
                    get secret(): string { return get("READ_SECRET")!; }
                    set secret(value: string) { get("WRITE_SECRET"); }
                }
                export function run(): string {
                    const vault = new Vault();
                    vault.secret = "replacement";
                    return vault.secret;
                }
            "#,
        )]);
        let route = route(&map, "#run");
        let names = route
            .effects
            .iter()
            .filter_map(|effect| effect.effect.known_bindings.get("name"))
            .collect::<BTreeSet<_>>();
        assert!(names.contains(&"\"READ_SECRET\"".to_string()), "{names:?}");
        assert!(names.contains(&"\"WRITE_SECRET\"".to_string()), "{names:?}");
        assert_eq!(
            route
                .effects
                .iter()
                .filter(|effect| effect.effect.capability.is_some())
                .count(),
            2,
            "{route:#?}"
        );
    }

    #[test]
    fn implicit_constructor_includes_parent_initialization_effects() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                class Base { constructor() { get("PARENT"); } }
                class Child extends Base {}
                export function run(): void { new Child(); }
            "#,
        )]);
        assert!(route(&map, "#run").effects.iter().any(|effect| {
            effect.effect.capability.as_deref() == Some("secrets.get")
                && effect.effect.known_bindings.get("name") == Some(&"\"PARENT\"".to_string())
        }));
    }

    #[test]
    fn virtual_dispatch_is_uncertain_and_inherited_public_methods_are_routes() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                export class Base {
                    run(): void {}
                }
                class Child extends Base { run(): void { get("OVERRIDE"); } }
                class Hidden {
                    static readonly action: (() => void) | null = (): void => { get("STATIC"); };
                    inherited(): void { get("INHERITED"); }
                }
                export class Exposed extends Hidden {}
                export function dispatch(value: Base): void { value.run(); }
            "#,
        )]);
        assert!(route(&map, "#dispatch").effects.iter().any(|effect| {
            effect.effect.reason.as_deref() == Some("virtual method target may be overridden")
        }));
        assert!(route(&map, "#inherited").effects.iter().any(|effect| {
            effect.effect.known_bindings.get("name") == Some(&"\"INHERITED\"".to_string())
        }));
        assert!(
            map.routes.iter().any(|route| {
                route.effects.iter().any(|effect| {
                    effect.effect.known_bindings.get("name") == Some(&"\"STATIC\"".to_string())
                        || effect.effect.reason.as_deref()
                            == Some("static function field target is not directly recoverable")
                })
            }),
            "{map:#?}"
        );
    }

    #[test]
    fn exported_and_returned_function_values_become_public_routes() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                function helper(): void { get("ALIAS"); }
                export const alias = helper;
                function nested(): () => void {
                    return () => { get("RETURNED"); };
                }
                export function make(): () => () => void { return nested; }
                export const expressionMake = (): (() => void) =>
                    () => { get("EXPRESSION"); };
                function choose(): (() => void) | null {
                    return () => { get("INDIRECT"); };
                }
                export const indirect: (() => void) | null = choose();
            "#,
        )]);
        assert!(route(&map, "#helper").effects.iter().any(|effect| {
            effect.effect.known_bindings.get("name") == Some(&"\"ALIAS\"".to_string())
        }));
        assert!(map.routes.iter().any(|route| {
            route.callable.starts_with("closure:")
                && route.effects.iter().any(|effect| {
                    effect.effect.known_bindings.get("name") == Some(&"\"RETURNED\"".to_string())
                })
        }));
        assert!(map.routes.iter().any(|route| {
            route.callable.starts_with("closure:")
                && route.effects.iter().any(|effect| {
                    effect.effect.known_bindings.get("name") == Some(&"\"EXPRESSION\"".to_string())
                })
        }));
        assert!(map.routes.iter().any(|route| {
            route.callable.starts_with("global-function:")
                && route.effects.iter().any(|effect| {
                    effect.effect.reason.as_deref()
                        == Some("function-valued global target is not directly recoverable")
                })
        }));
    }

    #[test]
    fn rebindable_function_globals_remain_uncertain() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                let handler: () => void = (): void => {};
                export function install(): void {
                    handler = (): void => { get("REBOUND"); };
                }
                export function run(): void { handler(); }
            "#,
        )]);
        assert!(route(&map, "#run").effects.iter().any(|effect| {
            effect.effect.reason.as_deref() == Some("function-valued global may be rebound")
        }));
    }

    #[test]
    fn public_instance_function_fields_are_routes() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                class Base {
                    readonly inherited: (() => void) | null = (): void => { get("INHERITED_FIELD"); };
                }
                export class Child extends Base {
                    own: () => void = (): void => { get("OWN_FIELD"); };
                }
            "#,
        )]);
        for binding in ["\"INHERITED_FIELD\"", "\"OWN_FIELD\""] {
            assert!(map.routes.iter().any(|route| {
                route.effects.iter().any(|effect| {
                    effect.effect.known_bindings.get("name") == Some(&binding.to_string())
                })
            }));
        }
        assert!(map.routes.iter().any(|route| {
            route.effects.iter().any(|effect| {
                effect.effect.reason.as_deref() == Some("public function field may be reassigned")
            })
        }));
    }

    #[test]
    fn optional_inherited_accessor_links_to_the_getter() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                class Base { get secret(): string { return get("OPTIONAL")!; } }
                export class Child extends Base {}
                export function read(child: Child | null): string {
                    return child?.secret ?? "";
                }
            "#,
        )]);
        assert!(route(&map, "#read").effects.iter().any(|effect| {
            effect.effect.known_bindings.get("name") == Some(&"\"OPTIONAL\"".to_string())
        }));
    }

    #[test]
    fn known_direct_interface_methods_are_not_uncertain() {
        let map = compile(&[(
            "lib",
            r#"
                export function append(): number {
                    const values = [1];
                    values.push(2);
                    return values.length;
                }
            "#,
        )]);
        assert!(route(&map, "#append").effects.is_empty(), "{map:#?}");
    }

    #[test]
    fn optional_chain_advances_after_calls_and_structural_properties_are_uncertain() {
        let map = compile(&[(
            "lib",
            r#"
                import { get } from "submilli:secrets";
                export class Vault {
                    get secret(): string { return get("CHAINED")!; }
                }
                export class Factory { make(): Vault { return new Vault(); } }
                export interface Shaped { readonly property: string; }
                export function read(factory: Factory | null): string {
                    return factory?.make().secret ?? "";
                }
                export function readShaped(value: Shaped): string {
                    return value.property;
                }
            "#,
        )]);
        assert!(route(&map, "#read").effects.iter().any(|effect| {
            effect.effect.known_bindings.get("name") == Some(&"\"CHAINED\"".to_string())
        }));
        assert!(route(&map, "#readShaped").effects.iter().any(|effect| {
            effect.effect.reason.as_deref() == Some("structural property may invoke an accessor")
        }));
    }

    #[test]
    fn external_inheritance_surfaces_and_super_calls_are_uncertain() {
        let dependency_source = [PackageSourceModule {
            path: ModulePath::from("lib"),
            source: r#"
                export class Parent {
                    constructor() {}
                    /** @capability vendor.send { queue: $queue, region: "us" } */
                    send(queue: string): void {}
                    inherited(): void {}
                    get visible(): string { return "visible"; }
                    private get hidden(): string { return "hidden"; }
                }
            "#,
        }];
        let dependency = compile_package(
            "@vendor/base",
            ModulePath::from("lib"),
            &dependency_source,
            &[],
        )
        .expect("dependency compiles")
        .declaration;
        let map = compile_with_dependencies(
            &[(
                "lib",
                r#"
                    import { Parent } from "@vendor/base";
                    export class Child extends Parent {
                        constructor() { super(); }
                    }
                    export function read(parent: Parent): string { return parent.visible; }
                "#,
            )],
            &[&dependency],
        );
        assert!(map.routes.iter().any(|route| {
            route.callable.starts_with("inherited:")
                && route.effects.iter().any(|effect| {
                    effect.effect.reason.as_deref()
                        == Some("external inherited member body is not locally analyzable")
                })
        }));
        assert!(map.routes.iter().any(|route| {
            route.callable.starts_with("inherited:")
                && route.effects.iter().any(|effect| {
                    effect.effect.capability.as_deref() == Some("vendor.send")
                        && effect.effect.unresolved
                        && effect.effect.known_bindings.get("region") == Some(&"\"us\"".to_string())
                })
        }));
        assert!(
            !map.routes
                .iter()
                .any(|route| route.callable.contains("hidden"))
        );
        assert!(route(&map, "#constructor").effects.iter().any(|effect| {
            effect.effect.reason.as_deref() == Some("external super-constructor target")
        }));
        assert!(route(&map, "#read").effects.iter().any(|effect| {
            effect.effect.reason.as_deref()
                == Some("external accessor body is not locally analyzable")
        }));
    }

    #[test]
    fn analysis_limits_are_typed_compiler_failures_with_source_spans() {
        let span = Span::new(crate::FileId(7), 10, 20).expect("span");
        let error = check_limit(1, 1, span, "edge").expect_err("limit failure");
        assert!(matches!(
            error,
            CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                span: Some(actual),
                ..
            } if actual == span
        ));
        assert!(matches!(
            checked_authority_budget(1, 1, 1, span, "propagation work"),
            Err(CompilerFailure::Limit {
                stage: CompilerStage::Infer,
                span: Some(actual),
                ..
            }) if actual == span
        ));
    }
}
