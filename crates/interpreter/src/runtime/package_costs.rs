//! Costs packages report through `submilli:cost`, kept for the embedder.
//!
//! Unlike model calls, a report is not a durable effect: a resumed attempt runs
//! the program again and the program reports again, so each attempt's store
//! lists the whole run. Reports are summed by package and label as they arrive,
//! which bounds what a loop of reports can hold.

/// What one package reported under one label, summed over its reports.
///
/// `package` is the reporting package as the host saw it, never a name the
/// package chose. Reports are collected on every attempt, as guest code runs,
/// so a resumed durable run lists them once each.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageCost {
    pub package: String,
    pub label: Option<String>,
    /// The sum of the reports that gave an amount. `None` when none did.
    pub pico_usd: Option<u128>,
    /// Reports that gave usage and no amount.
    pub unpriced_reports: u64,
    /// Summed counts, by unit, in the order each unit first appeared.
    pub usage: Vec<(String, f64)>,
}

/// One `submilli:cost` report, before it is added to its package and label.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageReport {
    pub label: Option<String>,
    pub pico_usd: Option<u128>,
    pub usage: Vec<(String, f64)>,
}

/// The most distinct (package, label) entries one run keeps.
pub const MAX_PACKAGE_COST_ENTRIES: usize = 256;

/// The most distinct usage units one entry keeps.
pub const MAX_PACKAGE_COST_UNITS: usize = 32;

/// A run's package reports, summed by package and label as they arrive.
///
/// Summing on arrival bounds what a loop of reports can hold: at most
/// [`MAX_PACKAGE_COST_ENTRIES`] entries of at most [`MAX_PACKAGE_COST_UNITS`]
/// units. A report that would pass either bound is counted as dropped, and the
/// run's cost says it is incomplete.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageCosts {
    entries: Vec<PackageCost>,
    dropped: u64,
}

impl PackageCosts {
    pub fn entries(&self) -> &[PackageCost] {
        &self.entries
    }

    /// Reports past a bound, which no entry includes.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.dropped == 0
    }

    /// Add one report from `package` to its entry, or count it as dropped.
    pub fn add(&mut self, package: &str, report: PackageReport) {
        let found = self
            .entries
            .iter()
            .position(|entry| entry.package == package && entry.label == report.label);
        let known_units = found.map_or(&[][..], |index| &self.entries[index].usage);
        let new_units = report
            .usage
            .iter()
            .filter(|(unit, _)| !known_units.iter().any(|(known, _)| known == unit))
            .count();
        let fits_entries = found.is_some() || self.entries.len() < MAX_PACKAGE_COST_ENTRIES;
        if !fits_entries || known_units.len() + new_units > MAX_PACKAGE_COST_UNITS {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        // `found` came from this vector, unchanged since; a new entry is pushed
        // only now, once the report is known to fit.
        let index = found.unwrap_or_else(|| {
            self.entries.push(PackageCost {
                package: package.to_owned(),
                label: report.label.clone(),
                pico_usd: None,
                unpriced_reports: 0,
                usage: Vec::new(),
            });
            self.entries.len() - 1
        });
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        match report.pico_usd {
            Some(pico) => {
                entry.pico_usd = Some(entry.pico_usd.unwrap_or(0).saturating_add(pico));
            }
            None => entry.unpriced_reports = entry.unpriced_reports.saturating_add(1),
        }
        for (unit, count) in report.usage {
            match entry.usage.iter_mut().find(|(known, _)| *known == unit) {
                // Saturate rather than reach infinity, which JSON cannot carry.
                Some((_, total)) => *total = (*total + count).min(f64::MAX),
                None => entry.usage.push((unit, count)),
            }
        }
    }
}
