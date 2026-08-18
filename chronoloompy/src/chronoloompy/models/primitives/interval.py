"""The interval event primitive: a value attached to a span of time."""

from typing import Any

from pydantic import BaseModel


class TimeIntervalEvent(BaseModel):
    """A half-open time interval, optionally carrying an attribute."""

    start_timestamp: int
    end_timestamp: int
    attribute: Any | None = None
    left_open: bool = False
    right_open: bool = True

    ## TODO: add validator to ensure that start_timestamp < end_timestamp
