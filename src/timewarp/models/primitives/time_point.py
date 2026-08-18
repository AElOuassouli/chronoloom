"""The point event primitive: a value observed at a single instant."""

from typing import Any

from pydantic import BaseModel, PositiveInt


class TimePointEvent(BaseModel):
    """A single timestamped observation, optionally carrying an attribute."""

    timestamp: PositiveInt
    value: Any
    attribute: str | None = None
