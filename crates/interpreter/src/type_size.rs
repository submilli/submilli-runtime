//! Size and depth bounds for [`Type`] values.
//!
//! Types are trees: an alias or generic body that mentions its parameter twice
//! holds two copies of the argument, so instantiation can double a type's size
//! at every level. Every recursive walk over a type (clone, drop, equality,
//! assignability, lowering) costs time proportional to its size and stack
//! proportional to its depth. Substitution therefore builds under a
//! [`TypeBudget`] and stops at the limit instead of materializing the result,
//! and composed types are checked with [`check`] where they are recorded.
//!
//! A bound on each type does not bound how often a program rebuilds or compares
//! large types, so every substitution and comparison also draws from the work
//! allowance of its compiler phase, held in [`TypeLimits`].

use std::cell::Cell;

use crate::Type;
use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::compiler_limits::{MAX_TYPE_DEPTH, MAX_TYPE_NODES, MAX_TYPE_WORK};
use crate::span::Span;

/// A type limit or operational failure encountered while validating a type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeTooLarge {
    /// Temporary traversal storage could not be allocated.
    Allocation,
    Nodes,
    Depth,
    /// Dependency namespace containers or qualified paths exceed their limits.
    NamespaceMetadata,
    /// The phase's allowance for building and comparing types is spent.
    Work,
}

impl TypeTooLarge {
    pub fn into_failure(self, stage: CompilerStage, span: Option<Span>) -> CompilerFailure {
        if self == Self::Allocation {
            return CompilerFailure::Internal {
                stage,
                span,
                message: self.to_string(),
            };
        }
        let help = match self {
            Self::Allocation => Vec::new(),
            Self::NamespaceMetadata => vec![
                "reduce namespace nesting, exported namespace count, or qualified name lengths".into(),
            ],
            Self::Nodes => vec![
                "every place a type mentions another copies it: a value or type parameter used twice in an object, or an alias or generic that uses its parameter twice, doubles the type at each level of nesting".into(),
                "declare repeated shapes as an `interface` (interface references are not copied), or nest fewer levels".into(),
            ],
            Self::Depth => vec![
                "each object, array, union or alias wrapped around a type adds a level, including types inferred from nested values".into(),
                "declare intermediate shapes as an `interface`, or nest fewer levels".into(),
            ],
            Self::Work => vec![
                "building types by instantiating generics and inherited members, and comparing them, has a total budget per compilation; a runtime type check of an interface also compares each collection type built from the literals in its fields".into(),
                "instantiate or compare large types fewer times, shorten long `extends` chains, or name large literal unions in fewer runtime-checked interfaces".into(),
            ],
        };
        CompilerFailure::Limit {
            stage,
            span,
            message: self.to_string(),
            help,
        }
    }
}

impl std::fmt::Display for TypeTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allocation => f.write_str("could not allocate type traversal frames"),
            Self::NamespaceMetadata => write!(
                f,
                "namespace metadata exceeds compiler limits ({} levels, {} namespaces, {} qualified-path bytes)",
                crate::compiler_limits::MAX_NAMESPACE_DEPTH,
                crate::compiler_limits::MAX_NAMESPACE_NODES,
                crate::compiler_limits::MAX_NAMESPACE_PATH_BYTES,
            ),
            Self::Nodes => write!(
                f,
                "type is larger than the compiler limit of {MAX_TYPE_NODES} parts"
            ),
            Self::Depth => write!(
                f,
                "type nesting exceeds the compiler limit of {MAX_TYPE_DEPTH} levels"
            ),
            Self::Work => write!(
                f,
                "building and comparing types takes more than the compiler limit of {MAX_TYPE_WORK} steps"
            ),
        }
    }
}

impl std::error::Error for TypeTooLarge {}

/// Size and depth of a type, as counted by [`measure`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeExtent {
    /// Number of [`Type`] nodes.
    pub nodes: u64,
    /// Nodes on the longest root-to-leaf path; a leaf type has depth 1.
    pub depth: u32,
}

/// Measures `ty` without recursion, stopping once either count exceeds its
/// bound; a stopped measurement reports the count that crossed it. Temporary
/// ancestor-frame allocation failures are returned without a partial extent.
pub fn measure(ty: &Type, max_nodes: u64, max_depth: u32) -> Result<TypeExtent, TypeTooLarge> {
    crate::type_walk::measure(ty, max_nodes, max_depth, TypeChildren::new)
        .map_err(|_| TypeTooLarge::Allocation)
}

/// Rejects a type beyond [`MAX_TYPE_NODES`] or [`MAX_TYPE_DEPTH`].
pub fn check(ty: &Type) -> Result<(), TypeTooLarge> {
    let extent = measure(ty, MAX_TYPE_NODES, MAX_TYPE_DEPTH)?;
    if extent.depth > MAX_TYPE_DEPTH {
        return Err(TypeTooLarge::Depth);
    }
    if extent.nodes > MAX_TYPE_NODES {
        return Err(TypeTooLarge::Nodes);
    }
    Ok(())
}

/// Nodes built so far by one substitution, which must stay within the type
/// limits and the phase's work allowance. A substitution charges each node
/// before building it, so an oversized result stops within the limit rather
/// than after the fact.
#[derive(Debug)]
pub struct TypeBudget<'a> {
    nodes: u64,
    work_left: &'a Cell<u64>,
}

impl TypeBudget<'_> {
    /// Charges one node built at `depth` (the root is at depth 1).
    pub fn charge(&mut self, depth: u32) -> Result<(), TypeTooLarge> {
        self.charge_extent(TypeExtent { nodes: 1, depth })
    }

    /// Charges a copy of `ty` placed at `depth`.
    pub fn charge_copy(&mut self, ty: &Type, depth: u32) -> Result<(), TypeTooLarge> {
        let remaining_nodes = MAX_TYPE_NODES.saturating_sub(self.nodes);
        let remaining_depth = MAX_TYPE_DEPTH.saturating_sub(depth.saturating_sub(1));
        let extent = measure(ty, remaining_nodes, remaining_depth)?;
        self.charge_extent(TypeExtent {
            nodes: extent.nodes,
            depth: depth.saturating_sub(1).saturating_add(extent.depth),
        })
    }

    fn charge_extent(&mut self, extent: TypeExtent) -> Result<(), TypeTooLarge> {
        if extent.depth > MAX_TYPE_DEPTH {
            return Err(TypeTooLarge::Depth);
        }
        self.nodes = self.nodes.saturating_add(extent.nodes);
        if self.nodes > MAX_TYPE_NODES {
            return Err(TypeTooLarge::Nodes);
        }
        self.charge_work(extent.nodes)
    }

    /// Charges traversal work that does not build a result node.
    pub fn charge_work(&mut self, units: u64) -> Result<(), TypeTooLarge> {
        let work_left = self
            .work_left
            .get()
            .checked_sub(units)
            .ok_or(TypeTooLarge::Work)?;
        self.work_left.set(work_left);
        Ok(())
    }
}

/// Type limit state for one compiler phase: the work allowance its
/// substitutions and comparisons draw from, and the first limit met where the caller could not
/// return an error, such as inside assignability or diagnostic rendering. Such
/// a caller continues with [`Type::Error`], which suppresses follow-on
/// diagnostics, and the phase turns the recorded failure into a compiler limit
/// error at its next checkpoint, so compilation never succeeds past it.
#[derive(Debug)]
pub struct TypeLimits {
    work_left: Cell<u64>,
    exceeded: Cell<Option<TypeTooLarge>>,
}

impl Default for TypeLimits {
    fn default() -> Self {
        Self {
            work_left: Cell::new(MAX_TYPE_WORK),
            exceeded: Cell::new(None),
        }
    }
}

impl TypeLimits {
    /// Limits whose phase may build only `nodes` by substitution.
    #[cfg(test)]
    pub(crate) fn with_work_allowance(nodes: u64) -> Self {
        Self {
            work_left: Cell::new(nodes),
            exceeded: Cell::new(None),
        }
    }

    /// A budget for one substitution, drawing on this phase's work allowance.
    pub fn budget(&self) -> TypeBudget<'_> {
        TypeBudget {
            nodes: 0,
            work_left: &self.work_left,
        }
    }

    /// The type, or [`Type::Error`] after recording why it was not built.
    pub fn type_or_error(&self, result: Result<Type, TypeTooLarge>) -> Type {
        result.unwrap_or_else(|exceeded| {
            self.record(exceeded);
            Type::Error
        })
    }

    /// Spends `units` of the phase's work allowance on work other than
    /// building types, such as comparing them. False once a limit has been
    /// recorded, including the allowance running out now.
    pub fn spend_work(&self, units: u64) -> bool {
        if self.limit_reached() {
            return false;
        }
        let Some(left) = self.work_left.get().checked_sub(units) else {
            self.record(TypeTooLarge::Work);
            return false;
        };
        self.work_left.set(left);
        true
    }

    /// The value, or `None` after recording why it was not built.
    pub fn ok_or_record<T>(&self, result: Result<T, TypeTooLarge>) -> Option<T> {
        result.map_err(|exceeded| self.record(exceeded)).ok()
    }

    /// Whether a failure is recorded and not yet taken. Diagnostics reported
    /// meanwhile may describe the `Type::Error` stand-in rather than the source.
    pub fn limit_reached(&self) -> bool {
        self.exceeded.get().is_some()
    }

    /// Records `exceeded` unless a failure is already recorded: later ones may
    /// be consequences of the first.
    pub fn record(&self, exceeded: TypeTooLarge) {
        if self.exceeded.get().is_none() {
            self.exceeded.set(Some(exceeded));
        }
    }

    /// The recorded failure, clearing it.
    pub fn take(&self) -> Result<(), TypeTooLarge> {
        match self.exceeded.take() {
            Some(exceeded) => Err(exceeded),
            None => Ok(()),
        }
    }
}

/// Calls `visit` on each type directly inside `ty`.
pub fn for_each_child<'t>(ty: &'t Type, mut visit: impl FnMut(&'t Type)) {
    match ty {
        Type::Function {
            params,
            ret,
            predicate,
            ..
        } => {
            params.iter().for_each(&mut visit);
            visit(ret);
            if let Some(predicate) = predicate {
                visit(&predicate.asserted_type);
            }
        }
        Type::Object { fields, index } => {
            fields.values().for_each(|field| visit(&field.ty));
            if let Some(index) = index {
                visit(&index.value);
            }
        }
        Type::Array(inner) | Type::Readonly(inner) => visit(inner),
        Type::Tuple(members) | Type::Union(members) => members.iter().for_each(visit),
        Type::Refined { original, ty } => {
            visit(original);
            visit(ty);
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().for_each(visit),
        Type::Alias { args, ty, .. } => {
            args.iter().for_each(&mut visit);
            visit(ty);
        }
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Unknown
        | Type::Error
        | Type::Never
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. } => {}
    }
}

/// [`map_children`] for a `map` that can't fail.
pub fn map_children_infallible(ty: &Type, mut map: impl FnMut(&Type) -> Type) -> Type {
    let mapped: Result<Type, std::convert::Infallible> = map_children(ty, |child| Ok(map(child)));
    match mapped {
        Ok(ty) => ty,
        Err(never) => match never {},
    }
}

/// `ty` with each type directly inside it replaced by `map`'s result, in the
/// order [`for_each_child`] visits them; a leaf is cloned. Unions are rebuilt
/// through [`Type::union`], so substitutions that collapse members keep the
/// canonical form.
pub fn map_children<E>(
    ty: &Type,
    mut map: impl FnMut(&Type) -> Result<Type, E>,
) -> Result<Type, E> {
    let mut map_all = |types: &[Type]| types.iter().map(&mut map).collect::<Result<Vec<_>, E>>();
    Ok(match ty {
        Type::Function {
            params,
            ret,
            predicate,
            has_rest,
        } => {
            let params = map_all(params)?;
            Type::Function {
                params,
                ret: Box::new(map(ret)?),
                predicate: match predicate {
                    Some(predicate) => Some(Box::new(crate::TypePredicate {
                        parameter_index: predicate.parameter_index,
                        asserted_type: map(&predicate.asserted_type)?,
                    })),
                    None => None,
                },
                has_rest: *has_rest,
            }
        }
        Type::Object { fields, index } => Type::Object {
            fields: fields
                .iter()
                .map(|(name, field)| {
                    Ok((
                        name.clone(),
                        crate::ObjectField {
                            ty: map(&field.ty)?,
                            optional: field.optional,
                            readonly: field.readonly,
                            method: field.method,
                        },
                    ))
                })
                .collect::<Result<_, E>>()?,
            index: index
                .as_ref()
                .map(|index| index.try_map_value(&mut map))
                .transpose()?,
        },
        Type::Array(inner) => Type::Array(Box::new(map(inner)?)),
        Type::Readonly(inner) => Type::Readonly(Box::new(map(inner)?)),
        Type::Tuple(members) => Type::Tuple(map_all(members)?),
        Type::Union(members) => Type::union(map_all(members)?),
        Type::Refined { original, ty } => Type::Refined {
            original: Box::new(map(original)?),
            ty: Box::new(map(ty)?),
        },
        Type::InterfaceRef {
            mangled,
            package,
            name,
            args,
        } => Type::InterfaceRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: map_all(args)?,
        },
        Type::ClassRef {
            mangled,
            package,
            name,
            args,
        } => Type::ClassRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: map_all(args)?,
        },
        Type::AliasRef {
            mangled,
            package,
            name,
            args,
        } => Type::AliasRef {
            mangled: mangled.clone(),
            package: package.clone(),
            name: name.clone(),
            args: map_all(args)?,
        },
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ty: inner,
        } => {
            let args = map_all(args)?;
            Type::Alias {
                mangled: mangled.clone(),
                package: package.clone(),
                name: name.clone(),
                args,
                ty: Box::new(map(inner)?),
            }
        }
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Void
        | Type::Unknown
        | Type::Error
        | Type::Never
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. } => ty.clone(),
    })
}

/// Borrowed child sources in reverse declaration order. Fixed trailing children
/// (return types, predicates and index values) precede the collection iterator.
struct TypeChildren<'a> {
    members: std::slice::Iter<'a, Type>,
    fields: Option<std::collections::btree_map::Values<'a, String, crate::ObjectField>>,
    trailing: [Option<&'a Type>; 2],
}

impl<'a> TypeChildren<'a> {
    fn new(ty: &'a Type) -> Self {
        let mut children = Self {
            members: [].iter(),
            fields: None,
            trailing: [None, None],
        };
        match ty {
            Type::Function {
                params,
                ret,
                predicate,
                ..
            } => {
                children.members = params.iter();
                children.trailing = [Some(ret), predicate.as_ref().map(|p| &p.asserted_type)];
            }
            Type::Object { fields, index } => {
                children.fields = Some(fields.values());
                children.trailing[0] = index.as_ref().map(|index| index.value.as_ref());
            }
            Type::Array(inner) | Type::Readonly(inner) => {
                children.trailing[0] = Some(inner);
            }
            Type::Tuple(members)
            | Type::Union(members)
            | Type::InterfaceRef { args: members, .. }
            | Type::ClassRef { args: members, .. }
            | Type::AliasRef { args: members, .. } => children.members = members.iter(),
            Type::Refined { original, ty } => children.trailing = [Some(original), Some(ty)],
            Type::Alias { args, ty, .. } => {
                children.members = args.iter();
                children.trailing[0] = Some(ty);
            }
            Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Void
            | Type::Unknown
            | Type::Error
            | Type::Never
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. } => {}
        }
        children
    }
}

impl<'a> Iterator for TypeChildren<'a> {
    type Item = &'a Type;

    fn next(&mut self) -> Option<Self::Item> {
        self.trailing
            .iter_mut()
            .rev()
            .find_map(Option::take)
            .or_else(|| self.fields.as_mut()?.next_back().map(|field| &field.ty))
            .or_else(|| self.members.next_back())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::ObjectField;
    use crate::typechecker::type_param_substitution::TypeParamSubstitution;

    /// A tuple of `n - 1` numbers: `n` nodes, depth 2.
    fn nodes(n: u64) -> Type {
        Type::Tuple((1..n).map(|_| Type::Number).collect())
    }

    /// `depth - 1` arrays around a number.
    fn nested(depth: u32) -> Type {
        (1..depth).fold(Type::Number, |inner, _| Type::Array(Box::new(inner)))
    }

    /// `{ a: T; b: T }`: each application doubles its argument.
    fn pair_of_t() -> Type {
        let field = || ObjectField::required(Type::TypeVar("T".into()));
        Type::Object {
            fields: BTreeMap::from([("a".into(), field()), ("b".into(), field())]),
            index: None,
        }
    }

    fn binding(ty: Type) -> TypeParamSubstitution {
        TypeParamSubstitution::from_pairs(&["T".to_string()], &[ty])
    }

    /// Runs `test` on a thread of the stack compilation is given: substitution
    /// and drop recurse once per level of a type at the depth limit.
    pub(crate) fn on_compiler_stack(test: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(crate::compiler_limits::COMPILER_STACK_BYTES)
            .spawn(test)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn measure_counts_every_node_and_the_longest_path() {
        let ty = Type::union(vec![nested(4), Type::Null]);
        assert_eq!(
            measure(&ty, u64::MAX, u32::MAX).unwrap(),
            TypeExtent { nodes: 6, depth: 5 }
        );
    }

    #[test]
    fn measure_stops_once_a_bound_is_crossed() {
        on_compiler_stack(|| {
            let extent = measure(&nodes(1_000), 10, u32::MAX).unwrap();
            assert_eq!(extent.nodes, 11);
            let extent = measure(&nested(1_000), u64::MAX, 10).unwrap();
            assert_eq!(extent.depth, 11);
        });
    }

    #[test]
    fn wide_types_use_only_ancestor_frames() {
        let wide = nodes(100_001);
        for max_nodes in [0, 1, 7, 100_001] {
            let (extent, observed) = crate::type_walk::tests::observe(None, || {
                measure(&wide, max_nodes, u32::MAX).unwrap()
            });
            assert_eq!(extent.nodes, 100_001.min(max_nodes + 1));
            assert!(observed.peak_frames <= 1);
            assert!(observed.reservations <= 1);
        }
        // Once a limit is crossed, even a pending reservation failure is irrelevant.
        let (extent, observed) =
            crate::type_walk::tests::observe(Some(0), || measure(&wide, 0, u32::MAX).unwrap());
        assert_eq!(extent.nodes, 1);
        assert_eq!(observed.reservations, 0);
    }

    #[test]
    fn allocation_failure_is_preserved_by_validation_and_copy_charging() {
        let ty = nodes(3);
        let (result, _) = crate::type_walk::tests::observe(Some(0), || check(&ty));
        assert_eq!(result, Err(TypeTooLarge::Allocation));
        let limits = TypeLimits::default();
        let (result, _) =
            crate::type_walk::tests::observe(Some(0), || limits.budget().charge_copy(&ty, 1));
        assert_eq!(result, Err(TypeTooLarge::Allocation));
        assert_eq!(limits.work_left.get(), MAX_TYPE_WORK);
        let span = Span::at(crate::FileId(7));
        assert!(matches!(
            TypeTooLarge::Allocation.into_failure(CompilerStage::Infer, Some(span)),
            CompilerFailure::Internal { stage: CompilerStage::Infer, span: Some(s), .. } if s == span
        ));
        assert_eq!(check(&ty), Ok(()));
    }

    #[test]
    fn mixed_width_and_depth_keep_exact_counts_and_lifo_order() {
        let ty = Type::Tuple(vec![nested(6), nodes(100_001), nested(3)]);
        let (extent, observed) =
            crate::type_walk::tests::observe(None, || measure(&ty, u64::MAX, u32::MAX).unwrap());
        assert_eq!(
            extent,
            TypeExtent {
                nodes: 100_011,
                depth: 7
            }
        );
        assert_eq!(observed.peak_frames, 6);
        // The rightmost member is visited first, as in the former eager walk.
        assert_eq!(
            measure(&ty, 3, u32::MAX).unwrap(),
            TypeExtent { nodes: 4, depth: 4 }
        );
        assert_eq!(
            measure(&ty, u64::MAX, 0).unwrap(),
            TypeExtent { nodes: 1, depth: 1 }
        );
        let (failed, _) =
            crate::type_walk::tests::observe(Some(1), || measure(&nested(20), u64::MAX, u32::MAX));
        assert_eq!(failed, Err(TypeTooLarge::Allocation));
    }

    #[test]
    fn check_admits_types_exactly_at_the_limits() {
        on_compiler_stack(|| {
            assert_eq!(check(&nodes(MAX_TYPE_NODES)), Ok(()));
            assert_eq!(check(&nodes(MAX_TYPE_NODES + 1)), Err(TypeTooLarge::Nodes));
            assert_eq!(check(&nested(MAX_TYPE_DEPTH)), Ok(()));
            assert_eq!(check(&nested(MAX_TYPE_DEPTH + 1)), Err(TypeTooLarge::Depth));
        });
    }

    #[test]
    fn a_copy_is_charged_at_the_depth_it_is_placed() {
        on_compiler_stack(a_copy_is_charged_at_the_depth_it_is_placed_inner);
    }

    fn a_copy_is_charged_at_the_depth_it_is_placed_inner() {
        let limits = TypeLimits::default();
        let mut budget = limits.budget();
        assert_eq!(budget.charge_copy(&nested(10), MAX_TYPE_DEPTH - 9), Ok(()));
        assert_eq!(
            budget.charge_copy(&nested(10), MAX_TYPE_DEPTH - 8),
            Err(TypeTooLarge::Depth)
        );
        let mut budget = limits.budget();
        assert_eq!(budget.charge_copy(&nodes(MAX_TYPE_NODES), 1), Ok(()));
        assert_eq!(budget.charge(1), Err(TypeTooLarge::Nodes));
    }

    #[test]
    fn substitution_stops_at_the_size_limit_instead_of_building_it() {
        let limits = TypeLimits::default();
        let half = nodes(MAX_TYPE_NODES / 2);
        // `{ a: T; b: T }` is one node plus two copies of the argument.
        assert_eq!(
            binding(half).apply(&pair_of_t(), &limits),
            Err(TypeTooLarge::Nodes)
        );
        let fits = nodes(MAX_TYPE_NODES / 2 - 1);
        let applied = binding(fits).apply(&pair_of_t(), &limits).unwrap();
        assert_eq!(
            measure(&applied, u64::MAX, u32::MAX).unwrap().nodes,
            MAX_TYPE_NODES - 1
        );
    }

    #[test]
    fn substitution_stops_at_the_depth_limit() {
        on_compiler_stack(substitution_stops_at_the_depth_limit_inner);
    }

    fn substitution_stops_at_the_depth_limit_inner() {
        let limits = TypeLimits::default();
        let deep = binding(nested(MAX_TYPE_DEPTH - 1));
        // The object adds one level above the argument.
        assert!(deep.apply(&pair_of_t(), &limits).is_ok());
        let deeper = binding(nested(MAX_TYPE_DEPTH));
        assert_eq!(
            deeper.apply(&pair_of_t(), &limits),
            Err(TypeTooLarge::Depth)
        );
    }

    #[test]
    fn substitutions_share_the_phase_work_allowance() {
        let limits = TypeLimits::with_work_allowance(29);
        let sub = binding(nodes(5));
        // Each application builds 1 + 2 * 5 nodes and chases two bindings.
        assert!(sub.apply(&pair_of_t(), &limits).is_ok());
        assert!(sub.apply(&pair_of_t(), &limits).is_ok());
        assert_eq!(sub.apply(&pair_of_t(), &limits), Err(TypeTooLarge::Work));
    }

    #[test]
    fn the_latch_keeps_the_first_failure_until_taken() {
        let limits = TypeLimits::default();
        assert!(!limits.limit_reached());
        assert_eq!(limits.type_or_error(Err(TypeTooLarge::Depth)), Type::Error);
        assert_eq!(limits.ok_or_record::<()>(Err(TypeTooLarge::Nodes)), None);
        assert_eq!(limits.type_or_error(Ok(Type::Number)), Type::Number);
        assert!(limits.limit_reached());
        assert_eq!(limits.take(), Err(TypeTooLarge::Depth));
        assert_eq!(limits.take(), Ok(()));
        assert!(!limits.limit_reached());
    }

    #[test]
    fn map_children_rebuilds_every_variant_unchanged() {
        let object = Type::Object {
            fields: BTreeMap::from([("f".into(), ObjectField::required(Type::String))]),
            index: Some(crate::IndexSignature {
                value: Box::new(Type::Boolean),
                readonly: true,
            }),
        };
        let function = Type::Function {
            params: vec![Type::Number, object.clone()],
            ret: Box::new(Type::Void),
            predicate: Some(Box::new(crate::TypePredicate {
                parameter_index: 0,
                asserted_type: Type::Number,
            })),
            has_rest: true,
        };
        let package = crate::Package("main".into());
        let mangled = crate::mangle::prelude("Thing");
        let ty = Type::Tuple(vec![
            function,
            Type::Readonly(Box::new(Type::Array(Box::new(Type::Number)))),
            Type::union(vec![Type::Null, Type::String]),
            Type::Refined {
                original: Box::new(Type::TypeVar("T".into())),
                ty: Box::new(Type::Number),
            },
            Type::interface_ref(package.clone(), "I", mangled.clone(), vec![Type::Number]),
            Type::class_ref(package.clone(), "C", mangled.clone(), vec![Type::String]),
            Type::alias_ref(package.clone(), "R", mangled.clone(), vec![Type::Null]),
            Type::alias_ty(package, "A", mangled, vec![Type::Number], Box::new(object)),
        ]);
        let mut visited = 0;
        for_each_child(&ty, |_| visited += 1);
        assert_eq!(visited, 8);
        check_type_child_order(&ty);
        let rebuilt = map_children(&ty, |child| {
            map_children(child, |grandchild| Ok::<_, ()>(grandchild.clone()))
        });
        assert_eq!(rebuilt, Ok(ty));
    }

    fn check_type_child_order(ty: &Type) {
        let mut expected = Vec::new();
        for_each_child(ty, |child| expected.push(child));
        expected.reverse();
        let actual: Vec<_> = TypeChildren::new(ty).collect();
        assert_eq!(actual, expected);
        for child in actual {
            check_type_child_order(child);
        }
    }
}
