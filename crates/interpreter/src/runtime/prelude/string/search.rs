//! Constant-space Two-Way search over UTF-16 units. Preprocessing finds a
//! critical factorization and period; failed comparisons skip that period
//! instead of retrying every unit of a long needle at every input position.

pub(crate) struct Search<'a> {
    needle: &'a [u16],
    reverse: bool,
    critical: usize,
    period: usize,
    periodic: bool,
    #[cfg(test)]
    comparisons: std::cell::Cell<usize>,
}

impl<'a> Search<'a> {
    pub(crate) fn new(needle: &'a [u16], reverse: bool) -> Self {
        let mut search = Self {
            needle,
            reverse,
            critical: 0,
            period: 1,
            periodic: false,
            #[cfg(test)]
            comparisons: std::cell::Cell::new(0),
        };
        let (left, left_period) = search.factorization(false);
        let (right, right_period) = search.factorization(true);
        (search.critical, search.period) = if left > right {
            (left, left_period)
        } else {
            (right, right_period)
        };
        search.periodic = search.period <= needle.len()
            && search.critical <= needle.len() - search.period
            && (0..search.critical)
                .all(|i| search.equal(search.unit(i), search.unit(i + search.period)));
        if !search.periodic {
            search.period = search.critical.max(needle.len() - search.critical) + 1;
        }
        search
    }

    pub(crate) fn find(&self, haystack: &[u16], from: usize) -> Option<usize> {
        if self.needle.is_empty() {
            return (from <= haystack.len()).then_some(from);
        }
        let max_start = haystack.len().checked_sub(self.needle.len())?;
        let mut start = from;
        let mut memory = 0;
        while start <= max_start {
            let mut index = self.critical.max(memory);
            while index < self.needle.len()
                && self.equal(self.unit(index), self.hay_unit(haystack, start + index))
            {
                index += 1;
            }
            if index < self.needle.len() {
                start += index - self.critical + 1;
                memory = 0;
                continue;
            }
            index = self.critical;
            while index > memory
                && self.equal(
                    self.unit(index - 1),
                    self.hay_unit(haystack, start + index - 1),
                )
            {
                index -= 1;
            }
            if index <= memory {
                return Some(start);
            }
            start = start.checked_add(self.period)?;
            memory = if self.periodic {
                self.needle.len() - self.period
            } else {
                0
            };
        }
        None
    }

    fn factorization(&self, opposite: bool) -> (usize, usize) {
        // `critical` encodes maximal-suffix index + 1, so the sentinel -1
        // never needs a signed conversion or an unchecked subtraction.
        let (mut critical, mut candidate, mut offset, mut period) =
            (0_usize, 0_usize, 1_usize, 1_usize);
        while let Some(candidate_index) = candidate.checked_add(offset) {
            let (Some(a), Some(b)) = (self.unit(candidate_index), self.unit(critical + offset - 1))
            else {
                break;
            };
            if self.equal(Some(a), Some(b)) {
                if offset == period {
                    candidate += period;
                    offset = 1;
                } else {
                    offset += 1;
                }
            } else if (a < b) != opposite {
                candidate += offset;
                offset = 1;
                period = candidate + 1 - critical;
            } else {
                critical = candidate + 1;
                candidate = critical;
                offset = 1;
                period = 1;
            }
        }
        (critical, period)
    }

    fn unit(&self, index: usize) -> Option<u16> {
        self.hay_unit(self.needle, index)
    }

    fn hay_unit(&self, units: &[u16], index: usize) -> Option<u16> {
        let index = if self.reverse {
            units.len().checked_sub(index.checked_add(1)?)?
        } else {
            index
        };
        units.get(index).copied()
    }

    fn equal(&self, a: Option<u16>, b: Option<u16>) -> bool {
        #[cfg(test)]
        self.comparisons.set(self.comparisons.get() + 1);
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::Search;

    #[test]
    fn search_matches_all_short_binary_inputs_and_positions() {
        for length in 0..=7 {
            for bits in 0..1_u32 << length {
                let input: Vec<u16> = (0..length).map(|i| ((bits >> i) & 1) as u16).collect();
                for needle_length in 0..=4 {
                    for needle_bits in 0..1_u32 << needle_length {
                        let needle: Vec<u16> = (0..needle_length)
                            .map(|i| ((needle_bits >> i) & 1) as u16)
                            .collect();
                        for reverse in [false, true] {
                            let search = Search::new(&needle, reverse);
                            let mut hay = input.clone();
                            let mut pattern = needle.clone();
                            if reverse {
                                hay.reverse();
                                pattern.reverse();
                            }
                            for from in 0..=hay.len() + 1 {
                                let expected = if pattern.is_empty() {
                                    (from <= hay.len()).then_some(from)
                                } else {
                                    hay.windows(pattern.len())
                                        .enumerate()
                                        .skip(from)
                                        .find(|(_, window)| *window == pattern)
                                        .map(|(i, _)| i)
                                };
                                assert_eq!(
                                    search.find(&input, from),
                                    expected,
                                    "input={input:?},needle={needle:?},from={from},reverse={reverse}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_prefix_comparisons_grow_linearly() {
        let mut measured = Vec::new();
        for n in [256, 512] {
            let mut input = vec![97; n];
            input.push(98);
            let mut needle = vec![97; n / 2];
            needle.push(99);
            let search = Search::new(&needle, false);
            assert_eq!(search.find(&input, 0), None);
            let work = search.comparisons.get();
            let old_comparisons = (input.len() - needle.len() + 1) * needle.len();
            assert!(work < old_comparisons / 4, "{n}: {work}");
            measured.push(work);
        }
        assert!(measured[1] * 10 <= measured[0] * 21, "{measured:?}");
    }
}
