from timewarp._rust_lib import sum_as_int, sum_as_string
from timewarp.algebra import interval_union
from timewarp.models import TimeInterval


def test_interval_union(interval_a, interval_b):
    assert interval_union(
        interval_a,
        interval_b,
    ) == [
        TimeInterval(start_timestamp=0, end_timestamp=1),
        TimeInterval(start_timestamp=2, end_timestamp=3),
    ]

    assert sum_as_string(1, 2) == "3"
    assert sum_as_int(1, 2) == 3
