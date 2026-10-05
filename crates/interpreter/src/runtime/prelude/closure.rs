//! A guest closure pulled from a `$Closure`, shared by the ported higher-order
//! methods. Calling one re-enters the guest, so any host fn that does is async.

use wasmtime::{Caller, Func, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;

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
    /// arguments the function ignores or packs into its rest parameter, but it
    /// cannot omit a required parameter.
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

    /// Whether `supplied` arguments can call this function through its
    /// argument metadata: every parameter past them has a default or is the
    /// rest parameter, and any beyond its parameters are dropped, as
    /// JavaScript drops them. A function without argument metadata has neither
    /// defaults nor a rest parameter, so only its arity matters.
    pub(crate) fn accepts_arguments(
        &self,
        caller: &mut Caller<'_, StoreData>,
        supplied: usize,
    ) -> wasmtime::Result<bool> {
        let declared = self.declared_arity(caller)?;
        let Some(params) = super::arguments::metadata(caller, &self.env)? else {
            return Ok(declared <= supplied);
        };
        Ok(params
            .iter()
            .skip(supplied)
            .all(|(default, rest)| *rest || default.is_some()))
    }

    /// Whether this function declares `count` parameters, the last of them a
    /// rest parameter: the packed calling convention of a rest function type.
    pub(crate) fn ends_in_rest(
        &self,
        caller: &mut Caller<'_, StoreData>,
        count: usize,
    ) -> wasmtime::Result<bool> {
        Ok(
            super::arguments::metadata(caller, &self.env)?.is_some_and(|params| {
                params.len() == count && params.last().is_some_and(|(_, rest)| *rest)
            }),
        )
    }

    /// Re-enter the guest: call the funcref with the uniform ABI (env as the
    /// leading argument), filling `out` with the results in place.
    async fn invoke(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
        out: &mut [Val],
    ) -> wasmtime::Result<()> {
        self.declared_arity(caller)?;
        let count = args.len().checked_add(1).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("Closure argument count overflow")
        })?;
        let _arguments = reserve_arguments(caller, count)?;
        let mut call_args = Vec::new();
        call_args
            .try_reserve_exact(count)
            .map_err(crate::runtime::host::fatal_host_error)?;
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
        let declared = self.declared_arity(caller)?;
        Ok(match super::arguments::metadata(caller, &self.env)? {
            Some(params) if params.iter().any(|(_, rest)| *rest) => usize::MAX,
            Some(params) => params.len(),
            None => declared,
        })
    }

    /// How many results the function returns: 0 for the void convention, 1
    /// for a value.
    pub(crate) fn result_count(&self, caller: &mut Caller<'_, StoreData>) -> usize {
        self.func.ty(&*caller).results().len()
    }

    /// The parameters the function declares: its Wasm parameters but the
    /// leading environment.
    fn declared_arity(&self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<usize> {
        self.func
            .ty(&*caller)
            .params()
            .len()
            .checked_sub(1)
            .ok_or_else(|| {
                crate::runtime::host::fatal_host_error(
                    "Closure function is missing its environment parameter",
                )
            })
    }

    /// Call with as many of `args` as the function declares, its defaults
    /// filling any it omits, whatever its arity and return convention: the
    /// host's way to call a function from an erased slot, as JavaScript would.
    pub(crate) async fn call_dynamic(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
    ) -> wasmtime::Result<Val> {
        let declared = self.declared_arity(caller)?;
        // The host's overhead of one callback; the callee pays its own fuel.
        fuel::charge_call(&mut *caller)?;
        let signature = self.func.ty(&*caller);
        let (inputs, _arguments) =
            if let Some(params) = super::arguments::metadata(caller, &self.env)? {
                let reservation = reserve_arguments(caller, params.len())?;
                (super::arguments::bind(caller, &params, args)?, reservation)
            } else {
                let reservation = reserve_arguments(caller, declared)?;
                let mut inputs = Vec::new();
                inputs
                    .try_reserve_exact(declared)
                    .map_err(crate::runtime::host::fatal_host_error)?;
                inputs.extend(args.iter().take(declared).copied());
                inputs.resize(declared, Val::null_any_ref());
                (inputs, reservation)
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

fn reserve_arguments(
    caller: &Caller<'_, StoreData>,
    count: usize,
) -> wasmtime::Result<crate::runtime::limits::HostBytes> {
    let bytes = u64::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(std::mem::size_of::<Val>() as u64))
        .ok_or_else(|| {
            crate::runtime::host::fatal_host_error("Closure argument storage size overflow")
        })?;
    Ok(crate::runtime::limits::HostBytes::new(
        &caller.data().tenant_limits,
        bytes,
    )?)
}

/// The function an adapter wraps, following adapters of adapters; any other
/// value is returned as is. An adapter's vtable has an extra field holding
/// the original (see `closure_coercions::ADAPTER_ORIGINAL_FIELD`); ordinary
/// closure vtables don't. An adapter declares its target's arity, not what the
/// original accepts, so arity checks and dynamic calls look through it.
pub(crate) fn original(caller: &mut Caller<'_, StoreData>, val: Val) -> wasmtime::Result<Val> {
    let mut current = val;
    while let Some(inner) = adapter_target(caller, &current)? {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
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

pub(super) fn receiver_type(
    caller: &mut Caller<'_, StoreData>,
) -> wasmtime::Result<wasmtime::StructType> {
    if let Some(ty) = &caller.data().closure_receiver_type {
        return Ok(ty.clone());
    }
    let object = crate::runtime::intrinsic_types::intrinsic_types(&mut *caller)?
        .object
        .clone();
    let ty = build_receiver_type(caller.engine(), object)?;
    caller.data_mut().closure_receiver_type = Some(ty.clone());
    Ok(ty)
}

fn build_receiver_type(
    engine: &wasmtime::Engine,
    object: wasmtime::StructType,
) -> wasmtime::Result<wasmtime::StructType> {
    use wasmtime::{FieldType, Finality, HeapType, Mutability, RefType, StorageType, ValType};
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
                    HeapType::ConcreteStruct(object),
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
    let ty = receiver_type(caller)?;
    if !object.matches_ty(&*caller, &ty)? {
        return Ok(env);
    }
    let inner = object.field(&mut *caller, 0)?;
    let pre = wasmtime::StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        wasmtime::StructRef::new(&mut *caller, &pre, &[inner, receiver])?.to_anyref(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs};
    use wasmtime::{FuncType, ValType};

    #[tokio::test]
    async fn missing_environment_is_fatal_and_store_remains_usable() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let invalid = Func::new(&mut store, FuncType::new(&engine, [], []), |_, _, _| Ok(()));
        let healthy = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::ANYREF], []),
            |_, _, _| Ok(()),
        );
        let callback = Func::new_async(
            &mut store,
            FuncType::new(&engine, [], []),
            move |mut caller, _, _| {
                Box::new(async move {
                    let invalid = Closure {
                        func: invalid,
                        env: Val::null_any_ref(),
                    };
                    let error = invalid.accepts_arguments(&mut caller, 0).unwrap_err();
                    assert!(
                        error
                            .downcast_ref::<crate::runtime::host::FatalHostError>()
                            .is_some()
                    );
                    assert!(invalid.arguments_read(&mut caller).is_err());
                    assert!(invalid.call_dynamic(&mut caller, &[]).await.is_err());
                    assert!(invalid.call_void_args(&mut caller, &[]).await.is_err());
                    let healthy = Closure {
                        func: healthy,
                        env: Val::null_any_ref(),
                    };
                    assert!(healthy.accepts_arguments(&mut caller, 0)?);
                    assert!(matches!(
                        healthy.call_dynamic(&mut caller, &[]).await?,
                        Val::AnyRef(None)
                    ));
                    Ok(())
                })
            },
        );
        store.set_fuel(10000).unwrap();
        callback.call_async(&mut store, &[], &mut []).await.unwrap();
        assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
    }
    #[tokio::test]
    async fn metadata_cannot_bypass_environment_validation_or_argument_admission() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let mut linker = wasmtime::Linker::new(&engine);
        crate::runtime::install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let invalid = Func::new(&mut store, FuncType::new(&engine, [], []), |_, _, _| Ok(()));
        let healthy = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::ANYREF, ValType::ANYREF], []),
            |_, _, _| Ok(()),
        );
        let callback = Func::new_async(
            &mut store,
            FuncType::new(&engine, [], []),
            move |mut caller, _, _| {
                Box::new(async move {
                    let ty = super::super::arguments::metadata_type(&mut caller)?;
                    let encoded = crate::runtime::host::write_submilli_string_struct(
                        &mut caller,
                        "[[null,false]]",
                    )?;
                    let pre = wasmtime::StructRefPre::new(&mut caller, ty);
                    let wrapper = wasmtime::StructRef::new(
                        &mut caller,
                        &pre,
                        &[
                            Val::null_any_ref(),
                            Val::AnyRef(Some(encoded.to_anyref())),
                            Val::I64(0),
                        ],
                    )?;
                    let env = Val::AnyRef(Some(wrapper.to_anyref()));
                    let before = caller.data().tenant_limits.host_attached_bytes();
                    let invalid = Closure { func: invalid, env };
                    for error in [
                        invalid.accepts_arguments(&mut caller, 0).unwrap_err(),
                        invalid.arguments_read(&mut caller).unwrap_err(),
                        invalid
                            .call_with_arguments(&mut caller, Val::null_any_ref(), &[])
                            .await
                            .unwrap_err(),
                        invalid.call_dynamic(&mut caller, &[]).await.unwrap_err(),
                    ] {
                        assert!(
                            error
                                .downcast_ref::<crate::runtime::host::FatalHostError>()
                                .is_some()
                        );
                    }
                    assert!(matches!(wrapper.field(&mut caller, 2)?, Val::I64(0)));
                    assert_eq!(caller.data().tenant_limits.host_attached_bytes(), before);
                    let healthy = Closure { func: healthy, env };
                    assert!(healthy.accepts_arguments(&mut caller, 1)?);
                    healthy
                        .call_dynamic(&mut caller, &[Val::null_any_ref()])
                        .await?;
                    let retained = caller.data().tenant_limits.host_attached_bytes();
                    let old_cap = caller.data().tenant_limits.max_total_bytes;
                    caller.data_mut().tenant_limits.max_total_bytes =
                        caller.data().tenant_limits.observed_bytes();
                    assert!(
                        healthy
                            .call_dynamic(&mut caller, &[Val::null_any_ref()])
                            .await
                            .is_err()
                    );
                    assert_eq!(caller.data().tenant_limits.host_attached_bytes(), retained);
                    caller.data_mut().tenant_limits.max_total_bytes = old_cap;
                    healthy
                        .call_dynamic(&mut caller, &[Val::null_any_ref()])
                        .await?;
                    assert_eq!(caller.data().tenant_limits.host_attached_bytes(), retained);
                    Ok(())
                })
            },
        );
        store.set_fuel(100000).unwrap();
        callback.call_async(&mut store, &[], &mut []).await.unwrap();
    }
}
