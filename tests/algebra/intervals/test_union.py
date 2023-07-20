from timewrap.algebra import interval_union
from timewrap.models import TimeInterval


def test_interval_union(interval_a, interval_b):
    assert interval_union(
        interval_a,
        interval_b,
    ) == [
        TimeInterval(start_timestamp=0, end_timestamp=1),
        TimeInterval(start_timestamp=2, end_timestamp=3),
    ]
