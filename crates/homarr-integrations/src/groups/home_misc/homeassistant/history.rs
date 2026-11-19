//! History bucketing (`homeassistant-history.ts`).

use crate::capabilities::smart_home::SmartHomeHistoryPoint;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryState {
    pub state: String,
    /// Unix milliseconds.
    pub time: f64,
}

pub fn bucket_history(
    states: &[HistoryState],
    start: f64,
    end: f64,
    samples: u32,
) -> Vec<SmartHomeHistoryPoint> {
    let mut observations: Vec<(f64, Option<f64>)> = states
        .iter()
        .map(|item| {
            let parsed = item
                .state
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite());
            // JS `Number(" ")` is 0 but the empty check above already excludes blank strings.
            let value = if item.state.trim().is_empty() {
                None
            } else {
                parsed
            };
            (item.time.max(start), value)
        })
        .collect();
    observations.sort_by(|left, right| left.0.total_cmp(&right.0));
    if observations.is_empty() || samples < 2 {
        return Vec::new();
    }

    let step = (end - start) / f64::from(samples - 1);
    let mut points = Vec::with_capacity(samples as usize);
    let mut cursor = 0;
    let mut current: Option<f64> = None;

    for index in 0..samples {
        let time = (start + step * f64::from(index)).round();
        let mut sum = 0.0;
        let mut count = 0;
        while cursor < observations.len() && observations[cursor].0 <= time {
            let value = observations[cursor].1;
            current = value;
            cursor += 1;
            match value {
                None => {
                    sum = 0.0;
                    count = 0;
                }
                Some(value) => {
                    sum += value;
                    count += 1;
                }
            }
        }
        if count > 0 {
            points.push(SmartHomeHistoryPoint {
                time,
                value: Some(((sum / f64::from(count)) * 100.0).round() / 100.0),
            });
        } else {
            points.push(SmartHomeHistoryPoint {
                time,
                value: current,
            });
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: &str, time: f64) -> HistoryState {
        HistoryState {
            state: value.into(),
            time,
        }
    }

    #[test]
    fn averages_within_a_bucket_and_carries_the_last_value() {
        let points = bucket_history(
            &[state("10", 0.0), state("20", 10.0), state("30", 40.0)],
            0.0,
            100.0,
            3,
        );
        let values: Vec<_> = points.iter().map(|p| (p.time, p.value)).collect();
        assert_eq!(
            values,
            vec![(0.0, Some(10.0)), (50.0, Some(25.0)), (100.0, Some(30.0))]
        );
    }

    #[test]
    fn non_numeric_states_reset_the_series() {
        let points = bucket_history(
            &[state("5", 0.0), state("unavailable", 40.0)],
            0.0,
            100.0,
            3,
        );
        assert_eq!(points[0].value, Some(5.0));
        assert_eq!(points[1].value, None);
        assert_eq!(points[2].value, None);
    }

    #[test]
    fn empty_or_too_few_samples_yield_nothing() {
        assert!(bucket_history(&[], 0.0, 10.0, 5).is_empty());
        assert!(bucket_history(&[state("1", 0.0)], 0.0, 10.0, 1).is_empty());
    }
}
