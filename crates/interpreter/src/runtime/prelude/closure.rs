//! A guest closure pulled from a `$Closure`, shared by the ported higher-order
//! methods. Calling one re-enters the guest, so any host fn that does is async.

use wasmtime::{Caller, Func, Val};

use crate::runtime::StoreData;

/// A guest closure: its function plus the captured env, which the uniform
/// closure ABI passes as the leading call argument.
pub struct Closure {
    func: Func,
    env: Val,
}

impl Closure {
    pub(crate) async fn call_with_receiver(
        &self,
        caller: &mut Caller<'_, StoreData>,
        receiver: Val,
        args: &[Val],
    ) -> wasmtime::Result<Val> {
        let bound = Self {
            func: self.func,
            env: bind_receiver(caller, self.env, receiver)?,
        };
        bound.call_dynamic(caller, args).await
    }

    /// An erased structural signature may omit trailing defaults or pass
    /// arguments the function ignores, but it cannot change the packed rest ABI
    /// or omit a required parameter.
    pub(crate) async fn call_with_arguments(
        &self,
        caller: &mut Caller<'_, StoreData>,
        receiver: Val,
        args: &[Val],
    ) -> wasmtime::Result<Val> {
        if !self.accepts_arguments(caller, args.len())? {
            return Err(crate::runtime::host::type_error(
                "Function has incompatible arity",
            ));
        }
        self.call_with_receiver(caller, receiver, args).await
    }

    /// Whether `supplied` arguments can call this function: every parameter
    /// past them has a default, and any beyond its parameters are dropped, as
    /// JavaScript drops them. A function without argument metadata has neither
    /// defaults nor a rest parameter, so only its arity matters.
    pub(crate) fn accepts_arguments(
        &self,
        caller: &mut Caller<'_, StoreData>,
        supplied: usize,
    ) -> wasmtime::Result<bool> {
        let Some(params) = super::arguments::metadata(caller, &self.env)? else {
            return Ok(self.declared_arity(caller) <= supplied);
        };
        Ok(!params.iter().any(|(_, rest)| *rest)
            && params
                .iter()
                .skip(supplied)
                .all(|(default, _)| default.is_some()))
    }

    /// Re-enter the guest: call the funcref with the uniform ABI (env as the
    /// leading argument), filling `out` with the results in place.
    async fn invoke(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
        out: &mut [Val],
    ) -> wasmtime::Result<()> {
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(self.env);
        call_args.extend_from_slice(args);
        self.func.call_async(&mut *caller, &call_args, out).await
    }

    /// Invoke a void closure on already-boxed args. The funcref has no result
    /// slot, so there is nothing to return.
    pub(crate) async fn call_void_args(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
    ) -> wasmtime::Result<()> {
        self.invoke(caller, args, &mut []).await
    }

    /// Invoke a value-returning closure and return its boxed result verbatim.
    /// `null` is a legal result. Arity is the caller's.
    pub(crate) async fn call(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
    ) -> wasmtime::Result<Val> {
        let mut out = [Val::null_any_ref()];
        self.invoke(caller, args, &mut out).await?;
        let [result] = out;
        Ok(result)
    }

    /// How many leading arguments this function reads: its parameters, or
    /// every argument when the last is a rest parameter. A callback passed
    /// through an erased slot is called with no more than this, so an argument
    /// it ignores need not be built.
    pub(crate) fn arguments_read(
        &self,
        caller: &mut Caller<'_, StoreData>,
    ) -> wasmtime::Result<usize> {
        Ok(match super::arguments::metadata(caller, &self.env)? {
            Some(params) if params.iter().any(|(_, rest)| *rest) => usize::MAX,
            Some(params) => params.len(),
            None => self.declared_arity(caller),
        })
    }

    /// How many results the function returns: 0 for the void convention, 1
    /// for a value.
    pub(crate) fn result_count(&self, caller: &mut Caller<'_, StoreData>) -> usize {
        self.func.ty(&*caller).results().len()
    }

    /// The parameters the function declares: its Wasm parameters but the
    /// leading environment.
    fn declared_arity(&self, caller: &mut Caller<'_, StoreData>) -> usize {
        self.func.ty(&*caller).params().len() - 1
    }

    /// Call with as many of `args` as the function declares, its defaults
    /// filling any it omits, whatever its arity and return convention: the
    /// host's way to call a function from an erased slot, as JavaScript would.
    pub(crate) async fn call_dynamic(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
    ) -> wasmtime::Result<Val> {
        let signature = self.func.ty(&*caller);
        let inputs = if let Some(params) = super::arguments::metadata(caller, &self.env)? {
            super::arguments::bind(caller, &params, args)?
        } else {
            let count = signature.params().len().saturating_sub(1);
            let mut inputs = args.iter().take(count).copied().collect::<Vec<_>>();
            inputs.resize(count, Val::null_any_ref());
            inputs
        };
        if signature.results().len() == 0 {
            self.invoke(caller, &inputs, &mut []).await?;
            return Ok(Val::null_any_ref());
        }
        self.call(caller, &inputs).await
    }

    /// Call a comparator `(a, b) => number`, which may declare fewer than two
    /// parameters, and convert its result to `f64`. Consumed by `sort`.
    pub(crate) async fn compare(
        &self,
        caller: &mut Caller<'_, StoreData>,
        a: Val,
        b: Val,
    ) -> wasmtime::Result<f64> {
        let result = self.call_dynamic(caller, &[a, b]).await?;
        super::value::to_number(caller, &result).await
    }
}

/// The function an adapter wraps, following adapters of adapters; any other
/// value is returned as is. An adapter's vtable has an extra field holding
/// the original (see `closure_coercions::ADAPTER_ORIGINAL_FIELD`); ordinary
/// closure vtables don't. An adapter declares its target's arity, not what the
/// original accepts, so arity checks and dynamic calls look through it.
pub(crate) fn original(caller: &mut Caller<'_, StoreData>, val: Val) -> wasmtime::Result<Val> {
    let mut current = val;
    while let Some(inner) = adapter_target(caller, &current)? {
        current = inner;
    }
    Ok(current)
}

/// The function `val` wraps, if it is an adapter.
fn adapter_target(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<Option<Val>> {
    let Val::AnyRef(Some(any)) = val else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let Val::AnyRef(Some(vtable)) = st.field(&mut *caller, 0)? else {
        return Ok(None);
    };
    let Some(vtable) = vtable.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let field = crate::codegen::ADAPTER_ORIGINAL_FIELD as usize;
    if vtable.ty(&mut *caller)?.fields().len() <= field {
        return Ok(None);
    }
    Ok(match vtable.field(&mut *caller, field)? {
        wrapped @ Val::AnyRef(Some(_)) => Some(wrapped),
        _ => None,
    })
}

/// Read a callback the host calls with the arguments JavaScript passes: the
/// function an adapter wraps, not the adapter, which would drop those past its
/// declared arity.
pub(crate) fn read_callback(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Closure> {
    let function = original(caller, *val)?;
    read(caller, &function, name)
}

/// Read a `$Closure` struct — `(struct vtable funcref env)` — into a [`Closure`].
pub(crate) fn read(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Closure> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(wasmtime::Error::msg(format!(
            "{name} expects a closure, got {val:?}"
        )));
    };
    let st = any
        .as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg(format!("{name}: expected a $Closure struct")))?;
    let func = match st.field(&mut *caller, 1)? {
        Val::FuncRef(Some(func)) => func,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "{name}: closure funcref is {other:?}"
            )));
        }
    };
    let env = st.field(&mut *caller, 2)?;
    Ok(Closure { func, env })
}

pub(super) fn receiver_type(engine: &wasmtime::Engine) -> wasmtime::Result<wasmtime::StructType> {
    use wasmtime::{FieldType, Finality, HeapType, Mutability, RefType, StorageType, ValType};
    let intr = crate::runtime::intrinsic_types::build_intrinsic_types(engine)?;
    crate::runtime::gc_singleton::singleton_struct(
        engine,
        Finality::Final,
        None,
        vec![
            FieldType::new(
                Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(false, HeapType::Any))),
            ),
            FieldType::new(
                Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(
                    true,
                    HeapType::ConcreteStruct(intr.object),
                ))),
            ),
        ],
    )
}

fn bind_receiver(
    caller: &mut Caller<'_, StoreData>,
    env: Val,
    receiver: Val,
) -> wasmtime::Result<Val> {
    let Val::AnyRef(Some(reference)) = env else {
        return Ok(env);
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(env);
    };
    let ty = receiver_type(caller.engine())?;
    if !object.matches_ty(&*caller, &ty)? {
        return Ok(env);
    }
    let inner = object.field(&mut *caller, 0)?;
    let pre = wasmtime::StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        wasmtime::StructRef::new(&mut *caller, &pre, &[inner, receiver])?.to_anyref(),
    )))
}
