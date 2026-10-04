//! The submilli `Uint8Array` operations — the Rust port of the prelude's
//! `Uint8Array` and `Uint8ArrayConstructor` surface. Bytes live as a `Vec<u8>`;
//! the higher-order methods box each byte into a `$boxed_number` before calling
//! back into a *guest* closure. The `$Uint8Array` marshalling and host-fn
//! registration live in [`install`]; the shared
//! [`Closure`](crate::runtime::prelude::closure::Closure) handles callbacks.
//!
//! In-place mutators (`reverse`/`fill`/`copyWithin`/`set`/`sort`) overwrite the
//! receiver's existing `$rawUint8Array` backing in place and return the
//! receiver: unlike `$Array`, the `$Uint8Array` struct's field 1 is an immutable
//! reference, so the backing is never swapped.

mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use base64::Engine as _;
use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, NO_PAD, PAD};
use wasmtime::{ArrayRef, Caller, Rooted, StructRef, StructRefPre, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    fatal_host_error, host_boxed_number_vtable, read_uint8, read_uint8_array_arg,
    read_uint8_array_range, uint8_array_backing, write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::number::format_number_js;
use crate::runtime::prelude::array::{ElementCallback, merge_sort};
use crate::runtime::prelude::closure::Closure;
use crate::runtime::prelude::iterator::as_struct;
use crate::runtime::prelude::keep::KeptValue;
use crate::runtime::prelude::vtable::{read_object_entries, read_string_units};

// ---------------------------------------------------------------------------
// Marshalling
// ---------------------------------------------------------------------------

/// Read a `$Uint8Array` (or bare `$rawUint8Array`) `Val` into its bytes — a local
/// name so the method bodies that need the whole buffer open uniformly with
/// `super::read_bytes`, mirroring the Array port's `read_array`; the accessors
/// read in place instead. The actual struct/payload decode lives in `host`.
pub(crate) fn read_bytes(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u8>> {
    read_uint8_array_arg(caller, val, name)
}

/// Build a fresh `$Uint8Array` `Val` from bytes (reuses the host-owned vtable).
fn build(caller: &mut Caller<'_, StoreData>, bytes: &[u8]) -> wasmtime::Result<Val> {
    let st = write_submilli_uint8array_struct(caller, bytes)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Overwrite the receiver's `$rawUint8Array` backing from its start
/// (`bytes.len()` must not exceed the backing length) — the in-place
/// primitive for the mutators.
fn store_bytes(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    bytes: &[u8],
) -> wasmtime::Result<()> {
    let raw = backing(caller, receiver)?;
    fuel::charge(&mut *caller, fuel::COPY, bytes.len() as u64)?;
    raw.write_i8(&mut *caller, 0, bytes)
}

/// The receiver's field-1 `$rawUint8Array` backing.
fn backing(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let st = as_struct(caller, receiver, "Uint8Array mutate receiver")?;
    match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(arr)) => arr.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "Uint8Array: malformed $rawUint8Array backing {other:?}"
        ))),
    }
}

/// `ToUint8` as the Wasm bodies do it: `i32.trunc_sat_f64_u` then keep the low
/// byte (saturates NaN/negatives to 0, not JS `% 256`, matching the prelude).
fn to_byte(n: f64) -> u8 {
    (n as u32 & 0xff) as u8
}

/// Box a byte into a `$boxed_number` for a callback argument or boxed return.
fn box_byte(caller: &mut Caller<'_, StoreData>, b: u8) -> wasmtime::Result<Val> {
    let boxed = intrinsic_types(&mut *caller)?.boxed_number.clone();
    let vtable = host_boxed_number_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, boxed);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::F64(f64::from(b).to_bits())],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Unbox a `$boxed_number` callback result back to a byte (`map`).
fn unbox_byte(caller: &mut Caller<'_, StoreData>, v: &Val, name: &str) -> wasmtime::Result<u8> {
    let Val::AnyRef(Some(any)) = v else {
        return Err(wasmtime::Error::msg(format!(
            "{name} callback returned {v:?}, expected a number"
        )));
    };
    let st = any
        .as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg(format!("{name} callback result is not a number")))?;
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(to_byte(f64::from_bits(bits))),
        other => Err(wasmtime::Error::msg(format!(
            "{name} callback result field was {other:?}, not f64"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Index math (ported 1:1 from the Wasm bodies; identical to the Array port)
// ---------------------------------------------------------------------------

fn trunc_sat(x: f64) -> i32 {
    x as i32
}

fn norm_clamp(x: f64, len: i32) -> i32 {
    let mut i = trunc_sat(x);
    if i < 0 {
        i += len;
    }
    i.clamp(0, len)
}

fn at_index(x: f64, len: i32) -> Option<usize> {
    let mut i = trunc_sat(x);
    if i < 0 {
        i += len;
    }
    if i < 0 || i >= len {
        None
    } else {
        Some(i as usize)
    }
}

fn fwd_from(x: f64, len: i32) -> i32 {
    let mut fi = trunc_sat(x);
    if fi < 0 {
        fi += len;
    }
    fi.max(0)
}

fn last_from(x: f64, len: i32) -> Option<i32> {
    let mut fi = trunc_sat(x);
    if fi < 0 {
        fi += len;
    }
    if fi < 0 {
        return None;
    }
    Some(fi.min(len - 1))
}

// ---------------------------------------------------------------------------
// Accessors / pure value methods
// ---------------------------------------------------------------------------

/// `length` / `byteLength`, read off the backing without copying it.
fn length(caller: &mut Caller<'_, StoreData>, receiver: &Val, name: &str) -> wasmtime::Result<f64> {
    let arr = uint8_array_backing(caller, receiver, name)?;
    Ok(f64::from(arr.len(&mut *caller)?))
}

/// `at(index)` → the byte boxed as a `$boxed_number`, or `null` when out of range.
fn at(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    index: f64,
    name: &str,
) -> wasmtime::Result<Val> {
    let arr = uint8_array_backing(caller, receiver, name)?;
    let len = arr.len(&mut *caller)?;
    match at_index(index, len as i32) {
        Some(i) => {
            let byte = read_uint8(caller, arr, i)?;
            box_byte(caller, byte)
        }
        None => Ok(Val::null_any_ref()),
    }
}

/// `slice` / `subarray`: only the kept range is copied.
fn slice(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    start: f64,
    end: f64,
    name: &str,
) -> wasmtime::Result<Vec<u8>> {
    let arr = uint8_array_backing(caller, receiver, name)?;
    let len = arr.len(&mut *caller)? as i32;
    let si = norm_clamp(start, len);
    let count = (norm_clamp(end, len) - si).max(0) as usize;
    read_uint8_array_range(caller, arr, si as usize, count, name)
}

/// `with(index, value)` → a copy with `index` replaced; `None` when out of range
/// ([`install`] raises the catchable `RangeError("index out of range")`).
fn with(bytes: &[u8], index: f64, value: f64) -> Option<Vec<u8>> {
    let i = at_index(index, bytes.len() as i32)?;
    let mut out = bytes.to_vec();
    out[i] = to_byte(value);
    Some(out)
}

fn index_of(bytes: &[u8], target: f64, from: f64) -> f64 {
    let len = bytes.len() as i32;
    let target = to_byte(target);
    let mut i = fwd_from(from, len);
    while i < len {
        if bytes[i as usize] == target {
            return f64::from(i);
        }
        i += 1;
    }
    -1.0
}

fn last_index_of(bytes: &[u8], target: f64, from: f64) -> f64 {
    let len = bytes.len() as i32;
    let target = to_byte(target);
    let Some(mut i) = last_from(from, len) else {
        return -1.0;
    };
    while i >= 0 {
        if bytes[i as usize] == target {
            return f64::from(i);
        }
        i -= 1;
    }
    -1.0
}

fn includes(bytes: &[u8], target: f64, from: f64) -> bool {
    index_of(bytes, target, from) >= 0.0
}

/// Bytes formatted as decimals and joined by `sep` (`toString` passes `","`).
/// Shared by the `join`/`toString` host fns and the vtable's `toString` slot.
pub(crate) fn join(bytes: &[u8], sep: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(sep);
        }
        out.extend(format_number_js(f64::from(b)).encode_utf16());
    }
    out
}

fn equals(a: &[u8], b: &[u8]) -> bool {
    a == b
}

// ---------------------------------------------------------------------------
// In-place mutators (overwrite the receiver backing, return the receiver)
// ---------------------------------------------------------------------------

fn reverse(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut bytes: Vec<u8>,
) -> wasmtime::Result<Val> {
    bytes.reverse();
    store_bytes(caller, receiver, &bytes)?;
    Ok(*receiver)
}

fn fill(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    value: f64,
    start: f64,
    end: f64,
) -> wasmtime::Result<Val> {
    let raw = backing(caller, receiver)?;
    let len = i32::try_from(raw.len(&mut *caller)?).map_err(fatal_host_error)?;
    let start = norm_clamp(start, len) as u32;
    let end = norm_clamp(end, len) as u32;
    let count = end.saturating_sub(start);
    fuel::charge(&mut *caller, fuel::COPY, u64::from(count))?;
    let chunk = [to_byte(value); 4096];
    let mut offset = start;
    while offset < end {
        let count = (end - offset).min(chunk.len() as u32);
        let bytes = chunk
            .get(..count as usize)
            .ok_or_else(|| fatal_host_error("invalid fill chunk"))?;
        raw.write_i8(&mut *caller, offset, bytes)?;
        offset += count;
    }
    Ok(*receiver)
}

fn copy_within(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    target: f64,
    start: f64,
    end: f64,
) -> wasmtime::Result<Val> {
    let raw = backing(caller, receiver)?;
    let len = i32::try_from(raw.len(&mut *caller)?).map_err(fatal_host_error)?;
    let target = norm_clamp(target, len) as usize;
    let start = norm_clamp(start, len) as usize;
    let end = norm_clamp(end, len) as usize;
    let count = end.saturating_sub(start).min(len as usize - target);
    // A snapshot of only the requested span preserves overlapping copies.
    let bytes = read_uint8_array_range(caller, raw, start, count, "Uint8Array#copyWithin")?;
    fuel::charge(&mut *caller, fuel::COPY, count as u64)?;
    raw.write_i8(&mut *caller, target as u32, &bytes)?;
    Ok(*receiver)
}

/// Validate both ranges before copying or changing the receiver.
fn set(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    source: &Val,
    offset: f64,
) -> wasmtime::Result<()> {
    let target = backing(caller, receiver)?;
    let source = backing(caller, source)?;
    let target_len = target.len(&mut *caller)?;
    let source_len = source.len(&mut *caller)?;
    let offset = trunc_sat(offset);
    if offset < 0
        || (offset as u32)
            .checked_add(source_len)
            .is_none_or(|end| end > target_len)
    {
        return Err(crate::runtime::host::range_error("offset is out of bounds"));
    }
    let bytes = read_uint8_array_range(
        caller,
        source,
        0,
        source_len as usize,
        "Uint8Array#set source",
    )?;
    fuel::charge(&mut *caller, fuel::COPY, u64::from(source_len))?;
    target.write_i8(&mut *caller, offset as u32, &bytes)
}

fn allocate(caller: &mut Caller<'_, StoreData>, len: usize) -> wasmtime::Result<Val> {
    use wasmtime::ArrayRefPre;
    let intr = intrinsic_types(&mut *caller)?;
    fuel::charge(&mut *caller, fuel::COPY, len as u64)?;
    let len = u32::try_from(len).map_err(fatal_host_error)?;
    let pre = ArrayRefPre::new(&mut *caller, intr.raw_uint8_array.clone());
    let raw = ArrayRef::new_zeroed_i8(&mut *caller, &pre, len)?;
    let vtable = crate::runtime::host::host_uint8_array_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, intr.uint8_array.clone());
    let value = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw.to_anyref()))],
    )?;
    Ok(Val::AnyRef(Some(value.to_anyref())))
}

async fn sort_bytes(
    caller: &mut Caller<'_, StoreData>,
    bytes: &mut Vec<u8>,
    cmp: Option<&Closure>,
) -> wasmtime::Result<()> {
    let Some(cmp) = cmp else {
        bytes.sort_unstable();
        return Ok(());
    };
    if bytes.len() < 2 {
        return Ok(());
    }
    // Every box stays alive until the host call returns, so each byte value is
    // boxed once rather than once per comparison.
    let mut boxes = Vec::with_capacity(256);
    for byte in 0..=u8::MAX {
        boxes.push(box_byte(caller, byte)?);
    }
    merge_sort(caller, bytes, cmp, |_, byte| {
        boxes
            .get(usize::from(byte))
            .copied()
            .ok_or_else(|| fatal_host_error("Uint8Array#sort: byte has no box"))
    })
    .await
}

async fn sort(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut bytes: Vec<u8>,
    cmp: Option<Closure>,
) -> wasmtime::Result<Val> {
    sort_bytes(caller, &mut bytes, cmp.as_ref()).await?;
    store_bytes(caller, receiver, &bytes)?;
    Ok(*receiver)
}

// ---------------------------------------------------------------------------
// Immutable variants (build a fresh result; receiver untouched)
// ---------------------------------------------------------------------------

fn to_reversed(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.reverse();
    bytes
}

async fn to_sorted(
    caller: &mut Caller<'_, StoreData>,
    mut bytes: Vec<u8>,
    cmp: Option<Closure>,
) -> wasmtime::Result<Vec<u8>> {
    sort_bytes(caller, &mut bytes, cmp.as_ref()).await?;
    Ok(bytes)
}

// ---------------------------------------------------------------------------
// Higher-order methods (box each byte, re-enter a guest closure)
// ---------------------------------------------------------------------------

async fn for_each(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    f: &Closure,
) -> wasmtime::Result<()> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    for (index, b) in bytes.into_iter().enumerate() {
        let boxed = box_byte(caller, b)?;
        f.call(caller, None, boxed, index).await?;
    }
    Ok(())
}

async fn map(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    f: &Closure,
) -> wasmtime::Result<Vec<u8>> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    let mut out = Vec::with_capacity(bytes.len());
    for (index, b) in bytes.into_iter().enumerate() {
        let boxed = box_byte(caller, b)?;
        let r = f.call(caller, None, boxed, index).await?;
        out.push(unbox_byte(caller, &r, "Uint8Array#map")?);
    }
    Ok(out)
}

async fn filter(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    pred: &Closure,
) -> wasmtime::Result<Vec<u8>> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    let mut out = Vec::new();
    for (index, b) in bytes.into_iter().enumerate() {
        let boxed = box_byte(caller, b)?;
        if pred.test(caller, boxed, index).await? {
            out.push(b);
        }
    }
    Ok(out)
}

async fn reduce(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    f: &Closure,
    mut acc: Val,
    reverse: bool,
) -> wasmtime::Result<Val> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    let order: Vec<usize> = if reverse {
        (0..bytes.len()).rev().collect()
    } else {
        (0..bytes.len()).collect()
    };
    // The next iteration boxes its byte before it passes the accumulator on.
    let kept_acc = KeptValue::new(caller)?;
    for i in order {
        let boxed = box_byte(caller, bytes[i])?;
        acc = f.call(caller, Some(acc), boxed, i).await?;
        kept_acc.set(caller, acc)?;
    }
    Ok(acc)
}

async fn some(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    pred: &Closure,
) -> wasmtime::Result<bool> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    for (index, b) in bytes.into_iter().enumerate() {
        let boxed = box_byte(caller, b)?;
        if pred.test(caller, boxed, index).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn every(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: Vec<u8>,
    pred: &Closure,
) -> wasmtime::Result<bool> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    for (index, b) in bytes.into_iter().enumerate() {
        let boxed = box_byte(caller, b)?;
        if !pred.test(caller, boxed, index).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Index of the first (or, with `reverse`, last) byte matching `pred`.
async fn find_match(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    bytes: &[u8],
    pred: &Closure,
    reverse: bool,
) -> wasmtime::Result<Option<usize>> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    let order: Vec<usize> = if reverse {
        (0..bytes.len()).rev().collect()
    } else {
        (0..bytes.len()).collect()
    };
    for i in order {
        let boxed = box_byte(caller, bytes[i])?;
        if pred.test(caller, boxed, i).await? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// base64 / hex codecs
// ---------------------------------------------------------------------------

/// Standard padded base64 — the `$Uint8Array` `toJson` form.
pub(crate) fn to_base64_standard(bytes: &[u8]) -> String {
    encode_base64(bytes, false, false)
}

fn encode_base64(bytes: &[u8], url_safe: bool, omit_padding: bool) -> String {
    let alpha = if url_safe {
        &alphabet::URL_SAFE
    } else {
        &alphabet::STANDARD
    };
    let config: GeneralPurposeConfig = if omit_padding { NO_PAD } else { PAD };
    GeneralPurpose::new(alpha, config).encode(bytes)
}

/// Accept canonical padding or no padding without decoding the input twice.
fn decode_base64(s: &str, url_safe: bool) -> wasmtime::Result<Vec<u8>> {
    let alpha = if url_safe {
        &alphabet::URL_SAFE
    } else {
        &alphabet::STANDARD
    };
    let config = if s.as_bytes().last() == Some(&b'=') {
        PAD
    } else {
        NO_PAD
    };
    GeneralPurpose::new(alpha, config)
        .decode(s.as_bytes())
        .map_err(|e| crate::runtime::host::syntax_error(format!("Uint8Array.fromBase64: {e}")))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(hex_digit(b >> 4));
        out.push(hex_digit(b & 0xf));
    }
    out
}

fn hex_digit(nibble: u8) -> char {
    char::from(if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + (nibble - 10)
    })
}

/// Parse a hex string (case-insensitive). Throws the prelude's messages on an odd
/// length or an invalid digit so the error fixtures match.
fn from_hex(units: &[u16]) -> wasmtime::Result<Vec<u8>> {
    if !units.len().is_multiple_of(2) {
        return Err(crate::runtime::host::syntax_error(
            "Uint8Array.fromHex: string must have an even number of characters",
        ));
    }
    let mut out = Vec::with_capacity(units.len() / 2);
    for pair in units.as_chunks::<2>().0 {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(c: u16) -> wasmtime::Result<u8> {
    match c {
        0x30..=0x39 => Ok((c - 0x30) as u8),
        0x61..=0x66 => Ok((c - 0x61 + 10) as u8),
        0x41..=0x46 => Ok((c - 0x41 + 10) as u8),
        _ => Err(crate::runtime::host::syntax_error(
            "Uint8Array.fromHex: invalid hex digit",
        )),
    }
}

/// Read `Base64Options` — `(url_safe, omit_padding)`. A null/absent options object
/// means standard alphabet, padded.
fn read_base64_options(
    caller: &mut Caller<'_, StoreData>,
    opt: &Val,
) -> wasmtime::Result<(bool, bool)> {
    if matches!(opt, Val::AnyRef(None)) {
        return Ok((false, false));
    }
    let (mut url_safe, mut omit_padding) = (false, false);
    for (name, value) in read_object_entries(caller, opt, "Base64Options")? {
        // An optional field the caller omitted is materialized as `null`; leave
        // the default for it.
        if matches!(value, Val::AnyRef(None)) {
            continue;
        }
        match String::from_utf16_lossy(&name).as_str() {
            "alphabet" => {
                let alphabet = read_string_units(caller, &value, "Base64Options.alphabet")?;
                url_safe = String::from_utf16_lossy(&alphabet) == "base64url";
            }
            "omitPadding" => omit_padding = read_boolean(caller, &value)?,
            _ => {}
        }
    }
    Ok((url_safe, omit_padding))
}

fn read_boolean(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<bool> {
    let st = as_struct(caller, val, "Base64Options.omitPadding")?;
    match st.field(&mut *caller, 1)? {
        Val::I32(b) => Ok(b != 0),
        other => wasmtime::bail!("Base64Options.omitPadding: field 1 is {other:?}, not i32"),
    }
}

// ---------------------------------------------------------------------------
// Constructor statics
// ---------------------------------------------------------------------------

/// The byte count `Uint8Array.alloc(n)` / `new Uint8Array(n)` reserve, or a
/// `RangeError` when `n` is not a length at all.
///
/// Truncation toward zero matches JS (`2.7` → 2, `NaN` → 0). The two rejected
/// ends are the ones where saturating instead would be silently wrong: `f64 as
/// u32` saturates a large positive to `u32::MAX` — a 4 GiB request — and a
/// negative to `0`, an empty buffer where the program asked for a sized one.
/// JS raises `RangeError: Invalid typed array length` for both.
pub(crate) fn alloc_len(n: f64) -> wasmtime::Result<usize> {
    // Truncate before testing the sign, the way JS's `ToIndex` does: `-0.5`
    // truncates to `0` and is a legal empty length, while `-1` is not.
    if n.trunc() < 0.0 {
        return Err(crate::runtime::host::range_error(format!(
            "invalid Uint8Array length {} — a length cannot be negative",
            format_number_js(n)
        )));
    }
    if n > MAX_ALLOC_LEN as f64 {
        return Err(crate::runtime::host::range_error(format!(
            "invalid Uint8Array length {} — the maximum is {MAX_ALLOC_LEN}",
            format_number_js(n)
        )));
    }
    Ok(n as u32 as usize)
}

/// Ceiling on a single `Uint8Array` allocation. Well past any real payload and
/// far below the point where the request is a denial of service in itself; the
/// store's `ResourceLimiter` still caps the aggregate.
pub(crate) const MAX_ALLOC_LEN: usize = 1 << 30;

/// Read an `$Array` of boxed numbers into bytes — backs `new`/`of`/`fromArray`.
fn read_number_array(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u8>> {
    let elements = crate::runtime::prelude::array::read_array(caller, val, name)?;
    let mut bytes = Vec::with_capacity(elements.len());
    for e in &elements {
        bytes.push(unbox_byte(caller, e, name)?);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn to_byte_saturates_like_trunc_sat_u() {
        // Saturates NaN/negatives to 0 (not JS `% 256`), then keeps the low byte —
        // matching the Wasm `i32.trunc_sat_f64_u` + `& 0xff` path.
        assert_eq!(to_byte(7.0), 7);
        assert_eq!(to_byte(256.0), 0);
        assert_eq!(to_byte(257.0), 1);
        assert_eq!(to_byte(-1.0), 0);
        assert_eq!(to_byte(f64::NAN), 0);
    }

    #[test]
    fn base64_round_trips_both_alphabets() {
        let bytes = [0xfb_u8, 0xff, 0x00, 0x10, 0x41];
        for url_safe in [false, true] {
            let padded = encode_base64(&bytes, url_safe, false);
            assert_eq!(decode_base64(&padded, url_safe).unwrap(), bytes);
            // Decoding is forgiving on padding: an unpadded form still decodes.
            let unpadded = encode_base64(&bytes, url_safe, true);
            assert_eq!(decode_base64(&unpadded, url_safe).unwrap(), bytes);
        }
        // url-safe encodes 0xfb 0xff as `-_`, standard as `+/`.
        assert!(encode_base64(&[0xfb, 0xff], true, false).contains('-'));
        assert!(encode_base64(&[0xfb, 0xff], false, false).contains('+'));
    }

    #[test]
    fn base64_padding_selection_preserves_previous_acceptance() {
        // Enumerate short padding, alphabet and trailing-bit combinations
        // against the previous two-decoder acceptance rule.
        let alphabet = b"AQf=_-";
        for length in 0..=5_u32 {
            for mut code in 0..alphabet.len().pow(length) {
                let mut input = Vec::new();
                for _ in 0..length {
                    input.push(alphabet[code % alphabet.len()]);
                    code /= alphabet.len();
                }
                let text = std::str::from_utf8(&input).unwrap();
                for url_safe in [false, true] {
                    let alpha = if url_safe {
                        &alphabet::URL_SAFE
                    } else {
                        &alphabet::STANDARD
                    };
                    let old = GeneralPurpose::new(alpha, PAD)
                        .decode(&input)
                        .or_else(|_| GeneralPurpose::new(alpha, NO_PAD).decode(&input));
                    let new = decode_base64(text, url_safe);
                    assert_eq!(new.ok(), old.ok(), "{text:?}, url_safe={url_safe}");
                }
            }
        }
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        let bytes = [0x00_u8, 0x0f, 0xa0, 0xff];
        assert_eq!(to_hex(&bytes), "000fa0ff");
        assert_eq!(from_hex(&units("000fA0Ff")).unwrap(), bytes);
        assert!(
            from_hex(&units("0"))
                .unwrap_err()
                .to_string()
                .contains("even number of characters")
        );
        assert!(
            from_hex(&units("0g"))
                .unwrap_err()
                .to_string()
                .contains("invalid hex digit")
        );
    }
}
