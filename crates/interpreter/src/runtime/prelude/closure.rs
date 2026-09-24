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

    /// Invoke a `(_) => void` closure on one (already boxed) argument. The
    /// funcref has no result slot, so there is nothing to return.
    pub(crate) async fn call_void(
        &self,
        caller: &mut Caller<'_, StoreData>,
        arg: Val,
    ) -> wasmtime::Result<()> {
        self.invoke(caller, &[arg], &mut []).await
    }

    /// Invoke a void closure on already-boxed args (`Map#forEach`'s
    /// `(value, key) => void`). The funcref has no result slot.
    pub(crate) async fn call_void_args(
        &self,
        caller: &mut Caller<'_, StoreData>,
        args: &[Val],
    ) -> wasmtime::Result<()> {
        self.invoke(caller, args, &mut []).await
    }

    /// Invoke a value-returning closure and return its boxed result verbatim.
    /// `null` is a legal result (e.g. `map` over `T | null`). Arity is the
    /// caller's: one arg for `map`, two for `reduce`. Consumed by the Array
    /// higher-order port.
    #[allow(dead_code)]
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

    /// Invoke a comparator closure `(_, _) => number` and unbox its
    /// `$boxed_number` result to `f64`. Consumed by `Array#sort`.
    #[allow(dead_code)]
    pub(crate) async fn call_number(
        &self,
        caller: &mut Caller<'_, StoreData>,
        a: Val,
        b: Val,
    ) -> wasmtime::Result<f64> {
        let mut out = [Val::null_any_ref()];
        self.invoke(caller, &[a, b], &mut out).await?;
        super::value::to_number(caller, &out[0]).await
    }

    /// Invoke a predicate closure `(_) => boolean` and unbox its
    /// `$boxed_boolean` result to `bool`. Consumed by `Array#filter`/`some`/`every`.
    #[allow(dead_code)]
    pub(crate) async fn call_predicate(
        &self,
        caller: &mut Caller<'_, StoreData>,
        arg: Val,
    ) -> wasmtime::Result<bool> {
        let mut out = [Val::null_any_ref()];
        self.invoke(caller, &[arg], &mut out).await?;
        super::value::truthy(caller, &out[0])
    }
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
