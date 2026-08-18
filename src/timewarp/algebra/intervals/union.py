"""Set-algebra operations on time intervals."""

from timewarp.models import TimeIntervalEvent


# TODO: implement this function
def interval_union(
    interval_a: TimeIntervalEvent, interval_b: TimeIntervalEvent
) -> list[TimeIntervalEvent]:
    """Return the union of two time intervals."""
    return [interval_a, interval_b]


# TODO: implement this function
def interval_intersection(
    interval_a: TimeIntervalEvent, interval_b: TimeIntervalEvent
) -> list[TimeIntervalEvent]:
    """Return the intersection of two time intervals."""
    return [interval_a, interval_b]
