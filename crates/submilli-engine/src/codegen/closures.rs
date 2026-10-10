//! Per-program closure type collection + emission.
//!
//! A `$closure_<sig>` rec-group type is needed wherever a function-typed value
//! occupies a slot — which is wider than the set of closures the module builds,
//! since the closure may be built by a caller in another package. The module's
//! own closure literals and adapters cover what it constructs; three collectors
//! cover the shapes it only names:
//!
//! * `CodegenAnalysis`'s type funnel — every type the module *mentions*, after
//!   substitution: expression types, slot annotations, cast targets, plus the
//!   sigs the shape-dispatch paths build for themselves — a method call's, from
//!   its own arity, and a property access's `() -> R` / `(W) -> void` accessor
//!   branch, which is emitted whether or not an accessor is declared.
//! * [`class_member_sigs`] — local class members, whose ABI is fixed by the
//!   declaration whether or not the module mentions them.
//! * [`collect_from_dependencies`] — the imported surface of other packages.
//!
//! Local `interface` and `type` declarations need no collector of their own: an
//! object shape's payload is a uniform `(ref null $Object)` array and casts test
//! function fields signature-blind, so a member's type never becomes a Wasm
//! type on its own. The one thing a member *does* fix is the sig its dispatch
//! goes through, and the funnel takes that from the call site — where the
//! generics are already substituted, which the declaration's spelling isn't.
//!
//! Missing a sig is an internal compiler failure, so these over-collect
//! freely: `emit_arity_closures` dedups, and an unused shape costs one func
//! type plus one struct type.

use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::{
    CompositeInnerType, CompositeType, FieldType, FuncType, HeapType, RefType, StorageType,
    StructType, SubType, TypeSection, ValType,
};

use crate::codegen::dependency_usage::{DependencyType, DependencyUsage};
use crate::codegen::intrinsics::IntrinsicTypeIndices;
use crate::codegen::symbol_table::SymbolTable;
use crate::{
    CapturedVar, ClosureBody, Dispatch, ExprId, Shape, Type, TypeKind, TypedAst, TypedParam,
    ValueKind, ValueSymbol,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClosureSig {
    pub arity: u8,
    pub is_void: bool,
}

impl ClosureSig {
    /// The closure ABI reduces a signature to these two facts, so callers that
    /// know an arity and a return type need not build a `Type::Function`.
    pub fn of(arity: usize, ret: &Type) -> Result<Self, crate::compiler_error::CompilerFailure> {
        let arity = crate::compiler_limits::checked_closure_arity(arity).map_err(|error| {
            crate::compiler_error::CompilerFailure::Limit {
                stage: crate::compiler_error::CompilerStage::Codegen,
                span: None,
                message: error.to_string(),
                help: vec!["group arguments into an object or a rest parameter".into()],
            }
        })?;
        Ok(Self {
            arity,
            is_void: ret.is_void(),
        })
    }
}

pub fn classify(sig: &Type) -> Result<ClosureSig, crate::compiler_error::CompilerFailure> {
    let Type::Function { params, ret, .. } = sig.peel() else {
        return Err(super::internal_failure(
            "closure classification requires a function type",
        ));
    };
    ClosureSig::of(params.len(), ret)
}

#[derive(Clone, Debug)]
pub struct ClosureMeta {
    pub this_type: Option<Type>,
    pub self_name: Option<crate::Ident>,
    pub runtime_generics: Vec<String>,
    pub expr_id: ExprId,
    pub signature: Type,
    pub captured: Vec<CapturedVar>,
    pub params: Vec<TypedParam>,
    pub body: ClosureBody,
    pub return_type: Type,
}

/// Consumer modules must re-declare these as a local closure rec group so
/// import signatures and cast sites using `(ref $closure_<sig>)` resolve
/// under WasmGC structural canonicalization.
pub fn collect_from_dependencies<'a>(
    values: impl IntoIterator<Item = &'a ValueSymbol>,
    shapes: impl IntoIterator<Item = &'a Shape>,
    types: impl IntoIterator<Item = &'a DependencyType<'a>>,
    usage: &DependencyUsage,
) -> Result<Vec<ClosureSig>, crate::compiler_error::CompilerFailure> {
    let mut out: Vec<ClosureSig> = Vec::new();
    for value_sym in values {
        let ValueKind::Function { params, ret, .. } = &value_sym.kind else {
            continue;
        };
        for p in params {
            walk_type(&p.ty, &mut out)?;
        }
        walk_type(ret, &mut out)?;
    }
    for shape in shapes {
        match shape {
            crate::Shape::Object { fields, index } => {
                if let Some(index) = index {
                    walk_type(&index.value, &mut out)?;
                }
                for f in fields.values() {
                    walk_type(&f.ty, &mut out)?;
                }
            }
            crate::Shape::Array(elem) => walk_type(elem, &mut out)?,
            crate::Shape::Tuple(elements) => {
                for e in elements {
                    walk_type(e, &mut out)?;
                }
            }
            crate::Shape::Union(members) => {
                for m in members {
                    walk_type(m, &mut out)?;
                }
            }
        }
    }
    for dependency_type in types {
        let ty_sym = dependency_type.symbol;
        if let TypeKind::Interface {
            dispatch,
            methods,
            properties,
            ..
        } = &ty_sym.kind
        {
            if *dispatch == Dispatch::VTable {
                for (name, sig) in methods {
                    if usage.is_interface_member_used(ty_sym, name) {
                        dispatched_member_sigs(
                            sig.params.iter().map(|p| &p.ty),
                            &sig.ret,
                            &mut out,
                        )?;
                    }
                }
                for (name, prop) in properties {
                    if usage.is_interface_member_used(ty_sym, name) {
                        walk_type(&prop.ty, &mut out)?;
                    }
                }
            } else {
                for (name, sig) in methods {
                    if usage.is_interface_member_used(ty_sym, name) {
                        walk_signature_types(sig.params.iter().map(|p| &p.ty), &sig.ret, &mut out)?;
                    }
                }
                for (name, prop) in properties {
                    if usage.is_interface_member_used(ty_sym, name) {
                        walk_type(&prop.ty, &mut out)?;
                    }
                }
            }
        } else if let TypeKind::Class {
            accessors,
            methods,
            constructor,
            fields,
            statics,
            static_fields,
            ..
        } = &ty_sym.kind
        {
            // An imported class's members carry their own closure sigs: each
            // method becomes a payload closure for interface-typed dispatch,
            // and any closure-typed parameter or return needs its rec-group
            // type to exist here even though the closure itself is built in
            // the producing package.
            for sig in methods.values() {
                if sig.generics.is_empty() {
                    dispatched_member_sigs(sig.params.iter().map(|p| &p.ty), &sig.ret, &mut out)?;
                }
            }
            for p in constructor {
                walk_type(&p.ty, &mut out)?;
            }
            for f in fields.values() {
                walk_type(&f.ty, &mut out)?;
            }
            for sig in statics.values() {
                walk_signature_types(sig.params.iter().map(|p| &p.ty), &sig.ret, &mut out)?;
            }
            for f in static_fields.values() {
                walk_type(&f.ty, &mut out)?;
            }
            accessor_sigs(accessors, &mut out)?;
        }
    }
    Ok(out)
}

/// Closure signatures for each local class's members — its own, and the ones it
/// inherits from a *dependency* class. An instance flowing through an
/// interface-typed receiver carries every method in its chain as a
/// `$closure_<sig>` value in the object-fields payload.
pub fn class_member_sigs(
    ta: &TypedAst,
    dependencies: &[&crate::PackageDeclaration],
) -> Result<Vec<ClosureSig>, crate::compiler_error::CompilerFailure> {
    let mut out: Vec<ClosureSig> = Vec::new();
    for decl in &ta.types {
        let crate::TypedTypeDecl::Class(class) = decl else {
            continue;
        };
        for f in &class.fields {
            walk_type(&f.ty, &mut out)?;
        }
        for p in class.effective_ctor_params() {
            walk_type(&p.ty, &mut out)?;
        }
        for method in &class.methods {
            if method.generics.is_empty() {
                dispatched_member_sigs(
                    method.params.iter().map(|p| &p.ty),
                    &method.return_type,
                    &mut out,
                )
                .map_err(|error| error.with_span(method.name.span))?;
            }
        }
        class_accessor_sigs(&class.accessors, &mut out)?;
        inherited_dependency_sigs(class.extends.clone(), dependencies, &mut out)?;
    }
    Ok(out)
}

/// Every function-typed position nested inside a member's own signature.
fn walk_signature_types<'a>(
    params: impl IntoIterator<Item = &'a Type>,
    ret: &Type,
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for ty in params {
        walk_type(ty, out)?;
    }
    walk_type(ret, out)?;

    Ok(())
}

/// The sig the member itself dispatches through, plus everything
/// [`walk_signature_types`] finds. A member carried in an object-fields payload
/// needs both.
fn dispatched_member_sigs<'a>(
    params: impl ExactSizeIterator<Item = &'a Type>,
    ret: &Type,
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    out.push(ClosureSig::of(params.len(), ret)?);
    walk_signature_types(params, ret, out)?;

    Ok(())
}

/// An accessor lowers to a getter `() -> R` or setter `(W) -> void` method,
/// which dispatches like any other member.
fn accessor_sigs(
    accessors: &[crate::AccessorSig],
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for acc in accessors {
        match acc {
            crate::AccessorSig::Getter { ret_ty, .. } => {
                dispatched_member_sigs(std::iter::empty(), ret_ty, out)?;
            }
            crate::AccessorSig::Setter { param, .. } => {
                dispatched_member_sigs(std::iter::once(&param.ty), &Type::Void, out)?;
            }
        }
    }
    Ok(())
}

/// The local-class mirror of [`accessor_sigs`] — same lowering, different enum.
fn class_accessor_sigs(
    accessors: &[crate::TypedClassAccessor],
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for acc in accessors {
        match acc {
            crate::TypedClassAccessor::Getter { ret_ty, .. } => {
                dispatched_member_sigs(std::iter::empty(), ret_ty, out)?;
            }
            crate::TypedClassAccessor::Setter { param, .. } => {
                dispatched_member_sigs(std::iter::once(&param.ty), &Type::Void, out)?;
            }
        }
    }
    Ok(())
}

/// Closure sigs for the methods a local class inherits from *dependency*
/// classes. Its instances carry those in their payload too, and neither
/// [`collect_from_dependencies`] nor the local walk above reaches them: a class
/// named solely in an `extends` clause is not in `dependency_types`.
fn inherited_dependency_sigs(
    mut parent: Option<crate::MangledName>,
    dependencies: &[&crate::PackageDeclaration],
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let mut seen: std::collections::BTreeSet<crate::MangledName> =
        std::collections::BTreeSet::new();
    while let Some(mangled) = parent {
        if !seen.insert(mangled.clone()) {
            return Ok(()); // malformed cyclic chain; the typechecker reports it
        }
        let Some(sym) = dependencies
            .iter()
            .flat_map(|d| d.types.values())
            .find(|t| t.mangled_name == mangled)
        else {
            return Ok(());
        };
        let crate::TypeKind::Class {
            methods,
            accessors,
            extends,
            ..
        } = &sym.kind
        else {
            return Ok(());
        };
        for sig in methods.values() {
            if sig.generics.is_empty() {
                dispatched_member_sigs(sig.params.iter().map(|p| &p.ty), &sig.ret, out)?;
            }
        }
        accessor_sigs(accessors, out)?;
        parent = extends.as_ref().map(|e| e.parent.clone());
    }
    Ok(())
}

/// Every function-typed position reachable from `ty`.
pub(crate) fn walk_type(
    ty: &Type,
    out: &mut Vec<ClosureSig>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    match ty {
        Type::Refined { ty, .. } | Type::Readonly(ty) => walk_type(ty, out)?,
        Type::Function { params, ret, .. } => {
            out.push(classify(ty)?);
            for p in params {
                walk_type(p, out)?;
            }
            walk_type(ret, out)?;
        }
        Type::Array(elem) => walk_type(elem, out)?,
        Type::Tuple(elements) => {
            for e in elements {
                walk_type(e, out)?;
            }
        }
        Type::Object { fields, index } => {
            if let Some(index) = index {
                walk_type(&index.value, out)?;
            }
            for f in fields.values() {
                walk_type(&f.ty, out)?;
            }
        }
        Type::InterfaceRef { args, .. } | Type::ClassRef { args, .. } => {
            for a in args {
                walk_type(a, out)?;
            }
        }
        // A recursion back-edge has no inline body to walk — its
        // function-typed positions were collected from the carrying
        // `Alias`'s body. Args only (like `InterfaceRef`), which also
        // stops the otherwise-infinite recursion.
        Type::AliasRef { args, .. } => {
            for a in args {
                walk_type(a, out)?;
            }
        }
        Type::Union(members) => {
            for m in members {
                walk_type(m, out)?;
            }
        }
        Type::Alias { ty: inner, .. } => walk_type(inner, out)?,
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::BigIntLiteral(_)
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::String
        | Type::StringLiteral(_)
        | Type::Uint8Array
        | Type::Void
        | Type::Never
        | Type::Null
        | Type::Undefined
        | Type::Error
        | Type::TypeVar(_)
        | Type::GenericParam { .. }
        | Type::Unknown
        | Type::NumberEnum { .. }
        | Type::StringEnum { .. } => {}
    };
    Ok(())
}

pub fn emit_func_and_struct_types<I>(
    signatures: I,
    types: &mut TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
) -> Result<(), crate::compiler_error::CompilerFailure>
where
    I: IntoIterator<Item = ClosureSig>,
{
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let signatures = signatures.into_iter().flat_map(|sig| {
        [
            sig,
            ClosureSig {
                is_void: !sig.is_void,
                ..sig
            },
        ]
    });
    let assigned = emit_arity_closures(signatures, types, intrinsics, next_type_idx)?;
    super::closure_coercions::emit_vtable_type(types, symbols, next_type_idx)?;
    for (sig, (fn_idx, struct_idx)) in assigned {
        symbols.record_closure_func_type(sig, fn_idx);
        symbols.record_closure_struct_type(sig, struct_idx);
        symbols.record_struct_supertype(struct_idx, closure_struct_supertype(intrinsics));
    }
    Ok(())
}

/// Structural canonicalization unifies prelude and consumer copies at instantiation time.
pub fn emit_arity_closures<I>(
    signatures: I,
    types: &mut TypeSection,
    intrinsics: IntrinsicTypeIndices,
    next_type_idx: &mut u32,
) -> Result<BTreeMap<ClosureSig, (u32, u32)>, crate::compiler_error::CompilerFailure>
where
    I: IntoIterator<Item = ClosureSig>,
{
    let mut seen: BTreeSet<ClosureSig> = BTreeSet::new();
    let unique: Vec<ClosureSig> = signatures
        .into_iter()
        .filter(|sig| seen.insert(*sig))
        .collect();

    ensure_index_capacity(*next_type_idx, unique.len().checked_mul(2), 0)?;
    let mut assigned: BTreeMap<ClosureSig, (u32, u32)> = BTreeMap::new();

    let mut fn_indices: Vec<u32> = Vec::with_capacity(unique.len());
    for sig in &unique {
        let func_ty = closure_func_type(*sig, intrinsics);
        types.ty().subtype(&SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Func(func_ty),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
        fn_indices.push(*next_type_idx);
        *next_type_idx += 1;
    }

    for (sig, &fn_idx) in unique.iter().zip(fn_indices.iter()) {
        let fields = vec![
            FieldType {
                element_type: StorageType::Val(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(intrinsics.vtable),
                })),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::Concrete(fn_idx),
                })),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::Ref(RefType {
                    nullable: false,
                    heap_type: HeapType::ANY,
                })),
                mutable: false,
            },
            FieldType {
                element_type: StorageType::Val(ValType::I64),
                mutable: true,
            },
        ];
        types.ty().subtype(&SubType {
            is_final: false,
            supertype_idx: Some(closure_struct_supertype(intrinsics)),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: fields.into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
        let struct_idx = *next_type_idx;
        *next_type_idx += 1;
        assigned.insert(*sig, (fn_idx, struct_idx));
    }

    Ok(assigned)
}

/// Must run after `box_types::emit` so env-field lookups for boxed captures resolve.
pub fn emit_env_types(
    metas: &[ClosureMeta],
    types: &mut TypeSection,
    symbols: &mut SymbolTable,
    next_type_idx: &mut u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    ensure_index_capacity(*next_type_idx, Some(metas.len()), 2)?;
    let string = symbols
        .string_type_idx()
        .ok_or_else(|| crate::codegen::internal_failure("string intrinsic"))?;
    types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::ANY,
            })),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(string),
            })),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::I64),
            mutable: true,
        },
    ]);
    symbols.call_metadata_type = Some(*next_type_idx);
    *next_type_idx += 1;
    let object = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics"))?
        .object;
    types.ty().struct_([
        FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::ANY,
            })),
            mutable: false,
        },
        FieldType {
            element_type: StorageType::Val(ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Concrete(object),
            })),
            mutable: false,
        },
    ]);
    symbols.this_environment_type = Some(*next_type_idx);
    *next_type_idx += 1;
    for meta in metas {
        let mut fields: Vec<FieldType> = meta
            .captured
            .iter()
            .map(|c| {
                Ok(FieldType {
                    element_type: StorageType::Val(env_field_type(c, symbols)?),
                    mutable: false,
                })
            })
            .collect::<Result<_, crate::compiler_error::CompilerFailure>>()?;
        if !meta.runtime_generics.is_empty() {
            fields.push(FieldType {
                element_type: StorageType::Val(super::runtime_descriptors::environment_type(
                    symbols,
                )?),
                mutable: false,
            });
        }
        if meta.self_name.is_some() {
            fields.push(FieldType {
                element_type: StorageType::Val(ValType::Ref(RefType {
                    nullable: true,
                    heap_type: HeapType::Concrete(
                        symbols
                            .intrinsic_type_indices()
                            .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared"))?
                            .object,
                    ),
                })),
                mutable: true,
            });
        }
        types.ty().subtype(&SubType {
            is_final: false,
            supertype_idx: None,
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: fields.into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
        symbols.record_env_type(meta.expr_id, *next_type_idx);
        *next_type_idx += 1;
    }
    Ok(())
}

/// Read by both the declaration and the recorded coercion edge, so the two
/// can't drift.
fn closure_struct_supertype(intrinsics: IntrinsicTypeIndices) -> u32 {
    intrinsics.closure
}

fn closure_func_type(sig: ClosureSig, intrinsics: IntrinsicTypeIndices) -> FuncType {
    let object_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(intrinsics.object),
    });
    let mut wasm_params = vec![ValType::Ref(RefType {
        nullable: false,
        heap_type: HeapType::ANY,
    })];
    for _ in 0..sig.arity {
        wasm_params.push(object_ref);
    }
    let results = vec![object_ref];
    FuncType::new(wasm_params, results)
}

fn env_field_type(
    c: &CapturedVar,
    symbols: &SymbolTable,
) -> Result<ValType, crate::compiler_error::CompilerFailure> {
    Ok(if c.boxed {
        let box_idx = symbols.box_type_idx(&c.ty)?.ok_or_else(|| {
            crate::codegen::internal_failure("box type registered for every boxed capture")
        })?;
        ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(box_idx),
        })
    } else {
        symbols.value_type(&c.ty)?
    })
}

#[derive(Clone, Copy, Debug)]
pub struct ClosureMethods {
    pub to_string_func: u32,
    pub to_json_func: u32,
    pub equals_func: u32,
    pub hash_func: u32,
    /// Filled in by [`emit_vtable_global`].
    pub vtable_global_idx: u32,
}

pub fn allocate_methods(
    next_func_idx: &mut u32,
) -> Result<ClosureMethods, crate::compiler_error::CompilerFailure> {
    ensure_index_capacity(*next_func_idx, Some(4), 0)?;
    let to_string_func = take(next_func_idx);
    let to_json_func = take(next_func_idx);
    let equals_func = take(next_func_idx);
    let hash_func = take(next_func_idx);
    Ok(ClosureMethods {
        to_string_func,
        to_json_func,
        equals_func,
        hash_func,
        vtable_global_idx: 0,
    })
}

fn take(counter: &mut u32) -> u32 {
    let v = *counter;
    *counter += 1;
    v
}

/// Must be called after existing function-section entries so pre-allocated indices match.
pub fn emit_method_function_entries(
    functions: &mut wasm_encoder::FunctionSection,
    intrinsics: crate::codegen::intrinsics::IntrinsicTypeIndices,
) {
    functions.function(intrinsics.to_string_fn);
    functions.function(intrinsics.to_json_fn);
    functions.function(intrinsics.equals_fn);
    functions.function(intrinsics.hash_fn);
}

pub fn emit_vtable_global(
    globals: &mut wasm_encoder::GlobalSection,
    methods: &mut ClosureMethods,
    symbols: &mut SymbolTable,
    next_global_idx: &mut u32,
    intrinsics: crate::codegen::intrinsics::IntrinsicTypeIndices,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    ensure_index_capacity(*next_global_idx, Some(1), 0)?;
    let init = wasm_encoder::ConstExpr::extended([
        wasm_encoder::Instruction::RefFunc(methods.to_string_func),
        wasm_encoder::Instruction::RefFunc(methods.to_json_func),
        wasm_encoder::Instruction::RefFunc(methods.equals_func),
        wasm_encoder::Instruction::RefFunc(methods.hash_func),
        wasm_encoder::Instruction::StructNew(intrinsics.vtable),
    ]);
    globals.global(
        wasm_encoder::GlobalType {
            val_type: ValType::Ref(RefType {
                nullable: false,
                heap_type: HeapType::Concrete(intrinsics.vtable),
            }),
            mutable: false,
            shared: false,
        },
        &init,
    );
    let idx = take(next_global_idx);
    methods.vtable_global_idx = idx;
    symbols.set_closure_vtable_global(idx);

    Ok(())
}

pub fn emit_method_bodies(
    code: &mut wasm_encoder::CodeSection,
    _methods: ClosureMethods,
    symbols: &SymbolTable,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let intrinsics = symbols
        .intrinsic_type_indices()
        .ok_or_else(|| crate::codegen::internal_failure("intrinsics declared by codegen entry"))?;
    let string_vtable_global_idx = symbols
        .prelude_global_idx("string_vtable")
        .ok_or_else(|| crate::codegen::internal_failure("string_vtable imported from prelude"))?;

    let mut to_string = wasm_encoder::Function::new(std::iter::empty());
    crate::codegen::intrinsics::push_string_literal(
        &mut to_string,
        intrinsics,
        string_vtable_global_idx,
        "[object Function]",
    )?;
    to_string.instruction(&wasm_encoder::Instruction::End);
    code.function(&to_string);

    let mut to_json = wasm_encoder::Function::new(std::iter::empty());
    crate::codegen::intrinsics::push_string_literal(
        &mut to_json,
        intrinsics,
        string_vtable_global_idx,
        "null",
    )?;
    to_json.instruction(&wasm_encoder::Instruction::End);
    code.function(&to_json);

    code.function(&super::closure_coercions::emit_equals(symbols)?);

    let mut hash = wasm_encoder::Function::new(std::iter::empty());
    let host_vtable = symbols
        .prelude_global_idx("closure_vtable")
        .ok_or_else(|| crate::codegen::internal_failure("closure_vtable imported from prelude"))?;
    hash.instruction(&wasm_encoder::Instruction::LocalGet(0));
    hash.instruction(&wasm_encoder::Instruction::GlobalGet(host_vtable));
    hash.instruction(&wasm_encoder::Instruction::StructGet {
        struct_type_index: intrinsics.vtable,
        field_index: 3,
    });
    hash.instruction(&wasm_encoder::Instruction::CallRef(intrinsics.hash_fn));
    hash.instruction(&wasm_encoder::Instruction::End);
    code.function(&hash);

    Ok(())
}

/// `ref.func` in a const-expr requires declarative element coverage; these are the target indices.
pub fn declared_funcs(methods: ClosureMethods) -> Vec<u32> {
    vec![
        methods.to_string_func,
        methods.to_json_func,
        methods.equals_func,
        methods.hash_func,
    ]
}

/// Validate the whole index range before emitting any of its declarations.
pub(super) fn ensure_index_capacity(
    next: u32,
    count: Option<usize>,
    fixed: u32,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let end = count
        .and_then(|count| u32::try_from(count).ok())
        .and_then(|count| count.checked_add(fixed))
        .and_then(|count| next.checked_add(count));
    if end.is_none() {
        return Err(crate::compiler_error::CompilerFailure::Limit {
            stage: crate::compiler_error::CompilerStage::Codegen,
            span: None,
            message: "closure declarations exceed the Wasm index space".into(),
            help: vec!["reduce the number of declarations".into()],
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_collection_checks_nested_callable_types() {
        let signature = Type::Function {
            params: vec![Type::Number; 256],
            ret: Box::new(Type::Void),
            predicate: None,
            optional: 0,
            has_rest: false,
        };
        let shapes = [Shape::Array(Box::new(Type::Array(Box::new(signature))))];
        let error = collect_from_dependencies([], shapes.iter(), [], &DependencyUsage::empty())
            .unwrap_err();
        assert!(matches!(
            error,
            crate::compiler_error::CompilerFailure::Limit {
                stage: crate::compiler_error::CompilerStage::Codegen,
                span: None,
                ..
            }
        ));
        assert!(error.to_string().contains("256"));
    }
}
