//! Lazy depth-first measurement shared by types and surface annotations.

use std::collections::TryReserveError;

use crate::type_size::TypeExtent;

/// `children` yields the reverse of declaration order, preserving the former
/// eager LIFO walk. Only ancestor iterators are retained, never a child frontier.
pub(crate) fn measure<'a, T, I: Iterator<Item = &'a T>>(
    root: &'a T,
    max_nodes: u64,
    max_depth: u32,
    children: impl Fn(&'a T) -> I,
) -> Result<TypeExtent, TryReserveError> {
    let mut extent = TypeExtent::default();
    let mut ancestors = Vec::new();
    let mut current = Some((root, 1u32));
    while let Some((node, depth)) = current.take() {
        extent.nodes = extent.nodes.saturating_add(1);
        extent.depth = extent.depth.max(depth);
        if extent.nodes > max_nodes || extent.depth > max_depth {
            return Ok(extent);
        }
        let mut descendants = children(node);
        if let Some(child) = descendants.next() {
            reserve_frame(&mut ancestors)?;
            ancestors.push((descendants, depth));
            current = Some((child, depth.saturating_add(1)));
            continue;
        }
        while let Some((siblings, parent_depth)) = ancestors.last_mut() {
            if let Some(sibling) = siblings.next() {
                current = Some((sibling, parent_depth.saturating_add(1)));
                break;
            }
            ancestors.pop();
        }
    }
    Ok(extent)
}

fn reserve_frame<T>(frames: &mut Vec<T>) -> Result<(), TryReserveError> {
    #[cfg(test)]
    tests::before_reservation(frames.len(), frames.capacity())?;
    if frames.len() == frames.capacity() {
        frames.try_reserve(1)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Clone, Copy, Default)]
    pub(crate) struct Observation {
        pub peak_frames: usize,
        pub reservations: usize,
        fail_after: Option<usize>,
    }

    thread_local! {
        static OBSERVATION: Cell<Observation> = Cell::new(Observation::default());
    }

    pub(crate) fn observe<R>(
        fail_after: Option<usize>,
        run: impl FnOnce() -> R,
    ) -> (R, Observation) {
        struct Restore(Observation);
        impl Drop for Restore {
            fn drop(&mut self) {
                OBSERVATION.set(self.0);
            }
        }
        let _restore = Restore(OBSERVATION.replace(Observation {
            fail_after,
            ..Observation::default()
        }));
        let result = run();
        (result, OBSERVATION.get())
    }

    pub(super) fn before_reservation(len: usize, capacity: usize) -> Result<(), TryReserveError> {
        let mut observation = OBSERVATION.get();
        observation.peak_frames = observation.peak_frames.max(len + 1);
        if len == capacity {
            if observation.fail_after == Some(observation.reservations) {
                // Capacity overflow produces a real TryReserveError without
                // exhausting the allocator or affecting other allocations.
                return Vec::<u8>::new().try_reserve(usize::MAX);
            }
            observation.reservations += 1;
        }
        OBSERVATION.set(observation);
        Ok(())
    }
}
