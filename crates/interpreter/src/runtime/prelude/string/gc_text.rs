//! Reading and building strings in place in the GC heap. A native copy of a
//! string counts against the memory cap beside the GC string it mirrors, so
//! these helpers move units between GC arrays through small fixed buffers.
//! An operation then needs only its inputs and its result under the cap.

use wasmtime::{ArrayRef, ArrayRefPre, ArrayType, Caller, Rooted, Val};

use super::MAX_RESULT_UNITS;
use super::transforms::{Text, Units};
use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{fatal_host_error, range_error};
use crate::runtime::limits::HostBytes;

/// Units moved per GC read and write. The heap lends out one array at a time,
/// so a copy between arrays goes through a buffer this size.
const CHUNK_UNITS: usize = 2048;
// `Spill::grow` clamps between these, and `clamp` panics when min > max.
const _: () = assert!(CHUNK_UNITS <= MAX_RESULT_UNITS);

/// A stretch of a built string: `len` units cycling through the `period` units
/// of `source` that begin at `start`. A copy is a run as long as its period;
/// `repeat` and a pad's fill are runs that wrap around.
pub(super) struct Run {
    pub source: Rooted<ArrayRef>,
    pub start: usize,
    pub period: usize,
    pub len: usize,
}

impl Run {
    /// The units of `source` in `range`, copied once.
    pub fn copy(source: Rooted<ArrayRef>, range: std::ops::Range<usize>) -> Self {
        Self {
            source,
            start: range.start,
            period: range.len(),
            len: range.len(),
        }
    }
}

/// A `$rawString` payload of `len` units, admitted by the GC limiter before its
/// body exists. Its units are zero until written.
pub(super) fn allocate(
    caller: &mut Caller<'_, StoreData>,
    payload_ty: &ArrayType,
    len: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let len = u32::try_from(len).map_err(|_| range_error("Invalid string length"))?;
    let pre = ArrayRefPre::new(&mut *caller, payload_ty.clone());
    ArrayRef::new(&mut *caller, &pre, &Val::I32(0), len)
}

/// Writes `run` into `dst` from unit `at`. `dst` was allocated to hold every
/// run, so the offsets stay within its `u32` length.
pub(super) fn fill_run(
    caller: &mut Caller<'_, StoreData>,
    dst: Rooted<ArrayRef>,
    at: usize,
    run: &Run,
) -> wasmtime::Result<()> {
    if run.len == 0 {
        return Ok(());
    }
    if run.period == 0 {
        return Err(fatal_host_error(
            "string run cycles through an empty source",
        ));
    }
    if run.period <= CHUNK_UNITS {
        fill_repeating(caller, dst, at, run)
    } else {
        fill_streaming(caller, dst, at, run)
    }
}

/// Reads a short source once and repeats it across the buffer, so a long pad
/// of one unit takes `len / CHUNK_UNITS` writes, not `len`.
fn fill_repeating(
    caller: &mut Caller<'_, StoreData>,
    dst: Rooted<ArrayRef>,
    at: usize,
    run: &Run,
) -> wasmtime::Result<()> {
    let mut buffer = [0u16; CHUNK_UNITS];
    let block = CHUNK_UNITS / run.period * run.period;
    read_units_at(
        caller,
        run.source,
        run.start,
        chunk(&mut buffer, run.period)?,
    )?;
    // `fill_run` sends only periods up to CHUNK_UNITS here, and each copy ends
    // at or before `block`, a multiple of the period no larger than the buffer.
    for start in (run.period..block).step_by(run.period) {
        buffer.copy_within(..run.period, start);
    }
    let mut written = 0;
    while written < run.len {
        let n = (run.len - written).min(block);
        write_units_at(caller, dst, at + written, chunk(&mut buffer, n)?)?;
        written += n;
    }
    Ok(())
}

/// Streams a long source chunk by chunk. A chunk stops at the end of the
/// period, so the next one restarts at its first unit.
fn fill_streaming(
    caller: &mut Caller<'_, StoreData>,
    dst: Rooted<ArrayRef>,
    at: usize,
    run: &Run,
) -> wasmtime::Result<()> {
    let mut buffer = [0u16; CHUNK_UNITS];
    let mut written = 0;
    while written < run.len {
        let offset = written % run.period;
        let n = (run.len - written)
            .min(run.period - offset)
            .min(CHUNK_UNITS);
        let units = chunk(&mut buffer, n)?;
        read_units_at(caller, run.source, run.start + offset, units)?;
        write_units_at(caller, dst, at + written, units)?;
        written += n;
    }
    Ok(())
}

/// A GC string read in place for a transform, one cached chunk at a time.
pub(super) struct GcUnits<'a, 'b> {
    caller: &'a mut Caller<'b, StoreData>,
    source: Rooted<ArrayRef>,
    len: usize,
    window: [u16; CHUNK_UNITS],
    window_start: usize,
    window_len: usize,
}

impl<'a, 'b> GcUnits<'a, 'b> {
    pub(super) fn new(
        caller: &'a mut Caller<'b, StoreData>,
        source: Rooted<ArrayRef>,
        len: usize,
    ) -> Self {
        Self {
            caller,
            source,
            len,
            window: [0; CHUNK_UNITS],
            window_start: 0,
            window_len: 0,
        }
    }
}

impl Units for GcUnits<'_, '_> {
    fn len(&self) -> usize {
        self.len
    }

    fn unit(&mut self, index: usize) -> wasmtime::Result<u16> {
        let cached = index
            .checked_sub(self.window_start)
            .filter(|offset| *offset < self.window_len)
            .and_then(|offset| self.window.get(offset));
        if let Some(unit) = cached {
            return Ok(*unit);
        }
        if index >= self.len {
            return Err(fatal_host_error("string read past its end"));
        }
        // Chunk-aligned windows serve forward and backward scans alike.
        let start = index / CHUNK_UNITS * CHUNK_UNITS;
        let n = (self.len - start).min(CHUNK_UNITS);
        read_units_at(
            &mut *self.caller,
            self.source,
            start,
            chunk(&mut self.window, n)?,
        )?;
        self.window_start = start;
        self.window_len = n;
        self.window
            .get(index - start)
            .copied()
            .ok_or_else(|| fatal_host_error("string window misses its index"))
    }
}

/// A transform's input read in place and its result written straight into a
/// GC array sized for the input. That guess holds unless a mapping changes the
/// length; the units past it then spill into a native buffer charged against
/// the cap, and [`GcText::finish`] copies both into an array of the exact length.
/// Input and output share one value because both reach the heap through the
/// one `&mut Caller`.
pub(super) struct GcText<'a, 'b> {
    input: GcUnits<'a, 'b>,
    payload_ty: ArrayType,
    guess: Rooted<ArrayRef>,
    guess_len: usize,
    written: usize,
    pending: [u16; CHUNK_UNITS],
    pending_len: usize,
    spill: Spill,
}

impl<'a, 'b> GcText<'a, 'b> {
    pub(super) fn new(
        input: GcUnits<'a, 'b>,
        payload_ty: ArrayType,
        guess: Rooted<ArrayRef>,
        guess_len: usize,
    ) -> Self {
        Self {
            input,
            payload_ty,
            guess,
            guess_len,
            written: 0,
            pending: [0; CHUNK_UNITS],
            pending_len: 0,
            spill: Spill::default(),
        }
    }

    /// The result payload. Fuel for writing `guess_len` units is the caller's;
    /// a result of another length pays here for its spilled units and for the
    /// copy into its exact array.
    pub(super) fn finish(mut self) -> wasmtime::Result<Rooted<ArrayRef>> {
        self.flush()?;
        if self.written == self.guess_len && self.spill.units.is_empty() {
            return Ok(self.guess);
        }
        let caller = &mut *self.input.caller;
        let total = self.written + self.spill.units.len();
        fuel::charge(
            &mut *caller,
            fuel::COPY,
            total as u64 + self.spill.units.len() as u64,
        )?;
        let exact = allocate(caller, &self.payload_ty, total)?;
        fill_run(caller, exact, 0, &Run::copy(self.guess, 0..self.written))?;
        write_units_at(caller, exact, self.written, &self.spill.units)?;
        Ok(exact)
    }

    fn push(&mut self, unit: u16) -> wasmtime::Result<()> {
        let total = self.written + self.pending_len + self.spill.units.len();
        if total >= MAX_RESULT_UNITS {
            return Err(range_error("Invalid string length"));
        }
        if total >= self.guess_len {
            return self.spill.push(&mut *self.input.caller, unit);
        }
        let slot = self
            .pending
            .get_mut(self.pending_len)
            .ok_or_else(|| fatal_host_error("string output buffer overrun"))?;
        *slot = unit;
        self.pending_len += 1;
        if self.pending_len == CHUNK_UNITS {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> wasmtime::Result<()> {
        let units = self
            .pending
            .get(..self.pending_len)
            .ok_or_else(|| fatal_host_error("string output buffer overrun"))?;
        write_units_at(&mut *self.input.caller, self.guess, self.written, units)?;
        self.written += self.pending_len;
        self.pending_len = 0;
        Ok(())
    }
}

impl Units for GcText<'_, '_> {
    fn len(&self) -> usize {
        self.input.len()
    }

    fn unit(&mut self, index: usize) -> wasmtime::Result<u16> {
        self.input.unit(index)
    }
}

impl Text for GcText<'_, '_> {
    fn emit(&mut self, units: &[u16]) -> wasmtime::Result<()> {
        for &unit in units {
            self.push(unit)?;
        }
        Ok(())
    }
}

/// Output units past the guessed length, admitted against the cap as the
/// buffer grows.
#[derive(Default)]
struct Spill {
    units: Vec<u16>,
    // Fields drop in declaration order: free the buffer before refunding bytes.
    _reservation: Option<HostBytes>,
}

impl Spill {
    fn push(&mut self, caller: &mut Caller<'_, StoreData>, unit: u16) -> wasmtime::Result<()> {
        if self.units.len() == self.units.capacity() {
            self.grow(caller)?;
        }
        self.units.push(unit);
        Ok(())
    }

    fn grow(&mut self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<()> {
        let capacity = self
            .units
            .capacity()
            .saturating_mul(2)
            .clamp(CHUNK_UNITS, MAX_RESULT_UNITS);
        let bytes = capacity
            .checked_mul(std::mem::size_of::<u16>())
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| fatal_host_error("string output size overflow"))?;
        // `GcText::push` spills only while the result is below MAX_RESULT_UNITS,
        // so the doubled, clamped capacity always exceeds the current length.
        let additional = capacity
            .checked_sub(self.units.len())
            .ok_or_else(|| fatal_host_error("string spill outgrew its limit"))?;
        // Admit the new buffer while the old one still exists, then refund the old.
        let reservation = HostBytes::new(&caller.data().tenant_limits, bytes)?;
        self.units
            .try_reserve_exact(additional)
            .map_err(fatal_host_error)?;
        self._reservation = Some(reservation);
        Ok(())
    }
}

fn chunk(buffer: &mut [u16; CHUNK_UNITS], n: usize) -> wasmtime::Result<&mut [u16]> {
    buffer
        .get_mut(..n)
        .ok_or_else(|| fatal_host_error("string chunk exceeds its buffer"))
}

fn read_units_at(
    caller: &mut Caller<'_, StoreData>,
    source: Rooted<ArrayRef>,
    at: usize,
    units: &mut [u16],
) -> wasmtime::Result<()> {
    let at = u32::try_from(at).map_err(fatal_host_error)?;
    source
        .read_i16(&mut *caller, at, units)
        .map_err(fatal_host_error)
}

fn write_units_at(
    caller: &mut Caller<'_, StoreData>,
    dst: Rooted<ArrayRef>,
    at: usize,
    units: &[u16],
) -> wasmtime::Result<()> {
    let at = u32::try_from(at).map_err(fatal_host_error)?;
    dst.write_i16(&mut *caller, at, units)
        .map_err(fatal_host_error)
}
