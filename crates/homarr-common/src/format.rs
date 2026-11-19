use std::time::Instant;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ByteUnitSystem {
    Binary,
    #[default]
    Decimal,
}

const BINARY_UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
const DECIMAL_UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
const BIT_RATE_UNITS: [&str; 7] = ["b/s", "Kb/s", "Mb/s", "Gb/s", "Tb/s", "Pb/s", "Eb/s"];

fn sanitize(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

fn pick_rounded_unit_index(
    value: f64,
    base: f64,
    last_index: usize,
    fraction_digits: usize,
) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let mut index = 0usize;
    let mut current = value;
    while current >= base && index < last_index {
        current /= base;
        index += 1;
    }
    if index >= last_index {
        return index;
    }
    let scaled = value / base.powi(index as i32);
    let rounded: f64 = format!("{scaled:.fraction_digits$}")
        .parse()
        .unwrap_or(scaled);
    if rounded >= base {
        index + 1
    } else {
        index
    }
}

pub fn format_bytes(bytes: f64, unit: ByteUnitSystem) -> String {
    let (units, base) = match unit {
        ByteUnitSystem::Binary => (&BINARY_UNITS, 1024.0),
        ByteUnitSystem::Decimal => (&DECIMAL_UNITS, 1000.0),
    };
    let safe = sanitize(bytes);
    let index = pick_rounded_unit_index(safe, base, units.len() - 1, 1);
    let scaled = safe / base.powi(index as i32);
    format!("{scaled:.1} {}", units[index])
}

pub fn format_bytes_pair(used: f64, total: f64, unit: ByteUnitSystem) -> (String, String) {
    let (units, base) = match unit {
        ByteUnitSystem::Binary => (&BINARY_UNITS, 1024.0),
        ByteUnitSystem::Decimal => (&DECIMAL_UNITS, 1000.0),
    };
    let safe_used = sanitize(used);
    let safe_total = sanitize(total);
    let index = pick_rounded_unit_index(safe_used.max(safe_total), base, units.len() - 1, 1);
    let suffix = units[index];
    let div = base.powi(index as i32);
    (
        format!("{:.1} {suffix}", safe_used / div),
        format!("{:.1} {suffix}", safe_total / div),
    )
}

pub fn format_byte_rate(bytes: f64, unit: ByteUnitSystem) -> String {
    format!("{}/s", format_bytes(bytes, unit))
}

pub fn format_bit_rate(bits_per_second: f64, max_fraction_digits: usize) -> String {
    let safe = sanitize(bits_per_second);
    let digits = max_fraction_digits.min(20);
    let index = pick_rounded_unit_index(safe, 1000.0, BIT_RATE_UNITS.len() - 1, digits);
    let scaled = safe / 1000f64.powi(index as i32);
    let rounded = format!("{scaled:.digits$}");
    let trimmed = if rounded.contains('.') {
        rounded.trim_end_matches('0').trim_end_matches('.')
    } else {
        rounded.as_str()
    };
    format!("{trimmed} {}", BIT_RATE_UNITS[index])
}

pub fn split_to_chunks<T>(items: &[T], chunk_size: usize) -> impl Iterator<Item = &[T]> {
    items.chunks(chunk_size.max(1))
}

#[derive(Debug, Clone)]
pub struct Stopwatch {
    start: Instant,
}

impl Default for Stopwatch {
    fn default() -> Self {
        Self::new()
    }
}

impl Stopwatch {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn elapsed_formatted_ms(&self) -> String {
        format!("{:.0}ms", self.elapsed_ms())
    }

    pub fn reset(&mut self) {
        self.start = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes_and_rates() {
        assert_eq!(format_bytes(0.0, ByteUnitSystem::Decimal), "0.0 B");
        assert_eq!(format_bytes(1000.0, ByteUnitSystem::Decimal), "1.0 KB");
        assert_eq!(
            format_bytes(985_828_802_560.0, ByteUnitSystem::Decimal),
            "985.8 GB"
        );
        assert_eq!(
            format_bytes(985_828_802_560.0, ByteUnitSystem::Binary),
            "918.1 GiB"
        );
        assert_eq!(format_bit_rate(1_500_000.0, 1), "1.5 Mb/s");
        assert_eq!(format_bit_rate(1_000.0, 1), "1 Kb/s");
    }
}
