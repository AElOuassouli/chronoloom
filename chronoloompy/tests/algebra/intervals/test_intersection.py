"""Covers the pyo3 binding, and through it the chronoloom Rust core."""

from chronoloompy._core import intersection


def test_overlapping_intervals_intersect():
    assert intersection((0, 5), (3, 9)) == (3, 5)


def test_disjoint_intervals_do_not_intersect():
    assert intersection((0, 2), (5, 9)) is None


def test_touching_intervals_do_not_intersect():
    # Intervals are half-open, so a shared endpoint is not an overlap.
    assert intersection((0, 5), (5, 9)) is None
