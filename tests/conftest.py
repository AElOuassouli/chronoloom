import pytest

from timewarp.models import TimeInterval


@pytest.fixture
def interval_a() -> TimeInterval:
    return TimeInterval(start_timestamp=0, end_timestamp=1)


@pytest.fixture
def interval_b() -> TimeInterval:
    return TimeInterval(start_timestamp=2, end_timestamp=3)
