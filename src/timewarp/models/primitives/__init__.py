"""Primitive event models: single points and intervals in time."""

from timewarp.models.primitives.interval import TimeIntervalEvent
from timewarp.models.primitives.time_point import TimePointEvent

__all__ = ["TimeIntervalEvent", "TimePointEvent"]
