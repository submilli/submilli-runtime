//! Compositional nullishness queries for `FieldNarrowingTest::NonNull`.
//!
//! `nullish_set` computes a sound upper bound on which nullish values `ty` may
//! semantically admit (matching `ty.without_aliases()` so `{}`, `object`, and
//! `Type::Refined` stay distinct from bare `unknown`), while `definite_nullish`
//! computes a sound lower bound (`DefiniteCoverage`) for antitone positions
//! such as the right-hand side of `Exclude<T, U>`.

use std::collections::BTreeSet;

use submilli_front_end::compiler_limits::{MAX_TYPE_DEPTH, MAX_TYPE_WORK};
use submilli_front_end::utility_names::{Utility, utility};

use crate::Type;
use crate::codegen::CodegenCtx;
use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::type_size::TypeLimits;
use crate::typechecker::type_param_substitution::TypeParamSubstitution;

/// Compositional upper bound on the nullish values `ty` may semantically admit.
pub(super) fn nullish_set(ctx: &CodegenCtx, ty: &Type) -> Result<NullishSet, CompilerFailure> {
    NullishQuery::new(ctx).nullish_set(ty, 0)
}

/// Five-bit nullishness upper bound over `Type`. Keeping definite `unknown`
/// distinct from `erased` (type parameters, deferred operators, or types that
/// are only *possibly* `unknown`) and `void` distinct from `undefined` makes
/// `Partial`/`Required`/`Readonly`, `Exclude`, `Extract`, `ReturnType`,
/// `FlatArray`, unions, and intersections compose soundly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NullishSet {
    null: bool,
    undefined: bool,
    void: bool,
    /// Definite `unknown`: absorbs union siblings, maps to `{}` under
    /// homomorphic mapped utilities, survives `Exclude<unknown, null | undefined>`,
    /// and is dropped by `Extract<unknown, null | undefined>`.
    /// Must only be set when the type is definitely `unknown`; types that may
    /// instantiate to `never` or a non-`unknown` subtype use `erased` instead
    /// because homomorphic mapped utilities are non-monotone in `unknown`.
    unknown: bool,
    /// Erased type parameter, deferred operator, or possibly-`unknown` type:
    /// could instantiate to any nullish or non-nullish type.
    erased: bool,
}

impl NullishSet {
    const NONE: Self = Self {
        null: false,
        undefined: false,
        void: false,
        unknown: false,
        erased: false,
    };

    const NULL: Self = Self {
        null: true,
        ..Self::NONE
    };

    const UNDEFINED: Self = Self {
        undefined: true,
        ..Self::NONE
    };

    const VOID: Self = Self {
        void: true,
        ..Self::NONE
    };

    const UNKNOWN: Self = Self {
        unknown: true,
        ..Self::NONE
    };

    const ERASED: Self = Self {
        erased: true,
        ..Self::NONE
    };

    pub(super) fn allows_null(self) -> bool {
        self.null || self.unknown || self.erased
    }

    pub(super) fn allows_undefined(self) -> bool {
        self.undefined || self.void || self.unknown || self.erased
    }

    fn allows_void(self) -> bool {
        self.void || self.unknown || self.erased
    }

    /// Demotes concrete `unknown` to `ERASED` when a type is only *possibly*
    /// `unknown` (e.g. `Exclude<unknown, T>`, `FlatArray<unknown[], D>`, or a
    /// `Refined` type) so `Partial`/`Required`/`Readonly` cannot falsely drop
    /// union siblings or `null`/`undefined`.
    fn demote_unknown(self) -> Self {
        if self.unknown { Self::ERASED } else { self }
    }

    fn union(self, other: Self) -> Self {
        if self.unknown || other.unknown {
            return Self::UNKNOWN;
        }
        Self {
            null: self.null || other.null,
            undefined: self.undefined || other.undefined,
            void: self.void || other.void,
            unknown: false,
            erased: self.erased || other.erased,
        }
    }

    fn intersect(self, other: Self) -> Self {
        let unknown = self.unknown && other.unknown;
        let erased =
            (self.erased && (other.erased || other.unknown)) || (other.erased && self.unknown);
        if unknown || erased {
            return Self {
                unknown,
                erased,
                ..Self::NONE
            };
        }
        let null = self.allows_null() && other.allows_null();
        let void = self.allows_void() && other.allows_void();
        let undefined = ((self.undefined || self.erased) && other.allows_undefined())
            || ((other.undefined || other.erased) && self.allows_undefined());
        Self {
            null,
            undefined,
            void,
            unknown: false,
            erased: false,
        }
    }

    /// Homomorphic mapped utilities (`Partial`, `Required`, `Readonly`) map
    /// definite `unknown` to `{}` while preserving other nullish members.
    fn homomorphic(self) -> Self {
        Self {
            unknown: false,
            ..self
        }
    }

    /// `Exclude<self, rhs_def>`: `unknown` is excluded by true `unknown` or
    /// `{} | null | undefined`; `void` is excluded only by `void` or true
    /// `unknown` (never bare `{} | null | undefined`); `undefined` is excluded
    /// by `undefined`, `void`, `unknown`, or `{} | null | undefined`.
    fn exclude(self, rhs_def: DefiniteCoverage) -> Self {
        if rhs_def.covers_every_value() && rhs_def.covers_void() {
            return Self::NONE;
        }
        let erased_leaves_void = self.erased && rhs_def.covers_every_value();
        Self {
            null: self.null && !rhs_def.covers_null(),
            undefined: self.undefined && !rhs_def.covers_undefined(),
            void: (self.void && !rhs_def.covers_void()) || erased_leaves_void,
            unknown: self.unknown && !rhs_def.covers_every_value(),
            erased: self.erased && !rhs_def.covers_every_value(),
        }
    }

    /// `Extract<self, rhs>`: keeps a member only when it may be assignable to
    /// `rhs`. `unknown` stays concrete `UNKNOWN` only when `rhs` definitely
    /// covers every value (`rhs.unknown || rhs_def_all`), becomes `ERASED` when
    /// `rhs` might cover every value (`rhs.erased || rhs_may_all`), and drops
    /// otherwise. `void` is assignable only to `void`, `unknown`, or `erased`
    /// (never bare `undefined` or `{} | null | undefined`).
    fn extract(self, rhs: Self, rhs_def_all: bool, rhs_may_all: bool) -> Self {
        let rhs_is_def_top = rhs.unknown || rhs_def_all;
        if self.unknown && rhs_is_def_top {
            return Self::UNKNOWN;
        }
        let rhs_may_cover_unknown = rhs_is_def_top || rhs.erased || rhs_may_all;
        Self {
            null: (self.null || self.erased) && (rhs.allows_null() || rhs_may_all),
            undefined: (self.undefined || self.erased) && (rhs.allows_undefined() || rhs_may_all),
            void: (self.void || self.erased) && rhs.allows_void(),
            unknown: false,
            erased: (self.erased || self.unknown) && rhs_may_cover_unknown,
        }
    }
}

/// Sound lower bound of **direct, distributable** nullish/top members `ty`
/// definitely covers regardless of erased type parameters. Only direct union
/// members (never indirect `Type::Intersection` or recursive `Type::AliasRef`
/// expansions) contribute here, matching `Type::holds_every_value` and
/// `utility_types::members` so `definite_nullish` can never over-approximate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DefiniteCoverage {
    null: bool,
    undefined: bool,
    void: bool,
    /// True `Type::Unknown` (covers every value including `void`).
    unknown: bool,
    /// Union directly containing `{}`, `null`, and `undefined`.
    union_top: bool,
}

impl DefiniteCoverage {
    const NONE: Self = Self {
        null: false,
        undefined: false,
        void: false,
        unknown: false,
        union_top: false,
    };

    const NULL: Self = Self {
        null: true,
        ..Self::NONE
    };

    const UNDEFINED: Self = Self {
        undefined: true,
        ..Self::NONE
    };

    const VOID: Self = Self {
        void: true,
        ..Self::NONE
    };

    const UNKNOWN: Self = Self {
        unknown: true,
        ..Self::NONE
    };

    fn covers_every_value(self) -> bool {
        self.unknown || self.union_top
    }

    fn covers_null(self) -> bool {
        self.null || self.unknown || self.union_top
    }

    fn covers_undefined(self) -> bool {
        self.undefined || self.void || self.unknown || self.union_top
    }

    fn covers_void(self) -> bool {
        self.void || self.unknown
    }

    fn union(self, other: Self) -> Self {
        if self.unknown || other.unknown {
            return Self::UNKNOWN;
        }
        Self {
            null: self.null || other.null,
            undefined: self.undefined || other.undefined,
            void: self.void || other.void,
            unknown: false,
            union_top: self.union_top || other.union_top,
        }
    }

    fn homomorphic(self) -> Self {
        Self {
            null: self.null || self.union_top,
            undefined: self.undefined || self.union_top,
            void: self.void,
            unknown: false,
            union_top: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlatDepth {
    Never,
    Exact(u32),
    Unbounded,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NullishMode {
    NullishSet,
    MayCoverEmptyObject,
    ReturnType,
    FlatArray(Option<u32>),
    FlatArrayReturnType(Option<u32>),
    FlatArrayDepth,
}

/// Shared state for bounded, cycle-safe nullishness queries over `Type`.
struct NullishQuery<'a> {
    ctx: &'a CodegenCtx<'a>,
    seen: BTreeSet<(NullishMode, Type)>,
    limits: TypeLimits,
    work: u64,
}

impl<'a> NullishQuery<'a> {
    fn new(ctx: &'a CodegenCtx<'a>) -> Self {
        Self {
            ctx,
            seen: BTreeSet::new(),
            limits: TypeLimits::default(),
            work: 0,
        }
    }

    fn enter(&mut self, depth: u32) -> Result<(), CompilerFailure> {
        if depth > MAX_TYPE_DEPTH {
            return Err(
                crate::type_size::TypeTooLarge::Depth.into_failure(CompilerStage::Codegen, None)
            );
        }
        self.work = self.work.saturating_add(1);
        if self.work > MAX_TYPE_WORK {
            return Err(
                crate::type_size::TypeTooLarge::Work.into_failure(CompilerStage::Codegen, None)
            );
        }
        Ok(())
    }

    /// Cycle-guarded `Type::AliasRef` expansion scoped to `mode`.
    fn expand_alias<T>(
        &mut self,
        mode: NullishMode,
        alias: &Type,
        cycle_default: T,
        unexpanded_default: T,
        f: impl FnOnce(&mut Self, &Type) -> Result<T, CompilerFailure>,
    ) -> Result<T, CompilerFailure> {
        let key = (mode, alias.clone());
        if !self.seen.insert(key.clone()) {
            return Ok(cycle_default);
        }
        let expanded = self
            .ctx
            .validator_bodies
            .expand_one(alias)
            .map_err(|exceeded| exceeded.into_failure(CompilerStage::Codegen, None))?;
        let result = if &expanded == alias {
            unexpanded_default
        } else {
            f(self, &expanded)?
        };
        self.seen.remove(&key);
        Ok(result)
    }

    fn nullish_set(&mut self, ty: &Type, depth: u32) -> Result<NullishSet, CompilerFailure> {
        self.enter(depth)?;
        match ty.without_aliases() {
            Type::Unknown => Ok(NullishSet::UNKNOWN),
            Type::Null => Ok(NullishSet::NULL),
            Type::Undefined => Ok(NullishSet::UNDEFINED),
            Type::Void => Ok(NullishSet::VOID),
            // `keyof T` is always a key subtype (`string | number`), never nullish.
            Type::KeyOf(_) => Ok(NullishSet::NONE),
            Type::Readonly(inner) => self.nullish_set(inner, depth + 1),
            Type::Refined { original, ty } => Ok(self
                .nullish_set(original, depth + 1)?
                .intersect(self.nullish_set(ty, depth + 1)?)),
            Type::Union(members) => {
                let mut acc = NullishSet::NONE;
                for member in members {
                    acc = acc.union(self.nullish_set(member, depth + 1)?);
                }
                Ok(acc)
            }
            Type::Intersection(members) => {
                let mut iter = members.iter();
                let Some(first) = iter.next() else {
                    return Ok(NullishSet::UNKNOWN);
                };
                let mut acc = self.nullish_set(first, depth + 1)?;
                for member in iter {
                    acc = acc.intersect(self.nullish_set(member, depth + 1)?);
                }
                Ok(acc)
            }
            alias @ Type::AliasRef { mangled, args, .. } => {
                if let Some(util) = utility(mangled) {
                    return self.utility_nullish(util, args, depth);
                }
                self.expand_alias(
                    NullishMode::NullishSet,
                    alias,
                    NullishSet::NONE,
                    NullishSet::ERASED,
                    // Recursive aliases stay unexpanded when `Partial`
                    // distributes over a union (`Partial<null | Rec<unknown>>`
                    // yields `{} | null`), so demote concrete `UNKNOWN`.
                    |this, expanded| Ok(this.nullish_set(expanded, depth + 1)?.demote_unknown()),
                )
            }
            member
                if matches!(member, Type::GenericParam { .. } | Type::TypeVar(_))
                    || member.is_type_operator() =>
            {
                Ok(NullishSet::ERASED)
            }
            _ => Ok(NullishSet::NONE),
        }
    }

    fn utility_nullish(
        &mut self,
        util: Utility,
        args: &[Type],
        depth: u32,
    ) -> Result<NullishSet, CompilerFailure> {
        match util {
            Utility::NonNullable
            | Utility::Pick
            | Utility::Omit
            | Utility::Parameters
            | Utility::Uppercase
            | Utility::Lowercase
            | Utility::Capitalize
            | Utility::Uncapitalize => Ok(NullishSet::NONE),
            Utility::Partial | Utility::Required | Utility::Readonly => match args.first() {
                Some(inner) => Ok(self.nullish_set(inner, depth + 1)?.homomorphic()),
                None => Ok(NullishSet::ERASED),
            },
            Utility::NoInfer => match args.first() {
                Some(inner) => self.nullish_set(inner, depth + 1),
                None => Ok(NullishSet::ERASED),
            },
            Utility::Extract => match (args.first(), args.get(1)) {
                (Some(lhs), Some(rhs)) => {
                    let l = self.nullish_set(lhs, depth + 1)?;
                    let r = self.nullish_set(rhs, depth + 1)?;
                    let r_def_all = self.definite_nullish(rhs, depth + 1)?.covers_every_value();
                    let r_may_all = r_def_all || self.may_cover_every_value(rhs, r, depth + 1)?;
                    Ok(l.extract(r, r_def_all, r_may_all))
                }
                _ => Ok(NullishSet::ERASED),
            },
            Utility::Exclude => match (args.first(), args.get(1)) {
                (Some(lhs), Some(rhs)) => {
                    let r_def = self.definite_nullish(rhs, depth + 1)?;
                    let excluded = self.nullish_set(lhs, depth + 1)?.exclude(r_def);
                    // If `unknown` survived `Exclude<unknown, rhs>` only because
                    // `rhs` is not *definitely* top, check whether `rhs` *may*
                    // cover every value; if so, `Exclude` could evaluate to
                    // `never` and must not carry concrete `UNKNOWN`.
                    if excluded.unknown {
                        let r_upper = self.nullish_set(rhs, depth + 1)?;
                        if self.may_cover_every_value(rhs, r_upper, depth + 1)? {
                            return Ok(excluded.demote_unknown());
                        }
                    }
                    Ok(excluded)
                }
                _ => Ok(NullishSet::ERASED),
            },
            Utility::ReturnType => match args.first() {
                Some(callee) => self.return_type_map(
                    callee,
                    NullishMode::ReturnType,
                    depth + 1,
                    &|this, ret, d| this.nullish_set(ret, d),
                ),
                None => Ok(NullishSet::ERASED),
            },
            Utility::FlatArray => self.flat_array_application(args, Some(0), depth),
        }
    }

    fn may_cover_every_value(
        &mut self,
        ty: &Type,
        upper: NullishSet,
        depth: u32,
    ) -> Result<bool, CompilerFailure> {
        Ok(upper.unknown
            || upper.erased
            || (upper.allows_null()
                && upper.allows_undefined()
                && self.may_cover_empty_object(ty, depth)?))
    }

    /// Sound lower bound of direct, distributable nullish/top members `ty`
    /// definitely covers regardless of erased type parameter instantiations.
    /// Indirect forms (`Type::Refined`, non-empty `Type::Intersection`,
    /// recursive `Type::AliasRef`, and nested `Exclude`/`Extract`) conservatively
    /// return `NONE` so this lower bound matches `Type::holds_every_value` and
    /// `utility_types::members` without over-approximating.
    fn definite_nullish(
        &mut self,
        ty: &Type,
        depth: u32,
    ) -> Result<DefiniteCoverage, CompilerFailure> {
        self.enter(depth)?;
        match ty.without_aliases() {
            Type::Unknown => Ok(DefiniteCoverage::UNKNOWN),
            Type::Null => Ok(DefiniteCoverage::NULL),
            Type::Undefined => Ok(DefiniteCoverage::UNDEFINED),
            Type::Void => Ok(DefiniteCoverage::VOID),
            Type::Union(members) => {
                let mut acc = DefiniteCoverage::NONE;
                let mut has_empty_object = false;
                for member in members {
                    if !has_empty_object {
                        has_empty_object = self.definite_covers_empty_object(member, depth + 1)?;
                    }
                    acc = acc.union(self.definite_nullish(member, depth + 1)?);
                }
                if !acc.unknown && has_empty_object && acc.null && acc.undefined {
                    acc.union_top = true;
                }
                Ok(acc)
            }
            Type::Intersection(members) if members.is_empty() => Ok(DefiniteCoverage::UNKNOWN),
            Type::AliasRef { mangled, args, .. } => match utility(mangled) {
                Some(util) => self.definite_utility_nullish(util, args, depth),
                None => Ok(DefiniteCoverage::NONE),
            },
            _ => Ok(DefiniteCoverage::NONE),
        }
    }

    fn definite_utility_nullish(
        &mut self,
        util: Utility,
        args: &[Type],
        depth: u32,
    ) -> Result<DefiniteCoverage, CompilerFailure> {
        match util {
            Utility::NoInfer => match args.first() {
                Some(inner) => self.definite_nullish(inner, depth + 1),
                None => Ok(DefiniteCoverage::NONE),
            },
            Utility::Partial | Utility::Required | Utility::Readonly => match args.first() {
                Some(inner) => {
                    // Homomorphic mapped utilities map `unknown` to `{}`: if
                    // `inner` may instantiate to `unknown` (e.g. `Partial<T | null>`
                    // at `T = unknown`), `unknown` absorbs union siblings and
                    // drops nullish coverage.
                    let upper = self.nullish_set(inner, depth + 1)?;
                    if upper.unknown || upper.erased {
                        return Ok(DefiniteCoverage::NONE);
                    }
                    Ok(self.definite_nullish(inner, depth + 1)?.homomorphic())
                }
                None => Ok(DefiniteCoverage::NONE),
            },
            Utility::ReturnType => match args.first() {
                Some(callee) => self.definite_return_type(callee, depth + 1),
                None => Ok(DefiniteCoverage::NONE),
            },
            _ => Ok(DefiniteCoverage::NONE),
        }
    }

    fn definite_return_type(
        &mut self,
        callee: &Type,
        depth: u32,
    ) -> Result<DefiniteCoverage, CompilerFailure> {
        self.enter(depth)?;
        match callee.without_aliases() {
            Type::Function { generics, ret, .. } => {
                if generics.is_empty() {
                    self.definite_nullish(ret, depth + 1)
                } else {
                    let instantiated = self.instantiate_own_function_return(generics, ret)?;
                    self.definite_nullish(&instantiated, depth + 1)
                }
            }
            Type::Union(members) => {
                let mut acc = DefiniteCoverage::NONE;
                for member in members {
                    acc = acc.union(self.definite_return_type(member, depth + 1)?);
                }
                Ok(acc)
            }
            Type::AliasRef { mangled, args, .. } => match utility(mangled) {
                Some(Utility::NoInfer | Utility::NonNullable) => match args.first() {
                    Some(inner) => self.definite_return_type(inner, depth + 1),
                    None => Ok(DefiniteCoverage::NONE),
                },
                _ => Ok(DefiniteCoverage::NONE),
            },
            _ => Ok(DefiniteCoverage::NONE),
        }
    }

    /// Sound lower bound: whether `ty` definitely covers every non-nullish value (`{}`).
    fn definite_covers_empty_object(
        &mut self,
        ty: &Type,
        depth: u32,
    ) -> Result<bool, CompilerFailure> {
        self.enter(depth)?;
        let unwrapped = ty.without_aliases();
        if unwrapped.is_empty_object() {
            return Ok(true);
        }
        match unwrapped {
            Type::Unknown => Ok(true),
            Type::Readonly(inner) => self.definite_covers_empty_object(inner, depth + 1),
            Type::Union(members) => {
                for member in members {
                    if self.definite_covers_empty_object(member, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Type::Intersection(members) => Ok(members.is_empty()),
            Type::AliasRef { mangled, args, .. } => {
                let Some(util) = utility(mangled) else {
                    // `Type::holds_every_value` does not expand recursive `AliasRef`s.
                    return Ok(false);
                };
                match util {
                    Utility::NoInfer => match args.first() {
                        Some(inner) => self.definite_covers_empty_object(inner, depth + 1),
                        None => Ok(false),
                    },
                    Utility::Partial
                    | Utility::Required
                    | Utility::Readonly
                    | Utility::NonNullable => match args.first() {
                        Some(inner) => Ok(self.definite_covers_empty_object(inner, depth + 1)?
                            || self
                                .definite_nullish(inner, depth + 1)?
                                .covers_every_value()),
                        None => Ok(false),
                    },
                    _ => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }

    /// Sound upper bound: whether `ty` may cover every non-nullish value (`{}`).
    fn may_cover_empty_object(&mut self, ty: &Type, depth: u32) -> Result<bool, CompilerFailure> {
        self.enter(depth)?;
        let unwrapped = ty.without_aliases();
        if unwrapped.is_empty_object() {
            return Ok(true);
        }
        match unwrapped {
            Type::Unknown | Type::GenericParam { .. } | Type::TypeVar(_) | Type::Refined { .. } => {
                Ok(true)
            }
            Type::KeyOf(_) => Ok(false),
            Type::Readonly(inner) => self.may_cover_empty_object(inner, depth + 1),
            Type::Union(members) => {
                for member in members {
                    if self.may_cover_empty_object(member, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Type::Intersection(members) => {
                for member in members {
                    if !self.may_cover_empty_object(member, depth + 1)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            alias @ Type::AliasRef { mangled, args, .. } => {
                if let Some(util) = utility(mangled) {
                    return match util {
                        Utility::Uppercase
                        | Utility::Lowercase
                        | Utility::Capitalize
                        | Utility::Uncapitalize
                        | Utility::Parameters => Ok(false),
                        Utility::NoInfer => match args.first() {
                            Some(inner) => self.may_cover_empty_object(inner, depth + 1),
                            None => Ok(true),
                        },
                        _ => Ok(true),
                    };
                }
                self.expand_alias(
                    NullishMode::MayCoverEmptyObject,
                    alias,
                    false,
                    true,
                    |this, expanded| this.may_cover_empty_object(expanded, depth + 1),
                )
            }
            other if other.is_type_operator() => Ok(true),
            _ => Ok(false),
        }
    }

    /// Maps `map_ret` over the return type(s) of `ReturnType<callee>`,
    /// instantiating any generic function's own type parameters to their bounds
    /// (or `unknown`) to match tsc's `ReturnType`.
    fn return_type_map(
        &mut self,
        callee: &Type,
        mode: NullishMode,
        depth: u32,
        map_ret: &impl Fn(&mut Self, &Type, u32) -> Result<NullishSet, CompilerFailure>,
    ) -> Result<NullishSet, CompilerFailure> {
        self.enter(depth)?;
        match callee.without_aliases() {
            Type::Never | Type::Null | Type::Undefined | Type::Void => Ok(NullishSet::NONE),
            Type::Readonly(inner) => self.return_type_map(inner, mode, depth + 1, map_ret),
            Type::Refined { ty, .. } => Ok(self
                .return_type_map(ty, mode, depth + 1, map_ret)?
                .demote_unknown()),
            Type::Function { generics, ret, .. } => {
                if generics.is_empty() {
                    map_ret(self, ret, depth + 1)
                } else {
                    let instantiated = self.instantiate_own_function_return(generics, ret)?;
                    map_ret(self, &instantiated, depth + 1)
                }
            }
            Type::Union(members) => {
                let mut acc = NullishSet::NONE;
                for member in members {
                    acc = acc.union(self.return_type_map(member, mode, depth + 1, map_ret)?);
                }
                Ok(acc)
            }
            alias @ Type::AliasRef { mangled, args, .. } => {
                if let Some(util) = utility(mangled) {
                    return match util {
                        Utility::NoInfer | Utility::NonNullable => match args.first() {
                            Some(inner) => self.return_type_map(inner, mode, depth + 1, map_ret),
                            None => Ok(NullishSet::ERASED),
                        },
                        _ => Ok(NullishSet::ERASED),
                    };
                }
                self.expand_alias(
                    mode,
                    alias,
                    NullishSet::NONE,
                    NullishSet::ERASED,
                    |this, expanded| this.return_type_map(expanded, mode, depth + 1, map_ret),
                )
            }
            _ => Ok(NullishSet::ERASED),
        }
    }

    /// Evaluates `FlatArray<args[0], args[1]>` composed with `extra_remaining`
    /// outer unwrap steps (`None` = unbounded).
    fn flat_array_application(
        &mut self,
        args: &[Type],
        extra_remaining: Option<u32>,
        depth: u32,
    ) -> Result<NullishSet, CompilerFailure> {
        let Some(arr) = args.first() else {
            return Ok(NullishSet::ERASED);
        };
        let inner_depth = match args.get(1) {
            Some(d) => self.flat_array_depth(d, depth + 1)?,
            None => FlatDepth::Unbounded,
        };
        match (extra_remaining, inner_depth) {
            (_, FlatDepth::Never) => Ok(NullishSet::NONE),
            (Some(outer), FlatDepth::Exact(count)) => {
                self.flat_array_nullish(arr, outer.checked_add(count), depth + 1)
            }
            // When depth is erased or multi-valued (e.g. generic `D` or `-1 | 0`),
            // `FlatArray<unknown[], D>` may keep `unknown[]` at `D = -1` or
            // unwrap all layers, so demote concrete `UNKNOWN`.
            _ => Ok(self
                .flat_array_nullish(arr, None, depth + 1)?
                .demote_unknown()),
        }
    }

    /// Unwrap count (`d + 1`) for a `FlatArray<A, D>` depth argument (matching
    /// the prelude depth table `[-1..=21]` in `utility_types.rs`). A union of
    /// two or more distinct literal depths returns `Unbounded` because
    /// `flat_predecessor(-1)` widens to `all_flat_depths()` once the smaller
    /// depth hits `-1`, unwrapping all remaining layers.
    fn flat_array_depth(&mut self, ty: &Type, depth: u32) -> Result<FlatDepth, CompilerFailure> {
        self.enter(depth)?;
        let literal_depth = |n: f64| {
            if n.fract() == 0.0 && (-1.0..=21.0).contains(&n) {
                FlatDepth::Exact((n as i32 + 1) as u32)
            } else {
                FlatDepth::Unbounded
            }
        };
        match ty.without_aliases() {
            Type::Never => Ok(FlatDepth::Never),
            Type::Readonly(inner) => self.flat_array_depth(inner, depth + 1),
            Type::NumberLiteral(n) => Ok(literal_depth(n.0)),
            enum_ty @ Type::NumberEnum { .. } => {
                Ok(enum_ty
                    .enum_member_value()
                    .map_or(FlatDepth::Unbounded, |value| match value.literal_type() {
                        Type::NumberLiteral(n) => literal_depth(n.0),
                        _ => FlatDepth::Unbounded,
                    }))
            }
            Type::Union(members) => {
                let mut acc = FlatDepth::Never;
                for member in members {
                    let part = self.flat_array_depth(member, depth + 1)?;
                    acc = match (acc, part) {
                        (FlatDepth::Never, other) | (other, FlatDepth::Never) => other,
                        (FlatDepth::Exact(a), FlatDepth::Exact(b)) if a == b => FlatDepth::Exact(a),
                        _ => return Ok(FlatDepth::Unbounded),
                    };
                }
                Ok(acc)
            }
            alias @ Type::AliasRef { mangled, args, .. } => {
                if let Some(Utility::NoInfer) = utility(mangled) {
                    return match args.first() {
                        Some(inner) => self.flat_array_depth(inner, depth + 1),
                        None => Ok(FlatDepth::Unbounded),
                    };
                }
                self.expand_alias(
                    NullishMode::FlatArrayDepth,
                    alias,
                    FlatDepth::Unbounded,
                    FlatDepth::Unbounded,
                    |this, expanded| this.flat_array_depth(expanded, depth + 1),
                )
            }
            _ => Ok(FlatDepth::Unbounded),
        }
    }

    /// Nullish summary of flattening `arr` through `remaining` array/tuple
    /// layers (`None` = unbounded).
    fn flat_array_nullish(
        &mut self,
        arr: &Type,
        remaining: Option<u32>,
        depth: u32,
    ) -> Result<NullishSet, CompilerFailure> {
        self.enter(depth)?;
        let next = match remaining {
            Some(0) => return self.nullish_set(arr, depth + 1),
            Some(n) => Some(n - 1),
            None => None,
        };
        match arr.without_aliases() {
            Type::Never => Ok(NullishSet::NONE),
            Type::Readonly(inner) => self.flat_array_nullish(inner, remaining, depth + 1),
            Type::Refined { .. } | Type::Intersection(_) => {
                if is_definitely_non_array_leaf(arr.peel()) {
                    self.nullish_set(arr, depth + 1)
                } else {
                    Ok(NullishSet::ERASED)
                }
            }
            Type::Array(elem) => self.flat_array_nullish(elem, next, depth + 1),
            Type::Tuple(tuple) => {
                let mut acc = if tuple.optional > 0 {
                    NullishSet::UNDEFINED
                } else {
                    NullishSet::NONE
                };
                for elem in &tuple.elements {
                    acc = acc.union(self.flat_array_nullish(elem, next, depth + 1)?);
                }
                Ok(acc)
            }
            Type::Union(members) => {
                let mut acc = NullishSet::NONE;
                for member in members {
                    acc = acc.union(self.flat_array_nullish(member, remaining, depth + 1)?);
                }
                Ok(acc)
            }
            alias @ Type::AliasRef { mangled, args, .. } => {
                if let Some(util) = utility(mangled) {
                    return self.flat_array_utility_nullish(util, args, remaining, depth);
                }
                self.expand_alias(
                    NullishMode::FlatArray(remaining),
                    alias,
                    NullishSet::NONE,
                    NullishSet::ERASED,
                    |this, expanded| this.flat_array_nullish(expanded, remaining, depth + 1),
                )
            }
            other => self.nullish_set(other, depth + 1),
        }
    }

    fn flat_array_utility_nullish(
        &mut self,
        util: Utility,
        args: &[Type],
        remaining: Option<u32>,
        depth: u32,
    ) -> Result<NullishSet, CompilerFailure> {
        match util {
            Utility::NoInfer => match args.first() {
                Some(inner) => self.flat_array_nullish(inner, remaining, depth + 1),
                None => Ok(NullishSet::ERASED),
            },
            Utility::Readonly => match args.first() {
                Some(inner) => self.flat_array_top_mapped(inner, remaining, false, depth + 1),
                None => Ok(NullishSet::ERASED),
            },
            Utility::NonNullable => match args.first() {
                Some(inner) => self.flat_array_top_mapped(inner, remaining, true, depth + 1),
                None => Ok(NullishSet::ERASED),
            },
            Utility::FlatArray => self.flat_array_application(args, remaining, depth),
            Utility::ReturnType => match args.first() {
                Some(callee) => self.return_type_map(
                    callee,
                    NullishMode::FlatArrayReturnType(remaining),
                    depth + 1,
                    &|this, ret, d| this.flat_array_nullish(ret, remaining, d),
                ),
                None => Ok(NullishSet::ERASED),
            },
            Utility::Pick
            | Utility::Omit
            | Utility::Uppercase
            | Utility::Lowercase
            | Utility::Capitalize
            | Utility::Uncapitalize => Ok(NullishSet::NONE),
            // Utilities that can produce arrays or tuples with transformed
            // elements stay conservative while unwrapping.
            Utility::Partial
            | Utility::Required
            | Utility::Parameters
            | Utility::Exclude
            | Utility::Extract => Ok(NullishSet::ERASED),
        }
    }

    /// Flattens `inner` under a top-level `Readonly<inner>` (`strip_nullish = false`)
    /// or `NonNullable<inner>` (`strip_nullish = true`) without applying the outer
    /// wrapper to inner array or tuple elements.
    fn flat_array_top_mapped(
        &mut self,
        inner: &Type,
        remaining: Option<u32>,
        strip_nullish: bool,
        depth: u32,
    ) -> Result<NullishSet, CompilerFailure> {
        self.enter(depth)?;
        match inner.without_aliases() {
            Type::Never | Type::Unknown => Ok(NullishSet::NONE),
            Type::Readonly(sub) => {
                self.flat_array_top_mapped(sub, remaining, strip_nullish, depth + 1)
            }
            Type::Null | Type::Undefined | Type::Void => {
                if strip_nullish {
                    Ok(NullishSet::NONE)
                } else {
                    self.nullish_set(inner, depth + 1)
                }
            }
            Type::Array(_) | Type::Tuple(_) => self.flat_array_nullish(inner, remaining, depth + 1),
            Type::Union(members) => {
                let mut acc = NullishSet::NONE;
                let mut may_collapse_to_unknown = false;
                for member in members {
                    let upper = self.nullish_set(member, depth + 1)?;
                    if upper.unknown {
                        // Any `unknown` member absorbs the whole union before
                        // `Readonly`/`NonNullable` runs, mapping it to `{}`.
                        return Ok(NullishSet::NONE);
                    }
                    if upper.erased {
                        may_collapse_to_unknown = true;
                    }
                    acc = acc.union(self.flat_array_top_mapped(
                        member,
                        remaining,
                        strip_nullish,
                        depth + 1,
                    )?);
                }
                Ok(if may_collapse_to_unknown {
                    acc.demote_unknown()
                } else {
                    acc
                })
            }
            Type::GenericParam { .. }
            | Type::TypeVar(_)
            | Type::AliasRef { .. }
            | Type::Refined { .. }
            | Type::Intersection(_) => Ok(NullishSet::ERASED),
            other if other.is_type_operator() => Ok(NullishSet::ERASED),
            _ => Ok(NullishSet::NONE),
        }
    }

    /// Replaces a generic function's own type parameters in `ret` with their
    /// bounds (with every own parameter in a bound replaced by `unknown`), or
    /// `unknown`, using the query's bounded `TypeLimits` budget.
    fn instantiate_own_function_return(
        &self,
        generics: &crate::GenericParameters,
        ret: &Type,
    ) -> Result<Type, CompilerFailure> {
        let unknowns = vec![Type::Unknown; generics.names.len()];
        let clear_own = TypeParamSubstitution::instantiation(&generics.names, &unknowns);
        let mut args = Vec::with_capacity(generics.names.len());
        for name in &generics.names {
            let arg = match generics.constraints.get(name) {
                Some(bound) => clear_own
                    .apply(bound, &self.limits)
                    .map_err(|exceeded| exceeded.into_failure(CompilerStage::Codegen, None))?,
                None => Type::Unknown,
            };
            args.push(arg);
        }
        TypeParamSubstitution::instantiation(&generics.names, &args)
            .apply(ret, &self.limits)
            .map_err(|exceeded| exceeded.into_failure(CompilerStage::Codegen, None))
    }
}

fn is_definitely_non_array_leaf(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Never
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::TemplateLiteral { .. }
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
            | Type::Uint8Array
            | Type::Function { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::{DefiniteCoverage, NullishSet};

    #[test]
    fn nullish_set_intersect_preserves_undefined_with_void() {
        let t_and_void = NullishSet::ERASED.intersect(NullishSet::VOID);
        assert_eq!(
            t_and_void.extract(NullishSet::UNDEFINED, false, false),
            NullishSet::UNDEFINED
        );
        assert_eq!(
            t_and_void.exclude(DefiniteCoverage::UNDEFINED),
            NullishSet::VOID
        );

        let void_or_undef = NullishSet::VOID.union(NullishSet::UNDEFINED);
        let intersected = void_or_undef.intersect(NullishSet::VOID);
        assert_eq!(
            intersected.extract(NullishSet::UNDEFINED, false, false),
            NullishSet::UNDEFINED
        );

        let unknown_and_void = NullishSet::UNKNOWN.intersect(NullishSet::VOID);
        assert_eq!(unknown_and_void, NullishSet::VOID);
        assert_eq!(
            unknown_and_void.extract(NullishSet::UNDEFINED, false, false),
            NullishSet::NONE
        );
    }

    #[test]
    fn nullish_set_union_extract_and_exclude_distinguish_unknown_from_union_top() {
        // `unknown | null = unknown`, so `Partial<unknown | null> = {}`.
        assert_eq!(
            NullishSet::UNKNOWN.union(NullishSet::NULL).homomorphic(),
            NullishSet::NONE
        );
        assert_eq!(
            NullishSet::UNKNOWN.union(NullishSet::ERASED).homomorphic(),
            NullishSet::NONE
        );

        // `Extract<unknown, U>` and `Extract<unknown, NonNullable<U> | null | undefined>`
        // become `ERASED`, not concrete `UNKNOWN`.
        let extracted_erased = NullishSet::UNKNOWN.extract(NullishSet::ERASED, false, false);
        assert_eq!(extracted_erased, NullishSet::ERASED);
        assert!(
            extracted_erased
                .union(NullishSet::NULL)
                .homomorphic()
                .allows_null()
        );
        let extracted_may_top =
            NullishSet::UNKNOWN.extract(NullishSet::NULL.union(NullishSet::UNDEFINED), false, true);
        assert_eq!(extracted_may_top, NullishSet::ERASED);

        // `Extract<unknown, {} | null | undefined>` stays concrete `UNKNOWN`.
        assert_eq!(
            NullishSet::UNKNOWN.extract(NullishSet::NONE, true, true),
            NullishSet::UNKNOWN
        );

        // Union-top `{} | null | undefined` excludes `unknown`, `null`, and
        // `undefined`, but does NOT exclude `void` (`Exclude<void, {} | null | undefined> = void`).
        let union_top = DefiniteCoverage {
            null: true,
            undefined: true,
            union_top: true,
            ..DefiniteCoverage::NONE
        };
        assert_eq!(NullishSet::VOID.exclude(union_top), NullishSet::VOID);
        assert_eq!(NullishSet::UNKNOWN.exclude(union_top), NullishSet::NONE);
        assert_eq!(NullishSet::ERASED.exclude(union_top), NullishSet::VOID);
        assert_eq!(
            NullishSet::VOID.exclude(DefiniteCoverage::UNKNOWN),
            NullishSet::NONE
        );
        assert_eq!(
            NullishSet::ERASED.exclude(DefiniteCoverage::UNKNOWN),
            NullishSet::NONE
        );
    }
}
