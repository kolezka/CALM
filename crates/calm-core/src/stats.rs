/// Nearest-rank percentile, `p` in 0..=100. Sorts `values` in place.
/// Returns 0.0 for empty input so a run with no samples still prints.
#[allow(clippy::ptr_arg)] // Signature fixed by the plan; a slice would work too.
pub fn percentile(values: &mut Vec<f64>, p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let count = values.len();
    let rank = ((p.clamp(0.0, 100.0) / 100.0) * count as f64).ceil() as usize;
    values[rank.clamp(1, count) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<f64> {
        // 1 through 20, shuffled, so the helper has to sort.
        vec![
            7.0, 19.0, 3.0, 12.0, 1.0, 20.0, 15.0, 9.0, 4.0, 18.0, 11.0, 2.0, 16.0, 6.0, 14.0,
            10.0, 5.0, 17.0, 8.0, 13.0,
        ]
    }

    #[test]
    fn percentile_p50_is_nearest_rank_median() {
        assert_eq!(percentile(&mut sample(), 50.0), 10.0);
    }

    #[test]
    fn percentile_p95_is_nearest_rank() {
        assert_eq!(percentile(&mut sample(), 95.0), 19.0);
    }

    #[test]
    fn percentile_p100_is_max_and_p0_is_min() {
        assert_eq!(percentile(&mut sample(), 100.0), 20.0);
        assert_eq!(percentile(&mut sample(), 0.0), 1.0);
    }

    #[test]
    fn percentile_of_empty_is_zero() {
        assert_eq!(percentile(&mut Vec::new(), 50.0), 0.0);
    }
}
