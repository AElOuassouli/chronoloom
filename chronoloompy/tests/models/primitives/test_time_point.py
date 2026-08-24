import pytest
from pydantic import ValidationError

from chronoloompy.models import TimePointEvent


def test_timestamp_and_value_are_exposed():
    event = TimePointEvent(timestamp=1_700_000_000, value=21.5)

    assert event.timestamp == 1_700_000_000
    assert event.value == 21.5


def test_zero_and_negative_timestamps_are_accepted():
    assert TimePointEvent(timestamp=0, value=None).timestamp == 0
    assert TimePointEvent(timestamp=-1, value=None).timestamp == -1


def test_value_may_be_any_payload():
    tags = {"alpha", "beta"}

    assert TimePointEvent(timestamp=1, value=tags).value == tags


def test_value_is_required():
    # Omitting `value` is a static type error too; this pins the runtime half.
    with pytest.raises(ValidationError, match="value"):
        TimePointEvent(timestamp=1)  # type: ignore[call-arg]
