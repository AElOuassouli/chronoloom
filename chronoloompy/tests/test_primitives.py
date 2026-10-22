import pytest

from chronoloompy import IntervalError, TimeIntervalEvent, TimePointEvent


def test_point_event_exposes_timestamp_and_any_value():
    tags = {"alpha", "beta"}
    event = TimePointEvent(-1, tags)

    assert event.timestamp == -1
    assert event.value is tags


def test_point_events_compare_by_timestamp_and_value():
    assert TimePointEvent(1, 2.0) == TimePointEvent(1, 2.0)
    assert TimePointEvent(1, 2.0) != TimePointEvent(1, 3.0)


def test_interval_value_defaults_to_none():
    interval = TimeIntervalEvent(0, 60)

    assert interval.bounds() == (0, 60)
    assert interval.duration() == 60
    assert interval.value is None


@pytest.mark.parametrize(("start", "end"), [(5, 5), (9, 0)])
def test_empty_and_inverted_intervals_are_rejected(start, end):
    with pytest.raises(IntervalError, match="strictly after"):
        TimeIntervalEvent(start, end)


def test_interval_error_is_a_value_error():
    assert issubclass(IntervalError, ValueError)


def test_touching_intervals_do_not_intersect_but_do_merge():
    a = TimeIntervalEvent(0, 5, "a")
    b = TimeIntervalEvent(5, 9, "b")

    assert a.intersection(b) is None
    assert a.merged(b) == TimeIntervalEvent(0, 9)
    assert a.union(b) == [TimeIntervalEvent(0, 9)]


def test_union_keeps_a_gap_as_two_spans():
    spans = TimeIntervalEvent(0, 5).union(TimeIntervalEvent(7, 9))

    assert [span.bounds() for span in spans] == [(0, 5), (7, 9)]
