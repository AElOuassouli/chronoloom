//! Set-algebra operations on time intervals.

/// Intersect two half-open intervals `[start, end)`.
///
/// Returns `None` when the intervals do not overlap. Intervals that merely
/// touch do not overlap, since `end` is exclusive.
///
/// # Examples
///
/// ```
/// use chronoloom::algebra::intersection;
///
/// assert_eq!(intersection((0, 5), (3, 9)), Some((3, 5)));
/// assert_eq!(intersection((0, 2), (5, 9)), None);
/// assert_eq!(intersection((0, 5), (5, 9)), None);
/// ```
pub fn intersection(a: (i64, i64), b: (i64, i64)) -> Option<(i64, i64)> {
    let start = a.0.max(b.0);
    let end = a.1.min(b.1);
    (start < end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::intersection;

    #[test]
    fn overlapping_intervals_intersect() {
        assert_eq!(intersection((0, 5), (3, 9)), Some((3, 5)));
    }

    #[test]
    fn disjoint_intervals_do_not_intersect() {
        assert_eq!(intersection((0, 2), (5, 9)), None);
    }

    #[test]
    fn touching_intervals_do_not_intersect() {
        assert_eq!(intersection((0, 5), (5, 9)), None);
    }
}
