//! Value formatting for the legacy `StatsResponse` adapter (port of `widgets/src/stats/format.ts`,
//! without locale support: en-US grouping, at most two fraction digits).

use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::types::{IntegrationContext, StatMetric, StatsResponse};

const EMPTY: &str = "\u{2014}";

pub fn to_legacy_stats_response(
    ctx: &IntegrationContext,
    metrics: &[StatsMetric],
    values: &StatsValues,
) -> StatsResponse {
    let formatted = metrics
        .iter()
        .map(|metric| {
            let value = values
                .get(&metric.key)
                .map(|value| format_stats_value(value, metric.unit))
                .unwrap_or_else(|| EMPTY.to_string());
            StatMetric {
                label: metric.label.clone(),
                value,
                unit: None,
            }
        })
        .collect();
    StatsResponse {
        integration_name: ctx.name.clone(),
        kind: ctx.kind.as_str().to_string(),
        metrics: formatted,
    }
}

fn format_number(value: f64) -> String {
    let rounded = format!("{:.2}", value.abs());
    let (int_part, frac_part) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let mut grouped = String::new();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let frac = frac_part.trim_end_matches('0');
    let negative = value < 0.0 && (int_part != "0" || !frac.is_empty());
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&grouped);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

pub fn format_stats_value(value: &StatsValue, unit: StatsUnit) -> String {
    let number = match value {
        StatsValue::Null => return EMPTY.to_string(),
        StatsValue::Bool(b) => return b.to_string(),
        StatsValue::Text(text) => return text.clone(),
        StatsValue::Number(number) => *number,
    };
    if !number.is_finite() {
        return EMPTY.to_string();
    }
    match unit {
        StatsUnit::Bytes | StatsUnit::BytesPerSecond => {
            const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
            let mut scaled = number;
            let mut index = 0;
            while scaled.abs() >= 1024.0 && index < UNITS.len() - 1 {
                scaled /= 1024.0;
                index += 1;
            }
            let suffix = if unit == StatsUnit::BytesPerSecond {
                "/s"
            } else {
                ""
            };
            format!("{} {}{suffix}", format_number(scaled), UNITS[index])
        }
        StatsUnit::Percent => format!("{}%", format_number(number)),
        StatsUnit::Seconds => format!("{} s", format_number(number)),
        StatsUnit::Milliseconds => format!("{} ms", format_number(number)),
        StatsUnit::Grams => format!("{} g", format_number(number)),
        StatsUnit::Count | StatsUnit::Text => format_number(number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_the_widget() {
        let n = StatsValue::Number;
        assert_eq!(format_stats_value(&n(1234.5), StatsUnit::Count), "1,234.5");
        assert_eq!(format_stats_value(&n(2048.0), StatsUnit::Bytes), "2 KiB");
        assert_eq!(
            format_stats_value(&n(1536.0), StatsUnit::BytesPerSecond),
            "1.5 KiB/s"
        );
        assert_eq!(format_stats_value(&n(12.345), StatsUnit::Percent), "12.35%");
        assert_eq!(
            format_stats_value(&StatsValue::Null, StatsUnit::Count),
            "\u{2014}"
        );
        assert_eq!(
            format_stats_value(&StatsValue::Text("v1".into()), StatsUnit::Text),
            "v1"
        );
    }
}
