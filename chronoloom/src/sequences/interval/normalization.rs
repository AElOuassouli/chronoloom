//! Putting spans into canonical form, and the arithmetic the covered total
//! is kept with. The invariant every other module relies on starts here.

use crate::primitives::TimeIntervalEvent;

/// Both of these are unreachable rather than unlikely — see [`span_ticks`] for
/// why the sum always fits, and the field doc on `active_duration` for why it
/// always accounts for exactly the spans present. They exist so a bug in that
/// bookkeeping surfaces here instead of as a silently wrong total.
pub(super) const OVERFLOW: &str =
    "covered ticks always fit in u64: spans are disjoint and bounded by i64";
pub(super) const UNDERFLOW: &str = "active_duration always accounts for every span present";

/// Put spans into canonical form: sorted by start, with everything that
/// combines combined.
///
/// The spans may arrive in any order and may overlap freely. Sorting first
/// guarantees that every span able to merge with the one still open comes
/// immediately next, so one fold through [`absorb`] finishes the job.
pub(super) fn normalize(mut spans: Vec<TimeIntervalEvent<()>>) -> Vec<TimeIntervalEvent<()>> {
    spans.sort_by_key(TimeIntervalEvent::start);

    let mut normalized: Vec<TimeIntervalEvent<()>> = Vec::with_capacity(spans.len());
    for span in spans {
        absorb(&mut normalized, span);
    }

    normalized
}

/// The ticks a span covers, exactly.
///
/// [`TimeIntervalEvent::duration`] saturates at [`i64::MAX`] to stay in `i64`,
/// which a running total must not do. A half-open span inside `i64` is at most
/// `i64::MAX - i64::MIN` ticks wide — exactly [`u64::MAX`] — so `u64` holds
/// every one of them, and the widest span reports its true width rather than a
/// clamped one.
pub(super) fn span_ticks(span: &TimeIntervalEvent<()>) -> u64 {
    let (start, end) = span.bounds();

    // Via `i128` because the difference can exceed `i64`; the subtraction
    // cannot be negative, since an interval always ends after it starts.
    u64::try_from(i128::from(end) - i128::from(start)).expect("a span always ends after it starts")
}

/// Fold `span` into an already-normalized `normalized`, merging when the two
/// combine.
///
/// `span` must start at or after the last span in `normalized`, which every
/// caller guarantees by walking its input in order. Whether two spans combine,
/// and into what, is [`TimeIntervalEvent::merged`]'s decision — this exists so
/// the sequence never restates that rule.
pub(super) fn absorb(normalized: &mut Vec<TimeIntervalEvent<()>>, span: TimeIntervalEvent<()>) {
    match normalized.pop() {
        Some(open) => match open.merged(&span) {
            Some(merged) => normalized.push(merged),
            None => {
                normalized.push(open);
                normalized.push(span);
            }
        },
        None => normalized.push(span),
    }
}
