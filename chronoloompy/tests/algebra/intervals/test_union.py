from chronoloompy.algebra import interval_union
from chronoloompy.models import TimeIntervalEvent


def test_interval_union(interval_a, interval_b):
    assert interval_union(
        interval_a,
        interval_b,
    ) == [
        TimeIntervalEvent(start_timestamp=0, end_timestamp=1),
        TimeIntervalEvent(start_timestamp=2, end_timestamp=3),
    ]
