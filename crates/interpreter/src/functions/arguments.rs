//! JSON construction for host-only function invocation. Guest calls bypass it.

use super::*;
use crate::runtime::json::JsonChild;

/// Match the prelude identity, so a similarly named user interface cannot
/// acquire constructor behavior. Type aliases peel to that same identity.
pub(crate) fn temporal_constructor(ty: &Type) -> Option<&'static str> {
    let Type::InterfaceRef { mangled, .. } = ty.peel() else {
        return None;
    };
    [
        "Instant",
        "Duration",
        "ZonedDateTime",
        "PlainDate",
        "PlainTime",
        "PlainDateTime",
        "PlainYearMonth",
        "PlainMonthDay",
    ]
    .into_iter()
    .find(|name| *mangled == crate::mangle::extend(&crate::mangle::prelude("Temporal"), name))
}

pub(super) fn allocate_argument(
    store: &mut Store<StoreData>,
    linker: &Linker<StoreData>,
    allocator: &JsonUnknownAllocator,
    param: &RuntimeParamPlan,
    value: &JsonValue,
) -> wasmtime::Result<Val> {
    match &param.cast {
        RuntimeParamCast::Temporal { shape, aliases } => ArgumentAllocator {
            linker,
            allocator,
            aliases,
        }
        .allocate(store, value, &[shape], 0),
        _ => allocator.allocate(store, value),
    }
}

struct ArgumentAllocator<'a> {
    linker: &'a Linker<StoreData>,
    allocator: &'a JsonUnknownAllocator,
    aliases: &'a BTreeMap<Type, Type>,
}

impl ArgumentAllocator<'_> {
    fn allocate(
        &self,
        store: &mut Store<StoreData>,
        value: &JsonValue,
        shapes: &[&Type],
        depth: u32,
    ) -> wasmtime::Result<Val> {
        check_depth(depth)?;
        // Resolve every union constraint before committing to a constructor:
        // overlapping indexes may admit only some of a broad union's members.
        for (index, shape) in shapes.iter().enumerate() {
            if let Type::Union(members) = self.resolve(store, shape)?.peel() {
                return self.allocate_union(store, value, shapes, (index, members), depth + 1);
            }
        }
        let mut constructor = None;
        for shape in shapes {
            if let Some(name) = temporal_constructor(self.resolve(store, shape)?) {
                if constructor.is_some_and(|previous| previous != name) {
                    return Err(host::type_error(
                        "incompatible Temporal argument constraints",
                    ));
                }
                constructor = Some(name);
            }
        }
        if let Some(name) = constructor {
            for shape in shapes {
                let shape = self.resolve(store, shape)?;
                if temporal_constructor(shape) != Some(name)
                    && !matches!(shape.peel(), Type::Unknown | Type::NonPrimitive)
                    && !shape.is_empty_object()
                {
                    return Err(host::type_error(
                        "incompatible Temporal argument constraints",
                    ));
                }
            }
            let raw = self.allocator.allocate(store, value)?;
            return construct_temporal_value(store, self.linker, name, raw);
        }
        self.allocator
            .allocate_with_children(store, value, |store, child, value| {
                let (children, _guard) = self.child_shapes(store, shapes, &child)?;
                if children.is_empty() {
                    self.allocator.allocate(store, value)
                } else {
                    self.allocate(store, value, &children, depth + 1)
                }
            })
    }

    fn child_shapes<'a>(
        &'a self,
        store: &mut Store<StoreData>,
        shapes: &[&'a Type],
        child: &JsonChild<'_>,
    ) -> wasmtime::Result<(Vec<&'a Type>, HostBytes)> {
        let mut capacity = 0usize;
        for shape in shapes {
            let count = match self.resolve(store, shape)?.peel() {
                Type::Object {
                    index,
                    more_indexes,
                    ..
                } => 1usize
                    .saturating_add(usize::from(index.is_some()))
                    .saturating_add(more_indexes.len()),
                _ => 1,
            };
            capacity = capacity.saturating_add(count);
        }
        let (mut children, _guard) = shape_list(store, capacity)?;
        for shape in shapes {
            match (self.resolve(store, shape)?.peel(), child) {
                (Type::Array(element), JsonChild::Element(_)) => {
                    children.push(element.as_ref());
                }
                (Type::Tuple(elements), JsonChild::Element(index)) => {
                    if let Some(element) = elements.get(*index) {
                        children.push(element);
                    }
                }
                (
                    Type::Object {
                        fields,
                        index,
                        more_indexes,
                    },
                    JsonChild::Field(name),
                ) => {
                    for (key, field) in fields {
                        if same_name(store, key, name)? {
                            children.push(&field.ty);
                            break;
                        }
                    }
                    for signature in crate::stored_index_signatures(index, more_indexes) {
                        fuel::charge(&mut *store, fuel::ELEM, 1)?;
                        if matches_key(store, &signature.key, name)? {
                            children.push(&signature.value);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok((children, _guard))
    }

    fn resolve<'a>(
        &'a self,
        store: &mut Store<StoreData>,
        mut shape: &'a Type,
    ) -> wasmtime::Result<&'a Type> {
        // Alias edges consume fuel even without descending into a JSON child;
        // malformed cyclic metadata cannot create an unbounded native walk.
        loop {
            fuel::charge(&mut *store, fuel::ELEM, 1)?;
            if !matches!(shape.peel(), Type::AliasRef { .. }) {
                return Ok(shape);
            }
            let Some(body) = self.aliases.get(shape.peel()) else {
                return Err(host::fatal_host_error(
                    "function argument alias body is missing",
                ));
            };
            shape = body;
        }
    }

    fn allocate_union(
        &self,
        store: &mut Store<StoreData>,
        value: &JsonValue,
        shapes: &[&Type],
        (index, members): (usize, &[Type]),
        depth: u32,
    ) -> wasmtime::Result<Val> {
        let mut last_error = None;
        // Preserve a plain-data representation when it already fits a union.
        // Constructor failures only discard an alternative, never a budget,
        // cancellation, or internal failure. Constructors have no effects.
        for temporal in [false, true] {
            for member in members {
                if contains_temporal(store, member)? != temporal
                    || !self.json_matches(store, value, member, depth)?
                {
                    continue;
                }
                let (mut alternative, _guard) = shape_list(store, shapes.len())?;
                fuel::charge(
                    &mut *store,
                    fuel::COPY,
                    std::mem::size_of_val(shapes) as u64,
                )?;
                alternative.extend_from_slice(shapes);
                let slot = alternative
                    .get_mut(index)
                    .ok_or_else(|| host::fatal_host_error("union constraint slot is missing"))?;
                *slot = member;
                match self.allocate(store, value, &alternative, depth) {
                    Ok(value) => return Ok(value),
                    Err(error) => {
                        let error = exec::uncaught_error(store, error);
                        if !error.is::<ThrownError>() && !error.is::<host::TypeError>() {
                            return Err(error);
                        }
                        last_error = Some(error);
                    }
                }
            }
        }
        match last_error {
            Some(error) => Err(error),
            None => Err(host::type_error(
                "argument does not match any JSON input alternative",
            )),
        }
    }

    fn admits_missing(
        &self,
        store: &mut Store<StoreData>,
        ty: &Type,
        depth: u32,
    ) -> wasmtime::Result<bool> {
        check_depth(depth)?;
        match self.resolve(store, ty)?.peel() {
            Type::Undefined | Type::Void => Ok(true),
            Type::Union(members) => {
                for member in members {
                    if self.admits_missing(store, member, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    /// Choose union alternatives by their JSON input shape. The generated
    /// argument export checks the constructed value against the full target.
    fn json_matches(
        &self,
        store: &mut Store<StoreData>,
        value: &JsonValue,
        ty: &Type,
        depth: u32,
    ) -> wasmtime::Result<bool> {
        check_depth(depth)?;
        let ty = self.resolve(store, ty)?;
        if temporal_constructor(ty).is_some() {
            return Ok(matches!(value, JsonValue::String(_) | JsonValue::Object(_)));
        }
        if ty.is_empty_object() {
            return Ok(!matches!(value, JsonValue::Null));
        }
        if ty.is_regexp_match_array() {
            let JsonValue::Array(values) = value else {
                return Ok(false);
            };
            fuel::charge(&mut *store, fuel::ELEM, values.len() as u64)?;
            return Ok(!values.is_empty()
                && values
                    .iter()
                    .all(|value| matches!(value, JsonValue::String(_))));
        }
        Ok(match (value, ty.peel()) {
            (_, Type::Unknown) => true,
            (_, Type::Union(members)) => {
                for member in members {
                    if self.json_matches(store, value, member, depth + 1)? {
                        return Ok(true);
                    }
                }
                false
            }
            (JsonValue::Null, Type::Null) => true,
            (JsonValue::Bool(_), Type::Boolean) => true,
            (JsonValue::Bool(value), Type::BooleanLiteral(expected)) => value == expected,
            (JsonValue::Number(_), Type::Number) => true,
            (JsonValue::Number(value), Type::NumberLiteral(expected)) => *value == expected.0,
            (
                JsonValue::String(value),
                Type::String | Type::StringLiteral(_) | Type::TemplateLiteral { .. },
            ) => matches_key(store, ty, value)?,
            (JsonValue::Array(values), Type::Array(element)) => {
                for value in values {
                    if !self.json_matches(store, value, element, depth + 1)? {
                        return Ok(false);
                    }
                }
                true
            }
            (JsonValue::Array(values), Type::Tuple(elements)) => {
                if values.len() < elements.required_len() || values.len() > elements.len() {
                    return Ok(false);
                }
                for (value, ty) in values.iter().zip(elements.iter()) {
                    if !self.json_matches(store, value, ty, depth + 1)? {
                        return Ok(false);
                    }
                }
                true
            }
            (
                JsonValue::Object(values),
                Type::Object {
                    fields,
                    index,
                    more_indexes,
                },
            ) => {
                for (name, field) in fields {
                    let mut matched = None;
                    for (key, value) in values {
                        if same_name(store, name, &key.0)? {
                            matched = Some(value);
                            break;
                        }
                    }
                    if let Some(value) = matched {
                        if !self.json_matches(store, value, &field.ty, depth + 1)? {
                            return Ok(false);
                        }
                    } else if !field.optional
                        && !self.admits_missing(store, &field.ty, depth + 1)?
                    {
                        return Ok(false);
                    }
                }
                for (name, value) in values {
                    for signature in crate::stored_index_signatures(index, more_indexes) {
                        if matches_key(store, &signature.key, &name.0)?
                            && !self.json_matches(store, value, &signature.value, depth + 1)?
                        {
                            return Ok(false);
                        }
                    }
                }
                true
            }
            (JsonValue::Array(_) | JsonValue::Object(_), Type::NonPrimitive) => true,
            _ => false,
        })
    }
}

fn shape_list<'a>(
    store: &mut Store<StoreData>,
    capacity: usize,
) -> wasmtime::Result<(Vec<&'a Type>, HostBytes)> {
    let bytes = (capacity as u64).saturating_mul(std::mem::size_of::<&Type>() as u64);
    let guard = HostBytes::new(&store.data().tenant_limits, bytes).map_err(wasmtime::Error::new)?;
    let mut shapes = Vec::new();
    shapes
        .try_reserve_exact(capacity)
        .map_err(host::fatal_host_error)?;
    Ok((shapes, guard))
}

fn construct_temporal_value(
    store: &mut Store<StoreData>,
    linker: &Linker<StoreData>,
    name: &str,
    raw: Val,
) -> wasmtime::Result<Val> {
    let constructor = crate::mangle::extend(
        &crate::mangle::extend(
            &crate::mangle::prelude("Temporal"),
            &format!("{name}Constructor"),
        ),
        "from",
    );
    let export = linker.get(
        &mut *store,
        crate::runtime::prelude::MODULE_NAME,
        constructor.as_str(),
    )?;
    let wasmtime::Extern::Func(func) = export else {
        return Err(host::fatal_host_error(
            "Temporal.from import is not a function",
        ));
    };
    let mut args = vec![raw];
    if !matches!(name, "Instant" | "Duration") {
        let undefined = store
            .data()
            .undefined_value
            .ok_or_else(|| host::fatal_host_error("undefined global is not installed"))?
            .get(&mut *store);
        args.push(undefined);
    }
    let mut result = [Val::null_any_ref()];
    func.call(&mut *store, &args, &mut result)?;
    let [value] = result;
    Ok(value)
}

// Shapes are immutable compiler output, built under TypeBudget's depth/node
// bounds. Alias back-edges stop this walk rather than recursively expanding.
fn contains_temporal(store: &mut Store<StoreData>, ty: &Type) -> wasmtime::Result<bool> {
    fuel::charge(&mut *store, fuel::ELEM, 1)?;
    if temporal_constructor(ty).is_some() || matches!(ty.peel(), Type::AliasRef { .. }) {
        return Ok(true);
    }
    let mut found = Ok(false);
    crate::type_size::for_each_child(ty, |child| {
        if matches!(found, Ok(false)) {
            found = contains_temporal(store, child);
        }
    });
    found
}

fn same_name(
    store: &mut Store<StoreData>,
    expected: &str,
    actual: &[u16],
) -> wasmtime::Result<bool> {
    fuel::charge(&mut *store, fuel::ELEM, 1)?;
    fuel::charge(&mut *store, fuel::SCAN, expected.len() as u64)?;
    let bytes = (expected.len() as u64).saturating_mul(std::mem::size_of::<u16>() as u64);
    let _guard =
        HostBytes::new(&store.data().tenant_limits, bytes).map_err(wasmtime::Error::new)?;
    let expected =
        crate::literal_units::try_literal_units(expected).map_err(host::fatal_host_error)?;
    fuel::charge(
        &mut *store,
        fuel::SCAN,
        expected.len().min(actual.len()) as u64,
    )?;
    Ok(expected == actual)
}

fn matches_key(
    store: &mut Store<StoreData>,
    domain: &Type,
    name: &[u16],
) -> wasmtime::Result<bool> {
    crate::property_keys::matches_units(domain, name, false, &mut |work| {
        fuel::charge(&mut *store, fuel::SCAN, work)
    })
    .map_err(|error| match error {
        crate::property_keys::MatchError::Work(error) => error,
        crate::property_keys::MatchError::Allocation => {
            host::fatal_host_error("could not allocate argument key matcher state")
        }
    })
}

fn check_depth(depth: u32) -> wasmtime::Result<()> {
    if depth >= crate::runtime::MAX_VTABLE_WALK_DEPTH {
        return Err(host::type_error(
            "function argument conversion nesting limit exceeded",
        ));
    }
    Ok(())
}
