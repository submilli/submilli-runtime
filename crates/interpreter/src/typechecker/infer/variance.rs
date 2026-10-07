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
    /// One that read what a declaration further up the walk is assumed to be
    /// is remembered while that assumption holds.
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
        if let Some((variances, assumed)) = measuring.within_walk.get(mangled) {
            return Measured {
                variances: variances.clone(),
                assumed: assumed.clone(),
                cut_short: false,
            };
        }
        let measured = self.walk_declaration(mangled, name, measuring);
        if measured.cut_short {
            return measured;
        }
        if measured.assumed.is_empty() {
            self.registry
                .remember_variances(mangled, measured.variances.clone());
        } else {
            measuring.within_walk.insert(
                mangled.clone(),
                (measured.variances.clone(), measured.assumed.clone()),
            );
        }
        measured
    }

    /// A declaration that refers back to itself (`cmp: (o: S<T>) => void`)
    /// is walked again with the variances the last walk found until they
    /// settle; each walk only adds positions, so this ends within the few
    /// steps from independent to invariant ([`MAX_REWALKS`] guards it).
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
        let markers: Vec<Type> = (0..generics.len())
            .map(|i| Type::TypeVar(marker(i)))
            .collect();
        let depth = measuring.stack.len();
        measuring.stack.push(OnStack {
            mangled: mangled.clone(),
            assumed: vec![Variance::Independent; generics.len()],
        });
        let mut rewalks = 0;
        let measured = loop {
            measuring.forget_from(depth);
            let mut walk = VarianceWalk {
                resolver: *self,
                found: vec![Occurrences::default(); generics.len()],
                measuring: &mut *measuring,
                assumed: BTreeSet::new(),
                cut_short: false,
            };
            let walked = walk.members(sym, &markers);
            let VarianceWalk {
                found,
                assumed,
                cut_short,
                ..
            } = walk;
            let variances: Option<Vec<Variance>> =
                walked.map(|()| found.into_iter().map(Occurrences::variance).collect());
            let read_itself = assumed.contains(&depth);
            let assumption = measuring.stack.last_mut().map(|entry| &mut entry.assumed);
            if let (true, false, Some(found), Some(assumption)) =
                (read_itself, cut_short, &variances, assumption)
                && found != assumption
            {
                assumption.clone_from(found);
                rewalks += 1;
                if rewalks <= MAX_REWALKS {
                    continue;
                }
            }
            break Measured {
                variances,
                // Its own settled variances are no assumption.
                assumed: assumed.into_iter().filter(|&index| index < depth).collect(),
                cut_short: cut_short || rewalks > MAX_REWALKS,
            };
        };
        measuring.stack.pop();
        measuring.forget_from(depth);
        measured
    }
}

/// More walks of one declaration than its variances can change in.
const MAX_REWALKS: usize = 8;

/// A declaration whose variance is being measured further up a walk, and
/// what a reference back to it is taken to be meanwhile.
struct OnStack {
    mangled: MangledName,
    assumed: Vec<Variance>,
}

/// The declarations whose variance is being measured further up one walk.
#[derive(Default)]
struct Measuring {
    stack: Vec<OnStack>,
    /// Measurements that read what declarations on the stack are assumed to
    /// be, at those stack positions, kept while the assumptions hold.
    within_walk: BTreeMap<MangledName, (Option<Vec<Variance>>, BTreeSet<usize>)>,
}

impl Measuring {
    /// Forget the measurements that read the assumption at stack position
    /// `depth` or deeper, which is being revised or leaving the stack.
    fn forget_from(&mut self, depth: usize) {
        self.within_walk
            .retain(|_, (_, assumed)| assumed.iter().all(|&index| index < depth));
    }
}

/// A declaration's variances, and whether they are partial.
/// - `assumed`: the stack positions of the declarations further up the walk
///   whose assumed variances this measurement read, directly or through a
///   nested measurement.
/// - `cut_short`: the work limit ended the walk.
///
/// A partial measurement depends on where the walk started, so it is not
/// remembered for other walks.
#[derive(Default)]
struct Measured {
    variances: Option<Vec<Variance>>,
    assumed: BTreeSet<usize>,
    cut_short: bool,
}

struct VarianceWalk<'r, 'a> {
    resolver: TypeResolver<'a>,
    found: Vec<Occurrences>,
    measuring: &'r mut Measuring,
    /// See [`Measured`].
    assumed: BTreeSet<usize>,
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

    fn method(&mut self, params: &[crate::Param], ret: &Type, bindings: &BTreeMap<String, Type>) {
        for param in params {
            self.substituted(&param.ty, bindings, Polarity::Bivariant);
        }
        self.substituted(ret, bindings, Polarity::Covariant);
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
        let on_stack = self
            .measuring
            .stack
            .iter()
            .position(|entry| entry.mangled == *mangled);
        let variances = if let Some(index) = on_stack {
            self.assumed.insert(index);
            self.measuring
                .stack
                .get(index)
                .map(|entry| entry.assumed.clone())
        } else {
            let measured = self
                .resolver
                .measure_variances(mangled, name, self.measuring);
            self.assumed.extend(measured.assumed);
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
