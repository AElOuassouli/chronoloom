import pytest
from pydantic import ValidationError

from chronoloompy.models import TimeIntervalEvent


def test_bounds_and_value_are_exposed():
    interval = TimeIntervalEvent(start_timestamp=0, end_timestamp=60, value="warm-up")

    assert interval.start_timestamp == 0
    assert interval.end_timestamp == 60
    assert interval.value == "warm-up"


def test_value_is_optional():
    assert TimeIntervalEvent(start_timestamp=0, end_timestamp=5).value is None


def test_value_may_be_any_payload():
    tags = {"alpha", "beta"}
    interval = TimeIntervalEvent(start_timestamp=0, end_timestamp=5, value=tags)

    assert interval.value == tags


def test_empty_intervals_are_rejected():
    with pytest.raises(ValidationError, match="strictly after"):
        TimeIntervalEvent(start_timestamp=5, end_timestamp=5)


def test_inverted_intervals_are_rejected():
    with pytest.raises(ValidationError, match="strictly after"):
        TimeIntervalEvent(start_timestamp=9, end_timestamp=0)


def test_negative_bounds_are_accepted():
    interval = TimeIntervalEvent(start_timestamp=-10, end_timestamp=-4)

    assert interval.start_timestamp == -10
    assert interval.end_timestamp == -4
