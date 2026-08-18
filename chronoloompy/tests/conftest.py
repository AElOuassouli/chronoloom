import pytest

from chronoloompy.models import TimeIntervalEvent


@pytest.fixture
def interval_a() -> TimeIntervalEvent:
    return TimeIntervalEvent(start_timestamp=0, end_timestamp=1)


@pytest.fixture
def interval_b() -> TimeIntervalEvent:
    return TimeIntervalEvent(start_timestamp=2, end_timestamp=3)
