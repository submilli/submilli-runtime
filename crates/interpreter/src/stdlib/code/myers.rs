//! Linear-space Myers diff with fallible allocation and incremental work charging.
// Adapted from similar 2.7.0's Apache-2.0 implementation (see repository LICENSE),
// itself based on Brandon Williams' implementation. This version replaces native
// recursion with tasks and adds checked indexing, allocation and work charging.
use similar::DiffOp;
use std::ops::Range;
use wasmtime::{Result, bail};

const MAX_LINES: usize = 1_000_000;

pub(super) fn diff(
    old: &[usize],
    new: &[usize],
    mut charge: impl FnMut(u64) -> Result<()>,
) -> Result<Vec<DiffOp>> {
    let total = old.len().saturating_add(new.len());
    if total > MAX_LINES {
        bail!("code.diff: comparison exceeds line-count limit; compare smaller sections");
    }
    charge(total as u64)?;
    let mut search = Search {
        old,
        new,
        forward: Frontier::new(total)?,
        backward: Frontier::new(total)?,
        charge: &mut charge,
    };
    let mut pending = Vec::new();
    pending
        .try_reserve(1)
        .map_err(crate::runtime::host::fatal_host_error)?;
    pending.push(Task::Compare(0..old.len(), 0..new.len()));
    let mut output = Operations::default();
    while let Some(task) = pending.pop() {
        match task {
            Task::Equal(old, new, len) => output.equal(old, new, len)?,
            Task::Compare(old, new) => search.compare(old, new, &mut pending, &mut output)?,
        }
    }
    output.finish()
}

enum Task {
    Compare(Range<usize>, Range<usize>),
    Equal(usize, usize, usize),
}

struct Search<'a, F> {
    old: &'a [usize],
    new: &'a [usize],
    forward: Frontier,
    backward: Frontier,
    charge: &'a mut F,
}
impl<F: FnMut(u64) -> Result<()>> Search<'_, F> {
    fn compare(
        &mut self,
        mut old: Range<usize>,
        mut new: Range<usize>,
        pending: &mut Vec<Task>,
        output: &mut Operations,
    ) -> Result<()> {
        let prefix = self.common(&old, &new, false)?;
        output.equal(old.start, new.start, prefix)?;
        old.start += prefix;
        new.start += prefix;
        let suffix = self.common(&old, &new, true)?;
        old.end -= suffix;
        new.end -= suffix;
        pending
            .try_reserve(3)
            .map_err(crate::runtime::host::fatal_host_error)?;
        if suffix != 0 {
            pending.push(Task::Equal(old.end, new.end, suffix));
        }
        if old.is_empty() {
            output.insert(old.start, new.start, new.len())?;
        } else if new.is_empty() {
            output.delete(old.start, old.len(), new.start)?;
        } else {
            let (x, y) = self.middle(&old, &new)?;
            if !(old.start..=old.end).contains(&x)
                || !(new.start..=new.end).contains(&y)
                || (x == old.start && y == new.start)
                || (x == old.end && y == new.end)
            {
                return Err(crate::runtime::host::fatal_host_error(
                    "code.diff: internal non-progressing split",
                ));
            }
            pending.push(Task::Compare(x..old.end, y..new.end));
            pending.push(Task::Compare(old.start..x, new.start..y));
        }
        Ok(())
    }

    fn common(&mut self, old: &Range<usize>, new: &Range<usize>, reverse: bool) -> Result<usize> {
        let mut len = 0;
        while len < old.len().min(new.len()) {
            (self.charge)(1)?;
            let a = if reverse {
                old.end - len - 1
            } else {
                old.start + len
            };
            let b = if reverse {
                new.end - len - 1
            } else {
                new.start + len
            };
            let (a, b) = self.old.get(a).zip(self.new.get(b)).ok_or_else(|| {
                crate::runtime::host::fatal_host_error("code.diff: internal line index")
            })?;
            if a != b {
                break;
            }
            len += 1;
        }
        Ok(len)
    }

    fn middle(&mut self, old: &Range<usize>, new: &Range<usize>) -> Result<(usize, usize)> {
        // The line cap bounds all signed coordinates and frontier arithmetic.
        let n = old.len();
        let m = new.len();
        let delta = n as isize - m as isize;
        let odd = delta & 1 == 1;
        self.forward.set(1, 0)?;
        self.backward.set(1, 0)?;
        let max_distance = (n + m).div_ceil(2) + 1;
        for distance in 0..max_distance as isize {
            for diagonal in (-distance..=distance).rev().step_by(2) {
                (self.charge)(1)?;
                let mut x = self.forward.next(diagonal, distance)?;
                let y = x as isize - diagonal;
                let origin = (x, y);
                if y >= 0 && x < n && (y as usize) < m {
                    x += self.common(
                        &(old.start + x..old.end),
                        &(new.start + y as usize..new.end),
                        false,
                    )?;
                }
                self.forward.set(diagonal, x)?;
                if odd
                    && (diagonal - delta).abs() < distance
                    && x + self.backward.get(delta - diagonal)? >= n
                {
                    let y = usize::try_from(origin.1).map_err(|_| {
                        crate::runtime::host::fatal_host_error("code.diff: internal negative split")
                    })?;
                    return Ok((old.start + origin.0, new.start + y));
                }
            }
            for diagonal in (-distance..=distance).rev().step_by(2) {
                (self.charge)(1)?;
                let mut x = self.backward.next(diagonal, distance)?;
                let mut y = x as isize - diagonal;
                if y >= 0 && x < n && (y as usize) < m {
                    let advance = self.common(
                        &(old.start..old.end - x),
                        &(new.start..new.end - y as usize),
                        true,
                    )?;
                    x += advance;
                    y += advance as isize;
                }
                self.backward.set(diagonal, x)?;
                if !odd
                    && (diagonal - delta).abs() <= distance
                    && x + self.forward.get(delta - diagonal)? >= n
                {
                    let x = n.checked_sub(x).ok_or_else(|| {
                        crate::runtime::host::fatal_host_error("code.diff: internal backward split")
                    })?;
                    let y = usize::try_from(y)
                        .ok()
                        .and_then(|y| m.checked_sub(y))
                        .ok_or_else(|| {
                            crate::runtime::host::fatal_host_error(
                                "code.diff: internal backward split",
                            )
                        })?;
                    return Ok((old.start + x, new.start + y));
                }
            }
        }
        Err(crate::runtime::host::fatal_host_error(
            "code.diff: internal missing middle split",
        ))
    }
}

struct Frontier {
    offset: isize,
    positions: Vec<usize>,
}
impl Frontier {
    fn new(total: usize) -> Result<Self> {
        let offset = total.div_ceil(2) + 1;
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(2 * offset)
            .map_err(crate::runtime::host::fatal_host_error)?;
        positions.resize(2 * offset, 0);
        Ok(Self {
            offset: offset as isize,
            positions,
        })
    }

    fn get(&self, diagonal: isize) -> Result<usize> {
        usize::try_from(diagonal + self.offset)
            .ok()
            .and_then(|index| self.positions.get(index))
            .copied()
            .ok_or_else(|| {
                crate::runtime::host::fatal_host_error("code.diff: internal frontier index")
            })
    }

    fn set(&mut self, diagonal: isize, position: usize) -> Result<()> {
        let slot = usize::try_from(diagonal + self.offset)
            .ok()
            .and_then(|index| self.positions.get_mut(index))
            .ok_or_else(|| {
                crate::runtime::host::fatal_host_error("code.diff: internal frontier index")
            })?;
        *slot = position;
        Ok(())
    }

    fn next(&self, diagonal: isize, distance: isize) -> Result<usize> {
        if diagonal == -distance
            || (diagonal != distance && self.get(diagonal - 1)? < self.get(diagonal + 1)?)
        {
            self.get(diagonal + 1)
        } else {
            self.get(diagonal - 1)?.checked_add(1).ok_or_else(|| {
                crate::runtime::host::fatal_host_error("code.diff: internal frontier overflow")
            })
        }
    }
}

#[derive(Default)]
struct Operations {
    ops: Vec<DiffOp>,
    equal: Option<(usize, usize, usize)>,
    delete: Option<(usize, usize, usize)>,
    insert: Option<(usize, usize, usize)>,
}
impl Operations {
    fn equal(&mut self, old: usize, new: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        self.flush_changes()?;
        if let Some((a, b, previous)) = self.equal.take() {
            if old != a + previous || new != b + previous {
                return Err(crate::runtime::host::fatal_host_error(
                    "code.diff: internal disjoint equality",
                ));
            }
            self.equal = Some((a, b, previous + len));
        } else {
            self.equal = Some((old, new, len));
        }
        Ok(())
    }

    fn delete(&mut self, old: usize, len: usize, new: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        self.flush_equal()?;
        if let Some((a, previous, b)) = self.delete.take() {
            if old != a + previous {
                return Err(crate::runtime::host::fatal_host_error(
                    "code.diff: internal disjoint deletion",
                ));
            }
            self.delete = Some((a, previous + len, b));
        } else {
            self.delete = Some((old, len, new));
        }
        Ok(())
    }

    fn insert(&mut self, old: usize, new: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        self.flush_equal()?;
        if let Some((a, b, previous)) = self.insert.take() {
            if new != b + previous {
                return Err(crate::runtime::host::fatal_host_error(
                    "code.diff: internal disjoint insertion",
                ));
            }
            self.insert = Some((a, b, previous + len));
        } else {
            self.insert = Some((old, new, len));
        }
        Ok(())
    }

    fn flush_equal(&mut self) -> Result<()> {
        if let Some((old_index, new_index, len)) = self.equal.take() {
            self.push(DiffOp::Equal {
                old_index,
                new_index,
                len,
            })?;
        }
        Ok(())
    }

    fn flush_changes(&mut self) -> Result<()> {
        match (self.delete.take(), self.insert.take()) {
            (Some((old_index, old_len, _)), Some((_, new_index, new_len))) => {
                self.push(DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                })?;
            }
            (Some((old_index, old_len, new_index)), None) => {
                self.push(DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                })?;
            }
            (None, Some((old_index, new_index, new_len))) => {
                self.push(DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                })?;
            }
            (None, None) => {}
        }
        Ok(())
    }

    fn push(&mut self, op: DiffOp) -> Result<()> {
        self.ops
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        self.ops.push(op);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<DiffOp>> {
        self.flush_equal()?;
        self.flush_changes()?;
        Ok(self.ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_similar_on_exhaustive_short_sequences() {
        let sequences: Vec<Vec<usize>> = (0..=6)
            .flat_map(|len| {
                (0..1_usize << len).map(move |bits| (0..len).map(|bit| (bits >> bit) & 1).collect())
            })
            .collect();
        for old in &sequences {
            for new in &sequences {
                let mut reference =
                    similar::algorithms::Replace::new(similar::algorithms::Capture::new());
                similar::algorithms::myers::diff(
                    &mut reference,
                    old,
                    0..old.len(),
                    new,
                    0..new.len(),
                )
                .unwrap();
                let expected = reference.into_inner().into_ops();
                let actual = diff(old, new, |_| Ok(())).unwrap();
                assert_eq!(actual, expected, "old={old:?} new={new:?}");
                let compact = similar::capture_diff_slices(similar::Algorithm::Myers, old, new);
                let distance = |ops: &[DiffOp]| {
                    ops.iter()
                        .filter(|op| !matches!(op, DiffOp::Equal { .. }))
                        .map(|op| op.old_range().len() + op.new_range().len())
                        .sum::<usize>()
                };
                assert_eq!(distance(&actual), distance(&compact));
                let rebuilt: Vec<_> = actual
                    .iter()
                    .flat_map(|op| op.iter_changes(old, new))
                    .filter(|change| change.tag() != similar::ChangeTag::Delete)
                    .map(|change| change.value())
                    .collect();
                assert_eq!(&rebuilt, new);
            }
        }
    }

    #[test]
    fn small_edits_charge_linear_work_and_disjoint_inputs_stop_early() {
        let mut measured = Vec::new();
        for len in [4096, 8192] {
            let old: Vec<_> = (0..len).collect();
            let mut new = old.clone();
            new[len / 4] = len;
            new[len * 3 / 4] = len + 1;
            let mut work = 0;
            diff(&old, &new, |steps| {
                work += steps;
                Ok(())
            })
            .unwrap();
            measured.push(work);
            assert!(work < (len * len / 2) as u64);
        }
        assert!(measured[1] <= measured[0] * 21 / 10, "{measured:?}");
        let mut work = 0;
        let error = diff(&vec![0; 8192], &vec![1; 8192], |steps| {
            if work + steps > 10_000 {
                return Err(wasmtime::Trap::OutOfFuel.into());
            }
            work += steps;
            Ok(())
        })
        .unwrap_err();
        assert!(error.is::<wasmtime::Trap>());
        assert!(work <= 10_000);
    }
}
