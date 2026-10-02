//! Guest value ⟷ stored payload, in UTF-16 code units throughout.
//!
//! A payload is JSON text held as `Vec<u16>`, never a Rust `String`: a Submilli
//! `string` may hold a lone surrogate, which UTF-8 cannot carry and which
//! `serde_json` refuses even as a `\u` escape. So both directions are written
//! here against code units rather than routed through `serde_json`.
//!
//! [`serialize`] rejects everything without a JSON form *before* the caller
//! touches the store, so a refused `set` leaves the previous entry intact. The
//! rejection cannot ride on `toJson` alone: a closure, a regex, and a host
//! backing all answer that slot with `{}` or `[object Object]`, which is valid
//! JSON and would silently replace the entry with a stand-in.

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Rooted, StructRef, StructRefPre, StructType, Val};

use crate::runtime::StoreData;
use crate::runtime::host::{
    host_array_vtable, host_boxed_boolean_vtable, host_boxed_number_vtable, host_object_vtable,
    host_opaque_vtable, read_code_units, type_error, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::intrinsic_types;

/// Bounds the pre-check walk. `toJson` has its own bound, but this pass runs
/// first, so a cycle must be caught here or it recurses on the native stack.
const MAX_DEPTH: u32 = crate::runtime::MAX_VTABLE_WALK_DEPTH;

/// `toJson` is slot 1 of the four-slot `$VTable`.
const TO_JSON_SLOT: usize = 1;

/// Serialize a guest value to a stored payload, refusing anything that is not
/// JSON-compatible data.
pub(crate) async fn serialize(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Vec<u16>> {
    reject_unsupported(caller, value, 0)?;
    if matches!(value, Val::AnyRef(None)) {
        return Ok("null".encode_utf16().collect());
    }
    let json =
        crate::runtime::prelude::vtable::dispatch_vtable_slot(caller, value, TO_JSON_SLOT, &[])
            .await?;
    read_units(caller, &json, "session value")
}

/// Rebuild a guest value from a stored payload. The payload is text this module
/// wrote, so a parse failure is a corrupt store rather than guest input.
pub(crate) fn deserialize(
    caller: &mut Caller<'_, StoreData>,
    payload: &[u16],
) -> wasmtime::Result<Val> {
    let mut parser = Parser {
        units: payload,
        pos: 0,
    };
    parser.skip_whitespace();
    let value = parser.parse_value(caller, 0)?;
    parser.skip_whitespace();
    if parser.pos != parser.units.len() {
        return Err(corrupt("trailing text after the stored value"));
    }
    Ok(value)
}

fn corrupt(detail: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!(
        "session: the stored payload is not valid JSON ({detail}); the session store \
         returned data this runtime did not write"
    ))
}

// ---------------------------------------------------------------------------
// Rejecting values with no JSON form
// ---------------------------------------------------------------------------

/// The types a walk classifies against, recovered once per `set` — each read
/// goes through the store, and the walk asks per node.
struct Shapes {
    closure: StructType,
    regex: StructType,
    regex_match: StructType,
    regex_match_box: StructType,
    map_backing: StructType,
    set_backing: StructType,
    array: StructType,
    object_shape: StructType,
    /// Host-only backing structs (`URL`, `Response`, fs `Stat`, …) share one
    /// host-owned vtable and have no common struct type, so they are recognized
    /// by that vtable rather than by shape.
    opaque_vtable: Val,
}

impl Shapes {
    fn recover(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Self> {
        let intr = intrinsic_types(&mut *caller)?;
        let (map_backing, set_backing) = {
            let abi = host_abi(caller)?;
            (abi.map_backing_type.clone(), abi.set_backing_type.clone())
        };
        Ok(Self {
            closure: intr.closure.clone(),
            regex: intr.regex.clone(),
            regex_match: intr.regex_match.clone(),
            regex_match_box: intr.regex_match_box.clone(),
            map_backing,
            set_backing,
            array: intr.array.clone(),
            object_shape: intr.object_shape.clone(),
            opaque_vtable: host_opaque_vtable(caller)?,
        })
    }

    /// Why `value` cannot be stored, or `None` if it is data.
    fn refusal(
        &self,
        caller: &mut Caller<'_, StoreData>,
        value: &Val,
    ) -> wasmtime::Result<Option<&'static str>> {
        for (ty, what) in [
            (&self.closure, "a function"),
            (&self.regex, "a RegExp"),
            (&self.regex_match, "a RegExp match"),
            (&self.regex_match_box, "a RegExp match"),
            (&self.map_backing, "a Map"),
            (&self.set_backing, "a Set"),
        ] {
            if is_a(caller, value, ty)? {
                return Ok(Some(what));
            }
        }
        let Val::AnyRef(Some(any)) = value else {
            return Ok(None);
        };
        let Some(st) = any.as_struct(&mut *caller)? else {
            return Ok(None);
        };
        let vtable = st.field(&mut *caller, 0)?;
        if same_ref(caller, &vtable, &self.opaque_vtable)? {
            return Ok(Some("a host handle"));
        }
        Ok(None)
    }
}

fn reject_unsupported(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    depth: u32,
) -> wasmtime::Result<()> {
    let shapes = Shapes::recover(caller)?;
    walk(caller, value, depth, &shapes)
}

fn walk(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    depth: u32,
    shapes: &Shapes,
) -> wasmtime::Result<()> {
    if depth > MAX_DEPTH {
        return Err(type_error(format!(
            "session: the value is nested deeper than {MAX_DEPTH} levels, so it has no \
             JSON form — a value reachable from itself reaches this bound too"
        )));
    }
    if let Some(what) = shapes.refusal(caller, value)? {
        return Err(unsupported(what));
    }
    if is_a(caller, value, &shapes.array)? {
        for element in crate::runtime::prelude::collection::read_array_vals(caller, value)? {
            walk(caller, &element, depth + 1, shapes)?;
        }
        return Ok(());
    }
    // A plain object and a class instance both keep their data in the
    // `$ObjectShape` name/value arrays. Anything else with a struct shape is a
    // primitive box or a string, which has no children to walk. A class whose
    // own `toJson` overrides the slot is still gated on the fields it exposes,
    // which is what the walk reaches.
    if is_a(caller, value, &shapes.object_shape)? {
        for (_, field) in
            crate::runtime::prelude::vtable::read_object_entries(caller, value, "session value")?
        {
            walk(caller, &field, depth + 1, shapes)?;
        }
    }
    Ok(())
}

fn unsupported(what: &str) -> wasmtime::Error {
    type_error(format!(
        "session: {what} has no JSON form and cannot be stored — store the data it \
         stands for instead (a plain object, an array, or a primitive)"
    ))
}

fn is_a(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    ty: &StructType,
) -> wasmtime::Result<bool> {
    crate::runtime::prelude::collection::is_a(caller, value, ty)
}

fn same_ref(caller: &mut Caller<'_, StoreData>, left: &Val, right: &Val) -> wasmtime::Result<bool> {
    match (left, right) {
        (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) => Rooted::ref_eq(&*caller, a, b),
        _ => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Reading a payload back into guest values
// ---------------------------------------------------------------------------

struct Parser<'a> {
    units: &'a [u16],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u16> {
        self.units.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0A | 0x0D)) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, ascii: u8) -> bool {
        if self.peek() == Some(u16::from(ascii)) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, ascii: u8) -> wasmtime::Result<()> {
        if self.eat(ascii) {
            return Ok(());
        }
        Err(corrupt(&format!(
            "expected `{}` at offset {}",
            ascii as char, self.pos
        )))
    }

    fn parse_value(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        depth: u32,
    ) -> wasmtime::Result<Val> {
        if depth > MAX_DEPTH {
            return Err(corrupt("nesting exceeds the runtime's structural bound"));
        }
        self.skip_whitespace();
        match self.peek() {
            Some(c) if c == u16::from(b'{') => self.parse_object(caller, depth),
            Some(c) if c == u16::from(b'[') => self.parse_array(caller, depth),
            Some(c) if c == u16::from(b'"') => {
                let units = self.parse_string()?;
                build_string(caller, &units)
            }
            Some(c) if c == u16::from(b't') => {
                self.expect_word("true")?;
                build_boolean(caller, true)
            }
            Some(c) if c == u16::from(b'f') => {
                self.expect_word("false")?;
                build_boolean(caller, false)
            }
            Some(c) if c == u16::from(b'n') => {
                self.expect_word("null")?;
                Ok(Val::AnyRef(None))
            }
            Some(_) => {
                let n = self.parse_number()?;
                build_number(caller, n)
            }
            None => Err(corrupt("the payload ended where a value was expected")),
        }
    }

    fn expect_word(&mut self, word: &str) -> wasmtime::Result<()> {
        for expected in word.bytes() {
            if !self.eat(expected) {
                return Err(corrupt(&format!(
                    "expected `{word}` at offset {}",
                    self.pos
                )));
            }
        }
        Ok(())
    }

    fn parse_object(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        depth: u32,
    ) -> wasmtime::Result<Val> {
        self.expect(b'{')?;
        let mut names = Vec::new();
        let mut values = Vec::new();
        self.skip_whitespace();
        if !self.eat(b'}') {
            loop {
                self.skip_whitespace();
                let name = self.parse_string()?;
                names.push(build_string(caller, &name)?);
                self.skip_whitespace();
                self.expect(b':')?;
                values.push(self.parse_value(caller, depth + 1)?);
                self.skip_whitespace();
                if self.eat(b',') {
                    continue;
                }
                self.expect(b'}')?;
                break;
            }
        }
        build_object(caller, names, values)
    }

    fn parse_array(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        depth: u32,
    ) -> wasmtime::Result<Val> {
        self.expect(b'[')?;
        let mut elements = Vec::new();
        self.skip_whitespace();
        if !self.eat(b']') {
            loop {
                elements.push(self.parse_value(caller, depth + 1)?);
                self.skip_whitespace();
                if self.eat(b',') {
                    continue;
                }
                self.expect(b']')?;
                break;
            }
        }
        build_array(caller, elements)
    }

    /// Reads a quoted string into code units. Unpaired surrogates pass through
    /// on both the raw and the `\u` path — the whole reason this parser exists.
    fn parse_string(&mut self) -> wasmtime::Result<Vec<u16>> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(corrupt("the payload ended inside a string"));
            };
            self.pos += 1;
            match c {
                c if c == u16::from(b'"') => return Ok(out),
                c if c == u16::from(b'\\') => out.push(self.parse_escape()?),
                c => out.push(c),
            }
        }
    }

    fn parse_escape(&mut self) -> wasmtime::Result<u16> {
        let Some(c) = self.peek() else {
            return Err(corrupt("the payload ended inside an escape"));
        };
        self.pos += 1;
        let byte = u8::try_from(c).unwrap_or(0);
        Ok(match byte {
            b'"' => u16::from(b'"'),
            b'\\' => u16::from(b'\\'),
            b'/' => u16::from(b'/'),
            b'b' => 0x08,
            b'f' => 0x0C,
            b'n' => 0x0A,
            b'r' => 0x0D,
            b't' => 0x09,
            b'u' => self.parse_hex4()?,
            _ => return Err(corrupt(&format!("unknown escape at offset {}", self.pos))),
        })
    }

    fn parse_hex4(&mut self) -> wasmtime::Result<u16> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let Some(c) = self.peek() else {
                return Err(corrupt("the payload ended inside a \\u escape"));
            };
            self.pos += 1;
            let digit = char::from_u32(u32::from(c))
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| corrupt(&format!("bad \\u escape at offset {}", self.pos)))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> wasmtime::Result<f64> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(c) if c < 0x80 && matches!(
                c as u8,
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
            )
        ) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(corrupt(&format!("expected a value at offset {start}")));
        }
        // The span is ASCII by construction, so this decode is exact.
        String::from_utf16_lossy(&self.units[start..self.pos])
            .parse::<f64>()
            .map_err(|_| corrupt(&format!("malformed number at offset {start}")))
    }
}

// ---------------------------------------------------------------------------
// Guest value construction
// ---------------------------------------------------------------------------

fn build_string(caller: &mut Caller<'_, StoreData>, units: &[u16]) -> wasmtime::Result<Val> {
    let st = write_submilli_string_struct_units(caller, units)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn build_boolean(caller: &mut Caller<'_, StoreData>, value: bool) -> wasmtime::Result<Val> {
    let ty = abi_struct(caller, |abi| abi.boxed_boolean_type.clone())?;
    let vtable = host_boxed_boolean_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(&mut *caller, &pre, &[vtable, Val::I32(i32::from(value))])?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn build_number(caller: &mut Caller<'_, StoreData>, value: f64) -> wasmtime::Result<Val> {
    let ty = abi_struct(caller, |abi| abi.boxed_number_type.clone())?;
    let vtable = host_boxed_number_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(&mut *caller, &pre, &[vtable, Val::F64(value.to_bits())])?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

pub(super) fn build_array(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
) -> wasmtime::Result<Val> {
    let (array_ty, raw_ty) = {
        let abi = host_abi(caller)?;
        (abi.array_type.clone(), abi.raw_array_type.clone())
    };
    let vtable = host_array_vtable(caller)?;
    let raw_pre = ArrayRefPre::new(&mut *caller, raw_ty);
    let raw = ArrayRef::new_fixed(&mut *caller, &raw_pre, &elements)?;
    let pre = StructRefPre::new(&mut *caller, array_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw.to_anyref()))],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn build_object(
    caller: &mut Caller<'_, StoreData>,
    names: Vec<Val>,
    values: Vec<Val>,
) -> wasmtime::Result<Val> {
    let (shape_ty, names_ty, fields_ty) = {
        let abi = host_abi(caller)?;
        (
            abi.object_shape_type.clone(),
            abi.field_names_type.clone(),
            abi.object_fields_type.clone(),
        )
    };
    let vtable = host_object_vtable(caller)?;
    let names_pre = ArrayRefPre::new(&mut *caller, names_ty);
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &names)?;
    let fields_pre = ArrayRefPre::new(&mut *caller, fields_ty);
    let values = ArrayRef::new_fixed(&mut *caller, &fields_pre, &values)?;
    let pre = StructRefPre::new(&mut *caller, shape_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

fn host_abi<'a>(
    caller: &'a Caller<'_, StoreData>,
) -> wasmtime::Result<&'a crate::runtime::host::HostAbi> {
    caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))
}

fn abi_struct(
    caller: &Caller<'_, StoreData>,
    select: impl FnOnce(&crate::runtime::host::HostAbi) -> wasmtime::StructType,
) -> wasmtime::Result<wasmtime::StructType> {
    Ok(select(host_abi(caller)?))
}

/// Read a `$string` value into code units without the lossy UTF-8 round trip
/// `read_string_arg` performs.
pub(super) fn read_units(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(type_error(format!("{name} expects a string, got null")));
    };
    let payload = match any.as_struct(&mut *caller)? {
        Some(st) => match st.field(&mut *caller, 1)? {
            Val::AnyRef(Some(inner)) => inner.unwrap_array(&mut *caller)?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: malformed $string payload {other:?}"
                )));
            }
        },
        None => any.unwrap_array(&mut *caller)?,
    };
    read_code_units(&mut *caller, payload, name)
}
