//! How a generic class or interface relates its instantiations: the variance
//! of each type parameter, measured from where the parameter appears in the
//! declaration's members, the way `tsc` measures it.
//!
//! - A property, a method's return and an array element are covariant.
//! - A parameter of a function *type* (a property `put: (a: A) => string`, a
//!   callback's own parameters) flips the position: `Sink<number>` is not a
//!   `Sink<string | number>`, since its `put` would be handed a string.
//! - A *method's* own parameters are bivariant, as in `tsc`, which compares
//!   method parameters both ways: `Logger<number>` is a `Logger<1>` and a
//!   `Logger<number | string>`, but not a `Logger<string>`.
//! - A parameter that appears nowhere relates any instantiation to any other.

use std::collections::{BTreeMap, BTreeSet};

use crate::{MangledName, Type, TypeKind};

use super::assignable::TypeResolver;
use super::generic::substitute_or_record;

/// How one type argument of a generic class or interface must relate for one
/// instantiation to be assignable to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Variance {
    /// The parameter appears nowhere: any argument fits.
    Independent,
    Covariant,
    Contravariant,
    /// Only in method parameters: the arguments must be related either way.
    Bivariant,
    Invariant,
}

impl Variance {
    /// Whether `actual_arg` may stand for `expected_arg` at this variance,
    /// given the assignability relation to use.
    pub(crate) fn relates(
        self,
        actual_arg: &Type,
        expected_arg: &Type,
        mut assignable: impl FnMut(&Type, &Type) -> bool,
    ) -> bool {
        match self {
            Variance::Independent => true,
            Variance::Covariant => assignable(actual_arg, expected_arg),
            Variance::Contravariant => assignable(expected_arg, actual_arg),
            Variance::Bivariant => {
                assignable(actual_arg, expected_arg) || assignable(expected_arg, actual_arg)
            }
            Variance::Invariant => {
                assignable(actual_arg, expected_arg) && assignable(expected_arg, actual_arg)
            }
        }
    }
}

/// The polarities a type parameter was found at.
#[derive(Clone, Copy, Default)]
struct Occurrences {
    covariant: bool,
    contravariant: bool,
    bivariant: bool,
}

impl Occurrences {
    fn variance(self) -> Variance {
        match (self.covariant, self.contravariant) {
            (true, true) => Variance::Invariant,
            (true, false) => Variance::Covariant,
            (false, true) => Variance::Contravariant,
            (false, false) if self.bivariant => Variance::Bivariant,
            (false, false) => Variance::Independent,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Covariant,
    Contravariant,
    Bivariant,
}

impl Polarity {
    fn flipped(self) -> Self {
        match self {
            Polarity::Covariant => Polarity::Contravariant,
            Polarity::Contravariant => Polarity::Covariant,
            Polarity::Bivariant => Polarity::Bivariant,
        }
    }
}

/// Marker names stand in for the declaration's parameters while its members
/// are walked; `#` can't start a source type parameter name, so no member
/// mentions one by accident.
fn marker(index: usize) -> String {
    format!("#variance{index}")
}

fn marker_index(name: &str) -> Option<usize> {
    name.strip_prefix("#variance")?.parse().ok()
}

impl<'a> TypeResolver<'a> {
    /// The variance of each type parameter of the class or interface named
    /// `mangled`, in declaration order. `None` when the symbol is not a generic
    /// class or interface, or its members can't all be resolved; the caller
    /// then keeps comparing arguments covariantly.
    pub(crate) fn type_param_variances(
        &self,
        mangled: &MangledName,
        name: &str,
    ) -> Option<Vec<Variance>> {
        let mut measuring = Measuring::default();
        self.measure_variances(mangled, name, &mut measuring)
            .variances
    }

    /// [`Self::type_param_variances`] for an instantiation with `arity`
    /// arguments, when they were measured in full rather than cut short by
    /// the work limit, so that they decide how its instantiations relate.
    pub(crate) fn settled_variances(
        &self,
        mangled: &MangledName,
        name: &str,
        arity: usize,
    ) -> Option<Vec<Variance>> {
        let measured = self.measure_variances(mangled, name, &mut Measuring::default());
        if measured.cut_short {
            return None;
        }
        measured
            .variances
            .filter(|variances| variances.len() == arity)
    }

    /// [`Self::type_param_variances`] for an instantiation with `arity`
    /// arguments, covariant for each when they can't be measured.
    pub(crate) fn variances_or_covariant(
        &self,
        mangled: &MangledName,
        name: &str,
        arity: usize,
    ) -> Vec<Variance> {
        self.type_param_variances(mangled, name)
            .filter(|variances| variances.len() == arity)
            .unwrap_or_else(|| vec![Variance::Covariant; arity])
    }

    /// Measured once per declaration and remembered: a declaration referenced
    /// from several members (`a: Next<T>; b: Next<T>`) would otherwise be
    /// measured again at each reference, doubling the work at every level.
    /// One that read the estimate of a declaration further up the walk is
    /// remembered while that estimate holds.
    fn measure_variances(
        &self,
        mangled: &MangledName,
        name: &str,
        measuring: &mut Measuring,
    ) -> Measured {
        if let Some(variances) = self.registry.measured_variances(mangled) {
            return Measured {
                variances,
                ..Measured::default()
            };
        }
        if let Some(measured) = measuring.within_walk.get(mangled) {
            return measured.clone();
        }
        let measured = self.walk_declaration(mangled, name, measuring);
        if measured.cut_short {
            return measured;
        }
        if measured.read.is_empty() {
            self.registry
                .remember_variances(mangled, measured.variances.clone());
        } else {
            measuring
                .within_walk
                .insert(mangled.clone(), measured.clone());
        }
        measured
    }

    /// A reference back to a declaration still being measured reads its
    /// estimate, which starts independent. The outermost declaration of a
    /// group that refers back to itself (`cmp: (o: S<T>) => void`, or `P`
    /// through `Q` to `P`) walks the group again until no estimate in it
    /// changes; the others take part in its walks rather than settling on
    /// their own, which would repeat the walks below them at every level.
    /// Each walk only adds positions, so the estimates settle within three
    /// changes per type parameter.
    fn walk_declaration(
        &self,
        mangled: &MangledName,
        name: &str,
        measuring: &mut Measuring,
    ) -> Measured {
        let Some(sym) = self.lookup(mangled, name) else {
            return Measured::default();
        };
        let (TypeKind::Class { generics, .. } | TypeKind::Interface { generics, .. }) = &sym.kind
        else {
            return Measured::default();
        };
        let depth = measuring.stack.len();
        measuring.stack.push(mangled.clone());
        let revised_outside = std::mem::take(&mut measuring.revised);
        let mut revised_inside = false;
        let mut walks = 0;
        let settled = loop {
            measuring.forget_from(depth);
            let pass = self.walk_once(sym, generics.len(), measuring);
            let revised_below = std::mem::take(&mut measuring.revised);
            let revised_here = measuring.revise_estimate(mangled, &pass);
            let revised = revised_below || revised_here;
            revised_inside |= revised;
            walks += 1;
            let heads_group = pass.read.first() == Some(&depth);
            let rewalk = revised && heads_group && !pass.cut_short;
            if !rewalk {
                break pass;
            }
            if walks > measuring.max_walks() {
                break Measured {
                    variances: Some(vec![Variance::Invariant; generics.len()]),
                    cut_short: true,
                    ..pass
                };
            }
        };
        measuring.revised = revised_outside || revised_inside;
        measuring.stack.pop();
        measuring.forget_from(depth);
        Measured {
            // A reference back to the declaration itself read an estimate
            // it settled, not one from further up the walk.
            read: settled
                .read
                .into_iter()
                .filter(|&index| index < depth)
                .collect(),
            ..settled
        }
    }

    fn walk_once(
        &self,
        sym: &crate::TypeSymbol,
        arity: usize,
        measuring: &mut Measuring,
    ) -> Measured {
        let markers: Vec<Type> = (0..arity).map(|i| Type::TypeVar(marker(i))).collect();
        let mut walk = VarianceWalk {
            resolver: *self,
            found: vec![Occurrences::default(); arity],
            measuring,
            read: BTreeSet::new(),
            cut_short: false,
        };
        let walked = walk.members(sym, &markers);
        Measured {
            variances: walked.map(|()| walk.found.into_iter().map(Occurrences::variance).collect()),
            read: walk.read,
            cut_short: walk.cut_short,
        }
    }
}

/// The declarations whose variance is being measured further up one walk.
#[derive(Default)]
struct Measuring {
    stack: Vec<MangledName>,
    /// What a reference back to a declaration on the stack takes its
    /// variances to be; see [`TypeResolver::walk_declaration`]. Estimates
    /// last for the whole measurement, so a group's later walks start from
    /// what its earlier ones found.
    estimates: BTreeMap<MangledName, Vec<Variance>>,
    /// Set when an estimate changes. Each `walk_declaration` takes it on
    /// entry and sets it again on exit if it or a declaration below it
    /// revised one, so a group's outermost declaration sees revisions made
    /// anywhere in its walk.
    revised: bool,
    /// Measurements that read the estimates of declarations on the stack, at
    /// those stack positions, kept while the estimates hold.
    within_walk: BTreeMap<MangledName, Measured>,
}

impl Measuring {
    /// Forget the measurements that read the estimate at stack position
    /// `depth` or deeper, which is being revised or leaving the stack.
    fn forget_from(&mut self, depth: usize) {
        self.within_walk
            .retain(|_, measured| measured.read.iter().all(|&index| index < depth));
    }

    fn estimate(&self, mangled: &MangledName, arity: usize) -> Vec<Variance> {
        self.estimates
            .get(mangled)
            .cloned()
            .unwrap_or_else(|| vec![Variance::Independent; arity])
    }

    /// Record what `measured` found as `mangled`'s estimate; whether it
    /// changed.
    fn revise_estimate(&mut self, mangled: &MangledName, measured: &Measured) -> bool {
        let Some(found) = &measured.variances else {
            return false;
        };
        if self.estimates.get(mangled) == Some(found) {
            return false;
        }
        self.estimates.insert(mangled.clone(), found.clone());
        true
    }

    /// More walks of a group than its estimates can change in: three
    /// changes per type parameter estimated so far, from independent to
    /// invariant.
    fn max_walks(&self) -> usize {
        3 * self.estimates.values().map(Vec::len).sum::<usize>() + 1
    }
}

/// A declaration's variances, and whether they are partial.
/// - `read`: the stack positions of the declarations further up the walk
///   whose estimates this measurement read, directly or through a nested
///   measurement, in ascending order.
/// - `cut_short`: the work limit ended the walk.
///
/// A partial measurement depends on where the walk started, so it is not
/// remembered for other walks.
#[derive(Clone, Default)]
struct Measured {
    variances: Option<Vec<Variance>>,
    read: BTreeSet<usize>,
    cut_short: bool,
}

struct VarianceWalk<'r, 'a> {
    resolver: TypeResolver<'a>,
    found: Vec<Occurrences>,
    measuring: &'r mut Measuring,
    /// See [`Measured`].
    read: BTreeSet<usize>,
    cut_short: bool,
}

impl VarianceWalk<'_, '_> {
    fn members(&mut self, sym: &crate::TypeSymbol, markers: &[Type]) -> Option<()> {
        match &sym.kind {
            TypeKind::Interface {
                generics,
                methods,
                properties,
                index,
                ..
            } => {
                let bindings: BTreeMap<String, Type> = generics
                    .iter()
                    .cloned()
                    .zip(markers.iter().cloned())
                    .collect();
                for sig in methods.values() {
                    self.method(&sig.params, &sig.ret, &bindings);
                }
                for sig in properties.values() {
                    self.substituted(&sig.ty, &bindings, Polarity::Covariant);
                }
                if let Some(index) = index {
                    self.substituted(&index.value, &bindings, Polarity::Covariant);
                }
                Some(())
            }
            TypeKind::Class { .. } => {
                let resolver = self.resolver;
                super::classes::for_each_class_in_chain(
                    |m| resolver.sym_by_mangled(m).cloned(),
                    resolver.limits,
                    &sym.mangled_name,
                    markers,
                    |class, bindings| self.class_members(class, bindings),
                )
            }
            _ => None,
        }
    }

    /// Private members count: `tsc` measures them too.
    fn class_members(&mut self, class: &crate::TypeSymbol, bindings: &BTreeMap<String, Type>) {
        let TypeKind::Class {
            fields,
            methods,
            accessors,
            ..
        } = &class.kind
        else {
            return;
        };
        for field in fields.values() {
            self.substituted(&field.ty, bindings, Polarity::Covariant);
        }
        for sig in methods.values() {
            self.method(&sig.params, &sig.ret, bindings);
        }
        // An accessor is a property: its setter's parameter is the property's
        // type, not a method parameter.
        for accessor in accessors {
            let ty = match accessor {
                crate::AccessorSig::Getter { ret_ty, .. } => ret_ty,
                crate::AccessorSig::Setter { param, .. } => &param.ty,
            };
            self.substituted(ty, bindings, Polarity::Covariant);
        }
    }

    /// A method's parameters are compared both ways, as tsc compares them:
    /// `Logger<number>` is a `Logger<1>`. A callback is the exception: tsc
    /// compares its parameters strictly and only its return both ways, so
    /// `subscribe(listener: (value: T) => void)` is covariant in `T`. A rest
    /// parameter is compared by its elements.
    fn method(&mut self, params: &[crate::Param], ret: &Type, bindings: &BTreeMap<String, Type>) {
        for param in params {
            let ty = substitute_or_record(&param.ty, bindings, self.resolver.limits);
            let compared = match ty.peel() {
                Type::Array(element) if param.rest => element.as_ref(),
                _ => &ty,
            };
            match callback_signature(compared) {
                Some((callback_params, callback_ret)) => {
                    for callback_param in callback_params {
                        self.walk(callback_param, Polarity::Covariant);
                    }
                    self.both_ways(callback_ret);
                }
                None => self.both_ways(compared),
            }
        }
        self.substituted(ret, bindings, Polarity::Covariant);
    }

    /// `ty` compared both ways: a type parameter found in it in one direction
    /// is bivariant, while one it is invariant in stays invariant, since
    /// neither direction then holds (`run(p: { k: (x: T) => T })`).
    fn both_ways(&mut self, ty: &Type) {
        let fresh = vec![Occurrences::default(); self.found.len()];
        let outer = std::mem::replace(&mut self.found, fresh);
        self.walk(ty, Polarity::Covariant);
        let found_inside = std::mem::replace(&mut self.found, outer);
        for (found, inside) in self.found.iter_mut().zip(found_inside) {
            match inside.variance() {
                Variance::Independent => {}
                Variance::Invariant => {
                    found.covariant = true;
                    found.contravariant = true;
                }
                Variance::Covariant | Variance::Contravariant | Variance::Bivariant => {
                    found.bivariant = true;
                }
            }
        }
    }

    fn substituted(&mut self, ty: &Type, bindings: &BTreeMap<String, Type>, polarity: Polarity) {
        let ty = substitute_or_record(ty, bindings, self.resolver.limits);
        self.walk(&ty, polarity);
    }

    fn walk(&mut self, ty: &Type, polarity: Polarity) {
        if !self.resolver.limits.spend_work(1) {
            self.cut_short = true;
            return;
        }
        match ty {
            Type::TypeVar(name) => {
                if let Some(found) = marker_index(name).and_then(|i| self.found.get_mut(i)) {
                    match polarity {
                        Polarity::Covariant => found.covariant = true,
                        Polarity::Contravariant => found.contravariant = true,
                        Polarity::Bivariant => found.bivariant = true,
                    }
                }
            }
            Type::Array(inner) | Type::Readonly(inner) => self.walk(inner, polarity),
            Type::Tuple(items) | Type::Union(items) => {
                for item in items {
                    self.walk(item, polarity);
                }
            }
            Type::Object { fields, index } => {
                for field in fields.values() {
                    self.walk(&field.ty, polarity);
                }
                if let Some(index) = index {
                    self.walk(&index.value, polarity);
                }
            }
            Type::Function { params, ret, .. } => {
                for param in params {
                    self.walk(param, polarity.flipped());
                }
                self.walk(ret, polarity);
            }
            Type::Refined { original, ty } => {
                self.walk(original, polarity);
                self.walk(ty, polarity);
            }
            Type::Alias { ty, .. } => self.walk(ty, polarity),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            }
            | Type::ClassRef {
                mangled,
                name,
                args,
                ..
            } => self.reference(mangled, name, args, polarity),
            // A recursive alias's back-edge repeats an instantiation the walk is
            // already inside; its arguments are where a parameter can appear.
            Type::AliasRef { args, .. } => {
                for arg in args {
                    self.walk(arg, Polarity::Covariant);
                    self.walk(arg, Polarity::Contravariant);
                }
            }
            _ => {}
        }
    }

    fn reference(&mut self, mangled: &MangledName, name: &str, args: &[Type], polarity: Polarity) {
        if args.is_empty() {
            return;
        }
        let on_stack = self.measuring.stack.iter().position(|m| m == mangled);
        let variances = if let Some(index) = on_stack {
            self.read.insert(index);
            Some(self.measuring.estimate(mangled, args.len()))
        } else {
            let measured = self
                .resolver
                .measure_variances(mangled, name, self.measuring);
            self.read.extend(measured.read);
            self.cut_short |= measured.cut_short;
            measured.variances
        };
        let variances = variances.unwrap_or_else(|| vec![Variance::Covariant; args.len()]);
        for (arg, variance) in args.iter().zip(variances) {
            match variance {
                Variance::Independent => {}
                Variance::Covariant => self.walk(arg, polarity),
                Variance::Contravariant => self.walk(arg, polarity.flipped()),
                Variance::Bivariant => self.walk(arg, Polarity::Bivariant),
                Variance::Invariant => {
                    self.walk(arg, polarity);
                    self.walk(arg, polarity.flipped());
                }
            }
        }
    }
}

/// The parameters and return of a function type, alone or beside `null`,
/// which tsc takes a parameter of that type to be a callback.
fn callback_signature(ty: &Type) -> Option<(&[Type], &Type)> {
    let function = match ty.peel() {
        function @ Type::Function { .. } => function,
        Type::Union(members) => {
            let mut rest = members
                .iter()
                .map(Type::peel)
                .filter(|member| !matches!(member, Type::Null));
            match (rest.next(), rest.next()) {
                (Some(function), None) => function,
                _ => return None,
            }
        }
        _ => return None,
    };
    let Type::Function { params, ret, .. } = function else {
        return None;
    };
    Some((params, ret))
}
