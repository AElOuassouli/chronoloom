import numpy as np
import pytest

from chronoloompy import IntervalError, TimeIntervalEvent, TimeIntervalSequence


def test_overlapping_and_touching_spans_merge():
    uptime = TimeIntervalSequence("up", [0, 20, 5], [5, 30, 25])

    assert uptime.spans.tolist() == [[0, 30]]
    assert len(uptime) == 1
    assert uptime.active_duration() == 30


def test_spans_round_trip_through_from_spans():
    original = TimeIntervalSequence("up", [0, 20], [10, 30])
    rebuilt = TimeIntervalSequence.from_spans("up", original.spans)

    assert rebuilt == original


def test_from_spans_requires_pairs():
    with pytest.raises(ValueError, match=r"shape \(n, 2\)"):
        TimeIntervalSequence.from_spans("up", np.arange(6).reshape(2, 3))


def test_empty_spans_are_refused():
    with pytest.raises(IntervalError):
        TimeIntervalSequence("up", [5], [5])


def test_coverage_is_half_open():
    uptime = TimeIntervalSequence("up", [0], [10])

    assert uptime.contains(0)
    assert not uptime.contains(10)
    assert uptime.at(9) == TimeIntervalEvent(0, 10)
    assert uptime.at(10) is None


def test_set_operations_describe_their_result():
    up = TimeIntervalSequence("up", [0], [100])
    busy = TimeIntervalSequence("busy", [10], [20])

    idle = up - busy
    assert idle.spans.tolist() == [[0, 10], [20, 100]]
    assert idle.label == "up \\ busy"
    label = (up.transform(1, 2) | busy).label
    assert label == "up[1, 2] ∪ busy"  # noqa: RUF001 -- the real union symbol
    assert (up & busy).spans.tolist() == [[10, 20]]
    assert (up ^ busy) == up.symmetric_difference(busy)


def test_transform_widens_and_narrows():
    alerts = TimeIntervalSequence("alerts", [0, 14], [10, 20])
    up = TimeIntervalSequence("up", [0, 10], [3, 30])

    assert alerts.transform(-2, 2).spans.tolist() == [[-2, 22]]
    assert up.transform(3, 0).spans.tolist() == [[13, 30]]
    assert up.transform(3, 0).transformation == (3, 0)


def test_transform_overflow_raises():
    edge = TimeIntervalSequence("edge", [0], [2**63 - 1])

    with pytest.raises(IntervalError):
        edge.transform(0, 1)


def test_covers_same_ignores_the_attribute():
    up = TimeIntervalSequence("up", [0], [10])
    reachable = TimeIntervalSequence("reachable", [0], [10])

    assert up != reachable
    assert up.covers_same(reachable)


def test_insert_extend_remove_and_iterate():
    uptime = TimeIntervalSequence("up")
    uptime.insert(0, 5)
    uptime.extend([20, 5], [30, 7])

    assert [span.bounds() for span in uptime] == [(0, 7), (20, 30)]
    assert uptime.remove(0) == TimeIntervalEvent(0, 7)
    assert uptime[-1] == TimeIntervalEvent(20, 30)
    with pytest.raises(IndexError):
        uptime.remove(5)


def test_set_attribute_forgets_the_derivation():
    idle = TimeIntervalSequence("up", [0], [10]).transform(1, 1)
    idle.set_attribute("idle")

    assert idle.label == "idle"
    assert idle.transformation == (0, 0)
